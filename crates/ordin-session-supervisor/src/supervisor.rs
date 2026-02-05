//! `SessionSupervisor`: the scheduler, the public calls, recovery, wake, idle stop and shutdown
//! (SessionSupervisor.md *Internal Logic* A, B, D.4, F, G, I, J). One task per run lives in
//! [`crate::run`]; the agent profile's workers in [`crate::agent`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use ordin_journal::{Datastore, EventSink, Journal, ModuleMigrations, PruneParticipant};
use ordin_run_queue::RunQueue;
use ordin_runtime_adapter::{
    check_resume, check_verb, AdapterContext, AdapterError, AdapterKind, AdapterRegistry,
    AdapterVerb, PauseReason, ResumeDecision, RunEnd, RunEvent, RunHandle, RunLimits, RunTarget,
    RuntimeAdapter, StartInput, ToolCaller,
};
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{
    Run, RunOrigin, RunPauseReason, RunState, Task, TaskTarget, UsageCounters,
};
use ordin_types::paging::{Cursor, Page, PAGE_SIZE};
use ordin_types::time::Timestamp;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{CrewRunId, RunId, ScopeId, WorkspaceId};
use ordin_usage_planner::{LevelChoice, UsagePlanner, WorkKind};
use tokio::sync::{broadcast, mpsc, watch, Notify};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::agent::QueueRun;
use crate::error::SupervisorError;
use crate::store::{self, enum_text, RunRow};
use crate::types::{
    ConversationStart, MetricsSink, RecoveryReport, RunView, ShutdownReason, ShutdownReport,
    StartRequest, SupervisorConfig, SupervisorHost,
};
use crate::validate::{self, is_ended, is_local};

/// The adapter call deadline (`AdapterContext.deadline`) for `start`, `resume`, `read`, `cancel`.
pub(crate) const ADAPTER_DEADLINE: Duration = Duration::from_secs(30);
/// Broadcast buffer per supervised run (*Performance*: 256 events).
const BROADCAST_EVENTS: usize = 256;
/// Most start-up reports kept for `get`.
const STARTUP_KEPT: usize = 1_000;
/// Longest conversation seed (2C `StartInput` limit).
const MAX_SEED: usize = 16_384;
/// Extra wait past the stop deadline for a run task to record its end.
const RECORD_MARGIN: Duration = Duration::from_secs(5);

/// A supervised run as `on_wake` reads it: id, adapter, handle, token.
type Supervised = (
    RunId,
    Arc<dyn RuntimeAdapter>,
    Option<RunHandle>,
    CancellationToken,
);

/// What the program asks of a run task from outside.
#[derive(Debug)]
pub(crate) enum Control {
    /// D.4: the person's Stop, a crew kill switch, a crew stop.
    Cancel,
    /// D.4 agent profile: `renew` answered `CancelRequested`.
    QueueCancel,
    /// I, J.2, J.3: stop and record `interrupted`, with an optional end reason.
    Interrupt { reason: Option<String> },
    /// J.6: `run_resuming` named a control.
    DutyLock(String),
    /// H.1: the lease was lost.
    LeaseLost,
    /// J.2: a snapshot read after a sleep: `Some(end)` ended, `None` the run is unknown.
    Snapshot(Option<RunEnd>),
    /// J.2: the run is still going after a sleep of this long.
    Woke(Duration),
}

/// How a run task begins.
pub(crate) enum LaunchMode {
    /// C.2: `adapter.start`.
    Start,
    /// G.2, G.3, H.4: an existing harness session, optionally resumed with a decision first.
    Reattach {
        handle: RunHandle,
        resume: Option<ResumeDecision>,
    },
}

/// Everything a run task needs to start or re-attach its run.
pub(crate) struct Launch {
    pub row: RunRow,
    pub kind: AdapterKind,
    pub adapter: Arc<dyn RuntimeAdapter>,
    pub target: RunTarget,
    pub input: StartInput,
    pub attachments: Vec<ordin_types::ArtefactId>,
    pub thread: Option<String>,
    pub coding_task_id: Option<ordin_types::CodingTaskId>,
    /// `StartRun.only_servers` (SUP-SRV-1); `None` for a re-attached run, whose MCP session is
    /// the one it already has.
    pub only_servers: Option<Vec<String>>,
    /// `StartRun.started_by_person` (SA-SUP-INPUT-01).
    pub started_by_person: bool,
    pub scope_id: Option<ScopeId>,
    pub caller: ToolCaller,
    pub choice: LevelChoice,
    pub crew_run_id: Option<CrewRunId>,
    pub round: Option<u32>,
    pub mode: LaunchMode,
    /// Wall clock left for the run; less than the limit after a pause or a restart.
    pub remaining: Duration,
    pub queue: Option<QueueRun>,
}

/// The in-memory view of a run this supervisor knows (queued, running or paused).
pub(crate) struct Live {
    pub state: RunState,
    pub kind: AdapterKind,
    pub adapter: Arc<dyn RuntimeAdapter>,
    pub handle: Option<RunHandle>,
    pub paused_on: Option<PauseReason>,
    pub paused_at: Option<Instant>,
    pub token: CancellationToken,
    /// One channel per subscriber (SA-SUP-PAUSE-01), so a late subscriber's channel can start
    /// with the current pause and no other subscriber receives it twice.
    pub subscribers: Vec<broadcast::Sender<RunEvent>>,
    /// The pause last broadcast, until `Resumed`, `Started` or `Ended` is broadcast.
    pub pause_sent: Option<PauseReason>,
    /// Present while a run task supervises the run.
    pub control: Option<mpsc::UnboundedSender<Control>>,
    /// Becomes true when the run's task has finished.
    pub done: watch::Sender<bool>,
    pub holds_slot: bool,
    /// A queued run waiting in the FIFO for a local session.
    pub pending: Option<Box<Launch>>,
}

#[derive(Default)]
pub(crate) struct State {
    pub live: HashMap<RunId, Live>,
    pub fifo: VecDeque<RunId>,
    pub running_local: u32,
    /// Agent profile: records executing on this instance (H.5).
    pub executing: u32,
    /// J.5: records the adapter saved, kept until H.3 hands them to the Run Queue.
    pub session_records: HashMap<RunId, (String, Vec<u8>)>,
    /// J.5: the record of the `TakenRun` this instance took for a run.
    pub taken_records: HashMap<RunId, Vec<u8>>,
    /// H.4 (C-80 fallback): runs this instance paused.
    pub paused_here: HashSet<RunId>,
    /// C.3 (C-29): the latest context-window use per live run.
    pub context_used: HashMap<RunId, u8>,
    /// E: the items of each run's start-up report, newest last.
    pub startup: VecDeque<(RunId, Vec<StartUpReportItem>)>,
}

