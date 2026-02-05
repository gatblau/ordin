//! The service object: `new`, `shutdown`, the probe calls, `harness_models`, and `RuntimeAdapter`
//! (CopilotLocalAdapter.md *Public Interface*, `start` steps 1–6 and 11, `resume`, `read`, `cancel`).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::BoxStream;
use ordin_hook_relay::HookRelay;
use ordin_identity::{CredError, IdentityService};
use ordin_mcp_front::{McpFront, ProgramProfile};
use ordin_rules::RulesEngine;
use ordin_runtime_adapter::{
    check_resume, check_start, AdapterContext, AdapterError, AdapterKind, AdapterVerb,
    HarnessModels, PauseReason, ResumeDecision, RunEndReason, RunEvent, RunHandle, RunSnapshot,
    RunTarget, RuntimeAdapter, StartRun,
};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::{AgentInstanceId, RunId};
use tokio::sync::oneshot;

use crate::config::{
    CopilotAdapterConfig, CopilotCli, CopilotCliSource, SessionSetupSource,
    C48_SERVER_SIGN_IN_CONFIRMED,
};
use crate::env::{server_environment, ParentEnv};
use crate::error::{CopilotAdapterError, Failure};
use crate::models::CopilotModels;
use crate::port::{CliServerSpec, CopilotAuthStatus, CopilotPort, CopilotServer};
use crate::ports::{HookPort, IdentityPort, McpPort, RulesPort};
use crate::run::{lock, run_log, Run, RunCommand, RunIdentity, MAX_ENDED_RUNS};
use crate::task::{run_task, TaskStart};

/// `resume`: the longest `Answer.text`, in characters (2C).
const MAX_ANSWER: usize = ordin_runtime_adapter::MAX_ANSWER_CHARS;

/// The durations of the adapter. [`Timings::SPEC`] holds the spec's values; the crate's own tests
/// build shorter sets so that a 15-minute sign-in does not take 15 minutes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timings {
    /// Sign-in status poll: 5 s (*chosen here*).
    pub sign_in_poll: Duration,
    /// Sign-in attempt: 15 minutes (*chosen here*).
    pub sign_in_give_up: Duration,
    /// The bound of one gate decision: 55 s (*chosen here: above the relay's 50 s deadline*).
    pub gate_bound: Duration,
}

impl Timings {
    pub(crate) const SPEC: Timings = Timings {
        sign_in_poll: Duration::from_secs(5),
        sign_in_give_up: Duration::from_secs(15 * 60),
        gate_bound: Duration::from_secs(55),
    };
}

/// What this process gives the adapter, replaced in tests.
pub(crate) struct Host {
    pub timings: Timings,
    pub parent_env: ParentEnv,
}

impl Host {
    fn real() -> Host {
        Host {
            timings: Timings::SPEC,
            parent_env: Arc::new(|name: &str| -> Option<OsString> { std::env::var_os(name) }),
        }
    }
}

/// The services the adapter and each run's task need.
pub(crate) struct Inner {
    pub config: CopilotAdapterConfig,
    pub port: Arc<dyn CopilotPort>,
    pub cli: Arc<dyn CopilotCliSource>,
    pub mcp: Arc<dyn McpPort>,
    pub hooks: Arc<dyn HookPort>,
    pub rules: Arc<dyn RulesPort>,
    pub identity: Arc<dyn IdentityPort>,
    pub setup: Arc<dyn SessionSetupSource>,
    pub host: Host,
    pub runs: Mutex<HashMap<RunId, Arc<Run>>>,
    /// `run_id`s between step 1 and the run entry of step 5.
    pub starting: Mutex<HashSet<RunId>>,
    /// The CLI server the probe calls use, and the CLI it was started from.
    probe: tokio::sync::Mutex<Option<(CopilotCli, Arc<dyn CopilotServer>)>>,
    /// While a sign-in started by `probe_start_sign_in` is not seen: when it started and when the probe
    /// server was last started again for it (CPA-CPX-6).
    login: Mutex<Option<(tokio::time::Instant, tokio::time::Instant)>>,
}

