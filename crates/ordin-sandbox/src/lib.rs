//! The Sandbox: runs code an agent writes in the WebAssembly room of `sandboxd`, linked
//! in-process (Sandbox.md). It builds `sandboxd`'s own parts directly, one controller per agent
//! instance, and replaces the daemon's interaction surface, secret resolver and audit sink with
//! ordin's Identity and Journal. It owns nothing durable.
//!
//! The program registers `tool_provider()` with the MCP Front before the MCP Front starts
//! (SA-RECON-02) and reads `status()` for the start-up report (SA-RECON-04).

// The shared test helpers (`tests/support`) name the crate; the unit tests reuse them.
#[cfg(test)]
extern crate self as ordin_sandbox;

mod agent;
mod args;
mod call;
mod error;
mod grants;
mod manifest;
mod work;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ordin_identity::{IdentityService, SecretName};
use ordin_journal::{EventSink, Journal, JournalEvent, SubscriptionId};
use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolCaller,
    ToolDispatch, ToolFailure, ToolKey, ToolOutput, ToolProvider, ToolSpec,
};
use ordin_types::domain::runs::RunState;
use ordin_types::{AgentInstanceId, RunId, WorkspaceId};
use sandboxd::types::AuditEntry;
use tokio::sync::{mpsc, Semaphore};
use tokio_util::sync::CancellationToken;

pub use error::ALL_CODES;
pub use grants::{
    GrantsUnavailable, KeyPlacementSpec, PlaceFileError, PlaceFiles, SandboxCapability,
    SandboxCapabilityKind, SandboxCredential, SandboxGrants,
};

use agent::{AgentController, ModuleRefs, ModuleRoute, RunContexts};
use work::WorkSession;

/// The Python guest, committed beside `sandboxd`.
const GUEST_PY: &[u8] = include_bytes!("../../../sandbox/sandbox-host/guest/pysandbox-guest.wasm");
/// Its SHA-256, computed by `build.rs`.
const GUEST_PY_DIGEST: &str = env!("ORDIN_SANDBOX_PY_DIGEST");

/// The MCP server this crate serves (SA-NAME-04).
const SERVER: &str = "sandbox";
const PYTHON_TOOL: &str = "python_sandbox";
/// Concurrent guest executions per program.
const GUEST_SLOTS: usize = 4;
/// How often the idle sweeper runs.
const SWEEP_EVERY: Duration = Duration::from_secs(60);
/// Controllers unused this long are dropped.
const CONTROLLER_IDLE: Duration = Duration::from_secs(30 * 60);
/// How long `shutdown` waits for runs in progress (SA-STOP-01).
const STOP_WAIT: Duration = Duration::from_secs(10);
/// How long ending a work session waits for its executing run.
const SESSION_END_WAIT: Duration = Duration::from_secs(35);
/// The audit key's secret name and length.
const AUDIT_KEY_NAME: &str = "sandbox/audit-hmac-key";
const AUDIT_KEY_LEN: usize = 32;
const AUDIT_KEY_REF: &str = "ordin:sandbox.audit_hmac_key";

