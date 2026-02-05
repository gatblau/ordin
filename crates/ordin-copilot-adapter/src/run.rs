//! One run in memory: its identity, state and replayable event buffer (CopilotLocalAdapter.md
//! `events` and `read`: "As the Claude Local Adapter").

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::stream::{BoxStream, StreamExt};
use ordin_runtime_adapter::{
    end_state, PauseReason, ProgressCount, RunEnd, RunEndReason, RunEvent, RunProgress,
    RunSnapshot, RunState, ToolCaller,
};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, ApprovalId, CodingTaskId, RunId, ScopeId, WorkspaceId};
use tokio::sync::{mpsc, oneshot, watch};

use crate::config::{CopilotSessionSetup, PackageMcpServer};
use crate::error::Failure;
use crate::port::CopilotServer;

/// Longest buffer before consecutive `Progress` events are merged.
pub(crate) const MAX_BUFFERED_EVENTS: usize = 10_000;
/// How many ended runs stay in memory for `read` and a late `events`.
pub(crate) const MAX_ENDED_RUNS: usize = 64;
/// `RunEnd.final_message` limit (LW-15).
pub(crate) const MAX_FINAL_MESSAGE: usize = 32_768;
/// Limit of the service's message on a used-up allowance (Session events table).
pub(crate) const MAX_SERVICE_MESSAGE: usize = 500;
/// A `Person` pause's question: the turn's final text, cut to 16,000 characters (WIRE-PQ-1).
pub(crate) const MAX_TURN_TEXT: usize = 16_000;

/// Writes a log line with the target `ordin_copilot_adapter` and the run's fields (Observability).
macro_rules! run_log {
    ($level:ident, $run:expr, $name:literal $(, $($field:tt)+)?) => {
        tracing::$level!(
            target: "ordin_copilot_adapter",
            run_id = %$run.run_id,
            runtime_session_id = $run.session_id().as_deref(),
            agent_instance_id = $run.agent_instance_id.to_string().as_str(),
            workspace_id = $run.workspace_id.to_string().as_str(),
            correlation_id = %$run.correlation_id,
            $($($field)+,)?
            $name
        )
    };
}
pub(crate) use run_log;

/// What the run's task is told to do.
pub(crate) enum RunCommand {
    /// A tool approval was decided: emit `Resumed` and send this turn.
    Resume {
        turn: String,
        ack: oneshot::Sender<Result<(), Failure>>,
    },
    /// The person's next message in a conversation run: emit `Resumed` and send this turn; a
    /// failed send ends the run `Failed`.
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
    Answer {
        turn: String,
        ack: oneshot::Sender<Result<(), Failure>>,
    },
    /// The person's `Answer` to a `SignIn` pause: `start_login` again.
    RestartSignIn,
    /// Stop the server and end the run with this reason.
    Stop {
        reason: RunEndReason,
        ack: oneshot::Sender<()>,
    },
}

#[derive(Debug, Clone)]
enum Phase {
    Running,
    Paused(PauseReason),
    Ended(RunEnd),
}

/// What was registered for the session, so that any path can remove exactly that.
#[derive(Debug, Default)]
pub(crate) struct Registered {
    pub mcp: bool,
    pub hook: bool,
}

/// The identity of a run, fixed when its entry is created.
pub(crate) struct RunIdentity {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub correlation_id: String,
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,
    pub coding_task_id: Option<CodingTaskId>,
    /// `StartRun.only_servers`, copied into the `SessionGrant` (amendment 2026-11-21).
    pub only_servers: Option<Vec<String>>,
    pub setup: CopilotSessionSetup,
}

