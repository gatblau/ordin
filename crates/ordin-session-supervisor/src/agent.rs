//! The agent profile (SessionSupervisor.md *Internal Logic* H, J.6, J.7): workers taking records
//! from the Run Queue with a lease, the lease keeper, the pause handed to the Run Queue (H.3), the
//! resume on whichever instance takes the record again (H.4), the run summary of each stretch
//! (H.2), the `ordin.run_stretch` and `ordin.model_turn` spans, and the counters of the profile.

use std::sync::Arc;
use std::time::Duration;

use ordin_run_queue::{
    Delivery, LeaseRenewal, PendingItem, PendingKind, RunEnd as QueueRunEnd, RunOutcome, RunPause,
    RunQueue, RunQueueError, TakenRun,
};
use ordin_runtime_adapter::{ResumeDecision, RunTarget, StartInput, ToolCaller, UsageDelta};
use ordin_types::domain::agent_profile::{
    CallPlace, ModelFailureCause, RunEndKind, RunRecord, RunRequest, RunStarter,
};
use ordin_types::domain::runs::{ExternalAction, ExternalActionKind, RunOrigin, RunState};
use ordin_types::{AgentInstanceId, ApprovalId, RunId, TaskId, WorkspaceId};
use ordin_usage_planner::WorkKind;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::run::{journal_code, Ending, RunTask};
use crate::store::{self, enum_text};
use crate::supervisor::{handle_of, Control, Extra, SessionSupervisor};
use crate::types::{RunControls, StartRequest};

/// A worker with a free place takes at most one record a second (wrightrun SA-77).
const TAKE_PAUSE: Duration = Duration::from_secs(1);
/// The lease is renewed every 10 s (2D-4 *Run lease*: 30 s, renewed every 10 s).
const RENEW_EVERY: Duration = Duration::from_secs(10);
/// C-80 fallback: a record this instance may not resume is queued again after this pause.
const REQUEUE_AFTER: Duration = Duration::from_secs(5);
/// Lists in a run summary are cut to this many entries (H.2).
const MAX_SUMMARY_LIST: usize = 200;

/// What a run taken from the Run Queue carries beside its `Launch` (H.1).
pub(crate) struct QueueRun {
    pub queue: Arc<dyn RunQueue>,
    pub record: RunRecord,
    /// `ordin.run_stretch` for this stretch of the run.
    pub span: tracing::Span,
    pub stretch_started: Instant,
    /// True when this stretch follows a pause (H.4).
    pub resumed: bool,
    /// True once J.7.2 ran the record again without its gathered context.
    pub retried: bool,
    /// The record carried gathered context (J.7.2).
    pub carried_context: bool,
    /// Content checks asked for in the stretch, and of them those that blocked (ADR-107).
    pub checks: u32,
    pub checks_blocked: u32,
}

impl QueueRun {
    fn new(queue: Arc<dyn RunQueue>, record: RunRecord, run_id: RunId, resumed: bool) -> QueueRun {
        let span = tracing::info_span!(
            "ordin.run_stretch",
            run_id = run_id.to_string().as_str(),
            started_by = starter_kind(&record.started_by),
            command = command_name(&record.request),
            outcome = tracing::field::Empty,
        );
        let carried_context = has_context(&record.gathered_context);
        QueueRun {
            queue,
            record,
            span,
            stretch_started: Instant::now(),
            resumed,
            retried: false,
            carried_context,
            checks: 0,
            checks_blocked: 0,
        }
    }
}

fn starter_kind(starter: &RunStarter) -> &'static str {
    match starter {
        RunStarter::Caller { .. } => "caller",
        RunStarter::ScheduleEntry { .. } => "schedule",
        RunStarter::Intake { .. } => "intake",
        RunStarter::ChatConnection { .. } => "chat",
    }
}

/// The command a record runs; `none` for a message (the Run Queue's own label).
fn command_name(request: &RunRequest) -> &str {
    match request {
        RunRequest::Command { command, .. } => command,
        RunRequest::Message { .. } => "none",
    }
}

fn has_context(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Object(map) => !map.is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        _ => true,
    }
}

// ---- H.1 Workers

