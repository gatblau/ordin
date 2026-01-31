//! The Hook Relay service object: construction, sessions, start and shutdown (HookRelay.md A, B, F.1, I).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use futures::stream::{BoxStream, StreamExt};
use ordin_approval::ApprovalService;
use ordin_journal::{Datastore, EventSink, Journal, ModuleMigrations};
use ordin_rules::RulesEngine;
use ordin_session_supervisor::SessionSupervisor;
use ordin_types::encoding::b64url_encode;
use ordin_types::time::Timestamp;
use ordin_types::wire::hook::{HookMessage, HookReply};
use ordin_types::RunId;
use tokio::sync::{broadcast, Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::consts::{Timings, LATE_MAX_AGE_MS, MAX_CONNECTIONS, PROTOCOL};
use crate::error::HookRelayError;
use crate::listen;
use crate::ports::{ApprovalPort, Ports, RulesPort, SupervisorPort};
use crate::session::Session;
use crate::store;
use crate::types::{HookRelayConfig, HookSessionEnv, HookSessionEvent, HookSessionSpec};
use crate::writes::AgentWrites;

pub struct HookRelay {
    pub(crate) me: Weak<HookRelay>,
    pub(crate) datastore: Datastore,
    pub(crate) journal: Arc<Journal>,
    pub(crate) ports: Ports,
    pub(crate) config: HookRelayConfig,
    pub(crate) endpoint: String,
    pub(crate) timings: Timings,
    pub(crate) sessions: Mutex<HashMap<RunId, Arc<Session>>>,
    /// Stops the accept loop and the late watchers.
    pub(crate) shutdown_signal: CancellationToken,
    /// Denies the connections still open when the shutdown grace ends.
    pub(crate) force_signal: CancellationToken,
    pub(crate) slots: Arc<Semaphore>,
    pub(crate) active: AtomicUsize,
    pub(crate) idle: Notify,
    /// Approvals whose late decision is being settled now (G): a watcher and the Journal sink may
    /// both see one decision, and only one of them settles it.
    pub(crate) settling: Mutex<HashSet<ordin_types::ApprovalId>>,
    pub(crate) started: AtomicBool,
    pub(crate) socket: Mutex<Option<std::path::PathBuf>>,
    pub(crate) accept_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl HookRelay {
    pub const MODULE: &'static str = "hook_relay";

    /// Module `hook_relay`: one table, `hook_late_decisions`, on SQLite and on PostgreSQL.
    // spec: ordin-desktop/components/HookRelay#SA-RECON-04
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: &[(1, "hook relay", include_str!("sql/sqlite_v1.sql"))],
            postgres: &[(1, "hook relay", include_str!("sql/postgres_v1.sql"))],
        }
    }

    /// Called by: the desktop and server programs at start.
    pub fn new(
        datastore: Datastore,
        journal: Arc<Journal>,
        rules: Arc<RulesEngine>,
        approvals: Arc<ApprovalService>,
        supervisor: Arc<SessionSupervisor>,
        writes: Arc<dyn AgentWrites>,
        config: HookRelayConfig,
    ) -> Arc<Self> {
        let ports = Ports {
            rules: rules as Arc<dyn RulesPort>,
            approvals: approvals as Arc<dyn ApprovalPort>,
            supervisor: supervisor as Arc<dyn SupervisorPort>,
            writes,
        };
        Self::build(datastore, journal, ports, config, Timings::SPEC)
    }

    pub(crate) fn build(
        datastore: Datastore,
        journal: Arc<Journal>,
        ports: Ports,
        config: HookRelayConfig,
        timings: Timings,
    ) -> Arc<Self> {
        let endpoint = listen::endpoint_of(&config);
        Arc::new_cyclic(|me| HookRelay {
            me: me.clone(),
            datastore,
            journal,
            ports,
            config,
            endpoint,
            timings,
            sessions: Mutex::new(HashMap::new()),
            shutdown_signal: CancellationToken::new(),
            force_signal: CancellationToken::new(),
            slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            active: AtomicUsize::new(0),
            idle: Notify::new(),
            settling: Mutex::new(HashSet::new()),
            started: AtomicBool::new(false),
            socket: Mutex::new(None),
            accept_task: Mutex::new(None),
        })
    }

    pub(crate) fn arc(&self) -> Option<Arc<HookRelay>> {
        self.me.upgrade()
    }

    /// False for a relay wired with [`crate::NoPlaces`]. A program with places refuses to start on
    /// false, because every agent write would then count as the person's.
    // spec: ordin-desktop/components/HookRelay#SA-RECON-05
    pub fn attributes_writes(&self) -> bool {
        self.ports.writes.attributes_writes()
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Opens the local channel and starts accepting. Called by: the desktop and server programs,
    /// once at start, before any session (there is no `listen`).
    pub async fn start(self: &Arc<Self>) -> Result<(), HookRelayError> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Err(HookRelayError::Listen(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "the relay is already started",
            )));
        }
        match self.start_inner().await {
            Ok(()) => Ok(()),
            Err(error) => {
                self.started.store(false, Ordering::SeqCst);
                Err(error)
            }
        }
    }

    async fn start_inner(self: &Arc<Self>) -> Result<(), HookRelayError> {
        // A.1: the path limit is checked before anything is created.
        listen::check_endpoint_length(&self.config)?;
        // A.3: rows older than 30 days go, the rest are reloaded.
        let cutoff = Timestamp::from_unix_millis(Timestamp::now().unix_millis() - LATE_MAX_AGE_MS)
            .unwrap_or_else(|_| Timestamp::now());
        store::delete_older_than(&self.datastore, cutoff).await?;
        let rows = store::list(&self.datastore).await?;
        // A.1, A.2: bind the channel.
        let bound = listen::bind(&self.config)?;
        *lock(&self.socket) = bound.socket_path();
        for row in rows {
            self.spawn_late_watcher(row.approval_id);
        }
        // A.4: the accept loop.
        let task = tokio::spawn(listen::accept_loop(self.clone(), bound));
        *lock(&self.accept_task) = Some(task);
        Ok(())
    }

    /// Synchronous. Called by: Claude Local Adapter (before starting the harness process; it places
    /// `HookSessionEnv::env_vars()` in the child's environment) and Copilot Local Adapter (before
    /// starting a session; it passes nothing to the CLI, the session's state serves `decide_in_process`).
    ///
    /// B: a session open for the run → `HOOK_SESSION_EXISTS` (a resumed session closes the old one
    /// first); 32 bytes from the operating system, base64url without padding.
    pub fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError> {
        let run_id = spec.run_id;
        let harness = spec.harness;
        let mut sessions = lock(&self.sessions);
        if sessions.contains_key(&run_id) {
            return Err(HookRelayError::SessionExists);
        }
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|_| HookRelayError::Random)?;
        let text = b64url_encode(&secret);
        sessions.insert(run_id, Arc::new(Session::new(spec, secret)));
        drop(sessions);
        hook_log!(
            info,
            run_id = run_id.to_string().as_str(),
            harness = harness_text(harness),
            "hook_session_opened"
        );
        Ok(HookSessionEnv {
            endpoint: self.endpoint.clone(),
            secret: text,
            session_id: run_id,
            protocol: PROTOCOL,
        })
    }

    /// Synchronous. Called by: both local adapters, when the harness session has ended or paused
    /// for good. I: drops the secret, the replay set and the tool-use map; connections already
    /// being served finish with the state they hold.
    pub fn close_session(&self, run_id: RunId) {
        let removed = lock(&self.sessions).remove(&run_id);
        if let Some(session) = removed {
            session.close();
            hook_log!(
                info,
                run_id = run_id.to_string().as_str(),
                "hook_session_closed"
            );
            self.end_writes(run_id);
        }
    }

    /// The File Service closes the run's claims (FileService.md Internal Logic 6a). Outside a runtime,
    /// or on a failure, they stay open and the File Service settles them at its next start.
    fn end_writes(&self, run_id: RunId) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let writes = self.ports.writes.clone();
        runtime.spawn(async move {
            if writes.run_ended(run_id).await.is_err() {
                hook_log!(
                    warn,
                    run_id = run_id.to_string().as_str(),
                    "hook_write_claims_left_open"
                );
            }
        });
    }

    pub(crate) fn session(&self, run_id: RunId) -> Option<Arc<Session>> {
        lock(&self.sessions).get(&run_id).cloned()
    }

    /// The session's hook events (F.1). Called by: both local adapters, right after `open_session`.
    /// An unknown run gives a stream that ends at once. Lagged events are dropped and logged.
    pub fn session_events(&self, run_id: RunId) -> BoxStream<'static, HookSessionEvent> {
        let Some(receiver) = self.session(run_id).and_then(|s| s.subscribe()) else {
            return futures::stream::empty().boxed();
        };
        futures::stream::unfold(receiver, move |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => return Some((event, receiver)),
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        hook_log!(
                            warn,
                            run_id = run_id.to_string().as_str(),
                            dropped,
                            "hook_events_lagged"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
        .boxed()
    }

    /// The decision path of one hook call, in process: no socket, no MAC, no replay set (F.2).
    /// Returns the final reply (never `Holding`; holding is reported through `session_events`).
    /// Called by: Copilot Local Adapter, for each `PreToolUse`, `PostToolUse` and the end of a turn
    /// (`Stop`) the Copilot SDK reports.
    pub async fn decide_in_process(&self, run_id: RunId, message: HookMessage) -> HookReply {
        self.in_process(run_id, message).await
    }

    /// The Journal subscription that resumes sessions after a late decision across a restart.
    /// Called by: the desktop and server programs (`journal.subscribe(hook_relay.event_sink())`).
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink> {
        Arc::new(crate::late::LateSink(Arc::downgrade(self)))
    }

    /// Stops accepting and removes the socket file. Called by: the desktop and server programs on
    /// quit (there is no `close`). I: waits up to 2 s for open connections; each one still open is
    /// then denied.
    pub async fn shutdown(&self) {
        self.shutdown_signal.cancel();
        let task = lock(&self.accept_task).take();
        if let Some(task) = task {
            // The accept loop ends at the signal; its listener is dropped with it.
            let _ = task.await;
        }
        let idle = async {
            loop {
                let notified = self.idle.notified();
                if self.active.load(Ordering::SeqCst) == 0 {
                    break;
                }
                notified.await;
            }
        };
        if tokio::time::timeout(self.timings.shutdown_grace, idle)
            .await
            .is_err()
        {
            self.force_signal.cancel();
            // The forced connections write their deny and finish; a short wait lets them.
            let settle = async {
                loop {
                    let notified = self.idle.notified();
                    if self.active.load(Ordering::SeqCst) == 0 {
                        break;
                    }
                    notified.await;
                }
            };
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), settle).await;
        }
        if let Some(path) = lock(&self.socket).take() {
            let _ = std::fs::remove_file(path);
        }
        self.started.store(false, Ordering::SeqCst);
    }
}

pub(crate) fn harness_text(harness: ordin_types::wire::hook::HarnessKind) -> &'static str {
    match harness {
        ordin_types::wire::hook::HarnessKind::ClaudeCode => "claude_code",
        ordin_types::wire::hook::HarnessKind::CopilotCli => "copilot_cli",
    }
}

/// Counts one open connection until it is dropped; wakes `shutdown` when the last one is gone.
pub(crate) struct ActiveGuard(pub Arc<HookRelay>);

impl ActiveGuard {
    pub(crate) fn new(relay: &Arc<HookRelay>) -> ActiveGuard {
        relay.active.fetch_add(1, Ordering::SeqCst);
        ActiveGuard(relay.clone())
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        if self.0.active.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}
