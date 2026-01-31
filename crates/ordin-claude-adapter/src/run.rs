//! One run in memory: its identity, state and replayable event buffer (ClaudeLocalAdapter.md
//! *Approach*, `events`, `read`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::stream::{BoxStream, StreamExt};
use ordin_runtime_adapter::{
    end_state, PauseReason, ProgressCount, RunEnd, RunEndReason, RunEvent, RunProgress,
    RunSnapshot, RunState, ToolCaller,
};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, CodingTaskId, RunId, ScopeId, WorkspaceId};
use tokio::sync::{mpsc, oneshot, watch};

use crate::config::{PackageMcpServer, SessionSetup};
use crate::error::Failure;

/// Longest buffer before consecutive `Progress` events are merged.
pub(crate) const MAX_BUFFERED_EVENTS: usize = 10_000;
/// How many ended runs stay in memory for `read` and a late `events`.
// spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-06
pub(crate) const MAX_ENDED_RUNS: usize = 64;
/// `RunEnd.final_message` limit (LW-15).
pub(crate) const MAX_FINAL_MESSAGE: usize = 32_768;
/// Longest text of a conversation's turn carried as `Person.question` (WIRE-PQ-1).
pub(crate) const MAX_TURN_TEXT: usize = 16_000;
/// Limit of the service's message on a used-up allowance.
pub(crate) const MAX_SERVICE_MESSAGE: usize = 500;

/// Writes a log line with the target `ordin_claude_adapter` and the run's fields (Observability).
macro_rules! run_log {
    ($level:ident, $run:expr, $name:literal $(, $($field:tt)+)?) => {
        tracing::$level!(
            target: "ordin_claude_adapter",
            run_id = %$run.run_id,
            runtime_session_id = %$run.runtime_session_id,
            agent_instance_id = %$run.agent_instance_id,
            workspace_id = %$run.workspace_id,
            correlation_id = %$run.correlation_id,
            $($($field)+,)?
            $name
        )
    };
}
pub(crate) use run_log;

/// What the run's task is told to do.
pub(crate) enum RunCommand {
    /// The person decided: emit `Resumed` and write this user turn, restarting the process first
    /// when it has ended.
    Resume {
        turn: String,
        ack: oneshot::Sender<Result<(), Failure>>,
    },
    /// The person's **Sign in again**.
    RestartSignIn,
    /// Stop the process and end the run with this reason.
    Stop {
        reason: RunEndReason,
        ack: oneshot::Sender<()>,
    },
}

/// What the Rules Engine's run-start decision leaves for the session.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionPlan {
    pub tool_subset: Option<Vec<String>>,
    pub single_call: bool,
    pub limit_seconds: Option<u32>,
}

/// What was registered for the session, so that any path can remove exactly that.
#[derive(Debug, Default)]
pub(crate) struct Registered {
    pub mcp: bool,
    pub hook: bool,
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
enum Phase {
    Running,
    Paused(PauseReason),
    Ended(RunEnd),
}

/// The identity of a run, fixed when its entry is created.
pub(crate) struct RunIdentity {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub runtime_session_id: String,
    pub correlation_id: String,
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,
    pub coding_task_id: Option<CodingTaskId>,
    /// `StartRun.started_by_person`: the free text is the person's own (SA-RECON-06).
    pub started_by_person: bool,
    pub harness_version: String,
    pub setup: SessionSetup,
}

pub(crate) struct Run {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub runtime_session_id: String,
    pub correlation_id: String,
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,
    pub coding_task_id: Option<CodingTaskId>,
    /// `StartRun.started_by_person`: the free text is the person's own (SA-RECON-06).
    pub started_by_person: bool,
    pub harness_version: String,
    pub setup: SessionSetup,
    /// `SessionSetup.package_servers`, whose environment may hold secrets; dropped when the run ends.
    pub package_servers: Mutex<Vec<PackageMcpServer>>,
    pub commands: mpsc::UnboundedSender<RunCommand>,
    /// True once a harness session exists to continue: the next start uses `--resume`.
    pub continued: AtomicBool,
    pub plan: Mutex<SessionPlan>,
    pub registered: Mutex<Registered>,
    /// The last 2 KiB of the child's stderr after a failure.
    // Kept with the run: this crate has no edge to a Diagnostics sink, and the tail is never logged.
    pub stderr_tail: Mutex<Vec<u8>>,
    events: Mutex<Vec<RunEvent>>,
    changed: watch::Sender<u64>,
    phase: Mutex<Phase>,
    checked_at: Mutex<Timestamp>,
    ended_at: Mutex<Option<Instant>>,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `text`, cut to `max` characters.
pub(crate) fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

impl Run {
    pub(crate) fn new(
        identity: RunIdentity,
        continued: bool,
    ) -> (Arc<Run>, mpsc::UnboundedReceiver<RunCommand>) {
        let (commands, inbox) = mpsc::unbounded_channel();
        let (changed, _) = watch::channel(0);
        let mut setup = identity.setup;
        let package_servers = Mutex::new(std::mem::take(&mut setup.package_servers));
        let run = Arc::new(Run {
            run_id: identity.run_id,
            agent_instance_id: identity.agent_instance_id,
            workspace_id: identity.workspace_id,
            runtime_session_id: identity.runtime_session_id,
            correlation_id: identity.correlation_id,
            caller: identity.caller,
            scope_id: identity.scope_id,
            coding_task_id: identity.coding_task_id,
            started_by_person: identity.started_by_person,
            harness_version: identity.harness_version,
            setup,
            package_servers,
            commands,
            continued: AtomicBool::new(continued),
            plan: Mutex::new(SessionPlan::default()),
            registered: Mutex::new(Registered::default()),
            stderr_tail: Mutex::new(Vec::new()),
            events: Mutex::new(Vec::new()),
            changed,
            phase: Mutex::new(Phase::Running),
            checked_at: Mutex::new(Timestamp::now()),
            ended_at: Mutex::new(None),
        });
        (run, inbox)
    }