/// One worker: take a record, run it to its end or its pause, take the next.
pub(crate) async fn worker(sup: Arc<SessionSupervisor>) {
    let Some(queue) = sup.run_queue.clone() else {
        return;
    };
    loop {
        let taken = {
            let _gate = sup.taking.read().await;
            if sup.not_taking.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            queue.take(&sup.config.instance_name).await
        };
        match taken {
            Ok(Some(taken)) => {
                sup.lock().executing += 1;
                execute(&sup, &queue, taken).await;
                {
                    let mut state = sup.lock();
                    state.executing = state.executing.saturating_sub(1);
                }
                sup.changed.notify_waiters();
            }
            Ok(None) => tokio::time::sleep(TAKE_PAUSE).await,
            Err(e) => {
                tracing::warn!(code = e.code(), "sup_take_failed");
                tokio::time::sleep(TAKE_PAUSE).await;
            }
        }
    }
}

/// Ends a record that never reached a run task (H.1: malformed id, expansion error, refused start).
async fn end_untouched(
    queue: &Arc<dyn RunQueue>,
    run_id: &str,
    instance: &str,
    code: Option<String>,
) {
    let end = QueueRunEnd {
        end_kind: RunEndKind::Failed,
        outcome: RunOutcome {
            error_code: code,
            ..RunOutcome::default()
        },
        stretch_summary: None,
    };
    if let Err(e) = queue.end(run_id, instance, end).await {
        tracing::warn!(run_id, code = e.code(), "sup_record_not_ended");
    }
}

/// H.1 for one taken record, up to the end of its stretch.
async fn execute(sup: &Arc<SessionSupervisor>, queue: &Arc<dyn RunQueue>, taken: TakenRun) {
    let instance = sup.config.instance_name.clone();
    let raw_id = taken.view.record.run_id.clone();
    let Ok(run_id) = RunId::parse(&raw_id) else {
        tracing::warn!(run_id = raw_id.as_str(), "sup_record_invalid");
        end_untouched(queue, &raw_id, &instance, None).await;
        return;
    };
    let lease = CancellationToken::new();
    tokio::spawn(lease_keeper(
        sup.clone(),
        queue.clone(),
        run_id,
        lease.clone(),
    ));
    let row = store::get_run(&sup.datastore, run_id).await;
    match row {
        Ok(Some(row)) if row.run.state == RunState::Paused => {
            resume_taken(sup, queue, taken, row).await;
        }
        Ok(Some(_)) => {
            // A row that is not paused under this id: the record cannot be run again here.
            tracing::warn!(run_id = raw_id.as_str(), "sup_record_invalid");
            end_untouched(queue, &raw_id, &instance, None).await;
        }
        Ok(None) => start_taken(sup, queue, taken, run_id).await,
        Err(_) => {
            tracing::error!(
                run_id = raw_id.as_str(),
                code = "SUP_STORE_UNAVAILABLE",
                "sup_store_unavailable"
            );
            end_untouched(
                queue,
                &raw_id,
                &instance,
                Some("SUP_STORE_UNAVAILABLE".into()),
            )
            .await;
        }
    }
    lease.cancel();
}

/// H.1: renew every 10 s while the record executes here.
async fn lease_keeper(
    sup: Arc<SessionSupervisor>,
    queue: Arc<dyn RunQueue>,
    run_id: RunId,
    stop: CancellationToken,
) {
    let id = run_id.to_string();
    loop {
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = tokio::time::sleep(RENEW_EVERY) => {}
        }
        match queue.renew(&id, &sup.config.instance_name).await {
            Ok(LeaseRenewal::Kept) => {}
            Ok(LeaseRenewal::CancelRequested) => {
                // D.4: the Caller API's cancel path.
                sup.send(run_id, Control::QueueCancel);
            }
            Ok(LeaseRenewal::Lost) | Err(RunQueueError::LeaseLost) => {
                // ADR-098 item 3: cancel the harness, record interrupted, never start it again.
                sup.send(run_id, Control::LeaseLost);
                return;
            }
            Err(e) => tracing::warn!(
                run_id = id.as_str(),
                code = e.code(),
                "sup_lease_not_renewed"
            ),
        }
    }
}

/// See `start_taken`: the identifiers an agent-profile run is recorded under.
fn record_ids(record: &RunRecord, run_id: RunId) -> (RunOrigin, WorkspaceId, RunTarget) {
    (
        RunOrigin::Task {
            task_id: TaskId(*run_id.as_uuid()),
        },
        WorkspaceId(*record.scope_id.as_uuid()),
        RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId(*run_id.as_uuid()),
        },
    )
}