/// CPA-CPX-6: how often, at most, the probe server is started again while a sign-in is awaited, and for
/// how long after the sign-in started.
#[cfg(not(test))]
const LOGIN_RECHECK: Duration = Duration::from_secs(15);
#[cfg(not(test))]
const LOGIN_RECHECK_FOR: Duration = Duration::from_secs(15 * 60);
/// The same rule at a test's pace.
#[cfg(test)]
const LOGIN_RECHECK: Duration = Duration::from_millis(150);
#[cfg(test)]
const LOGIN_RECHECK_FOR: Duration = Duration::from_secs(60);

impl Inner {
    pub(crate) fn run(&self, run_id: RunId) -> Option<Arc<Run>> {
        lock(&self.runs).get(&run_id).cloned()
    }

    /// Adds a run, first evicting the longest-ended runs past [`MAX_ENDED_RUNS`].
    fn hold_run(&self, run: Arc<Run>) {
        let mut runs = lock(&self.runs);
        let mut ended: Vec<_> = runs
            .iter()
            .filter_map(|(id, held)| held.ended_at().map(|at| (at, *id)))
            .collect();
        if ended.len() >= MAX_ENDED_RUNS {
            ended.sort();
            for (_, id) in &ended[..=ended.len() - MAX_ENDED_RUNS] {
                runs.remove(id);
            }
        }
        runs.insert(run.run_id, run);
    }