    /// A conversation run: Ordin's, and not narrowed to a single call. Its turns end in a
    /// `Person` pause instead of ending the run.
    // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-1
    pub(crate) fn is_conversation(&self) -> bool {
        matches!(self.caller, ToolCaller::Ordin) && !lock(&self.plan).single_call
    }

    pub(crate) fn is_continued(&self) -> bool {
        self.continued.load(Ordering::SeqCst)
    }

    /// Records the time of a stdout line or a hook event (`read().checked_at`).
    pub(crate) fn touch(&self) {
        *lock(&self.checked_at) = Timestamp::now();
    }

    /// Buffers one event and wakes the subscribers; nothing follows `Ended`. Past
    /// [`MAX_BUFFERED_EVENTS`] a `Progress` is merged into a `Progress` before it: the count is
    /// kept and the line is the latest's.
    pub(crate) fn emit(&self, event: RunEvent) {
        let mut events = lock(&self.events);
        {
            let mut phase = lock(&self.phase);
            if matches!(*phase, Phase::Ended(_)) {
                return;
            }
            match &event {
                RunEvent::Paused { reason } => *phase = Phase::Paused(reason.clone()),
                RunEvent::Resumed => *phase = Phase::Running,
                RunEvent::Ended { end } => {
                    *phase = Phase::Ended(end.clone());
                    *lock(&self.ended_at) = Some(Instant::now());
                }
                _ => {}
            }
        }
        if events.len() >= MAX_BUFFERED_EVENTS {
            if let (Some(RunEvent::Progress { progress: last }), RunEvent::Progress { progress }) =
                (events.last_mut(), &event)
            {
                *last = progress.clone();
                drop(events);
                self.changed.send_modify(|revision| *revision += 1);
                return;
            }
        }
        events.push(event);
        drop(events);
        self.changed.send_modify(|revision| *revision += 1);
    }

    /// Emits `Ended`.
    pub(crate) fn emit_end(&self, reason: RunEndReason, final_message: Option<String>) {
        self.emit(RunEvent::Ended {
            end: RunEnd {
                reason,
                final_message,
                artefacts: Vec::new(),
            },
        });
    }

    /// Emits a `Progress` with `line` and no count, or the count of tool calls so far.
    pub(crate) fn emit_progress(&self, line: String, done: Option<u64>) {
        self.emit(RunEvent::Progress {
            progress: RunProgress {
                line,
                count: done.map(|done| ProgressCount { done, total: None }),
            },
        });
    }

    /// Marks a run taken from another instance as paused, with no event: the Session Supervisor
    /// already recorded that pause.
    pub(crate) fn mark_paused(&self, reason: PauseReason) {
        *lock(&self.phase) = Phase::Paused(reason);
    }

    /// The reason the run is paused on, when it is.
    pub(crate) fn paused_on(&self) -> Option<PauseReason> {
        match &*lock(&self.phase) {
            Phase::Paused(reason) => Some(reason.clone()),
            _ => None,
        }
    }

    /// When the run ended, if it has.
    pub(crate) fn ended_at(&self) -> Option<Instant> {
        *lock(&self.ended_at)
    }

    pub(crate) fn is_ended(&self) -> bool {
        matches!(*lock(&self.phase), Phase::Ended(_))
    }

    /// `read`: running → `Running`, any pause → `Paused`, ended → the reason's state.
    pub(crate) fn snapshot(&self) -> RunSnapshot {
        let (state, end) = match &*lock(&self.phase) {
            Phase::Running => (RunState::Running, None),
            Phase::Paused(_) => (RunState::Paused, None),
            Phase::Ended(end) => (end_state(end.reason), Some(end.clone())),
        };
        RunSnapshot {
            run_id: self.run_id,
            state,
            end,
            checked_at: *lock(&self.checked_at),
        }
    }

    /// Replays the buffer from `Started`, then follows live events, ending after `Ended`.
    pub(crate) fn subscribe(self: &Arc<Run>) -> BoxStream<'static, RunEvent> {
        let changes = self.changed.subscribe();
        futures::stream::unfold(
            (self.clone(), 0usize, changes),
            |(run, next, mut changes)| async move {
                loop {
                    {
                        let events = lock(&run.events);
                        if let Some(event) = events.get(next) {
                            let event = event.clone();
                            drop(events);
                            return Some((event, (run, next + 1, changes)));
                        }
                        if matches!(events.last(), Some(RunEvent::Ended { .. })) {
                            return None;
                        }
                    }
                    if changes.changed().await.is_err() {
                        return None;
                    }
                }
            },
        )
        .boxed()
    }
}