/// H.1: expand the request, then B.2–B.4 and C.
async fn start_taken(
    sup: &Arc<SessionSupervisor>,
    queue: &Arc<dyn RunQueue>,
    taken: TakenRun,
    run_id: RunId,
) {
    let instance = sup.config.instance_name.clone();
    let record = taken.view.record;
    // An agent-profile run has no task, no workspace and no agent instance, yet the
    // `runs` row and `StartRun` need them. The run's own UUID stands for the origin's task and the
    // agent instance, the scope's UUID for the workspace; the work kind is `Talking` for a
    // message and `Routine` for a command.
    let (origin, workspace_id, target) = record_ids(&record, run_id);
    let (text, work_kind) = match &record.request {
        RunRequest::Command { command, arguments } => {
            // SUP-TF-1: the same rendering as a task's, which `expand_command` puts in place of
            // `${input:arguments}` (AgentLoader.md AL-TF-1).
            let joined = crate::supervisor::render_arguments(arguments);
            match sup.host.expand_command(&target, command, &joined).await {
                Ok(expanded) => (expanded, WorkKind::Routine),
                Err(e) => {
                    // The Agent Loader's code, passed through by the host.
                    end_untouched(queue, &record.run_id, &instance, Some(e.code)).await;
                    return;
                }
            }
        }
        RunRequest::Message { text } => (text.clone(), WorkKind::Talking),
    };
    let work_key = format!("agent:{}", command_name(&record.request));
    let scope_id = record.scope_id;
    let q = QueueRun::new(queue.clone(), record, run_id, false);
    let req = StartRequest {
        origin,
        workspace_id,
        target,
        input: StartInput::FreeText { text },
        attachments: vec![],
        limits: None,
        work_kind,
        work_key,
        inputs_used: vec![],
        thread: None,
        started_by_person: false,
        model_override: None,
        coding_task_id: None,
        only_servers: None,
    };
    let extra = Extra {
        caller: Some(ToolCaller::HostedAgent),
        scope_id: Some(scope_id),
        queue: Some(q),
        run_id: Some(run_id),
    };
    match sup.prepare(req, extra).await {
        Ok((_, launch)) => {
            sup.admit(launch);
            wait(done_of(sup, run_id)).await;
        }
        Err(e) => end_untouched(queue, &run_id.to_string(), &instance, Some(e.code().into())).await,
    }
}

/// The run's end signal; None when the run has already been forgotten.
fn done_of(sup: &SessionSupervisor, run_id: RunId) -> Option<tokio::sync::watch::Receiver<bool>> {
    sup.lock().live.get(&run_id).map(|l| l.done.subscribe())
}

async fn wait(done: Option<tokio::sync::watch::Receiver<bool>>) {
    if let Some(mut done) = done {
        let _ = done.wait_for(|d| *d).await;
    }
}

/// The decision the Run Queue keeps for a resumed record: `{"approval_id", "state"}`.
fn decision_of(value: &serde_json::Value) -> Option<ResumeDecision> {
    let approval_id = value
        .get("approval_id")
        .and_then(|v| v.as_str())
        .and_then(|t| ApprovalId::parse(t).ok())?;
    match value.get("state").and_then(|v| v.as_str()) {
        Some("approved") => Some(ResumeDecision::Approve { approval_id }),
        Some("rejected") => Some(ResumeDecision::Reject {
            approval_id,
            reason: None,
        }),
        _ => None,
    }
}