    /// Step 3 (CPA-CPX-3): the person's CLI, absolute, at a tested version.
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-3
    async fn current_cli(&self, run_id: RunId) -> Result<CopilotCli, Failure> {
        let Some(found) = self
            .cli
            .current()
            .await
            .filter(|found| found.path.is_absolute())
        else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                run_id = %run_id,
                code = "CPA_SERVER_START_FAILED",
                "copilot.cli.missing"
            );
            return Err(Failure::unreachable("CPA_SERVER_START_FAILED", true));
        };
        let tested = &self.config.tested_versions;
        if !tested.is_empty() && !tested.contains(&found.version) {
            return Err(Failure::new(
                "CPA_HARNESS_UNTESTED",
                false,
                AdapterError::HarnessUntested {
                    found: found.version,
                },
            ));
        }
        Ok(found)
    }

    /// Steps 4 and 5: one CLI server for this session, in the agent folder.
    async fn start_server(
        &self,
        run_id: RunId,
        cli: &CopilotCli,
        working_folder: &std::path::Path,
    ) -> Result<Arc<dyn CopilotServer>, Failure> {
        let spec = CliServerSpec {
            cli_path: cli.path.clone(),
            working_folder: working_folder.to_path_buf(),
            env: server_environment(&self.host.parent_env, &cli.path),
        };
        let server = self.port.start_server(spec).await.map_err(|_| {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                run_id = %run_id,
                code = "CPA_SERVER_START_FAILED",
                "copilot.server.start_failed"
            );
            Failure::unreachable("CPA_SERVER_START_FAILED", true)
        })?;
        tracing::info!(
            target: "ordin_copilot_adapter",
            run_id = %run_id,
            cli_version = cli.version.as_str(),
            "copilot.server.started"
        );
        Ok(Arc::from(server))
    }

    /// Step 6 up to the pause: `true` when signed in, after the silent GitHub refresh if needed.
    pub(crate) async fn signed_in(
        &self,
        run: &Run,
        server: &dyn CopilotServer,
    ) -> Result<bool, Failure> {
        let unreadable = |_| Failure::unreachable("CPA_AUTH_STATUS_UNREADABLE", true);
        let status = server.auth_status().await.map_err(unreadable)?;
        run_log!(
            info,
            run,
            "copilot.sign_in.status",
            signed_in = status.signed_in
        );
        if status.signed_in {
            return Ok(true);
        }
        // Step 6.1 (03 golden path). A refresh that fails is a refresh that did not help.
        if let Err(error) = self.identity.refresh_silently(CredentialKind::Github).await {
            let (code, retryable) = match error {
                CredError::Retryable { code } => (code, true),
                CredError::Permanent { code } => (code, false),
            };
            run_log!(
                warn,
                run,
                "copilot.sign_in.refresh_failed",
                code = code,
                retryable = retryable
            );
        }
        let status = server.auth_status().await.map_err(unreadable)?;
        run_log!(
            info,
            run,
            "copilot.sign_in.status",
            signed_in = status.signed_in
        );
        Ok(status.signed_in)
    }

    async fn start_steps(
        self: &Arc<Self>,
        ctx: &AdapterContext,
        req: StartRun,
    ) -> Result<RunHandle, Failure> {
        let run_id = req.run_id;
        let agent_instance_id = check_request(&req)?;
        tracing::info!(
            target: "ordin_copilot_adapter",
            run_id = %run_id,
            agent_instance_id = agent_instance_id.to_string().as_str(),
            workspace_id = req.workspace_id.to_string().as_str(),
            correlation_id = ctx.correlation_id.as_str(),
            "copilot.start.received"
        );
        // Step 2, with the start request's attachments passed as received (CPA-REC-1).
        // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-REC-1
        let setup = self
            .setup
            .session_setup(run_id, agent_instance_id, &req.attachments)
            .await
            .map_err(|error| {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    run_id = %run_id,
                    code = "CPA_SETUP_FAILED",
                    setup_code = error.code.as_str(),
                    "copilot.start.setup_failed"
                );
                Failure::unreachable("CPA_SETUP_FAILED", true)
            })?;
        if !setup.agent_folder.is_dir() {
            return Err(Failure::unreachable("CPA_AGENT_FOLDER_MISSING", false));
        }
        let cli = self.current_cli(run_id).await?;
        let server = self.start_server(run_id, &cli, &setup.agent_folder).await?;
        let resume_id = setup.resume_session_id.clone();
        let (run, inbox) = Run::new(RunIdentity {
            run_id,
            agent_instance_id,
            workspace_id: req.workspace_id,
            correlation_id: ctx.correlation_id.clone(),
            caller: req.caller,
            scope_id: req.scope_id,
            coding_task_id: req.coding_task_id,
            only_servers: req.only_servers.clone(),
            setup,
        });
        *lock(&run.server) = Some(server.clone());
        self.hold_run(run.clone());

        // Step 6.
        let signed_in = match self.signed_in(&run, server.as_ref()).await {
            Ok(signed_in) => signed_in,
            Err(failure) => {
                lock(&self.runs).remove(&run_id);
                self.close_session(&run).await;
                return Err(failure);
            }
        };
        if !signed_in {
            // Step 6.2.
            // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SIGNED_OUT
            run.emit_started(resume_id.clone());
            run.emit(RunEvent::Paused {
                reason: PauseReason::SignIn,
            });
            run_log!(
                info,
                run,
                "copilot.run.paused",
                reason_kind = "sign_in",
                code = "CPA_SIGNED_OUT"
            );
            self.start_login(&run, server.as_ref()).await;
            tokio::spawn(run_task(
                self.clone(),
                run,
                inbox,
                TaskStart::SignIn { input: req.input },
            ));
            return Ok(RunHandle {
                run_id,
                adapter: AdapterKind::CopilotLocal,
                runtime_session_id: resume_id,
                remote_thread_id: None,
            });
        }

        // Steps 7–10.
        match self.open_session(&run, &req.input).await {
            Ok(Some(live)) => {
                let session_id = run.session_id();
                tokio::spawn(run_task(self.clone(), run, inbox, TaskStart::Live(live)));
                Ok(RunHandle {
                    run_id,
                    adapter: AdapterKind::CopilotLocal,
                    runtime_session_id: session_id,
                    remote_thread_id: None,
                })
            }
            // The run ended on a run event; its entry stays for `events` and `read`.
            Ok(None) => Ok(RunHandle {
                run_id,
                adapter: AdapterKind::CopilotLocal,
                runtime_session_id: run.session_id().or(resume_id),
                remote_thread_id: None,
            }),
            Err(failure) => {
                lock(&self.runs).remove(&run_id);
                self.close_session(&run).await;
                Err(failure)
            }
        }
    }

    /// Step 6.2: the CLI's own `copilot login` (09 ADR-044). A flow that cannot start leaves the
    /// person to sign in from the Copilot CLI itself (the ADR-044 fallback); the poll still runs.
    pub(crate) async fn start_login(&self, run: &Run, server: &dyn CopilotServer) {
        match server.start_login().await {
            Ok(()) => run_log!(info, run, "copilot.sign_in.started"),
            Err(_) => run_log!(warn, run, "copilot.sign_in.started", started = false),
        }
    }

    /// The probe calls' server: started from the current CLI if none runs, or if the CLI changed.
    async fn probe_server(&self) -> Result<Arc<dyn CopilotServer>, CopilotAdapterError> {
        // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-4
        let Some(cli) = self
            .cli
            .current()
            .await
            .filter(|found| found.path.is_absolute())
        else {
            return Err(probe_failed("no_cli"));
        };
        let mut probe = self.probe.lock().await;
        if let Some((started_from, server)) = probe.as_ref() {
            if *started_from == cli {
                return Ok(server.clone());
            }
        }
        if let Some((_, old)) = probe.take() {
            old.stop(self.config.stop_deadline).await;
        }
        let spec = CliServerSpec {
            cli_path: cli.path.clone(),
            working_folder: self.config.app_data_root.clone(),
            env: server_environment(&self.host.parent_env, &cli.path),
        };
        let server: Arc<dyn CopilotServer> = Arc::from(
            self.port
                .start_server(spec)
                .await
                .map_err(|_| probe_failed("server_start"))?,
        );
        tracing::info!(
            target: "ordin_copilot_adapter",
            cli_version = cli.version.as_str(),
            "copilot.server.started"
        );
        *probe = Some((cli, server.clone()));
        Ok(server)
    }
}