pub struct SandboxDeps {
    pub dispatch: Arc<dyn ToolDispatch>,
    /// Implemented by the program (SA-RECON-05).
    pub grants: Arc<dyn SandboxGrants>,
    pub identity: Arc<IdentityService>,
    pub journal: Journal,
    pub place_files: Arc<dyn PlaceFiles>,
    /// Profile `sandbox.session_idle_minutes`, already clamped to 1–240 by the Organisation Profile.
    pub session_idle: Duration,
    pub data_root: PathBuf,
    pub log_level: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxStatus {
    Ready {
        guests: Vec<GuestKind>,
    },
    /// `code` is one of SBX_GUEST_DIGEST_MISMATCH, SBX_CONFIG_INVALID, SBX_AUDIT_KEY_UNAVAILABLE.
    Unavailable {
        code: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestKind {
    Python,
    Shell,
    JavaScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEndReason {
    RunEnded,
    Idle,
    Shutdown,
}

impl SessionEndReason {
    fn as_str(self) -> &'static str {
        match self {
            SessionEndReason::RunEnded => "run_ended",
            SessionEndReason::Idle => "idle",
            SessionEndReason::Shutdown => "shutdown",
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Everything a call and the background tasks share.
pub(crate) struct Core {
    dispatch: Arc<dyn ToolDispatch>,
    grants: Arc<dyn SandboxGrants>,
    identity: Arc<IdentityService>,
    journal: Journal,
    place_files: Arc<dyn PlaceFiles>,
    session_idle: Duration,
    sessions_root: PathBuf,
    status: SandboxStatus,
    audit_key: Vec<u8>,
    audit_tx: mpsc::UnboundedSender<(AuditEntry, WorkspaceId)>,
    guest_slots: Semaphore,
    runs: RunContexts,
    module: Arc<ModuleRoute>,
    controllers: tokio::sync::Mutex<HashMap<(AgentInstanceId, WorkspaceId), Arc<AgentController>>>,
    sessions: Mutex<HashMap<RunId, WorkSession>>,
    stop: CancellationToken,
    subscription: Mutex<Option<SubscriptionId>>,
    guest: &'static [u8],
    guest_digest: &'static str,
}

impl Core {
    fn ready(&self) -> bool {
        matches!(self.status, SandboxStatus::Ready { .. })
    }

    /// The tools of step B.1.
    fn tool_specs(&self) -> Vec<ToolSpec> {
        if !self.ready() {
            return Vec::new();
        }
        vec![ToolSpec {
            name: PYTHON_TOOL.to_owned(),
            description: "Run Python in the sandbox; call granted systems with api.<name>, ordin tools with ordin_tools.<tool>; files in /work.".to_owned(),
            input_schema: args::input_schema(),
            argument_form: "python_sandbox(code: string, requestedCapabilities?: [name], dryRun?: bool, files?: [{path, contentBase64}], inputs?: [{placeId, path, as?}], workSession?: bool)".to_owned(),
            sandbox_callable: false,
            plain: "Can run Python code in a sandbox with no network and no files of its own.".to_owned(),
        }]
    }

    /// Ends one work session (F.3).
    async fn end_work_session(&self, run_id: RunId, reason: SessionEndReason) {
        let deadline = Instant::now() + SESSION_END_WAIT;
        loop {
            let busy = lock(&self.sessions).get(&run_id).map(|s| s.busy);
            match busy {
                None => return,
                Some(true) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Some(_) => break,
            }
        }
        let Some(session) = lock(&self.sessions).remove(&run_id) else {
            return;
        };
        match work::remove(&session.dir) {
            Ok((files, bytes)) => tracing::info!(
                target: "ordin_sandbox",
                run_id = %run_id, reason = reason.as_str(), files, bytes,
                "sandbox.session_deleted"
            ),
            Err(_) => {
                tracing::warn!(target: "ordin_sandbox", run_id = %run_id, "sandbox.session_delete_failed");
                // Put back as idle, so the next sweep retries the delete (F.3).
                let mut retry = session;
                retry.busy = false;
                retry.last_used = Instant::now() - self.session_idle;
                lock(&self.sessions).insert(run_id, retry);
            }
        }
    }

    /// One pass of the sweeper (F.2).
    async fn sweep(&self) {
        let idle: Vec<RunId> = lock(&self.sessions)
            .iter()
            .filter(|(_, s)| !s.busy && s.last_used.elapsed() >= self.session_idle)
            .map(|(id, _)| *id)
            .collect();
        for run_id in idle {
            self.end_work_session(run_id, SessionEndReason::Idle).await;
        }
        self.controllers
            .lock()
            .await
            .retain(|_, c| lock(&c.last_used).elapsed() < CONTROLLER_IDLE);
        lock(&self.module.refs).retain(|_, (_, expires)| *expires > Instant::now());
    }
}

/// The run-end listener (F.1).
struct RunEndListener {
    core: Arc<Core>,
}

#[async_trait]
impl EventSink for RunEndListener {
    fn name(&self) -> &'static str {
        "sandbox_run_end"
    }

    async fn deliver(&self, event: &JournalEvent) {
        if event.kind != "run_state_changed" {
            return;
        }
        let run_id = event
            .data
            .get("run_id")
            .and_then(|v| serde_json::from_value::<RunId>(v.clone()).ok());
        let state = event
            .data
            .get("state")
            .and_then(|v| serde_json::from_value::<RunState>(v.clone()).ok());
        let terminal = matches!(
            state,
            Some(
                RunState::Completed
                    | RunState::Failed
                    | RunState::StoppedAtLimit
                    | RunState::Cancelled
                    | RunState::Interrupted
                    | RunState::Abandoned
                    | RunState::NoReply
            )
        );
        if let (Some(run_id), true) = (run_id, terminal) {
            self.core
                .end_work_session(run_id, SessionEndReason::RunEnded)
                .await;
        }
    }

    async fn lagged(&self, _next_seq: u64) {
        self.core.sweep().await;
    }
}

pub struct Sandbox {
    core: Arc<Core>,
    provider: Arc<SandboxToolProvider>,
}

/// Registered with the MCP Front by the program; implements `ToolProvider` for server `sandbox`.
pub struct SandboxToolProvider {
    core: Arc<Core>,
}

/// The in-process configuration map given to `sandboxd::config::Config::load` (2D-1f).
fn config_map(
    data_root: &std::path::Path,
    digest: &str,
    log_level: &str,
) -> HashMap<&'static str, String> {
    HashMap::from([
        ("PYS_REQUIRE_FIRST_CONNECT_CONSENT", "false".to_owned()),
        (
            "PYS_POLICY_PATH",
            data_root
                .join("sandbox")
                .join("policy.json")
                .to_string_lossy()
                .into_owned(),
        ),
        ("PYS_MAX_CALLS_PER_RUN", "50".to_owned()),
        ("PYS_MAX_CALLS_PER_SESSION", "500".to_owned()),
        ("PYS_RESPONSE_MAX_BYTES", "1048576".to_owned()),
        ("PYS_ALLOW_PLAINTEXT_LOOPBACK_EGRESS", "false".to_owned()),
        ("PYS_WASM_MAX_MEMORY_BYTES", "536870912".to_owned()),
        ("PYS_WASM_DEADLINE_SECONDS", "30".to_owned()),
        ("PYS_GUEST_ARTIFACT_DIGEST", digest.to_owned()),
        ("PYS_AUDIT_HMAC_KEY_REF", AUDIT_KEY_REF.to_owned()),
        ("PYS_LOG_LEVEL", log_level.to_owned()),
    ])
}

impl Sandbox {
    /// Builds the shared parts, verifies the guest digest, deletes leftover work-session
    /// folders and starts the idle sweeper and the run-end listener. Never fails the program's
    /// start: on a start-up fault its provider offers no tools and `status()` reports
    /// `SandboxStatus::Unavailable`.
    pub async fn start(deps: SandboxDeps) -> Arc<Sandbox> {
        Sandbox::start_with(deps, GUEST_PY, GUEST_PY_DIGEST).await
    }

    pub(crate) async fn start_with(
        deps: SandboxDeps,
        guest: &'static [u8],
        guest_digest: &'static str,
    ) -> Arc<Sandbox> {
        let (status, audit_key) = Sandbox::prepare(&deps, guest, guest_digest).await;
        let (audit_tx, mut audit_rx) = mpsc::unbounded_channel::<(AuditEntry, WorkspaceId)>();
        let runs: RunContexts = Arc::new(Mutex::new(HashMap::new()));
        let refs: ModuleRefs = Arc::new(Mutex::new(HashMap::new()));
        let core = Arc::new(Core {
            module: Arc::new(ModuleRoute {
                refs,
                runs: runs.clone(),
                dispatch: deps.dispatch.clone(),
            }),
            dispatch: deps.dispatch,
            grants: deps.grants,
            identity: deps.identity,
            journal: deps.journal,
            place_files: deps.place_files,
            session_idle: deps.session_idle,
            sessions_root: deps.data_root.join("sandbox").join("sessions"),
            status,
            audit_key,
            audit_tx,
            guest_slots: Semaphore::new(GUEST_SLOTS),
            runs,
            controllers: tokio::sync::Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            stop: CancellationToken::new(),
            subscription: Mutex::new(None),
            guest,
            guest_digest,
        });

        if core.ready() {
            // A.5: leftovers of a process that did not stop cleanly.
            let count = std::fs::read_dir(&core.sessions_root)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| std::fs::remove_dir_all(e.path()).is_ok())
                        .count()
                })
                .unwrap_or(0);
            let _ = work::create_private_dir(&core.sessions_root);
            tracing::info!(target: "ordin_sandbox", count, "sandbox.sessions_cleared");

            // A.6: the run-end listener, the idle sweeper and the audit drainer.
            let id = core
                .journal
                .subscribe(Arc::new(RunEndListener { core: core.clone() }));
            *lock(&core.subscription) = Some(id);
            let weak = Arc::downgrade(&core);
            let stop = core.stop.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(SWEEP_EVERY);
                tick.tick().await;
                loop {
                    tokio::select! {
                        _ = stop.cancelled() => return,
                        _ = tick.tick() => {}
                    }
                    let Some(core) = weak.upgrade() else { return };
                    core.sweep().await;
                }
            });
            let journal = core.journal.clone();
            tokio::spawn(async move {
                while let Some((entry, workspace)) = audit_rx.recv().await {
                    let new = agent::broker_call_entry(&entry, workspace);
                    if journal.append_entry(new).await.is_err() {
                        tracing::warn!(target: "ordin_sandbox", kind = "sandbox_broker_call", "sandbox.journal_failed");
                    }
                }
            });
        }