/// H.4: a record taken again after its approval was decided.
async fn resume_taken(
    sup: &Arc<SessionSupervisor>,
    queue: &Arc<dyn RunQueue>,
    taken: TakenRun,
    row: store::RunRow,
) {
    let instance = sup.config.instance_name.clone();
    let run_id = row.run.id;
    let id = run_id.to_string();
    // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-02
    // A taken record that carries the session record resumes anywhere; one
    // without it resumes only on the instance that paused it, and any other instance queues it
    // again after 5 s (a pause naming the same approval, then the same decision).
    let paused_here = sup.lock().paused_here.contains(&run_id);
    if taken.session_record.is_none() && !paused_here {
        tokio::time::sleep(REQUEUE_AFTER).await;
        let outcome = RunOutcome {
            pending: row
                .pause_approval_id
                .map(|approval_id| vec![approval_item(approval_id)])
                .unwrap_or_default(),
            ..RunOutcome::default()
        };
        let paused = queue
            .pause(
                &id,
                &instance,
                RunPause {
                    outcome,
                    session_record: vec![],
                    stretch_summary: None,
                },
            )
            .await;
        let resumed = match (paused, taken.decision.clone()) {
            (Ok(()), Some(decision)) => queue.resume(&id, decision).await.map(|_| ()),
            (Ok(()), None) => Ok(()),
            (Err(e), _) => Err(e),
        };
        if let Err(e) = resumed {
            tracing::warn!(
                run_id = id.as_str(),
                code = e.code(),
                "sup_record_not_requeued"
            );
        }
        return;
    }
    if let Some(record) = taken.session_record.clone() {
        sup.lock().taken_records.insert(run_id, record);
    }
    let q = QueueRun::new(queue.clone(), taken.view.record.clone(), run_id, true);
    let Some(decision) = taken.decision.as_ref().and_then(decision_of) else {
        tracing::warn!(run_id = id.as_str(), "sup_record_invalid");
        if let Err(e) = sup
            .record_end_outside(
                row,
                RunState::Failed,
                Some("the session ended unexpectedly".into()),
            )
            .await
        {
            end_not_recorded(id.as_str(), e.code());
        }
        end_untouched(queue, &id, &instance, None).await;
        return;
    };
    // J.6: the duty lock may refuse the resume.
    if let Some(control) = sup.host.run_resuming(run_id).await {
        journal_code(sup, run_id, "SUP_RESUME_REFUSED").await;
        let limit_seconds = row.run.limits.wall_clock_secs;
        if let Err(e) = sup
            .record_end_outside(
                row,
                RunState::Completed,
                Some(format!("ended by the control {control}")),
            )
            .await
        {
            end_not_recorded(id.as_str(), e.code());
        }
        let controls = sup.host.run_controls(run_id).await;
        let actions = read_actions(sup, run_id).await;
        let delivery = delivery_of(&actions, &q.record.thread_id);
        let ending = Some(control.clone());
        let summary = summary(
            &q,
            run_id,
            &Stretch {
                state: "ended-by-control",
                ending: ending.clone(),
                error_code: None,
                safe_reply: false,
                limit_seconds,
                model: String::new(),
                input_tokens: None,
                output_tokens: None,
                actions: Some(&actions),
            },
            &controls,
        );
        q.span.record("outcome", "ended-by-control");
        let end = QueueRunEnd {
            end_kind: RunEndKind::Control,
            outcome: RunOutcome {
                ending,
                actions_attached: kinds(&actions),
                delivery,
                ended_by_control: true,
                ..RunOutcome::default()
            },
            stretch_summary: Some(summary),
        };
        if let Err(e) = queue.end(&id, &instance, end).await {
            tracing::warn!(
                run_id = id.as_str(),
                code = e.code(),
                "sup_record_not_ended"
            );
        }
        return;
    }
    let Some(handle) = handle_of(&row) else {
        if let Err(e) = sup
            .record_end_outside(row, RunState::Interrupted, None)
            .await
        {
            end_not_recorded(id.as_str(), e.code());
        }
        return;
    };
    let Some(mut launch) = sup.reattach_launch(row, handle, Some(decision)) else {
        end_untouched(queue, &id, &instance, Some("SUP_NO_ADAPTER".into())).await;
        return;
    };
    launch.caller = ToolCaller::HostedAgent;
    launch.scope_id = Some(q.record.scope_id);
    launch.queue = Some(q);
    sup.admit_reattached(launch);
    wait(done_of(sup, run_id)).await;
}

fn approval_item(approval_id: ApprovalId) -> PendingItem {
    PendingItem {
        kind: PendingKind::ToolCall,
        tool: None,
        gate: None,
        reason: "approval-list".into(),
        approval_id: Some(approval_id),
    }
}

// ---- C.3 hooks of the run task

/// C.3 `Progress`, agent profile: the line reaches the Caller API's event stream.
pub(crate) async fn append_progress(task: &RunTask, line: &str) {
    let Some(q) = task.l.queue.as_ref() else {
        return;
    };
    let run_id = task.l.row.run.id.to_string();
    match q
        .queue
        .append_progress(&run_id, &task.sup.config.instance_name, line)
        .await
    {
        Ok(_) => {}
        Err(e) => tracing::warn!(
            run_id = run_id.as_str(),
            code = e.code(),
            "sup_progress_not_kept"
        ),
    }
}