/// Step 1: the 2C checks of `req`; the agent instance of the target.
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_TARGET_MISMATCH
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_INPUT_INVALID
fn check_request(req: &StartRun) -> Result<AgentInstanceId, Failure> {
    check_start(AdapterKind::CopilotLocal, req).map_err(|error| {
        let code = if error.code == "RA_TARGET_MISMATCH" {
            "CPA_TARGET_MISMATCH"
        } else {
            "CPA_INPUT_INVALID"
        };
        Failure::invalid(code, error.code)
    })?;
    match req.target {
        RunTarget::AgentInstance { agent_instance_id } => Ok(agent_instance_id),
        RunTarget::RemoteAgent { .. } => Err(Failure::invalid(
            "CPA_TARGET_MISMATCH",
            "RA_TARGET_MISMATCH",
        )),
    }
}

fn log_failure(run_id: RunId, failure: &Failure) {
    tracing::warn!(
        target: "ordin_copilot_adapter",
        run_id = %run_id,
        code = failure.code,
        retryable = failure.retryable,
        "copilot.call.failed"
    );
}

/// `CPA_PROBE_FAILED` for a sign-in probe call that failed at `stage`. The port's reason is not
/// logged: the CLI layer has already logged its kind, and the reason's text may hold a path.
fn probe_failed(stage: &'static str) -> CopilotAdapterError {
    tracing::warn!(
        target: "ordin_copilot_adapter",
        stage = stage,
        code = "CPA_PROBE_FAILED",
        "copilot.sign_in.probe_failed"
    );
    CopilotAdapterError::probe_failed()
}

fn unknown_run() -> Failure {
    Failure::new("CPA_UNKNOWN_RUN", false, AdapterError::UnknownRun)
}

fn mismatch() -> Failure {
    Failure::invalid("CPA_RESUME_MISMATCH", "RA_RESUME_MISMATCH")
}