pub(crate) struct Run {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub correlation_id: String,
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,
    pub coding_task_id: Option<CodingTaskId>,
    pub only_servers: Option<Vec<String>>,
    pub setup: CopilotSessionSetup,
    /// `CopilotSessionSetup.package_servers`, whose environment may hold secrets; dropped when the
    /// run ends.
    pub package_servers: Mutex<Vec<PackageMcpServer>>,
    pub commands: mpsc::UnboundedSender<RunCommand>,
    pub registered: Mutex<Registered>,
    /// The session's CLI server, once started (step 5).
    pub server: Mutex<Option<Arc<dyn CopilotServer>>>,
    /// The MCP Front's server names for this session (origin classification).
    pub front_servers: Mutex<Vec<String>>,
    /// The parent of every declared-tool call's token; cancelled when the session is taken down.
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-5
    pub calls: tokio_util::sync::CancellationToken,
    /// The Copilot session id, once created or resumed (step 9).
    session_id: Mutex<Option<String>>,
    /// A hook held for a person's approval (`HookSessionEvent::Holding`), cleared by `Decided`.
    pub holding: Mutex<Option<ApprovalId>>,
    /// The run-start decision's `single_call`, recorded once the decision is taken (step 7).
    single_call: AtomicBool,
    events: Mutex<Vec<RunEvent>>,
    changed: watch::Sender<u64>,
    phase: Mutex<Phase>,
    started_emitted: Mutex<bool>,
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
    pub(crate) fn new(identity: RunIdentity) -> (Arc<Run>, mpsc::UnboundedReceiver<RunCommand>) {
        let (commands, inbox) = mpsc::unbounded_channel();
        let (changed, _) = watch::channel(0);
        let mut setup = identity.setup;
        let package_servers = Mutex::new(std::mem::take(&mut setup.package_servers));
        let run = Arc::new(Run {
            run_id: identity.run_id,
            agent_instance_id: identity.agent_instance_id,
            workspace_id: identity.workspace_id,
            correlation_id: identity.correlation_id,
            caller: identity.caller,
            scope_id: identity.scope_id,
            coding_task_id: identity.coding_task_id,
            only_servers: identity.only_servers,
            setup,
            package_servers,
            commands,
            registered: Mutex::new(Registered::default()),
            server: Mutex::new(None),
            front_servers: Mutex::new(Vec::new()),
            calls: tokio_util::sync::CancellationToken::new(),
            session_id: Mutex::new(None),
            holding: Mutex::new(None),
            single_call: AtomicBool::new(false),
            events: Mutex::new(Vec::new()),
            changed,
            phase: Mutex::new(Phase::Running),
            started_emitted: Mutex::new(false),
            checked_at: Mutex::new(Timestamp::now()),
            ended_at: Mutex::new(None),
        });
        (run, inbox)
    }

    pub(crate) fn session_id(&self) -> Option<String> {
        lock(&self.session_id).clone()
    }

    pub(crate) fn set_session_id(&self, id: String) {
        *lock(&self.session_id) = Some(id);
    }

    pub(crate) fn set_single_call(&self, single_call: bool) {
        self.single_call.store(single_call, Ordering::SeqCst);
    }

    /// A conversation run: Ordin's, and not narrowed to a single call. Its turns end in a
    /// `Person` pause instead of ending the run.
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-1
    pub(crate) fn is_conversation(&self) -> bool {
        matches!(self.caller, ToolCaller::Ordin) && !self.single_call.load(Ordering::SeqCst)
    }

    pub(crate) fn server(&self) -> Option<Arc<dyn CopilotServer>> {
        lock(&self.server).clone()
    }

    /// Records the time of a Copilot or hook event (`read().checked_at`).
    pub(crate) fn touch(&self) {
        *lock(&self.checked_at) = Timestamp::now();
    }

    /// Emits `Started` once (`start` steps 6.2 and 7: "when not already emitted").
    pub(crate) fn emit_started(&self, runtime_session_id: Option<String>) {
        let mut emitted = lock(&self.started_emitted);
        if *emitted {
            return;
        }
        *emitted = true;
        drop(emitted);
        self.emit(RunEvent::Started {
            runtime_session_id,
            remote_thread_id: None,
        });
    }

    /// Buffers one event and wakes the subscribers; nothing follows `Ended`. Past
    /// [`MAX_BUFFERED_EVENTS`] a `Progress` is merged into a `Progress` before it.
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

    pub(crate) fn emit_end(&self, reason: RunEndReason, final_message: Option<String>) {
        self.emit(RunEvent::Ended {
            end: RunEnd {
                reason,
                final_message,
                artefacts: Vec::new(),
            },
        });
    }

    pub(crate) fn emit_progress(&self, line: String, done: Option<u64>) {
        self.emit(RunEvent::Progress {
            progress: RunProgress {
                line,
                count: done.map(|done| ProgressCount { done, total: None }),
            },
        });
    }

    pub(crate) fn paused_on(&self) -> Option<PauseReason> {
        match &*lock(&self.phase) {
            Phase::Paused(reason) => Some(reason.clone()),
            _ => None,
        }
    }

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