/// `ordin.model_turn`, one per assistant turn the harness reports, inside the stretch's span.
pub(crate) fn model_turn_span(task: &RunTask, usage: &UsageDelta) {
    let Some(q) = task.l.queue.as_ref() else {
        return;
    };
    if usage.model_turns == 0 {
        return;
    }
    let model = model_of(task);
    let span = tracing::info_span!(
        parent: &q.span,
        "ordin.model_turn",
        model = model.as_str(),
        input_tokens = usage.input_tokens.unwrap_or(0),
        output_tokens = usage.output_tokens.unwrap_or(0),
        outcome = "ok",
    );
    drop(span);
}

fn model_of(task: &RunTask) -> String {
    task.l
        .choice
        .model
        .clone()
        .unwrap_or_else(|| enum_text(&task.l.choice.level))
}

/// H.3: the harness paused for an approval; the stretch ends and the Run Queue keeps the record.
pub(crate) async fn after_pause(task: &mut RunTask, approval_id: ApprovalId) {
    let run_id = task.l.row.run.id;
    let id = run_id.to_string();
    let Some(q) = task.l.queue.take() else {
        return;
    };
    let session_record = task
        .sup
        .lock()
        .session_records
        .remove(&run_id)
        .map(|(_, bytes)| bytes)
        .unwrap_or_default();
    let controls = task.sup.host.run_controls(run_id).await;
    let text = summary(
        &q,
        run_id,
        &Stretch {
            state: "paused",
            ending: controls.ending.clone(),
            error_code: None,
            safe_reply: false,
            limit_seconds: task.l.row.run.limits.wall_clock_secs,
            model: model_of(task),
            input_tokens: task.input_tokens,
            output_tokens: task.output_tokens,
            actions: None,
        },
        &controls,
    );
    q.span.record("outcome", "paused");
    let pause = RunPause {
        outcome: RunOutcome {
            pending: vec![approval_item(approval_id)],
            ending: controls.ending.clone(),
            ..RunOutcome::default()
        },
        session_record,
        stretch_summary: Some(text),
    };
    if let Err(e) = q
        .queue
        .pause(&id, &task.sup.config.instance_name, pause)
        .await
    {
        tracing::warn!(
            run_id = id.as_str(),
            code = e.code(),
            "sup_record_not_paused"
        );
    }
    // The harness has ended; the run is known again only when a worker takes the record.
    task.sup.forget(run_id);
    task.sup.lock().paused_here.insert(run_id);
}

/// H.1, H.2, J.6, J.7: the end of a run taken from the Run Queue.
pub(crate) async fn end_record(task: &mut RunTask, ending: &Ending) {
    let run_id = task.l.row.run.id;
    let id = run_id.to_string();
    let Some(q) = task.l.queue.take() else {
        return;
    };
    let instance = task.sup.config.instance_name.clone();
    let controls = task.sup.host.run_controls(run_id).await;
    let actions = read_actions(&task.sup, run_id).await;
    let lease_lost = ending.code.as_deref() == Some("SUP_LEASE_LOST");
    let (end_kind, state, error_code) = if ending.control.is_some() {
        (RunEndKind::Control, "ended-by-control", None)
    } else if ending.limit {
        task.sup.count("wrightrun_runs_ended_at_limit_total", &[]);
        (
            RunEndKind::RunLimit,
            "ended-at-limit",
            Some("RUN_LIMIT".to_owned()),
        )
    } else {
        match ending.state {
            RunState::Completed => (RunEndKind::Completed, "finished", None),
            // wrightrun's states have no `cancelled`: a stopped run is reported `interrupted`.
            RunState::Interrupted | RunState::Cancelled => {
                (RunEndKind::Failed, "interrupted", ending.code.clone())
            }
            _ => (RunEndKind::Failed, "failed", ending.code.clone()),
        }
    };
    // J.7: a blocked question or answer ends with the Model Policy's safe reply.
    // SPEC-GAP-8: `RunEnd` carries no failure cause, so the model-failure reply of J.6 is never
    // asked for here; only the content-filter reply is.
    let (reply, safe_reply) = if ending.safe_reply {
        task.sup.count(
            "wrightrun_safe_replies_total",
            &[("reason", "content-filter")],
        );
        (
            task.sup
                .host
                .reply_for(ModelFailureCause::ContentFilter, CallPlace::Agent),
            true,
        )
    } else {
        (
            ending.result.as_ref().and_then(|r| r.final_message.clone()),
            false,
        )
    };
    let ending_name = ending.control.clone().or_else(|| controls.ending.clone());
    let text = summary(
        &q,
        run_id,
        &Stretch {
            state,
            ending: ending_name.clone(),
            error_code: error_code.clone(),
            safe_reply,
            limit_seconds: task.l.row.run.limits.wall_clock_secs,
            model: model_of(task),
            input_tokens: task.input_tokens,
            output_tokens: task.output_tokens,
            actions: Some(&actions),
        },
        &controls,
    );
    q.span.record("outcome", state);
    let outcome = RunOutcome {
        reply,
        ending: ending_name,
        actions_attached: kinds(&actions),
        delivery: delivery_of(&actions, &q.record.thread_id),
        error_code,
        safe_reply,
        ended_by_control: ending.control.is_some(),
        ..RunOutcome::default()
    };
    if lease_lost || (ending.state == RunState::Interrupted && !ending.queue_cancelled) {
        // H.1, H.5: the lease is gone or lapses; another instance's `mark_interrupted` records it.
        return;
    }
    let written = if ending.state == RunState::Cancelled {
        q.queue.record_cancelled(&id, &instance, outcome).await
    } else {
        q.queue
            .end(
                &id,
                &instance,
                QueueRunEnd {
                    end_kind,
                    outcome,
                    stretch_summary: Some(text),
                },
            )
            .await
    };
    if let Err(e) = written {
        tracing::warn!(
            run_id = id.as_str(),
            code = e.code(),
            "sup_record_not_ended"
        );
    }
}