/// The turn sent after a tool approval (*Internal Logic*, `resume`).
fn decision_turn(decision: &ResumeDecision) -> Option<String> {
    match decision {
        ResumeDecision::Approve { .. } => Some(
            "The person approved the action you asked for. Make the same call again now."
                .to_owned(),
        ),
        ResumeDecision::Reject { reason, .. } => Some(format!(
            "The person did not approve the action. Reason: {}. Continue without it.",
            reason.as_deref().unwrap_or("none given")
        )),
        ResumeDecision::Answer { .. } => None,
    }
}

pub struct CopilotLocalAdapter {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for CopilotLocalAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopilotLocalAdapter")
            .finish_non_exhaustive()
    }
}

/// The checks of `new`.
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_PROFILE_NOT_OFFERED
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SERVER_SIGN_IN_UNCONFIRMED
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-5
fn check_config(config: &CopilotAdapterConfig) -> Result<(), CopilotAdapterError> {
    match config.profile {
        ProgramProfile::AgentProfile => return Err(CopilotAdapterError::ProfileNotOffered),
        ProgramProfile::PersonalServer if !C48_SERVER_SIGN_IN_CONFIRMED => {
            return Err(CopilotAdapterError::ServerSignInUnconfirmed)
        }
        _ => {}
    }
    if !config.app_data_root.is_absolute() {
        return Err(CopilotAdapterError::ConfigInvalid("app_data_root"));
    }
    Ok(())
}