pub struct SessionSupervisor {
    pub(crate) me: Weak<SessionSupervisor>,
    pub(crate) datastore: Datastore,
    pub(crate) journal: Arc<Journal>,
    pub(crate) usage: Arc<UsagePlanner>,
    pub(crate) run_queue: Option<Arc<dyn RunQueue>>,
    pub(crate) host: Arc<dyn SupervisorHost>,
    pub(crate) config: SupervisorConfig,
    registry: RwLock<AdapterRegistry>,
    pub(crate) state: Mutex<State>,
    stopping: AtomicBool,
    pub(crate) not_taking: AtomicBool,
    /// Held (shared) by a worker from its `not_taking` check to the end of its `take`;
    /// `stop_taking_runs` takes it exclusively, so no take is in flight once it returns (H.5).
    pub(crate) taking: tokio::sync::RwLock<()>,
    pub(crate) metrics: RwLock<Option<Arc<dyn MetricsSink>>>,
    /// Signalled whenever a run ends or a record stops executing.
    pub(crate) changed: Notify,
}

/// The fields a new run is built from, beyond the `StartRequest` (agent profile).
#[derive(Default)]
pub(crate) struct Extra {
    pub caller: Option<ToolCaller>,
    pub scope_id: Option<ScopeId>,
    pub queue: Option<QueueRun>,
    /// Agent profile: the record's run id, kept for the `runs` row (H.1).
    pub run_id: Option<RunId>,
}

pub(crate) fn adapter_ctx(correlation: &str, parent: &CancellationToken) -> AdapterContext {
    AdapterContext {
        correlation_id: correlation.to_owned(),
        deadline: std::time::Instant::now() + ADAPTER_DEADLINE,
        cancel: parent.child_token(),
    }
}

/// After a wake, a running run's state could not be read; the run is left to its own stream.
pub(crate) fn wake_read_failed(run_id: &str, code: &str) {
    tracing::warn!(run_id, code, "sup_wake_read_failed");
}

/// An adapter call bounded by [`ADAPTER_DEADLINE`]; an overrun is `AdapterError::Deadline`.
pub(crate) async fn bounded<T>(
    call: impl std::future::Future<Output = Result<T, AdapterError>>,
) -> Result<T, AdapterError> {
    tokio::time::timeout(ADAPTER_DEADLINE, call)
        .await
        .unwrap_or(Err(AdapterError::Deadline))
}

impl SessionSupervisor {
    pub const MODULE: &'static str = "session_supervisor";