// ---- H.2 Run summary

async fn read_actions(sup: &SessionSupervisor, run_id: RunId) -> Vec<ExternalAction> {
    match sup.journal.external_actions_for_run(run_id).await {
        // Already in `sent_at` order (Journal).
        Ok(actions) => actions,
        Err(e) => {
            tracing::warn!(
                run_id = run_id.to_string().as_str(),
                code = ordin_types::CodedError::code(&e),
                "sup_summary_actions_unread"
            );
            vec![]
        }
    }
}

fn kinds(actions: &[ExternalAction]) -> Vec<String> {
    actions
        .iter()
        .take(MAX_SUMMARY_LIST)
        .map(|a| enum_text(&a.kind))
        .collect()
}

fn latest_post(actions: &[ExternalAction]) -> Option<&ExternalAction> {
    actions.iter().rev().find(|a| {
        matches!(
            a.kind,
            ExternalActionKind::SlackPost | ExternalActionKind::TelegramPost
        )
    })
}

/// `delivery` is `thread` when the latest post went to the record's thread (its
/// destination equals the record's `thread_id`), else `asker`; absent with no post.
fn delivery_of(actions: &[ExternalAction], thread_id: &str) -> Option<Delivery> {
    latest_post(actions).map(|post| {
        if post.destination == thread_id {
            Delivery::Thread
        } else {
            Delivery::Asker
        }
    })
}

/// What one stretch ended with, beside the record and the host's control facts.
pub(crate) struct Stretch<'a> {
    pub state: &'static str,
    pub ending: Option<String>,
    pub error_code: Option<String>,
    pub safe_reply: bool,
    pub limit_seconds: u32,
    pub model: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// The run's external actions, at the end of the run only (not at a pause).
    pub actions: Option<&'a [ExternalAction]>,
}

fn cut_list<T>(mut items: Vec<T>) -> Vec<T> {
    items.truncate(MAX_SUMMARY_LIST);
    items
}

/// One entry of the summary's `context` (ADR-107; Logging.md S-1a).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct ContextStep {
    pub step: u64,
    pub kind: String,
    pub outcome: String,
    pub items: u64,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_bytes: Option<u64>,
}

/// The step kind of a piece (ChannelGateway.md `ContextItem.piece`).
fn step_kind(piece: &str) -> &str {
    match piece {
        "transcript" => "transcript",
        "image" => "images",
        "document" => "channel-documents",
        "knowledge" => "knowledge-lookup",
        "recent" => "recent-messages",
        "note" => "precedence-note",
        other => other,
    }
}