impl CopilotLocalAdapter {
    /// Refuses `ProgramProfile::AgentProfile` with CPA_PROFILE_NOT_OFFERED, and `PersonalServer`
    /// with CPA_SERVER_SIGN_IN_UNCONFIRMED while [`C48_SERVER_SIGN_IN_CONFIRMED`] is false.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: CopilotAdapterConfig,
        port: Arc<dyn CopilotPort>,
        cli: Arc<dyn CopilotCliSource>,
        mcp: Arc<McpFront>,
        hooks: Arc<HookRelay>,
        rules: Arc<RulesEngine>,
        identity: Arc<IdentityService>,
        setup: Arc<dyn SessionSetupSource>,
    ) -> Result<Self, CopilotAdapterError> {
        Self::build(
            config,
            Services {
                port,
                cli,
                mcp,
                hooks,
                rules,
                identity,
                setup,
            },
            Host::real(),
        )
    }

    pub(crate) fn build(
        config: CopilotAdapterConfig,
        services: Services,
        host: Host,
    ) -> Result<Self, CopilotAdapterError> {
        check_config(&config)?;
        Ok(CopilotLocalAdapter {
            inner: Arc::new(Inner {
                config,
                port: services.port,
                cli: services.cli,
                mcp: services.mcp,
                hooks: services.hooks,
                rules: services.rules,
                identity: services.identity,
                setup: services.setup,
                host,
                runs: Mutex::new(HashMap::new()),
                starting: Mutex::new(HashSet::new()),
                probe: tokio::sync::Mutex::new(None),
                login: Mutex::new(None),
            }),
        })
    }

    /// Stops every live run within `stop_deadline` + 1 s; runs still going end `Interrupted`.
    pub async fn shutdown(&self) {
        let runs: Vec<Arc<Run>> = lock(&self.inner.runs).values().cloned().collect();
        let mut waits = Vec::new();
        for run in runs {
            if run.is_ended() {
                continue;
            }
            let (ack, done) = oneshot::channel();
            if run
                .commands
                .send(RunCommand::Stop {
                    reason: RunEndReason::Interrupted,
                    ack,
                })
                .is_ok()
            {
                waits.push(done);
            }
        }
        let limit = self.inner.config.stop_deadline + Duration::from_secs(1);
        let waiting = waits.len() as u64;
        if tokio::time::timeout(limit, futures::future::join_all(waits))
            .await
            .is_err()
        {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                stage = "runs",
                runs = waiting,
                limit_ms = limit.as_millis() as u64,
                "copilot.shutdown.timed_out"
            );
        }
        if let Some((_, server)) = self.inner.probe.lock().await.take() {
            if tokio::time::timeout(limit, server.stop(self.inner.config.stop_deadline))
                .await
                .is_err()
            {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    stage = "probe",
                    limit_ms = limit.as_millis() as u64,
                    "copilot.shutdown.timed_out"
                );
            }
        }
    }

    /// `CopilotServer::auth_status`, with the CLI server started if it is not running.
    ///
    /// CPA-CPX-6: the CLI server reads the sign-in when it starts, so a `copilot login` finished in its
    /// own process is not seen by a server already running. While a sign-in started here is awaited and
    /// the answer is still signed out, the server is started again, at most every [`LOGIN_RECHECK`] and
    /// for [`LOGIN_RECHECK_FOR`] after the sign-in started, and asked once more.
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-6
    pub async fn probe_sign_in(&self) -> Result<CopilotAuthStatus, CopilotAdapterError> {
        let server = self.inner.probe_server().await?;
        let mut status = server
            .auth_status()
            .await
            .map_err(|_| probe_failed("status"))?;
        if !status.signed_in && self.recheck_due() {
            self.probe_repair().await?;
            let server = self.inner.probe_server().await?;
            status = server
                .auth_status()
                .await
                .map_err(|_| probe_failed("recheck"))?;
        }
        if status.signed_in {
            *lock(&self.inner.login) = None;
        }
        Ok(status)
    }

    /// True, and the restart time noted, when a started sign-in is due a fresh server (CPA-CPX-6).
    fn recheck_due(&self) -> bool {
        let now = tokio::time::Instant::now();
        let mut login = lock(&self.inner.login);
        match *login {
            Some((started, _)) if now.duration_since(started) > LOGIN_RECHECK_FOR => {
                *login = None;
                false
            }
            Some((started, last)) if now.duration_since(last) >= LOGIN_RECHECK => {
                *login = Some((started, now));
                true
            }
            _ => false,
        }
    }

    /// `start_login` on the probe's CLI server; the sign-in is then awaited as CPA-CPX-6 states.
    pub async fn probe_start_sign_in(&self) -> Result<(), CopilotAdapterError> {
        let server = self.inner.probe_server().await?;
        server
            .start_login()
            .await
            .map_err(|_| probe_failed("start_login"))?;
        let now = tokio::time::Instant::now();
        *lock(&self.inner.login) = Some((now, now));
        Ok(())
    }

    /// Stops and restarts the probe's CLI server.
    pub async fn probe_repair(&self) -> Result<(), CopilotAdapterError> {
        if let Some((_, server)) = self.inner.probe.lock().await.take() {
            server.stop(self.inner.config.stop_deadline).await;
        }
        self.inner.probe_server().await.map(|_| ())
    }

    /// The `HarnessModels` the program gives the Usage Planner (RuntimeAdapterInterface.md, O63).
    pub fn harness_models(&self) -> Arc<dyn HarnessModels> {
        Arc::new(CopilotModels)
    }

    /// `start`, keeping the internal code and retryability of a failure.
    pub(crate) async fn start_checked(
        &self,
        ctx: &AdapterContext,
        req: StartRun,
    ) -> Result<RunHandle, Failure> {
        let inner = &self.inner;
        let run_id = req.run_id;
        let held = inner.run(run_id).is_some_and(|run| !run.is_ended());
        if held || !lock(&inner.starting).insert(run_id) {
            return Err(Failure::invalid("CPA_SESSION_EXISTS", "RA_HANDLE_INVALID"));
        }
        let deadline = tokio::time::Instant::from_std(ctx.deadline);
        let outcome = tokio::time::timeout_at(deadline, inner.start_steps(ctx, req)).await;
        lock(&inner.starting).remove(&run_id);
        match outcome {
            Ok(result) => result,
            Err(_) => {
                // Step 11: the server is stopped and the MCP and relay sessions closed.
                // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_DEADLINE
                let run = lock(&inner.runs).remove(&run_id);
                if let Some(run) = run {
                    inner.close_session(&run).await;
                }
                Err(Failure::new("CPA_DEADLINE", true, AdapterError::Deadline))
            }
        }
    }

    /// `resume`, keeping the internal code of a failure.
    pub(crate) async fn resume_checked(
        &self,
        handle: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), Failure> {
        let run = self.inner.run(handle.run_id).ok_or_else(unknown_run)?;
        // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_NOT_PAUSED
        let Some(paused_on) = run.paused_on() else {
            return Err(Failure::invalid("CPA_NOT_PAUSED", "RA_RESUME_MISMATCH"));
        };
        match paused_on {
            PauseReason::SignIn => match decision {
                ResumeDecision::Answer { text } => {
                    if !(1..=MAX_ANSWER).contains(&text.chars().count()) {
                        return Err(Failure::invalid("CPA_INPUT_INVALID", "RA_INPUT_TOO_LARGE"));
                    }
                    let _ = run.commands.send(RunCommand::RestartSignIn);
                    Ok(())
                }
                _ => Err(mismatch()),
            },
            PauseReason::Approval { .. } => {
                // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_RESUME_MISMATCH
                check_resume(&decision, &paused_on).map_err(|error| {
                    if error.code == "RA_RESUME_MISMATCH" {
                        mismatch()
                    } else {
                        Failure::invalid("CPA_INPUT_INVALID", error.code)
                    }
                })?;
                let turn = decision_turn(&decision).ok_or_else(mismatch)?;
                let (ack, done) = oneshot::channel();
                run.commands
                    .send(RunCommand::Resume { turn, ack })
                    .map_err(|_| unknown_run())?;
                done.await.map_err(|_| unknown_run())?
            }
            _ => Err(mismatch()),
        }
    }
}