    /// Module `session_supervisor`: SQLite creates `tasks` and `runs`; PostgreSQL (scaled shape)
    /// creates `runs` only, since the agent profile has no tasks.
    // spec: ordin-desktop/components/SessionSupervisor#SA-RECON-03
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: &[(1, "session supervisor", include_str!("sql/sqlite_v1.sql"))],
            postgres: &[(1, "session supervisor", include_str!("sql/postgres_v1.sql"))],
        }
    }

    pub fn new(
        datastore: Datastore,
        journal: Arc<Journal>,
        usage: Arc<UsagePlanner>,
        run_queue: Option<Arc<dyn RunQueue>>,
        host: Arc<dyn SupervisorHost>,
        config: SupervisorConfig,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| SessionSupervisor {
            me: me.clone(),
            datastore,
            journal,
            usage,
            run_queue,
            host,
            config,
            registry: RwLock::new(AdapterRegistry::new()),
            state: Mutex::new(State::default()),
            stopping: AtomicBool::new(false),
            not_taking: AtomicBool::new(false),
            taking: tokio::sync::RwLock::new(()),
            metrics: RwLock::new(None),
            changed: Notify::new(),
        })
    }

    /// The counter sink of the agent profile (Metrics families 5, 16, 17, 31).
    // The program sets it here after `new`; without one no counter is written (the desktop and
    // personal server have no metrics).
    pub fn set_metrics_sink(&self, sink: Arc<dyn MetricsSink>) {
        if let Ok(mut slot) = self.metrics.write() {
            *slot = Some(sink);
        }
    }

    pub(crate) fn count(&self, name: &str, labels: &[(&str, &str)]) {
        let sink = self.metrics.read().ok().and_then(|s| s.clone());
        if let Some(sink) = sink {
            sink.add(name, labels, 1);
        }
    }

    pub(crate) fn arc(&self) -> Option<Arc<SessionSupervisor>> {
        self.me.upgrade()
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// One adapter per kind, before `recover`; a second of one kind → `RA_ADAPTER_DUPLICATE`.
    pub fn register_adapter(
        &self,
        adapter: Arc<dyn RuntimeAdapter>,
    ) -> Result<(), SupervisorError> {
        let mut registry = self
            .registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry
            .register(adapter)
            .map_err(|_| SupervisorError::StartInvalid {
                code: "RA_ADAPTER_DUPLICATE",
            })
    }

    pub(crate) fn adapter(
        &self,
        kind: AdapterKind,
    ) -> Result<Arc<dyn RuntimeAdapter>, SupervisorError> {
        self.registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(kind)
            .map_err(|_| SupervisorError::NoAdapter(kind))
    }

    /// The Journal subscription (approval decisions resume agent-profile runs).
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink> {
        Arc::new(crate::hooks::SupervisorSink(Arc::downgrade(self)))
    }

    /// The Journal's pruning hook for `runs` and `tasks`.
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant> {
        Arc::new(crate::hooks::SupervisorPrune)
    }

    // ---- A. Tasks

    /// Records a task (immutable) and starts its run. `TaskTarget::Crew` is refused.
    pub async fn submit_task(
        &self,
        task: Task,
        work_kind: WorkKind,
        started_by_person: bool,
        coding_task_id: Option<ordin_types::CodingTaskId>,
    ) -> Result<RunId, SupervisorError> {
        validate::check_task(&task).map_err(|code| SupervisorError::TaskInvalid { code })?;
        let (target, target_key) = match task.target {
            TaskTarget::AgentInstance { agent_instance_id } => (
                RunTarget::AgentInstance { agent_instance_id },
                agent_instance_id.to_string(),
            ),
            TaskTarget::RemoteAgent { remote_agent_id } => (
                RunTarget::RemoteAgent { remote_agent_id },
                remote_agent_id.to_string(),
            ),
            TaskTarget::Crew { .. } => {
                return Err(SupervisorError::StartInvalid {
                    code: "SUP_TARGET_IS_CREW",
                })
            }
        };
        if self.stopping.load(Ordering::SeqCst) {
            return Err(SupervisorError::Stopping);
        }
        store::insert_task(&self.datastore, &task).await?;
        let input = task_input(&task);
        let work_key = format!(
            "task:{target_key}:{}",
            task.command.as_deref().unwrap_or("free_text")
        );
        self.start_run(StartRequest {
            origin: RunOrigin::Task { task_id: task.id },
            workspace_id: task.workspace_id,
            target,
            input,
            attachments: task.attached_artefacts.clone(),
            limits: None,
            work_kind,
            work_key,
            inputs_used: vec![],
            thread: None,
            started_by_person,
            model_override: None,
            coding_task_id,
            only_servers: None,
        })
        .await
    }

    // ---- B. Runs

    /// Starts a run (tasks and crew steps). Returns once the run is recorded as queued.
    pub async fn start_run(&self, req: StartRequest) -> Result<RunId, SupervisorError> {
        let (row, launch) = self.prepare(req, Extra::default()).await?;
        let run_id = row.run.id;
        self.admit(launch);
        Ok(run_id)
    }

    /// B.1–B.4: the adapter, the limits, the level and caller, and the `queued` row.
    pub(crate) async fn prepare(
        &self,
        req: StartRequest,
        extra: Extra,
    ) -> Result<(RunRow, Launch), SupervisorError> {
        if self.stopping.load(Ordering::SeqCst) {
            return Err(SupervisorError::Stopping);
        }
        // B.1
        let (kind, agent) = match req.target {
            RunTarget::AgentInstance { agent_instance_id } => (
                self.config.local_adapter,
                AgentRef::AgentInstance { agent_instance_id },
            ),
            RunTarget::RemoteAgent { remote_agent_id } => {
                let kind = self.host.remote_route(remote_agent_id).ok_or(
                    SupervisorError::StartInvalid {
                        code: "RA_TARGET_MISMATCH",
                    },
                )?;
                (kind, AgentRef::RemoteAgent { remote_agent_id })
            }
        };
        let adapter = self.adapter(kind)?;
        // B.2
        let limits = req.limits.unwrap_or(RunLimits {
            wall_clock_secs: self.config.default_wall_clock_secs,
            request_budget: None,
            idle_timeout_secs: self.config.idle_timeout_secs,
        });
        validate::check_limits(&limits).map_err(|code| SupervisorError::StartInvalid { code })?;
        // B.3
        let choice = req
            .model_override
            .clone()
            .unwrap_or_else(|| self.usage.choose(req.work_kind, kind));
        // spec: ordin-desktop/components/SessionSupervisor#SA-CLOSE-02
        let caller = extra.caller.unwrap_or(match req.origin {
            RunOrigin::Task { .. } => ToolCaller::Agent,
            RunOrigin::CrewRound { crew_run_id, .. } => ToolCaller::CrewMember { crew_run_id },
            RunOrigin::Conversation { .. } => ToolCaller::Ordin,
        });
        let (crew_run_id, round) = match req.origin {
            RunOrigin::CrewRound {
                crew_run_id,
                round_number,
            } => (Some(crew_run_id), Some(round_number)),
            _ => (None, None),
        };
        // Only a task the person started, outside the agent profile, has the person's own input.
        // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-INPUT-01
        let started_by_person = req.started_by_person
            && matches!(req.origin, RunOrigin::Task { .. })
            && !self.config.program.is_agent();
        // B.4
        // A task command may be 1–100 characters (2C Task) but `runs.command` holds
        // 1–64 (the `StartInput` name limit). A longer name is not kept in the column; the run
        // then fails `check_start` with `RA_COMMAND_UNKNOWN`.
        let command = match &req.input {
            StartInput::Command { name, .. }
                if (1..=validate::MAX_RUN_COMMAND).contains(&name.chars().count()) =>
            {
                Some(name.clone())
            }
            _ => None,
        };
        let run = Run {
            id: extra.run_id.unwrap_or_default(),
            origin: req.origin,
            agent,
            adapter: kind,
            runtime_session_id: None,
            remote_run_id: None,
            remote_thread_id: None,
            state: RunState::Queued,
            pause_reason: None,
            limits,
            usage: UsageCounters {
                model_turns: 0,
                tool_calls: 0,
                model_level: Some(enum_text(&choice.level)),
                tokens: None,
                tokens_from_cache: None,
            },
            started_at: None,
            ended_at: None,
            end_reason: None,
            inputs_used: req.inputs_used,
            queued_at: Timestamp::now(),
        };
        validate::check_run(&run).map_err(|code| SupervisorError::StartInvalid { code })?;
        let row = RunRow {
            run,
            workspace_id: req.workspace_id,
            pause_approval_id: None,
            work_key: req.work_key,
            command,
        };
        store::insert_run(&self.datastore, &row).await?;
        self.emit_state(row.run.id, RunState::Queued).await?;
        tracing::info!(
            run_id = row.run.id.to_string().as_str(),
            adapter = enum_text(&kind).as_str(),
            "sup_run_queued"
        );
        let launch = Launch {
            row: row.clone(),
            kind,
            adapter,
            target: req.target,
            input: req.input,
            attachments: req.attachments,
            thread: req.thread,
            coding_task_id: req.coding_task_id,
            only_servers: req.only_servers,
            started_by_person,
            scope_id: extra.scope_id,
            caller,
            choice,
            crew_run_id,
            round,
            mode: LaunchMode::Start,
            remaining: Duration::from_secs(u64::from(limits.wall_clock_secs)),
            queue: extra.queue,
        };
        Ok((row, launch))
    }

    /// B.5: launch now, or wait in the FIFO for a local session.
    pub(crate) fn admit(&self, launch: Launch) {
        let run_id = launch.row.run.id;
        let local = is_local(launch.kind);
        let mut state = self.lock();
        let (done, _) = watch::channel(false);
        let live = Live {
            state: RunState::Queued,
            kind: launch.kind,
            adapter: launch.adapter.clone(),
            handle: None,
            paused_on: None,
            paused_at: None,
            token: CancellationToken::new(),
            subscribers: Vec::new(),
            pause_sent: None,
            control: None,
            done,
            holds_slot: false,
            pending: None,
        };
        state.live.insert(run_id, live);
        // B.5: a local run waits in the FIFO while every local session is taken.
        if local && launch.queue.is_none() && state.running_local >= self.config.local_session_limit
        {
            state.fifo.push_back(run_id);
            let position = state.fifo.len();
            if let Some(live) = state.live.get_mut(&run_id) {
                live.pending = Some(Box::new(launch));
            }
            drop(state);
            tracing::info!(
                run_id = run_id.to_string().as_str(),
                position = position as u64,
                "sup_run_waiting"
            );
            return;
        }
        self.spawn_locked(&mut state, launch);
    }

    /// Starts the task of a run whose `Live` entry exists; takes a local slot.
    pub(crate) fn spawn_locked(&self, state: &mut State, launch: Launch) {
        let Some(me) = self.arc() else {
            return;
        };
        let run_id = launch.row.run.id;
        let (tx, rx) = mpsc::unbounded_channel();
        let local = is_local(launch.kind);
        if let Some(live) = state.live.get_mut(&run_id) {
            live.control = Some(tx);
            live.pending = None;
            if local && !live.holds_slot {
                live.holds_slot = true;
                state.running_local += 1;
            }
        }
        tokio::spawn(crate::run::run_task(me, launch, rx));
    }

    /// Frees a run's local slot and starts the FIFO's head while slots remain.
    pub(crate) fn release_slot(&self, run_id: RunId) {
        let mut state = self.lock();
        if let Some(live) = state.live.get_mut(&run_id) {
            if live.holds_slot {
                live.holds_slot = false;
                state.running_local = state.running_local.saturating_sub(1);
            }
        }
        self.launch_waiting(&mut state);
    }

    fn launch_waiting(&self, state: &mut State) {
        while state.running_local < self.config.local_session_limit {
            let Some(next) = state.fifo.pop_front() else {
                return;
            };
            let launch = state.live.get_mut(&next).and_then(|l| l.pending.take());
            if let Some(launch) = launch {
                self.spawn_locked(state, *launch);
            }
        }
    }

    /// Removes a finished run from memory and wakes whoever waits on it.
    pub(crate) fn forget(&self, run_id: RunId) {
        let mut state = self.lock();
        if let Some(live) = state.live.remove(&run_id) {
            if live.holds_slot {
                state.running_local = state.running_local.saturating_sub(1);
            }
            let _ = live.done.send(true);
        }
        state.context_used.remove(&run_id);
        state.session_records.remove(&run_id);
        state.taken_records.remove(&run_id);
        state.paused_here.remove(&run_id);
        self.launch_waiting(&mut state);
        drop(state);
        self.changed.notify_waiters();
    }

    pub(crate) fn keep_startup(&self, run_id: RunId, items: Vec<StartUpReportItem>) {
        let mut state = self.lock();
        state.startup.retain(|(id, _)| *id != run_id);
        if state.startup.len() >= STARTUP_KEPT {
            state.startup.pop_front();
        }
        state.startup.push_back((run_id, items));
    }

    /// `run_state_changed` for a run (B.4 and every change after it).
    pub(crate) async fn emit_state(
        &self,
        run_id: RunId,
        state: RunState,
    ) -> Result<(), SupervisorError> {
        self.journal
            .append_event(
                ordin_types::wire::events::EventKind::RunStateChanged.as_str(),
                serde_json::json!({ "run_id": run_id, "state": state }),
            )
            .await?;
        Ok(())
    }

    /// Follow a run: every `RunEvent` the adapter sends, after the supervisor has recorded it, and
    /// first the run's current pause when one is current. Made under the state lock, as every
    /// broadcast is, so the pause arrives once: as this replay or live, never both.
    // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-PAUSE-01
    pub fn subscribe(
        &self,
        run_id: RunId,
    ) -> Result<broadcast::Receiver<RunEvent>, SupervisorError> {
        let mut state = self.lock();
        let live = state
            .live
            .get_mut(&run_id)
            .ok_or(SupervisorError::RunNotFound)?;
        let (sender, receiver) = broadcast::channel(BROADCAST_EVENTS);
        if let Some(reason) = live.pause_sent.clone() {
            let _ = sender.send(RunEvent::Paused { reason });
        }
        live.subscribers.push(sender);
        Ok(receiver)
    }

    /// The state a run's task finished with, waited for at most `within`.
    async fn wait_done(&self, run_id: RunId, within: Duration) {
        let receiver = self.lock().live.get(&run_id).map(|l| l.done.subscribe());
        if let Some(mut receiver) = receiver {
            let _ = tokio::time::timeout(within, receiver.wait_for(|done| *done)).await;
        }
    }

    /// Sends a control to a run's task; false when no task supervises the run.
    pub(crate) fn send(&self, run_id: RunId, control: Control) -> bool {
        self.lock()
            .live
            .get(&run_id)
            .and_then(|l| l.control.clone())
            .is_some_and(|tx| tx.send(control).is_ok())
    }

    // ---- D.4 Cancel

    /// Person's Stop, crew kill switch, crew stop.
    pub async fn cancel(&self, run_id: RunId) -> Result<(), SupervisorError> {
        enum Found {
            Queued,
            Supervised,
        }
        let found = {
            let mut state = self.lock();
            match state.live.get_mut(&run_id) {
                Some(live) if live.pending.is_some() => {
                    live.pending = None;
                    state.fifo.retain(|id| *id != run_id);
                    Some(Found::Queued)
                }
                Some(live) if live.control.is_some() => Some(Found::Supervised),
                _ => None,
            }
        };
        match found {
            Some(Found::Queued) => {
                let row = store::get_run(&self.datastore, run_id)
                    .await?
                    .ok_or(SupervisorError::RunNotFound)?;
                self.record_end_outside(row, RunState::Cancelled, None)
                    .await?;
                Ok(())
            }
            Some(Found::Supervised) => {
                if self.send(run_id, Control::Cancel) {
                    self.wait_done(run_id, self.config.stop_deadline + RECORD_MARGIN)
                        .await;
                }
                Ok(())
            }
            None => {
                let row = store::get_run(&self.datastore, run_id)
                    .await?
                    .ok_or(SupervisorError::RunNotFound)?;
                if is_ended(row.run.state) {
                    return Err(SupervisorError::RunEnded);
                }
                // A paused run with no task (after `recover`, or an agent-profile run paused
                // for an approval): nothing runs, so it is recorded cancelled at once.
                self.record_end_outside(row, RunState::Cancelled, None)
                    .await?;
                Ok(())
            }
        }
    }

    /// Records an ended state for a run no task supervises, then forgets it.
    pub(crate) async fn record_end_outside(
        &self,
        mut row: RunRow,
        state: RunState,
        end_reason: Option<String>,
    ) -> Result<(), SupervisorError> {
        row.run.state = state;
        row.run.pause_reason = None;
        row.pause_approval_id = None;
        row.run.ended_at = Some(Timestamp::now());
        row.run.end_reason = end_reason;
        store::save_run(&self.datastore, &row).await?;
        self.emit_state(row.run.id, state).await?;
        self.ended_log(&row, None);
        self.host.run_ended(row.run.id).await;
        self.forget(row.run.id);
        Ok(())
    }

    /// `sup_run_ended`, with the ending's code when it has one (a fixed string, never an argument or
    /// content), so a refused start can be told apart in the log.
    // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-END-1
    pub(crate) fn ended_log(&self, row: &RunRow, code: Option<&str>) {
        let elapsed = row
            .run
            .started_at
            .map(|s| (Timestamp::now().unix_millis() - s.unix_millis()).max(0) / 1_000)
            .unwrap_or(0);
        tracing::info!(
            run_id = row.run.id.to_string().as_str(),
            state = enum_text(&row.run.state).as_str(),
            elapsed_seconds = elapsed,
            code,
            "sup_run_ended"
        );
    }

    // ---- F. Resume

    /// A decision for a paused run, a late hook decision, or a member's answer.
    pub async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ResumeDecision,
    ) -> Result<(), SupervisorError> {
        let live = {
            let state = self.lock();
            state.live.get(&run_id).map(|l| {
                (
                    l.adapter.clone(),
                    l.handle.clone(),
                    l.paused_on.clone(),
                    l.control.is_some(),
                    l.token.clone(),
                )
            })
        };
        let Some((adapter, handle, paused_on, supervised, token)) = live else {
            let row = store::get_run(&self.datastore, run_id)
                .await?
                .ok_or(SupervisorError::RunNotFound)?;
            if is_ended(row.run.state) {
                return Err(SupervisorError::RunEnded);
            }
            return Err(SupervisorError::RunNotPaused);
        };
        check_verb(adapter.as_ref(), AdapterVerb::Resume)
            .map_err(|_| SupervisorError::VerbNotOffered(AdapterVerb::Resume))?;
        let paused_on = paused_on.ok_or(SupervisorError::RunNotPaused)?;
        check_resume(&decision, &paused_on).map_err(|_| SupervisorError::RunNotPaused)?;
        // Agent profile (H.4): the Run Queue queues the record again; a worker resumes it.
        if let Some(queue) = &self.run_queue {
            if self.config.program.is_agent() && !supervised {
                let value = match &decision {
                    ResumeDecision::Approve { approval_id } => {
                        serde_json::json!({ "approval_id": approval_id, "state": "approved" })
                    }
                    ResumeDecision::Reject { approval_id, .. } => {
                        serde_json::json!({ "approval_id": approval_id, "state": "rejected" })
                    }
                    ResumeDecision::Answer { .. } => return Err(SupervisorError::RunNotPaused),
                };
                queue
                    .resume(&run_id.to_string(), value)
                    .await
                    .map_err(|_| SupervisorError::RunNotPaused)?;
                return Ok(());
            }
        }
        // J.6: the duty lock may refuse the resume.
        if let Some(control) = self.host.run_resuming(run_id).await {
            if supervised {
                self.send(run_id, Control::DutyLock(control));
                self.wait_done(run_id, self.config.stop_deadline + RECORD_MARGIN)
                    .await;
            } else if let Some(row) = store::get_run(&self.datastore, run_id).await? {
                crate::run::journal_code(self, run_id, "SUP_RESUME_REFUSED").await;
                self.record_end_outside(
                    row,
                    RunState::Completed,
                    Some(format!("ended by the control {control}")),
                )
                .await?;
            }
            return Ok(());
        }
        let handle = handle.ok_or(SupervisorError::RunNotPaused)?;
        let ctx = adapter_ctx(&run_id.to_string(), &token);
        bounded(adapter.resume(&ctx, &handle, decision))
            .await
            .map_err(SupervisorError::Adapter)?;
        if !supervised {
            // A paused run recovered after a restart: supervise its events from now on.
            if let Some(row) = store::get_run(&self.datastore, run_id).await? {
                if let Some(launch) = self.reattach_launch(row, handle, None) {
                    let mut state = self.lock();
                    self.spawn_locked(&mut state, launch);
                }
            }
        }
        Ok(())
    }

    // ---- Reads

    pub async fn get(&self, run_id: RunId) -> Result<RunView, SupervisorError> {
        let row = store::get_run(&self.datastore, run_id)
            .await?
            .ok_or(SupervisorError::RunNotFound)?;
        // The Journal's 2C `StartupReport` keeps each line as flattened text (no
        // `RuleRef`, `ToolOrigin` or reason), so `StartUpReportItem`s cannot be rebuilt from it.
        // The report is stored in the Journal (E) and the items are also kept in memory for the
        // most recent 1,000 runs; a run whose report is no longer in memory shows `None`.
        let startup = self
            .lock()
            .startup
            .iter()
            .find(|(id, _)| *id == run_id)
            .map(|(_, items)| items.clone());
        let task = match &row.run.origin {
            RunOrigin::Task { task_id } => store::task(&self.datastore, *task_id).await?,
            _ => None,
        };
        Ok(RunView {
            run: row.run,
            startup,
            workspace_id: row.workspace_id,
            task,
        })
    }

    /// Runs newest first by `(queued_at, id)`, 100 a page.
    // The cursor holds the pair `(queued_at DESC, id DESC)`.
    pub async fn list(
        &self,
        workspace_id: Option<WorkspaceId>,
        cursor: Option<String>,
    ) -> Result<Page<Run>, SupervisorError> {
        let after = match cursor {
            Some(text) => {
                let key: (String, String) =
                    serde_json::from_value::<Cursor>(serde_json::Value::String(text))
                        .map_err(|_| SupervisorError::StartInvalid {
                            code: "SRV_BAD_CURSOR",
                        })?
                        .decode()
                        .map_err(|_| SupervisorError::StartInvalid {
                            code: "SRV_BAD_CURSOR",
                        })?;
                Some(key)
            }
            None => None,
        };
        let mut rows = store::list(&self.datastore, workspace_id, after, PAGE_SIZE + 1).await?;
        let next_cursor = if rows.len() > PAGE_SIZE {
            rows.truncate(PAGE_SIZE);
            rows.last().map(|last| {
                Cursor::encode(&(last.run.queued_at.to_wire(), last.run.id.to_string()))
            })
        } else {
            None
        };
        Ok(Page {
            items: rows.into_iter().map(|r| r.run).collect(),
            next_cursor,
        })
    }

    /// The run exists and is queued, running or paused.
    pub fn is_live(&self, run_id: RunId) -> bool {
        self.lock().live.contains_key(&run_id)
    }

    /// The latest context-window use (0–100) the harness reported for a live run (SA-CLOSE-01).
    // spec: ordin-desktop/components/SessionSupervisor#SA-CLOSE-01
    // C-29 fallback: until 2C adds `context_used_percent` to `RunEvent::Usage`, no adapter reports
    // it and nothing fills the map, so the answer is `None`.
    pub fn context_used_percent(&self, run_id: RunId) -> Option<u8> {
        self.lock().context_used.get(&run_id).copied()
    }

    /// Local sessions running now (not queued).
    pub fn running_local_sessions(&self) -> u32 {
        self.lock().running_local
    }

    /// Local plus remote runs queued, running or paused.
    pub fn runs_going(&self) -> u32 {
        u32::try_from(self.lock().live.len()).unwrap_or(u32::MAX)
    }

    // ---- J.1 Conversation

    /// Ordin's conversation run (J.1).
    pub async fn start_conversation(
        &self,
        req: ConversationStart,
    ) -> Result<RunId, SupervisorError> {
        if req.seed.is_empty() {
            return Err(SupervisorError::StartInvalid {
                code: "SUP_SEED_EMPTY",
            });
        }
        if req.seed.chars().count() > MAX_SEED {
            return Err(SupervisorError::StartInvalid {
                code: "RA_INPUT_TOO_LARGE",
            });
        }
        if !matches!(req.target, RunTarget::AgentInstance { .. }) {
            return Err(SupervisorError::StartInvalid {
                code: "RA_TARGET_MISMATCH",
            });
        }
        self.start_run(StartRequest {
            origin: RunOrigin::Conversation {
                workspace_id: req.workspace_id,
            },
            workspace_id: req.workspace_id,
            target: req.target,
            input: StartInput::FreeText { text: req.seed },
            attachments: vec![],
            limits: Some(req.limits),
            work_kind: WorkKind::Talking,
            work_key: format!("conversation:{}", req.workspace_id),
            inputs_used: vec![],
            thread: None,
            started_by_person: true,
            model_override: req.model_override,
            coding_task_id: None,
            // spec: ordin-desktop/components/SessionSupervisor#SUP-SRV-1
            only_servers: req.only_servers,
        })
        .await
    }

    // ---- G. Recovery

    /// On application start, before any new run: *Recovery after restart*.
    pub async fn recover(&self) -> Result<RecoveryReport, SupervisorError> {
        let rows = store::in_progress(&self.datastore).await?;
        let mut report = RecoveryReport::default();
        for row in rows {
            let run_id = row.run.id;
            let kind = row.run.adapter;
            let Ok(adapter) = self.adapter(kind) else {
                self.interrupt_row(row).await?;
                report.interrupted += 1;
                continue;
            };
            match row.run.state {
                RunState::Queued if is_local(kind) => {
                    match self.rebuild_queued(row.clone(), adapter).await? {
                        Some(launch) => {
                            self.admit(launch);
                            report.resumed += 1;
                        }
                        None => {
                            self.interrupt_row(row).await?;
                            report.interrupted += 1;
                        }
                    }
                }
                // G.4: a run waiting for an approval stays paused; the approval survives.
                RunState::Paused if row.run.pause_reason == Some(RunPauseReason::Approval) => {
                    self.keep_paused(&row, adapter);
                    report.awaiting_decision += 1;
                }
                _ => {
                    let handle = handle_of(&row);
                    let Some(handle) = handle else {
                        self.interrupt_row(row).await?;
                        report.interrupted += 1;
                        continue;
                    };
                    let token = CancellationToken::new();
                    let ctx = adapter_ctx(&run_id.to_string(), &token);
                    let snapshot = bounded(adapter.read(&ctx, &handle))
                        .await
                        .ok()
                        .filter(|s| ordin_runtime_adapter::check_snapshot(s).is_ok());
                    match snapshot {
                        Some(snapshot) if snapshot.end.is_some() => {
                            let end = snapshot.end.unwrap_or(RunEnd {
                                reason: ordin_runtime_adapter::RunEndReason::Interrupted,
                                final_message: None,
                                artefacts: vec![],
                            });
                            self.apply_snapshot_end(row, end).await?;
                            report.ended_remotely += 1;
                        }
                        Some(snapshot)
                            if snapshot.state == RunState::Running
                                || (snapshot.state == RunState::Paused && !is_local(kind)) =>
                        {
                            // C-05 fallback: a session the adapter could re-attach is
                            // supervised again; anything else is interrupted below.
                            // spec: ordin-desktop/components/SessionSupervisor#C-05
                            match self.reattach_launch(row.clone(), handle, None) {
                                Some(launch) => {
                                    self.admit_reattached(launch);
                                    report.resumed += 1;
                                }
                                None => {
                                    self.interrupt_row(row).await?;
                                    report.interrupted += 1;
                                }
                            }
                        }
                        _ => {
                            self.interrupt_row(row).await?;
                            report.interrupted += 1;
                        }
                    }
                }
            }
        }
        let kept = self
            .journal
            .append_entry(ordin_journal::NewJournalEntry {
                kind: "sup_recovered".into(),
                component: Self::MODULE.into(),
                run_id: None,
                crew_run_id: None,
                workspace_id: None,
                summary: "Runs in progress at start were reconciled.".into(),
                data: serde_json::to_value(&report).unwrap_or_default(),
            })
            .await;
        if let Err(e) = kept {
            crate::run::journal_not_kept(None, "sup_recovered", ordin_types::CodedError::code(&e));
        }
        tracing::info!(
            resumed = report.resumed,
            interrupted = report.interrupted,
            ended_remotely = report.ended_remotely,
            awaiting_decision = report.awaiting_decision,
            "sup_recovered"
        );
        Ok(report)
    }

    /// A queued local run is rebuilt from its task.
    async fn rebuild_queued(
        &self,
        row: RunRow,
        adapter: Arc<dyn RuntimeAdapter>,
    ) -> Result<Option<Launch>, SupervisorError> {
        // `runs` keeps no start input. A queued run is rebuilt from its task row; a
        // queued crew-round or conversation run, whose input only its caller holds, is recorded
        // interrupted instead, and its caller starts it again.
        let RunOrigin::Task { task_id } = row.run.origin else {
            return Ok(None);
        };
        let Some(task) = store::task(&self.datastore, task_id).await? else {
            return Ok(None);
        };
        let target = match row.run.agent {
            AgentRef::AgentInstance { agent_instance_id } => {
                RunTarget::AgentInstance { agent_instance_id }
            }
            AgentRef::RemoteAgent { remote_agent_id } => RunTarget::RemoteAgent { remote_agent_id },
        };
        let level = row
            .run
            .usage
            .model_level
            .as_deref()
            .and_then(|l| serde_json::from_value(serde_json::Value::String(l.into())).ok());
        let choice = match level {
            Some(level) => {
                let chosen = self.usage.choose(WorkKind::Routine, row.run.adapter);
                LevelChoice { level, ..chosen }
            }
            None => self.usage.choose(WorkKind::Routine, row.run.adapter),
        };
        Ok(Some(Launch {
            kind: row.run.adapter,
            adapter,
            target,
            input: task_input(&task),
            attachments: task.attached_artefacts.clone(),
            thread: None,
            coding_task_id: None,
            only_servers: None,
            // The task row keeps no `started_by_person` (SA-SUP-INPUT-01).
            started_by_person: false,
            scope_id: None,
            caller: ToolCaller::Agent,
            choice,
            crew_run_id: None,
            round: None,
            mode: LaunchMode::Start,
            remaining: Duration::from_secs(u64::from(row.run.limits.wall_clock_secs)),
            queue: None,
            row,
        }))
    }

    /// A launch that re-attaches an existing session (G.2, G.3, F after a restart).
    pub(crate) fn reattach_launch(
        &self,
        row: RunRow,
        handle: RunHandle,
        resume: Option<ResumeDecision>,
    ) -> Option<Launch> {
        let adapter = self.adapter(row.run.adapter).ok()?;
        let target = match row.run.agent {
            AgentRef::AgentInstance { agent_instance_id } => {
                RunTarget::AgentInstance { agent_instance_id }
            }
            AgentRef::RemoteAgent { remote_agent_id } => RunTarget::RemoteAgent { remote_agent_id },
        };
        let caller = match row.run.origin {
            RunOrigin::Task { .. } => ToolCaller::Agent,
            RunOrigin::CrewRound { crew_run_id, .. } => ToolCaller::CrewMember { crew_run_id },
            RunOrigin::Conversation { .. } => ToolCaller::Ordin,
        };
        let (crew_run_id, round) = match row.run.origin {
            RunOrigin::CrewRound {
                crew_run_id,
                round_number,
            } => (Some(crew_run_id), Some(round_number)),
            _ => (None, None),
        };
        let level = row
            .run
            .usage
            .model_level
            .as_deref()
            .and_then(|l| serde_json::from_value(serde_json::Value::String(l.into())).ok())
            .unwrap_or(ordin_runtime_adapter::ModelLevel::Automatic);
        let used = row
            .run
            .started_at
            .map(|s| (Timestamp::now().unix_millis() - s.unix_millis()).max(0) as u64)
            .unwrap_or(0);
        let limit = u64::from(row.run.limits.wall_clock_secs) * 1_000;
        Some(Launch {
            kind: row.run.adapter,
            adapter,
            target,
            input: StartInput::FreeText {
                text: String::new(),
            },
            attachments: vec![],
            thread: None,
            coding_task_id: None,
            only_servers: None,
            // A re-attached run sends no first input (SA-SUP-INPUT-01).
            started_by_person: false,
            scope_id: None,
            caller,
            choice: LevelChoice {
                level,
                model: None,
                thinking: ordin_runtime_adapter::ThinkingLevel::Automatic,
                effort: None,
                stepped_up: false,
            },
            crew_run_id,
            round,
            mode: LaunchMode::Reattach { handle, resume },
            remaining: Duration::from_millis(limit.saturating_sub(used).max(1_000)),
            queue: None,
            row,
        })
    }

    /// Registers a re-attached run and starts its task at once (no FIFO: it already ran).
    pub(crate) fn admit_reattached(&self, launch: Launch) {
        let run_id = launch.row.run.id;
        let mut state = self.lock();
        let (done, _) = watch::channel(false);
        state.live.insert(
            run_id,
            Live {
                state: launch.row.run.state,
                kind: launch.kind,
                adapter: launch.adapter.clone(),
                handle: match &launch.mode {
                    LaunchMode::Reattach { handle, .. } => Some(handle.clone()),
                    LaunchMode::Start => None,
                },
                paused_on: None,
                paused_at: None,
                token: CancellationToken::new(),
                subscribers: Vec::new(),
                pause_sent: None,
                control: None,
                done,
                holds_slot: false,
                pending: None,
            },
        );
        self.spawn_locked(&mut state, launch);
    }

    /// G.4: a paused run waiting for an approval is known again, without a task, until a
    /// decision resumes it.
    fn keep_paused(&self, row: &RunRow, adapter: Arc<dyn RuntimeAdapter>) {
        let (done, _) = watch::channel(false);
        let paused_on = row
            .pause_approval_id
            .map(|approval_id| PauseReason::Approval { approval_id });
        self.lock().live.insert(
            row.run.id,
            Live {
                state: RunState::Paused,
                kind: row.run.adapter,
                adapter,
                handle: handle_of(row),
                paused_on,
                paused_at: Some(Instant::now()),
                token: CancellationToken::new(),
                subscribers: Vec::new(),
                pause_sent: None,
                control: None,
                done,
                holds_slot: false,
                pending: None,
            },
        );
    }

    async fn interrupt_row(&self, row: RunRow) -> Result<(), SupervisorError> {
        self.record_end_outside(row, RunState::Interrupted, None)
            .await
    }

    /// An ended snapshot read at recovery, applied as `Ended`.
    async fn apply_snapshot_end(
        &self,
        mut row: RunRow,
        end: RunEnd,
    ) -> Result<(), SupervisorError> {
        let state = ordin_runtime_adapter::end_state(end.reason);
        row.run.end_reason = end
            .final_message
            .as_deref()
            .map(|m| validate::cut(m, validate::MAX_END_REASON));
        let reason = row.run.end_reason.clone();
        crate::run::journal_result(self, row.run.id, &end).await;
        self.record_end_outside(row, state, reason).await
    }

    // ---- J.2 Wake

    /// The computer woke after `slept_for` (J.2).
    pub async fn on_wake(&self, slept_for: Duration) {
        let runs: Vec<Supervised> = {
            let state = self.lock();
            state
                .live
                .iter()
                .filter(|(_, l)| l.control.is_some() && l.state == RunState::Running)
                .map(|(id, l)| (*id, l.adapter.clone(), l.handle.clone(), l.token.clone()))
                .collect()
        };
        let mut interrupted = 0u64;
        for (run_id, adapter, handle, token) in runs {
            let Some(handle) = handle else {
                continue;
            };
            let ctx = adapter_ctx(&run_id.to_string(), &token);
            match bounded(adapter.read(&ctx, &handle)).await {
                Ok(snapshot) if snapshot.end.is_some() => {
                    self.send(run_id, Control::Snapshot(snapshot.end));
                }
                Ok(snapshot) if !is_ended(snapshot.state) => {
                    self.send(run_id, Control::Woke(slept_for));
                }
                Err(AdapterError::UnknownRun) | Ok(_) => {
                    if self.send(run_id, Control::Snapshot(None)) {
                        interrupted += 1;
                        self.wait_done(run_id, self.config.stop_deadline + RECORD_MARGIN)
                            .await;
                    }
                }
                Err(e) => wake_read_failed(
                    run_id.to_string().as_str(),
                    ordin_types::CodedError::code(&e),
                ),
            }
        }
        tracing::info!(slept_seconds = slept_for.as_secs(), interrupted, "sup_woke");
    }

    // ---- J.3 Idle sessions (personal server)

    /// Stops local runs paused for the person longer than `idle_for`; returns how many.
    pub async fn stop_idle_sessions(&self, idle_for: Duration) -> u32 {
        if self.config.program != crate::types::ProgramKind::PersonalServer {
            return 0;
        }
        let now = Instant::now();
        let idle: Vec<RunId> = {
            let state = self.lock();
            state
                .live
                .iter()
                .filter(|(_, l)| {
                    is_local(l.kind)
                        && l.state == RunState::Paused
                        && matches!(l.paused_on, Some(PauseReason::Person { .. }))
                        && l.paused_at
                            .is_some_and(|at| now.duration_since(at) > idle_for)
                })
                .map(|(id, _)| *id)
                .collect()
        };
        let reason = format!("stopped after {} idle minutes", idle_for.as_secs() / 60);
        let mut stopped = 0u32;
        for run_id in idle {
            if self.send(
                run_id,
                Control::Interrupt {
                    reason: Some(reason.clone()),
                },
            ) {
                stopped += 1;
                self.wait_done(run_id, self.config.stop_deadline + RECORD_MARGIN)
                    .await;
            }
        }
        stopped
    }

    // ---- J.4 Command facts

    pub async fn runs_with_command(&self, command: &str) -> Result<u32, SupervisorError> {
        Ok(store::count_with_command(&self.datastore, command).await?)
    }

    pub async fn paused_with_command(
        &self,
        commands: &[String],
        except: RunId,
    ) -> Result<bool, SupervisorError> {
        Ok(store::paused_with_command(&self.datastore, commands, except).await?)
    }

    /// Some(true) ended, Some(false) not ended, None unknown.
    pub async fn has_ended(&self, run_id: RunId) -> Result<Option<bool>, SupervisorError> {
        Ok(store::get_run(&self.datastore, run_id)
            .await?
            .map(|row| is_ended(row.run.state)))
    }

    // ---- J.5 Session records

    /// Holds the harness's session record in memory until H.3 hands it to the Run Queue (C-80).
    // spec: ordin-desktop/components/SessionSupervisor#C-80
    pub async fn save_session_record(
        &self,
        run_id: RunId,
        runtime_session_id: &str,
        record: Vec<u8>,
    ) -> Result<(), SupervisorError> {
        let mut state = self.lock();
        let ok = state
            .live
            .get(&run_id)
            .is_some_and(|l| matches!(l.state, RunState::Running | RunState::Paused));
        if !ok {
            return Err(SupervisorError::RunNotFound);
        }
        state
            .session_records
            .insert(run_id, (runtime_session_id.to_owned(), record));
        Ok(())
    }

    /// The record of the `TakenRun` this instance took for the run, when the session matches.
    pub async fn load_session_record(
        &self,
        run_id: RunId,
        runtime_session_id: &str,
    ) -> Result<Option<Vec<u8>>, SupervisorError> {
        let session = {
            let state = self.lock();
            if let Some((saved_for, bytes)) = state.session_records.get(&run_id) {
                return Ok((saved_for == runtime_session_id).then(|| bytes.clone()));
            }
            state.taken_records.get(&run_id).cloned()
        };
        let Some(bytes) = session else {
            return Ok(None);
        };
        let row = store::get_run(&self.datastore, run_id).await?;
        Ok(row
            .filter(|r| r.run.runtime_session_id.as_deref() == Some(runtime_session_id))
            .map(|_| bytes))
    }

    // ---- I. Shutdown and H.5 stop

    /// Quit, SIGTERM, sleep (desktop): stop every running local run within the stop deadline.
    // spec: ordin-desktop/components/SessionSupervisor#SA-CLOSE-03
    pub async fn shutdown(&self, reason: ShutdownReason) -> ShutdownReport {
        self.stopping.store(true, Ordering::SeqCst);
        let runs: Vec<RunId> = {
            let state = self.lock();
            state
                .live
                .iter()
                .filter(|(_, l)| {
                    is_local(l.kind)
                        && l.control.is_some()
                        && matches!(l.state, RunState::Running | RunState::Paused)
                })
                .map(|(id, _)| *id)
                .collect()
        };
        let mut report = ShutdownReport::default();
        let mut receivers = Vec::new();
        for run_id in &runs {
            let receiver = self.lock().live.get(run_id).map(|l| l.done.subscribe());
            if self.send(*run_id, Control::Interrupt { reason: None }) {
                report.stopped += 1;
                if let Some(receiver) = receiver {
                    receivers.push((*run_id, receiver));
                }
            }
        }
        let wait = self.config.stop_deadline + RECORD_MARGIN;
        for (_, mut receiver) in receivers {
            let _ = tokio::time::timeout(wait, receiver.wait_for(|done| *done)).await;
        }
        for run_id in runs {
            if let Ok(Some(row)) = store::get_run(&self.datastore, run_id).await {
                if row.run.state == RunState::Interrupted {
                    report.interrupted += 1;
                }
            }
        }
        tracing::info!(
            reason = format!("{reason:?}").to_lowercase().as_str(),
            stopped = report.stopped,
            interrupted = report.interrupted,
            "sup_shutdown"
        );
        report
    }

    /// Agent profile: workers take no new record (H.5).
    pub async fn stop_taking_runs(&self) {
        self.not_taking.store(true, Ordering::SeqCst);
        // Wait for every take already in flight; none starts after this.
        drop(self.taking.write().await);
        self.changed.notify_waiters();
    }

    /// Agent profile: returns when no run is executing or `stop` is cancelled (H.5).
    pub async fn wait_for_runs(&self, stop: CancellationToken) {
        loop {
            let notified = self.changed.notified();
            if self.lock().executing == 0 {
                return;
            }
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = notified => {}
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
    }

    /// Agent profile: start N workers taking runs from the Run Queue.
    pub fn start_queue_workers(self: &Arc<Self>) {
        if self.run_queue.is_none() {
            return;
        }
        for _ in 0..self.config.local_session_limit {
            tokio::spawn(crate::agent::worker(self.clone()));
        }
    }
}