/// `context`, `context_cut` and `context_left_out` from a record's gathered context.
// The `context` member is read, or the value itself when it is an array; one step may hold several
// items, which are added together.
pub(crate) fn context_facts(
    gathered: &serde_json::Value,
) -> (Vec<ContextStep>, Vec<String>, Vec<(String, String)>) {
    let items = match gathered {
        serde_json::Value::Array(items) => items.as_slice(),
        serde_json::Value::Object(map) => match map.get("context") {
            Some(serde_json::Value::Array(items)) => items.as_slice(),
            _ => &[],
        },
        _ => &[],
    };
    let mut steps: Vec<ContextStep> = Vec::new();
    let mut cut = Vec::new();
    let mut left_out = Vec::new();
    for item in items {
        let text = |key: &str| {
            item.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned()
        };
        let number = |key: &str| item.get(key).and_then(|v| v.as_u64());
        let piece = text("piece");
        let label = text("label");
        let is_piece = item.get("kind").and_then(|v| v.as_str()) == Some("piece");
        if is_piece && item.get("cut").and_then(|v| v.as_bool()).unwrap_or(false) {
            cut.push(label.clone());
        }
        if !is_piece {
            left_out.push((label.clone(), text("reason")));
        }
        let Some(step) = number("step") else {
            continue;
        };
        let at = match steps.iter().position(|s| s.step == step) {
            Some(at) => at,
            None => {
                steps.push(ContextStep {
                    step,
                    kind: step_kind(&piece).to_owned(),
                    outcome: String::new(),
                    items: 0,
                    bytes: 0,
                    observed_bytes: None,
                });
                steps.len() - 1
            }
        };
        let entry = &mut steps[at];
        if let Some(observed) = number("observed_bytes") {
            entry.observed_bytes = Some(entry.observed_bytes.unwrap_or(0) + observed);
        }
        if is_piece {
            entry.items += number("items").unwrap_or(0);
            entry.bytes += number("size_bytes").unwrap_or(0);
            let was_cut = item.get("cut").and_then(|v| v.as_bool()).unwrap_or(false);
            if was_cut || entry.outcome == "cut" {
                entry.outcome = "cut".into();
            } else {
                entry.outcome = "piece".into();
            }
        } else if entry.outcome.is_empty() {
            entry.outcome = match text("reason").as_str() {
                "empty" => "empty".into(),
                "skipped" => "skipped".into(),
                _ => "left-out".into(),
            };
        }
    }
    steps.sort_by_key(|s| s.step);
    (cut_list(steps), cut_list(cut), cut_list(left_out))
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// H.2: writes event `run_summary` and returns the same text for the Run Queue's stretch summary.
/// Never a message, an argument or an asker's identifier: `started_by` is the starter's kind,
/// `connection` the chat connection's name, `command` the command's name.
pub(crate) fn summary(
    q: &QueueRun,
    run_id: RunId,
    s: &Stretch<'_>,
    controls: &RunControls,
) -> String {
    let record = &q.record;
    let connection = match &record.started_by {
        RunStarter::ChatConnection { connection, .. } => Some(connection.clone()),
        _ => None,
    };
    let (context, context_cut, context_left_out) = if q.retried {
        (vec![], vec![], vec![])
    } else {
        context_facts(&record.gathered_context)
    };
    let mut tokens = serde_json::Map::new();
    if s.input_tokens.is_some() || s.output_tokens.is_some() {
        let mut pair = serde_json::Map::new();
        if let Some(input) = s.input_tokens {
            pair.insert("input".into(), input.into());
        }
        if let Some(output) = s.output_tokens {
            pair.insert("output".into(), output.into());
        }
        tokens.insert(s.model.clone(), serde_json::Value::Object(pair));
    }
    let (actions_attached, delivery, controls_decided) = match s.actions {
        Some(actions) => (
            kinds(actions),
            latest_post(actions).map(|p| p.destination.clone()),
            actions
                .iter()
                .take(MAX_SUMMARY_LIST)
                .map(|a| (enum_text(&a.kind), enum_text(&a.state)))
                .collect::<Vec<_>>(),
        ),
        None => (vec![], None, vec![]),
    };
    let elapsed_seconds = q.stretch_started.elapsed().as_secs();
    let text = serde_json::json!({
        "run_id": run_id,
        "thread_id": record.thread_id,
        "started_by": starter_kind(&record.started_by),
        "connection": connection,
        "scope": record.scope_id,
        "command": command_name(&record.request),
        "tools": cut_list(controls.tools.clone()),
        "tool_calls": cut_list(controls.tool_calls.clone()),
        "tokens": tokens,
        "used_fallback": false,
        "context": context,
        "context_cut": context_cut,
        "context_left_out": context_left_out,
        "retried_without_context": q.retried,
        "controls": cut_list(controls.controls.clone()),
        "gates_set_aside": cut_list(controls.gates_set_aside.clone()),
        "actions_left_out": [],
        "argument_rules": cut_list(controls.argument_rules.clone()),
        "events": [],
        "state": s.state,
        "ending": s.ending,
        "error_code": s.error_code,
        "safe_reply": s.safe_reply,
        "resumed": q.resumed,
        "elapsed_seconds": elapsed_seconds,
        "limit_seconds": s.limit_seconds,
        "actions_attached": actions_attached,
        "delivery": delivery,
        "controls_decided": controls_decided,
        "checks": q.checks,
        "checks_blocked": q.checks_blocked,
    })
    .to_string();
    tracing::info!(
        run_id = run_id.to_string().as_str(),
        thread_id = record.thread_id.as_str(),
        started_by = starter_kind(&record.started_by),
        connection = connection.as_deref(),
        scope = record.scope_id.to_string().as_str(),
        command = command_name(&record.request),
        tools = json(&cut_list(controls.tools.clone())).as_str(),
        tool_calls = json(&cut_list(controls.tool_calls.clone())).as_str(),
        tokens = json(&tokens).as_str(),
        used_fallback = false,
        context = json(&context).as_str(),
        context_cut = json(&context_cut).as_str(),
        context_left_out = json(&context_left_out).as_str(),
        retried_without_context = q.retried,
        controls = json(&cut_list(controls.controls.clone())).as_str(),
        gates_set_aside = json(&cut_list(controls.gates_set_aside.clone())).as_str(),
        actions_left_out = "[]",
        argument_rules = json(&cut_list(controls.argument_rules.clone())).as_str(),
        events = "[]",
        state = s.state,
        ending = s.ending.as_deref(),
        error_code = s.error_code.as_deref(),
        safe_reply = s.safe_reply,
        resumed = q.resumed,
        elapsed_seconds,
        limit_seconds = s.limit_seconds,
        actions_attached = json(&actions_attached).as_str(),
        delivery = delivery.as_deref(),
        controls_decided = json(&controls_decided).as_str(),
        checks = q.checks,
        checks_blocked = q.checks_blocked,
        "run_summary"
    );
    text
}

/// A queued run's end could not be written outside its task; `recover` reconciles the row.
pub(crate) fn end_not_recorded(run_id: &str, code: &str) {
    tracing::warn!(run_id, code, "sup_end_not_recorded");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_steps_follow_step_order_and_count_empty_steps() {
        let gathered = serde_json::json!({ "context": [
            { "kind": "left_out", "piece": "knowledge", "label": "faq", "reason": "empty", "step": 2 },
            { "kind": "piece", "piece": "transcript", "label": "thread", "media_type": "text/plain",
              "content": "…", "size_bytes": 900, "cut": true, "step": 1, "items": 12, "observed_bytes": 2000 },
        ]});
        let (steps, cut, left_out) = context_facts(&gathered);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, "transcript");
        assert_eq!(steps[0].outcome, "cut");
        assert_eq!(steps[0].items, 12);
        assert_eq!(steps[0].observed_bytes, Some(2000));
        assert_eq!(steps[1].kind, "knowledge-lookup");
        assert_eq!(steps[1].outcome, "empty");
        assert_eq!(steps[1].items, 0);
        assert_eq!(cut, vec!["thread".to_owned()]);
        assert_eq!(left_out, vec![("faq".to_owned(), "empty".to_owned())]);
        assert_eq!(context_facts(&serde_json::json!({})).0, vec![]);
        assert_eq!(context_facts(&serde_json::json!([])).0, vec![]);
    }

    #[test]
    fn decisions_are_read_from_the_queue_value() {
        let id = ApprovalId::new();
        assert!(matches!(
            decision_of(&serde_json::json!({ "approval_id": id, "state": "approved" })),
            Some(ResumeDecision::Approve { approval_id }) if approval_id == id
        ));
        assert!(matches!(
            decision_of(&serde_json::json!({ "approval_id": id, "state": "rejected" })),
            Some(ResumeDecision::Reject { .. })
        ));
        assert!(decision_of(&serde_json::json!({ "state": "approved" })).is_none());
        assert!(!has_context(&serde_json::json!({})));
        assert!(has_context(&serde_json::json!({ "context": [] })));
    }
}
