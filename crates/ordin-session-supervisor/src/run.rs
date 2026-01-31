//! One task per run (SessionSupervisor.md *Internal Logic* C, D.1–D.3, E): it starts or
//! re-attaches the run, applies every adapter event to the `runs` row before emitting and
//! broadcasting it, holds the run's wall-clock and idle timers, and records the end.

use std::sync::Arc;
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use ordin_journal::NewJournalEntry;
use ordin_runtime_adapter::RunTarget;
use ordin_runtime_adapter::{
    check_start, check_verb, end_state, guard_events, AdapterError, AdapterVerb, Guarded,
    PauseReason, RunEnd, RunEndReason, RunEvent, RunHandle, StartInput, StartRun, ToolCaller,
};
use ordin_types::domain::runs::{
    RuleNotEnforced, RunPauseReason, RunState, StartupReport, StartupSubject, StepSource,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::wire::startup::{StartUpReportItem, StartUpSubject};
use ordin_types::{RunId, StartupReportId};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::agent;
use crate::store::{self, enum_text};
use crate::supervisor::{adapter_ctx, bounded, Control, Launch, LaunchMode, SessionSupervisor};
use crate::types::{ContentCheck, ContentPoint};
use crate::validate::{cut, is_local, MAX_END_REASON};

/// `run_starting` is awaited for at most this long (SA-SUP-AMEM-01).
const RUN_STARTING_WAIT: Duration = Duration::from_secs(2);
/// C.5: one retry of a failed store write, after this pause.
const STORE_RETRY: Duration = Duration::from_millis(100);
/// C.2: a start refused with `ThreadBusy` is tried again after this pause.
const THREAD_BUSY_RETRY: Duration = Duration::from_secs(5);
/// E: most start-up report items (2C `START_TOO_MANY_ITEMS`).
const MAX_STARTUP_ITEMS: usize = 200;

/// How a run ends: what is recorded, journalled and handed to the Run Queue.
#[derive(Debug, Clone)]
pub(crate) struct Ending {
    pub state: RunState,
    pub end_reason: Option<String>,
    /// The adapter's end, journalled as `run_result`; None when nothing may be journalled.
    pub result: Option<RunEnd>,
    /// The code journalled with the end.
    pub code: Option<String>,
    pub limit: bool,
    pub safe_reply: bool,
    pub control: Option<String>,
    /// Agent profile: the Run Queue's cancel path (`record_cancelled`).
    pub queue_cancelled: bool,
}

impl Ending {
    pub(crate) fn plain(state: RunState, end_reason: Option<String>) -> Ending {
        Ending {
            state,
            end_reason,
            result: None,
            code: None,
            limit: false,
            safe_reply: false,
            control: None,
            queue_cancelled: false,
        }
    }

    fn failed(reason: &str, code: impl Into<String>) -> Ending {
        Ending {
            code: Some(code.into()),
            ..Ending::plain(RunState::Failed, Some(reason.to_owned()))
        }
    }

    /// The adapter's `Ended`, with the final message cut to 500 characters for the column.
    fn from_end(end: RunEnd) -> Ending {
        Ending {
            end_reason: end.final_message.as_deref().map(|m| cut(m, MAX_END_REASON)),
            result: Some(end.clone()),
            ..Ending::plain(end_state(end.reason), None)
        }
    }
}

enum Outcome {
    End(Ending),
    /// J.7.2: run the record once more without the gathered context.
    Retry,
    /// H.3: the run paused for an approval; the stretch ends here.
    QueuePaused(ordin_types::ApprovalId),
    /// C.5: the store failed twice; the run is failed in memory only.
    StoreLost,
}

pub(crate) struct RunTask {
    pub sup: Arc<SessionSupervisor>,
    pub l: Launch,
    token: CancellationToken,
    handle: Option<RunHandle>,
    running_since: Option<Instant>,
    last_event: Instant,
    reported: bool,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    input_checked: bool,
}

/// Journals a code with a run, as an internal entry (never fails the run).
pub(crate) async fn journal_code(sup: &SessionSupervisor, run_id: RunId, code: &str) {
    let _ = sup
        .journal
        .append_entry(NewJournalEntry {
            kind: "run_code".into(),
            component: SessionSupervisor::MODULE.into(),
            run_id: Some(run_id),
            crew_run_id: None,
            workspace_id: None,
            summary: "A run recorded a code.".into(),
            data: serde_json::json!({ "code": code }),
        })
        .await;
}

/// C.3 `Ended`: the full final message and artefacts, as entry `run_result`.
pub(crate) async fn journal_result(sup: &SessionSupervisor, run_id: RunId, end: &RunEnd) {
    let mut data = serde_json::json!({
        "reason": end.reason,
        "artefacts": end.artefacts,
    });
    if let Some(message) = &end.final_message {
        // The Journal caps an entry at 65,536 bytes; a final message is at most 32,768 characters.
        data["final_message"] = serde_json::Value::String(cut(message, 16_000));
    }
    let _ = sup
        .journal
        .append_entry(NewJournalEntry {
            kind: "run_result".into(),
            component: SessionSupervisor::MODULE.into(),
            run_id: Some(run_id),
            crew_run_id: None,
            workspace_id: None,
            summary: "A run ended with its result.".into(),
            data,
        })
        .await;
}

fn end_reason_of(state: RunState) -> RunEndReason {
    match state {
        RunState::Failed => RunEndReason::Failed,
        RunState::StoppedAtLimit => RunEndReason::StoppedAtLimit,
        RunState::Cancelled => RunEndReason::Cancelled,
        RunState::Interrupted => RunEndReason::Interrupted,
        RunState::Abandoned => RunEndReason::Abandoned,
        RunState::NoReply => RunEndReason::NoReply,
        _ => RunEndReason::Completed,
    }
}

pub(crate) async fn run_task(
    sup: Arc<SessionSupervisor>,
    launch: Launch,
    mut rx: UnboundedReceiver<Control>,
) {
    let token = sup
        .lock()
        .live
        .get(&launch.row.run.id)
        .map(|l| l.token.clone())
        .unwrap_or_default();
    let handle = match &launch.mode {
        LaunchMode::Reattach { handle, .. } => Some(handle.clone()),
        LaunchMode::Start => None,
    };
    let reported = matches!(launch.mode, LaunchMode::Reattach { .. });
    let span = launch.queue.as_ref().map(|q| q.span.clone());
    let mut task = RunTask {
        sup,
        l: launch,
        token,
        handle,
        running_since: None,
        last_event: Instant::now(),
        reported,
        input_tokens: None,
        output_tokens: None,
        input_checked: false,
    };
    match span {
        Some(span) => task.go(&mut rx).instrument(span).await,
        None => task.go(&mut rx).await,
    }
}

impl RunTask {
    fn run_id(&self) -> RunId {
        self.l.row.run.id
    }

    fn id_text(&self) -> String {
        self.run_id().to_string()
    }

    async fn go(&mut self, rx: &mut UnboundedReceiver<Control>) {
        loop {
            match self.attempt(rx).await {
                Outcome::Retry => {
                    if let Some(q) = self.l.queue.as_mut() {
                        q.retried = true;
                    }
                    self.l.mode = LaunchMode::Start;
                    continue;
                }
                Outcome::End(ending) => {
                    self.finish(ending).await;
                    return;
                }
                Outcome::QueuePaused(approval_id) => {
                    agent::after_pause(self, approval_id).await;
                    return;
                }
                Outcome::StoreLost => {
                    self.store_lost().await;
                    return;
                }
            }
        }
    }

    /// C.5: cancel the run through the adapter and keep it failed in memory; `recover`
    /// reconciles the row.
    async fn store_lost(&mut self) {
        tracing::error!(
            run_id = self.id_text().as_str(),
            code = "SUP_STORE_UNAVAILABLE",
            "sup_store_unavailable"
        );
        self.token.cancel();
        if let Some(handle) = self.handle.clone() {
            if check_verb(self.l.adapter.as_ref(), AdapterVerb::Cancel).is_ok() {
                let ctx = adapter_ctx(&self.id_text(), &CancellationToken::new());
                let _ = bounded(self.l.adapter.cancel(&ctx, &handle)).await;
            }
        }
        self.broadcast(RunEvent::Ended {
            end: RunEnd {
                reason: RunEndReason::Failed,
                final_message: None,
                artefacts: vec![],
            },
        });
        self.sup.forget(self.run_id());
    }

    /// One start (or re-attachment) and the event loop up to an end, a pause handed to the
    /// Run Queue, or a store failure.
    async fn attempt(&mut self, rx: &mut UnboundedReceiver<Control>) -> Outcome {
        let mode = std::mem::replace(&mut self.l.mode, LaunchMode::Start);
        let handle = match mode {
            LaunchMode::Start => match self.start(rx).await {
                Ok(handle) => handle,
                Err(outcome) => return outcome,
            },
            LaunchMode::Reattach { handle, resume } => {
                if let Some(decision) = resume {
                    let ctx = adapter_ctx(&self.id_text(), &self.token);
                    if let Err(e) = bounded(self.l.adapter.resume(&ctx, &handle, decision)).await {
                        let code = ordin_types::CodedError::code(&e);
                        return Outcome::End(Ending::failed("couldn't reach the agent", code));
                    }
                }
                handle
            }
        };
        self.handle = Some(handle.clone());
        self.set_live_handle(&handle);
        let ctx = adapter_ctx(&self.id_text(), &self.token);
        let stream = match self.l.adapter.events(&ctx, &handle) {
            Ok(stream) => guard_events(self.l.kind, self.run_id(), stream),
            Err(_) => return self.event_order(None).await,
        };
        self.supervise(stream, rx).await
    }

    fn set_live_handle(&self, handle: &RunHandle) {
        if let Some(live) = self.sup.lock().live.get_mut(&self.run_id()) {
            live.handle = Some(handle.clone());
        }
    }

    /// C.1–C.2 (with J.7.1 and the ADR-111 `run_starting` call).
    async fn start(&mut self, rx: &mut UnboundedReceiver<Control>) -> Result<RunHandle, Outcome> {
        let run_id = self.run_id();
        let local = is_local(self.l.kind);
        // J.7.1: the input check, before any harness or model (SA-HOST-06).
        // spec: ordin-desktop/components/SessionSupervisor#SA-HOST-06
        if self.l.queue.is_some() && !self.input_checked {
            self.input_checked = true;
            let text = match &self.l.input {
                StartInput::FreeText { text } => text.clone(),
                StartInput::Command { arguments, .. } => arguments.clone(),
            };
            let check = self
                .sup
                .host
                .check_content(run_id, ContentPoint::Input, &text)
                .await;
            if let Some(q) = self.l.queue.as_mut() {
                q.checks += 1;
            }
            if matches!(check, ContentCheck::Blocked { .. }) {
                if let Some(q) = self.l.queue.as_mut() {
                    q.checks_blocked += 1;
                }
                return Err(Outcome::End(Ending {
                    safe_reply: true,
                    ..Ending::plain(RunState::Completed, None)
                }));
            }
        }
        // spec: ordin-desktop/components/SessionSupervisor#SA-RECON-04
        let start = StartRun {
            run_id,
            workspace_id: self.l.row.workspace_id,
            target: self.l.target.clone(),
            input: self.l.input.clone(),
            attachments: self.l.attachments.clone(),
            limits: self.l.row.run.limits,
            thread: self.l.thread.clone(),
            model: local.then(|| self.l.choice.model_choice()),
            thinking: local.then_some(self.l.choice.thinking),
            effort: local.then(|| self.l.choice.effort.clone()).flatten(),
            caller: self.l.caller,
            scope_id: self.l.scope_id,
            coding_task_id: self.l.coding_task_id,
            started_by_person: self.l.started_by_person,
            // spec: ordin-desktop/components/SessionSupervisor#SUP-SRV-1
            only_servers: self.l.only_servers.clone(),
        };
        if let Err(e) = check_start(self.l.kind, &start) {
            return Err(Outcome::End(Ending::failed(
                "couldn't start the agent",
                e.code,
            )));
        }
        // ADR-111: memory rules for an agent task session, at most 2 s (SA-SUP-AMEM-01).
        // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-AMEM-01
        if start.caller == ToolCaller::Agent
            && matches!(start.target, RunTarget::AgentInstance { .. })
            && tokio::time::timeout(RUN_STARTING_WAIT, self.sup.host.run_starting(&start))
                .await
                .is_err()
        {
            tracing::warn!(run_id = self.id_text().as_str(), "sup_run_starting_slow");
        }
        loop {
            let ctx = adapter_ctx(&self.id_text(), &self.token);
            match bounded(self.l.adapter.start(&ctx, start.clone())).await {
                Ok(handle) => return Ok(handle),
                Err(AdapterError::SignedOut) => return Err(self.signed_out(rx).await),
                Err(AdapterError::ThreadBusy) => {
                    // C.2: stays queued and is tried again when the thread frees.
                    // The adapter gives no signal that the thread is free; the start
                    // is tried again every 5 s while the run is not stopped.
                    tokio::select! {
                        control = rx.recv() => {
                            if let Some(ending) = self.control_before_start(control) {
                                return Err(Outcome::End(ending));
                            }
                        }
                        _ = tokio::time::sleep(THREAD_BUSY_RETRY) => {}
                    }
                }
                Err(e) => {
                    let code = ordin_types::CodedError::code(&e).to_owned();
                    let ending = match e {
                        AdapterError::AllowanceExhausted { service_message } => Ending::failed(
                            &cut(&service_message, MAX_END_REASON),
                            "RA_ALLOWANCE_EXHAUSTED",
                        ),
                        AdapterError::HarnessUntested { .. } => {
                            Ending::failed("harness version not tested", code)
                        }
                        AdapterError::Unreachable | AdapterError::Deadline => {
                            Ending::failed("couldn't reach the agent", code)
                        }
                        AdapterError::InvalidRequest { code } | AdapterError::Refused { code } => {
                            Ending::failed("couldn't start the agent", code)
                        }
                        _ => Ending::failed("couldn't start the agent", code),
                    };
                    return Err(Outcome::End(ending));
                }
            }
        }
    }

    /// A stop asked for before the harness started: nothing to cancel at the adapter.
    fn control_before_start(&self, control: Option<Control>) -> Option<Ending> {
        match control {
            Some(Control::Cancel) => Some(Ending::plain(RunState::Cancelled, None)),
            Some(Control::QueueCancel) => Some(Ending {
                queue_cancelled: true,
                ..Ending::plain(RunState::Cancelled, None)
            }),
            Some(Control::Interrupt { reason }) => {
                Some(Ending::plain(RunState::Interrupted, reason))
            }
            Some(Control::LeaseLost) => Some(self.lease_lost_ending()),
            Some(Control::DutyLock(control)) => Some(control_ending(control)),
            None => Some(Ending::plain(RunState::Interrupted, None)),
            Some(Control::Snapshot(_) | Control::Woke(_)) => None,
        }
    }

    /// C.2 `SignedOut`: the run is paused for sign-in and waits for a stop (ADR-044).
    async fn signed_out(&mut self, rx: &mut UnboundedReceiver<Control>) -> Outcome {
        journal_code(&self.sup, self.run_id(), "RA_SIGNED_OUT").await;
        self.l.row.run.state = RunState::Paused;
        self.l.row.run.pause_reason = Some(RunPauseReason::SignIn);
        // Before the row: a reader who sees `paused` can resume at once.
        self.sync_live(Some(PauseReason::SignIn));
        if !self.commit(Some(RunState::Paused)).await {
            return Outcome::StoreLost;
        }
        self.sup.release_slot(self.run_id());
        tracing::info!(
            run_id = self.id_text().as_str(),
            reason = "sign_in",
            "sup_run_paused"
        );
        loop {
            let control = rx.recv().await;
            if let Some(ending) = self.control_before_start(control) {
                return Outcome::End(ending);
            }
        }
    }

    /// C.3 and D: the event loop with the run's timers.
    async fn supervise(
        &mut self,
        mut stream: BoxStream<'static, Guarded>,
        rx: &mut UnboundedReceiver<Control>,
    ) -> Outcome {
        let idle = Duration::from_secs(u64::from(self.l.row.run.limits.idle_timeout_secs));
        if self.l.row.run.state == RunState::Running {
            self.running_since = Some(Instant::now());
        }
        loop {
            let wall_at = self.running_since.map(|since| since + self.l.remaining);
            let idle_at = self.running_since.map(|_| self.last_event + idle);
            let far = Instant::now() + Duration::from_secs(365 * 24 * 3600);
            tokio::select! {
                biased;
                control = rx.recv() => {
                    if let Some(outcome) = self.on_control(control, &mut stream).await {
                        return outcome;
                    }
                }
                item = stream.next() => {
                    if let Some(outcome) = self.on_item(item, &mut stream).await {
                        return outcome;
                    }
                }
                _ = tokio::time::sleep_until(wall_at.unwrap_or(far)), if wall_at.is_some() => {
                    return self.stop_at_limit("wall-clock limit", &mut stream).await;
                }
                _ = tokio::time::sleep_until(idle_at.unwrap_or(far)), if idle_at.is_some() => {
                    return self.stop_at_limit("idle timeout", &mut stream).await;
                }
            }
        }
    }

    async fn on_control(
        &mut self,
        control: Option<Control>,
        stream: &mut BoxStream<'static, Guarded>,
    ) -> Option<Outcome> {
        let deadline = self.sup.config.stop_deadline;
        match control {
            Some(Control::Cancel) => {
                Some(self.stop(RunState::Cancelled, None, stream, deadline).await)
            }
            Some(Control::QueueCancel) => {
                let outcome = self.stop(RunState::Cancelled, None, stream, deadline).await;
                Some(match outcome {
                    Outcome::End(ending) => Outcome::End(Ending {
                        queue_cancelled: true,
                        ..ending
                    }),
                    other => other,
                })
            }
            Some(Control::Interrupt { reason }) => Some(
                self.stop(RunState::Interrupted, reason, stream, deadline)
                    .await,
            ),
            None => Some(
                self.stop(RunState::Interrupted, None, stream, deadline)
                    .await,
            ),
            Some(Control::DutyLock(control)) => {
                self.cancel_quietly().await;
                Some(Outcome::End(control_ending(control)))
            }
            Some(Control::LeaseLost) => {
                self.cancel_quietly().await;
                Some(Outcome::End(self.lease_lost_ending()))
            }
            Some(Control::Snapshot(Some(end))) => Some(Outcome::End(Ending::from_end(end))),
            Some(Control::Snapshot(None)) => {
                Some(Outcome::End(Ending::plain(RunState::Interrupted, None)))
            }
            Some(Control::Woke(slept)) => {
                // J.2: time asleep does not count.
                if let Some(since) = self.running_since.as_mut() {
                    *since += slept;
                }
                self.last_event += slept;
                None
            }
        }
    }

    fn lease_lost_ending(&self) -> Ending {
        tracing::warn!(run_id = self.id_text().as_str(), "sup_lease_lost");
        self.sup.count("wrightrun_run_leases_lost_total", &[]);
        Ending {
            code: Some("SUP_LEASE_LOST".into()),
            ..Ending::plain(RunState::Interrupted, None)
        }
    }

    /// Cancels the harness without waiting for its end (J.6, H.1 lease lost).
    async fn cancel_quietly(&mut self) {
        self.token.cancel();
        if let Some(handle) = self.handle.clone() {
            if check_verb(self.l.adapter.as_ref(), AdapterVerb::Cancel).is_ok() {
                let ctx = adapter_ctx(&self.id_text(), &CancellationToken::new());
                let _ = bounded(self.l.adapter.cancel(&ctx, &handle)).await;
            }
        }
    }

    /// D.4 and I: cancel the token, `adapter.cancel`, wait up to the stop deadline for `Ended`,
    /// then record `target`. A remote adapter with no `cancel` → `abandoned`.
    async fn stop(
        &mut self,
        target: RunState,
        reason: Option<String>,
        stream: &mut BoxStream<'static, Guarded>,
        deadline: Duration,
    ) -> Outcome {
        self.token.cancel();
        if check_verb(self.l.adapter.as_ref(), AdapterVerb::Cancel).is_err() {
            return Outcome::End(self.abandon_or(target, reason));
        }
        if let Some(handle) = self.handle.clone() {
            let ctx = adapter_ctx(&self.id_text(), &CancellationToken::new());
            if let Err(AdapterError::VerbNotOffered { .. }) =
                bounded(self.l.adapter.cancel(&ctx, &handle)).await
            {
                return Outcome::End(self.abandon_or(target, reason));
            }
        }
        let end = wait_for_end(stream, deadline).await;
        Outcome::End(Ending {
            result: end.filter(|e| !e.artefacts.is_empty()),
            ..Ending::plain(target, reason)
        })
    }

    /// D.3, D.4: a remote run whose adapter cannot cancel is abandoned; its result is not used.
    fn abandon_or(&self, target: RunState, reason: Option<String>) -> Ending {
        if is_local(self.l.kind) {
            return Ending::plain(target, reason);
        }
        self.sup.host.run_abandoned(self.run_id(), self.l.kind);
        Ending::plain(RunState::Abandoned, reason)
    }

    /// D.3: stop at a limit.
    async fn stop_at_limit(
        &mut self,
        limit: &'static str,
        stream: &mut BoxStream<'static, Guarded>,
    ) -> Outcome {
        tracing::info!(run_id = self.id_text().as_str(), limit, "sup_limit_reached");
        let deadline = self.sup.config.stop_deadline;
        let outcome = self
            .stop(
                RunState::StoppedAtLimit,
                Some(limit.to_owned()),
                stream,
                deadline,
            )
            .await;
        match outcome {
            Outcome::End(ending) if ending.state == RunState::StoppedAtLimit => {
                Outcome::End(Ending {
                    code: Some("SUP_LIMIT_REACHED".into()),
                    limit: true,
                    ..ending
                })
            }
            other => other,
        }
    }

    async fn on_item(
        &mut self,
        item: Option<Guarded>,
        stream: &mut BoxStream<'static, Guarded>,
    ) -> Option<Outcome> {
        let Some(item) = item else {
            return Some(self.event_order(Some(stream)).await);
        };
        let event = match item {
            Guarded::Event(event) => event,
            Guarded::Violation(violation) => {
                let _ = self
                    .sup
                    .journal
                    .append_entry(NewJournalEntry {
                        kind: "run_event_violation".into(),
                        component: SessionSupervisor::MODULE.into(),
                        run_id: Some(self.run_id()),
                        crew_run_id: None,
                        workspace_id: None,
                        summary: "An adapter event broke the event rules.".into(),
                        data: serde_json::json!({ "code": violation.code, "field": violation.field }),
                    })
                    .await;
                if violation.code == "RA_EVENT_ORDER" {
                    return Some(self.event_order(Some(stream)).await);
                }
                return None;
            }
        };
        self.last_event = Instant::now();
        match event {
            RunEvent::Started {
                runtime_session_id,
                remote_thread_id,
            } => self.on_started(runtime_session_id, remote_thread_id).await,
            RunEvent::Progress { progress } => {
                let emitted = self
                    .emit(
                        EventKind::RunProgress,
                        serde_json::json!({ "run_id": self.run_id(), "progress": progress }),
                    )
                    .await;
                if !emitted {
                    return Some(Outcome::StoreLost);
                }
                agent::append_progress(self, &progress.line).await;
                self.broadcast(RunEvent::Progress { progress });
                None
            }
            RunEvent::HarnessPlan { steps } => {
                let planned = steps
                    .iter()
                    .map(|line| ordin_journal::PlannedStep {
                        line: line.clone(),
                        came_from: StepSource::HarnessTodoList,
                    })
                    .collect();
                if let Err(e) = self.sup.journal.state_plan(self.run_id(), planned).await {
                    tracing::warn!(
                        run_id = self.id_text().as_str(),
                        code = ordin_types::CodedError::code(&e),
                        "sup_plan_not_kept"
                    );
                }
                self.broadcast(RunEvent::HarnessPlan { steps });
                None
            }
            RunEvent::Paused { reason } => self.on_paused(reason).await,
            RunEvent::Resumed => {
                self.l.row.run.state = RunState::Running;
                self.l.row.run.pause_reason = None;
                self.l.row.pause_approval_id = None;
                // The in-memory state moves first, so a caller that reads `running` from the store
                // (`get`) never meets an older live state (`save_session_record`).
                self.sync_live(None);
                if !self.commit(Some(RunState::Running)).await {
                    return Some(Outcome::StoreLost);
                }
                self.running_since = Some(Instant::now());
                self.broadcast(RunEvent::Resumed);
                None
            }
            RunEvent::Usage { usage } => self.on_usage(usage, stream).await,
            RunEvent::Ended { end } => Some(self.on_ended(end).await),
        }
    }

    async fn on_started(
        &mut self,
        runtime_session_id: Option<String>,
        remote_thread_id: Option<String>,
    ) -> Option<Outcome> {
        let run = &mut self.l.row.run;
        run.state = RunState::Running;
        run.pause_reason = None;
        self.l.row.pause_approval_id = None;
        if run.started_at.is_none() {
            run.started_at = Some(Timestamp::now());
        }
        if is_local(self.l.kind) {
            if runtime_session_id.is_some() {
                run.runtime_session_id = runtime_session_id.clone();
            }
        } else if runtime_session_id.is_some() {
            run.remote_run_id = runtime_session_id.clone();
        }
        if remote_thread_id.is_some() {
            run.remote_thread_id = remote_thread_id.clone();
        }
        // As for `Resumed`: the in-memory state moves before the store does.
        self.sync_live(None);
        if !self.commit(Some(RunState::Running)).await {
            return Some(Outcome::StoreLost);
        }
        self.running_since = Some(Instant::now());
        tracing::info!(
            run_id = self.id_text().as_str(),
            adapter = enum_text(&self.l.kind).as_str(),
            level = enum_text(&self.l.choice.level).as_str(),
            "sup_run_started"
        );
        if !self.reported {
            self.reported = true;
            self.startup_report().await;
        }
        self.broadcast(RunEvent::Started {
            runtime_session_id,
            remote_thread_id,
        });
        None
    }

    /// E: the start-up report, stored in the Journal and emitted.
    // spec: ordin-desktop/components/SessionSupervisor#SA-RECON-02
    async fn startup_report(&mut self) {
        let run_id = self.run_id();
        let mut items = self
            .sup
            .host
            .startup_items(
                &self.l.row.run,
                &self.l.target,
                self.l.adapter.unsupported_verbs(),
            )
            .await;
        let before = items.len();
        items.retain(|item| !plain_of(item).trim().is_empty());
        if items.len() < before {
            tracing::warn!(
                run_id = run_id.to_string().as_str(),
                code = "START_PLAIN_INVALID",
                dropped = (before - items.len()) as u64,
                "sup_startup_cut"
            );
        }
        if items.len() > MAX_STARTUP_ITEMS {
            tracing::warn!(
                run_id = run_id.to_string().as_str(),
                code = "START_TOO_MANY_ITEMS",
                dropped = (items.len() - MAX_STARTUP_ITEMS) as u64,
                "sup_startup_cut"
            );
            items.truncate(MAX_STARTUP_ITEMS);
        }
        let mut report = StartupReport {
            id: StartupReportId::new(),
            subject: StartupSubject::Run { run_id },
            enforced_rules: vec![],
            rules_not_enforced: vec![],
            unavailable_parts: vec![],
            available_tools: vec![],
            adapter_verbs_not_offered: vec![],
            generated_at: Timestamp::now(),
        };
        for item in &items {
            match item {
                StartUpReportItem::EnforcedRule { plain, .. } => {
                    report.enforced_rules.push(plain.clone())
                }
                StartUpReportItem::RuleNotEnforced { plain, reason, .. } => {
                    report.rules_not_enforced.push(RuleNotEnforced {
                        rule: plain.clone(),
                        reason: enum_text(reason),
                    })
                }
                StartUpReportItem::UnavailablePart { plain, .. } => {
                    report.unavailable_parts.push(plain.clone())
                }
                StartUpReportItem::AvailableTool { tool, .. } => {
                    report.available_tools.push(tool.clone())
                }
                StartUpReportItem::VerbNotOffered { verb, .. } => {
                    report.adapter_verbs_not_offered.push(enum_text(verb))
                }
                StartUpReportItem::OwnScreensNotShown { .. } => {}
            }
        }
        if let Err(e) = self.sup.journal.store_startup_report(report, false).await {
            tracing::warn!(
                run_id = run_id.to_string().as_str(),
                code = ordin_types::CodedError::code(&e),
                "sup_startup_not_kept"
            );
        }
        self.sup.keep_startup(run_id, items.clone());
        let _ = self
            .emit(
                EventKind::StartUpReportReady,
                serde_json::json!({
                    "subject": StartUpSubject::Run { run_id },
                    "items": items,
                }),
            )
            .await;
    }

    async fn on_paused(&mut self, reason: PauseReason) -> Option<Outcome> {
        let (pause, approval) = match &reason {
            PauseReason::Approval { approval_id } => (RunPauseReason::Approval, Some(*approval_id)),
            PauseReason::SignIn => (RunPauseReason::SignIn, None),
            PauseReason::Person { .. } | PauseReason::DecisionElsewhere { .. } => {
                (RunPauseReason::Person, None)
            }
            PauseReason::RemoteApprovers => (RunPauseReason::Approval, None),
        };
        self.l.row.run.state = RunState::Paused;
        self.l.row.run.pause_reason = Some(pause);
        self.l.row.pause_approval_id = approval;
        // Before the row: a reader who sees `paused` can resume at once.
        self.sync_live(Some(reason.clone()));
        if !self.commit(Some(RunState::Paused)).await {
            return Some(Outcome::StoreLost);
        }
        // Time waiting for a person does not count (SA-SUP-01).
        // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-01
        if let Some(since) = self.running_since.take() {
            self.l.remaining = self.l.remaining.saturating_sub(since.elapsed());
        }
        tracing::info!(
            run_id = self.id_text().as_str(),
            reason = enum_text(&pause).as_str(),
            "sup_run_paused"
        );
        self.broadcast(RunEvent::Paused { reason });
        if self.l.queue.is_some() {
            if let Some(approval_id) = approval {
                return Some(Outcome::QueuePaused(approval_id));
            }
        }
        None
    }

    async fn on_usage(
        &mut self,
        usage: ordin_runtime_adapter::UsageDelta,
        stream: &mut BoxStream<'static, Guarded>,
    ) -> Option<Outcome> {
        let counters = &mut self.l.row.run.usage;
        counters.model_turns = counters.model_turns.saturating_add(usage.model_turns);
        counters.tool_calls = counters.tool_calls.saturating_add(usage.tool_calls);
        if usage.input_tokens.is_some() || usage.output_tokens.is_some() {
            let add = usage
                .input_tokens
                .unwrap_or(0)
                .saturating_add(usage.output_tokens.unwrap_or(0));
            counters.tokens = Some(counters.tokens.unwrap_or(0).saturating_add(add));
        }
        if let Some(cached) = usage.cached_input_tokens {
            counters.tokens_from_cache = Some(
                counters
                    .tokens_from_cache
                    .unwrap_or(0)
                    .saturating_add(cached),
            );
        }
        if let (Some(cache), Some(tokens)) = (counters.tokens_from_cache, counters.tokens) {
            counters.tokens_from_cache = Some(cache.min(tokens));
        }
        if let Some(input) = usage.input_tokens {
            self.input_tokens = Some(self.input_tokens.unwrap_or(0).saturating_add(input));
        }
        if let Some(output) = usage.output_tokens {
            self.output_tokens = Some(self.output_tokens.unwrap_or(0).saturating_add(output));
        }
        if !self.commit(None).await {
            return Some(Outcome::StoreLost);
        }
        agent::model_turn_span(self, &usage);
        let (crew_run_id, round) = (self.l.crew_run_id, self.l.round);
        if let Err(e) = self
            .sup
            .usage
            .record_usage(ordin_usage_planner::RunUsage {
                run_id: self.run_id(),
                crew_run_id,
                round,
                level: self.l.choice.level,
                delta: usage,
            })
            .await
        {
            tracing::warn!(
                run_id = self.id_text().as_str(),
                code = e.code(),
                "sup_usage_not_counted"
            );
        }
        self.broadcast(RunEvent::Usage { usage });
        let budget = self.l.row.run.limits.request_budget;
        if budget.is_some_and(|b| self.l.row.run.usage.model_turns >= b) {
            return Some(self.stop_at_limit("request budget", stream).await);
        }
        None
    }

    /// C.3 `Ended`, with the agent profile's output check (J.7.2).
    async fn on_ended(&mut self, end: RunEnd) -> Outcome {
        if self.l.queue.is_some() && end.reason == RunEndReason::Completed {
            if let Some(text) = end.final_message.clone() {
                let check = self
                    .sup
                    .host
                    .check_content(self.run_id(), ContentPoint::Output, &text)
                    .await;
                let Some(q) = self.l.queue.as_mut() else {
                    return Outcome::End(Ending::from_end(end));
                };
                q.checks += 1;
                if let ContentCheck::Blocked {
                    retry_without_context,
                } = check
                {
                    q.checks_blocked += 1;
                    let carried = q.carried_context;
                    let already = q.retried;
                    if retry_without_context
                        && carried
                        && self.sup.host.retry_after_block(carried, already)
                        && !already
                    {
                        return Outcome::Retry;
                    }
                    // The blocked text is never delivered, journalled or logged.
                    return Outcome::End(Ending {
                        safe_reply: true,
                        ..Ending::plain(RunState::Completed, None)
                    });
                }
            }
        }
        Outcome::End(Ending::from_end(end))
    }

    /// C.4: a stream that ends without `Ended`, or yields `Started` twice → `adapter.read` once.
    async fn event_order(&mut self, _stream: Option<&mut BoxStream<'static, Guarded>>) -> Outcome {
        tracing::warn!(run_id = self.id_text().as_str(), "sup_event_order");
        if let Some(handle) = self.handle.clone() {
            let ctx = adapter_ctx(&self.id_text(), &self.token);
            if let Ok(snapshot) = bounded(self.l.adapter.read(&ctx, &handle)).await {
                if ordin_runtime_adapter::check_snapshot(&snapshot).is_ok() {
                    if let Some(end) = snapshot.end {
                        return Outcome::End(Ending::from_end(end));
                    }
                }
            }
        }
        Outcome::End(Ending::failed(
            "the session ended unexpectedly",
            "RA_EVENT_ORDER",
        ))
    }

    /// Writes the row (one retry after 100 ms), then emits `run_state_changed` when `state` is
    /// given (one retry). False when either failed twice (C.5).
    async fn commit(&mut self, state: Option<RunState>) -> bool {
        let saved = match store::save_run(&self.sup.datastore, &self.l.row).await {
            Ok(()) => true,
            Err(_) => {
                tokio::time::sleep(STORE_RETRY).await;
                store::save_run(&self.sup.datastore, &self.l.row)
                    .await
                    .is_ok()
            }
        };
        if !saved {
            return false;
        }
        match state {
            Some(state) => {
                self.emit(
                    EventKind::RunStateChanged,
                    serde_json::json!({ "run_id": self.run_id(), "state": state }),
                )
                .await
            }
            None => true,
        }
    }

    async fn emit(&self, kind: EventKind, data: serde_json::Value) -> bool {
        for attempt in 0..2 {
            if self
                .sup
                .journal
                .append_event(kind.as_str(), data.clone())
                .await
                .is_ok()
            {
                return true;
            }
            if attempt == 0 {
                tokio::time::sleep(STORE_RETRY).await;
            }
        }
        false
    }

    /// Sends `event` to every subscriber's channel and keeps the current pause, under the state
    /// lock so that `subscribe` sees each event either as sent or as not yet sent.
    // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-PAUSE-01
    pub(crate) fn broadcast(&self, event: RunEvent) {
        let mut state = self.sup.lock();
        let Some(live) = state.live.get_mut(&self.run_id()) else {
            return;
        };
        match &event {
            RunEvent::Paused { reason } => live.pause_sent = Some(reason.clone()),
            RunEvent::Resumed | RunEvent::Started { .. } | RunEvent::Ended { .. } => {
                live.pause_sent = None
            }
            RunEvent::Progress { .. } | RunEvent::HarnessPlan { .. } | RunEvent::Usage { .. } => {}
        }
        live.subscribers
            .retain(|sender| sender.receiver_count() > 0);
        for sender in &live.subscribers {
            let _ = sender.send(event.clone());
        }
    }

    /// Copies the run's state and pause into its `Live` entry.
    pub(crate) fn sync_live(&self, paused_on: Option<PauseReason>) {
        if let Some(live) = self.sup.lock().live.get_mut(&self.run_id()) {
            live.state = self.l.row.run.state;
            live.paused_at = paused_on.as_ref().map(|_| Instant::now());
            live.paused_on = paused_on;
        }
    }

    /// Records the end (C.3 `Ended`, D, I, J.6), in the spec's order.
    async fn finish(&mut self, ending: Ending) {
        let run_id = self.run_id();
        let run = &mut self.l.row.run;
        run.state = ending.state;
        run.pause_reason = None;
        self.l.row.pause_approval_id = None;
        run.ended_at = Some(Timestamp::now());
        run.end_reason = ending.end_reason.as_deref().map(|r| cut(r, MAX_END_REASON));
        if let Some(code) = &ending.code {
            journal_code(&self.sup, run_id, code).await;
        }
        if let Some(result) = &ending.result {
            journal_result(&self.sup, run_id, result).await;
        }
        if !self.commit(Some(ending.state)).await {
            self.store_lost().await;
            return;
        }
        let turns = self.l.row.run.usage.model_turns;
        if let Err(e) = self
            .sup
            .usage
            .record_run_finished(&self.l.row.work_key, run_id, turns)
            .await
        {
            tracing::warn!(
                run_id = self.id_text().as_str(),
                code = e.code(),
                "sup_usage_not_counted"
            );
        }
        self.sup.ended_log(&self.l.row, ending.code.as_deref());
        // C.3: the event is broadcast as the adapter gave it, final message included (SUP-END-1). It
        // was sent with no message, so a subscriber (the Ordin Host's card and answer index) had none.
        // A reply the output check blocked has no `result`, so it is never broadcast.
        // spec: ordin-desktop/components/SessionSupervisor#SUP-END-1
        self.broadcast(RunEvent::Ended {
            end: RunEnd {
                reason: end_reason_of(ending.state),
                final_message: ending.result.as_ref().and_then(|r| r.final_message.clone()),
                artefacts: ending
                    .result
                    .as_ref()
                    .map(|r| r.artefacts.clone())
                    .unwrap_or_default(),
            },
        });
        self.sup.host.run_ended(run_id).await;
        if self.l.queue.is_some() {
            agent::end_record(self, &ending).await;
        }
        self.sup.forget(run_id);
    }
}

fn control_ending(control: String) -> Ending {
    Ending {
        code: Some("SUP_RESUME_REFUSED".into()),
        control: Some(control.clone()),
        ..Ending::plain(
            RunState::Completed,
            Some(format!("ended by the control {control}")),
        )
    }
}

fn plain_of(item: &StartUpReportItem) -> &str {
    match item {
        StartUpReportItem::EnforcedRule { plain, .. }
        | StartUpReportItem::RuleNotEnforced { plain, .. }
        | StartUpReportItem::UnavailablePart { plain, .. }
        | StartUpReportItem::AvailableTool { plain, .. }
        | StartUpReportItem::VerbNotOffered { plain, .. }
        | StartUpReportItem::OwnScreensNotShown { plain, .. } => plain,
    }
}

/// Reads the stream up to `deadline` for the run's `Ended`.
async fn wait_for_end(
    stream: &mut BoxStream<'static, Guarded>,
    deadline: Duration,
) -> Option<RunEnd> {
    let wait = async {
        while let Some(item) = stream.next().await {
            if let Guarded::Event(RunEvent::Ended { end }) = item {
                return Some(end);
            }
        }
        None
    };
    tokio::time::timeout(deadline, wait).await.ok().flatten()
}