/// The services `build` takes: the public types in production, doubles in tests.
pub(crate) struct Services {
    pub port: Arc<dyn CopilotPort>,
    pub cli: Arc<dyn CopilotCliSource>,
    pub mcp: Arc<dyn McpPort>,
    pub hooks: Arc<dyn HookPort>,
    pub rules: Arc<dyn RulesPort>,
    pub identity: Arc<dyn IdentityPort>,
    pub setup: Arc<dyn SessionSetupSource>,
}

#[async_trait::async_trait]
impl RuntimeAdapter for CopilotLocalAdapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::CopilotLocal
    }

    /// 2C: `copilot_local` offers every verb.
    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        &[]
    }

    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError> {
        let run_id = req.run_id;
        self.start_checked(ctx, req).await.map_err(|failure| {
            log_failure(run_id, &failure);
            failure.error
        })
    }

    fn events(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError> {
        match self.inner.run(run.run_id) {
            Some(run) => Ok(run.subscribe()),
            None => {
                log_failure(run.run_id, &unknown_run());
                Err(AdapterError::UnknownRun)
            }
        }
    }

    async fn resume(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), AdapterError> {
        self.resume_checked(run, decision).await.map_err(|failure| {
            log_failure(run.run_id, &failure);
            failure.error
        })
    }

    async fn read(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<RunSnapshot, AdapterError> {
        match self.inner.run(run.run_id) {
            Some(run) => Ok(run.snapshot()),
            None => {
                // After a restart the Session Supervisor calls `start` with `resume_session_id`
                // (C-05) or marks the run interrupted.
                tracing::info!(
                    target: "ordin_copilot_adapter",
                    run_id = %run.run_id,
                    code = "CPA_UNKNOWN_RUN",
                    "copilot.read.unknown_run"
                );
                Err(AdapterError::UnknownRun)
            }
        }
    }

    async fn cancel(&self, _ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError> {
        let Some(run) = self.inner.run(run.run_id) else {
            log_failure(run.run_id, &unknown_run());
            return Err(AdapterError::UnknownRun);
        };
        if run.is_ended() {
            return Ok(());
        }
        let (ack, done) = oneshot::channel();
        if run
            .commands
            .send(RunCommand::Stop {
                reason: RunEndReason::Cancelled,
                ack,
            })
            .is_ok()
        {
            let _ = done.await;
        }
        Ok(())
    }
}