        let guests = match &core.status {
            SandboxStatus::Ready { guests } => format!("{guests:?}"),
            SandboxStatus::Unavailable { .. } => "[]".to_owned(),
        };
        let status = match &core.status {
            SandboxStatus::Ready { .. } => "ready",
            SandboxStatus::Unavailable { code } => code,
        };
        tracing::info!(target: "ordin_sandbox", status, guests = guests.as_str(), "sandbox.started");
        Arc::new(Sandbox {
            provider: Arc::new(SandboxToolProvider { core: core.clone() }),
            core,
        })
    }

    /// Steps A.1–A.4: the audit key, the configuration and the guest check.
    async fn prepare(
        deps: &SandboxDeps,
        guest: &'static [u8],
        guest_digest: &'static str,
    ) -> (SandboxStatus, Vec<u8>) {
        let unavailable = |code: &'static str, field: Option<String>| {
            tracing::warn!(target: "ordin_sandbox", code, field = field.as_deref(), "sandbox.unavailable");
            (SandboxStatus::Unavailable { code }, Vec::new())
        };
        // spec: ordin-desktop/components/Sandbox#SBX_AUDIT_KEY_UNAVAILABLE
        let key = match SecretName::new(AUDIT_KEY_NAME) {
            Ok(name) => deps
                .identity
                .secrets()
                .get_or_create_random(&name, AUDIT_KEY_LEN)
                .await
                .ok(),
            Err(_) => None,
        };
        let Some(key) = key else {
            return unavailable("SBX_AUDIT_KEY_UNAVAILABLE", None);
        };
        let key = key.to_vec();

        // spec: ordin-desktop/components/Sandbox#SBX_CONFIG_INVALID
        let map = config_map(&deps.data_root, guest_digest, &deps.log_level);
        let resolver =
            agent::PrefilledResolver(HashMap::from([(AUDIT_KEY_REF.to_owned(), key.clone())]));
        if let Err(e) = sandboxd::config::Config::load(&|k| map.get(k).cloned(), &resolver) {
            return unavailable("SBX_CONFIG_INVALID", Some(e.field));
        }

        // spec: ordin-desktop/components/Sandbox#SBX_GUEST_DIGEST_MISMATCH
        let probe = tokio::task::spawn_blocking(move || {
            sandboxd::runtime::SandboxRuntime::new(
                guest_digest,
                guest,
                Arc::new(agent::ProbeBroker),
            )
            .is_ok()
        })
        .await
        .unwrap_or(false);
        if !probe {
            return unavailable("SBX_GUEST_DIGEST_MISMATCH", None);
        }
        #[cfg(feature = "extra-guests")]
        for guest in ["shell", "javascript"] {
            // SH-3 of *Changes required in `sandbox/sandbox-host`*: `sandboxd` ships no shell or
            // JavaScript guest yet, so neither is ready.
            tracing::warn!(target: "ordin_sandbox", guest, "sandbox.guest_unavailable");
        }
        (
            SandboxStatus::Ready {
                guests: vec![GuestKind::Python],
            },
            key,
        )
    }

    /// The `sandbox` tool provider, for the program to register with the MCP Front before the
    /// MCP Front's `start` (SA-RECON-02). Every call returns the same provider.
    // spec: ordin-desktop/components/Sandbox#SA-RECON-02
    pub fn tool_provider(&self) -> Arc<dyn ToolProvider> {
        self.provider.clone()
    }

    /// For the start-up report (G9) and Diagnostics.
    // spec: ordin-desktop/components/Sandbox#SA-RECON-04
    pub fn status(&self) -> SandboxStatus {
        self.core.status.clone()
    }

    /// Ends one agent session's work session now (called on the run's end; also by the sweeper).
    pub async fn end_work_session(&self, run_id: RunId, reason: SessionEndReason) {
        self.core.end_work_session(run_id, reason).await;
    }

    /// Stops accepting calls, waits up to 10 s (SA-STOP-01) for runs in progress, deletes every
    /// work-session folder.
    // spec: ordin-desktop/components/Sandbox#SA-STOP-01
    pub async fn shutdown(&self) {
        self.core.stop.cancel();
        if let Some(id) = lock(&self.core.subscription).take() {
            self.core.journal.unsubscribe(id);
        }
        let all = GUEST_SLOTS as u32;
        if let Ok(Ok(permits)) =
            tokio::time::timeout(STOP_WAIT, self.core.guest_slots.acquire_many(all)).await
        {
            permits.forget();
        }
        self.core.guest_slots.close();
        let sessions: Vec<WorkSession> =
            lock(&self.core.sessions).drain().map(|(_, s)| s).collect();
        for session in sessions {
            let _ = work::remove(&session.dir);
        }
        tracing::info!(target: "ordin_sandbox", "sandbox.stopped");
    }

    /// One sweeper pass, for tests that cannot wait for the 60-second tick.
    #[cfg(test)]
    pub(crate) async fn sweep_now(&self) {
        self.core.sweep().await;
    }
}