/// The `RunHandle` of a recorded run (local: harness session; remote: remote run).
pub(crate) fn handle_of(row: &RunRow) -> Option<RunHandle> {
    let session = if is_local(row.run.adapter) {
        row.run.runtime_session_id.clone()
    } else {
        row.run.remote_run_id.clone()
    };
    session.as_ref()?;
    Some(RunHandle {
        run_id: row.run.id,
        adapter: row.run.adapter,
        runtime_session_id: session,
        remote_thread_id: row.run.remote_thread_id.clone(),
    })
}

/// SUP-TF-1: the arguments in the task's order, one per argument as `<name>: <value>`, joined by a
/// line feed. A value of several lines (CR LF and CR first turned into LF) continues on its own lines,
/// each indented by two spaces, so no line of a value reads as the start of another argument. An
/// argument whose value is empty is not written.
// spec: ordin-desktop/components/SessionSupervisor#SUP-TF-1
pub(crate) fn render_arguments(arguments: &[ordin_types::domain::runs::TaskArgument]) -> String {
    arguments
        .iter()
        .filter(|a| !a.value.is_empty())
        .map(|a| {
            let value = a.value.replace("\r\n", "\n").replace('\r', "\n");
            let mut lines = value.split('\n');
            let mut out = format!("{}: {}", a.name, lines.next().unwrap_or(""));
            for line in lines {
                out.push_str("\n  ");
                out.push_str(line);
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A.3: the task's input, arguments rendered `name: value` one per line (SUP-TF-1).
fn task_input(task: &Task) -> StartInput {
    match (&task.command, &task.free_text) {
        (Some(name), _) => StartInput::Command {
            name: name.clone(),
            arguments: render_arguments(&task.arguments),
        },
        (None, Some(text)) => StartInput::FreeText { text: text.clone() },
        (None, None) => StartInput::FreeText {
            text: String::new(),
        },
    }
}