#[async_trait]
impl ToolProvider for SandboxToolProvider {
    fn provider_id(&self) -> &'static str {
        "sandbox"
    }

    fn servers(&self) -> Vec<ServerSpec> {
        match ServerName::new(SERVER) {
            Ok(name) => vec![ServerSpec {
                name,
                tools: self.core.tool_specs(),
            }],
            Err(_) => Vec::new(),
        }
    }

    // spec: ordin-desktop/components/Sandbox#SBX_CALLER_NOT_SUPPORTED
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        if matches!(grant.caller, ToolCaller::HostedAgent) {
            return Ok(Vec::new());
        }
        let Ok(server) = ServerName::new(SERVER) else {
            return Ok(Vec::new());
        };
        Ok(self
            .core
            .tool_specs()
            .into_iter()
            .map(|spec| ToolKey {
                server: server.clone(),
                tool: spec.name,
            })
            .collect())
    }

    fn check_arguments(
        &self,
        _key: &ToolKey,
        arguments: &serde_json::Value,
    ) -> Result<(), ArgumentProblem> {
        args::check(arguments).map(|_| ()).inspect_err(|_| {
            tracing::info!(target: "ordin_sandbox", code = "SBX_ARGS_INVALID", "sandbox.args_refused");
        })
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolFailure> {
        call::call(&self.core, ctx, key, arguments).await
    }

    async fn session_closed(&self, run_id: RunId) {
        self.core
            .end_work_session(run_id, SessionEndReason::RunEnded)
            .await;
    }
}

#[cfg(test)]
mod tests;
