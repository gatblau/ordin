//! E. Dispatch one step to a member, follow its run, and H. approvals for steps.

use ordin_approval::{ApprovalRequest, WaitOutcome};
use ordin_journal::EntryFilter;
use ordin_runtime_adapter::{
    end_state, PauseReason, ResumeDecision, RunEvent, RunLimits, RunTarget, StartInput,
};
use ordin_session_supervisor::StartRequest;
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::domain::runs::{ApprovalState, CrewRunState, RunOrigin, RunState};
use ordin_types::wire::approval::{ApprovalAsker, RejectStyle};
use ordin_types::{ArtefactId, RunId};
use ordin_usage_planner::{LevelChoice, WorkKind};
use tokio::sync::broadcast::error::RecvError;

use crate::driver::{crew_log, Crew, EndKind, Flow, PauseKind, RoundCtx, Stop};
use crate::guard::{agent_key, cut, MAX_WALL_CLOCK_SECS};
use crate::store::StepRow;
use crate::types::{CrewInputValue, StepOutcome};

/// The idle timeout of a member run: the supervisor's own default (SessionSupervisor.md,
/// `SupervisorConfig.idle_timeout_secs`, 600).
const IDLE_TIMEOUT_SECS: u32 = 600;
/// E.5: a member's summary is cut to 2,000 characters.
const MAX_SUMMARY: usize = 2_000;
/// E.5: a member's question is cut to 500 characters before Ordin is asked (CO-PQ-1).
const MAX_MEMBER_QUESTION: usize = 500;
/// How often a run that sends no event is looked at, to find out that it ended.
const POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// What one dispatch needs; the fixed-code brief (ADR-062 *Focused briefs*) is built from it.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub(crate) member: AgentRef,
    /// "3", "2.1", "3a" in saved-steps mode; "round 4" in coordinated mode.
    pub(crate) label: String,
    /// The step's line, or the coordinator's brief.
    pub(crate) line: String,
    pub(crate) inputs: Vec<InputUsed>,
    /// Inputs given as words (and a For each item): label and text, shown in the brief only.
    pub(crate) texts: Vec<(String, String)>,
    /// Earlier results: label, summary, artefacts.
    pub(crate) earlier: Vec<(String, Option<String>, Vec<ArtefactId>)>,
    pub(crate) attachments: Vec<ArtefactId>,
    pub(crate) model_override: Option<LevelChoice>,
}

/// E.2. The conversation is never included.
pub(crate) fn build_brief(purpose: &str, step: &Step, minutes: u32) -> String {
    let mut text = format!("Purpose: {purpose}\nYour part: {}\n", step.line);
    if !step.inputs.is_empty() || !step.texts.is_empty() {
        text.push_str("Inputs:\n");
        for input in &step.inputs {
            text.push_str(&format!(
                "- {}: {} (version {})\n",
                input.name, input.location, input.version_read
            ));
        }
        for (label, words) in &step.texts {
            text.push_str(&format!("- {label}: {words}\n"));
        }
    }
    if !step.earlier.is_empty() {
        text.push_str("Use these earlier results:\n");
        for (label, summary, artefacts) in &step.earlier {
            let ids: Vec<String> = artefacts.iter().map(|a| a.to_string()).collect();
            text.push_str(&format!(
                "- {}: {} (artefacts: {})\n",
                label,
                summary.as_deref().unwrap_or("no summary"),
                ids.join(", ")
            ));
        }
    }
    text.push_str("Return a short summary and references to what you made.\n");
    text.push_str(&format!("Limits: {minutes} minutes."));
    text
}

/// A step that ends `failed`, `stopped_at_limit`, `interrupted`, `abandoned` or with no reply is a
/// failed attempt; a cancelled step is the crew's own doing and is not.
pub(crate) fn is_failure(state: RunState) -> bool {
    matches!(
        state,
        RunState::Failed
            | RunState::StoppedAtLimit
            | RunState::Interrupted
            | RunState::Abandoned
            | RunState::NoReply
    )
}

/// The versioned inputs, the inputs given as words, and the attachments of the inputs a step names.
pub(crate) fn input_lines(
    inputs: &[crate::types::CrewInput],
    labels: &[String],
) -> (Vec<InputUsed>, Vec<(String, String)>, Vec<ArtefactId>) {
    let mut used = Vec::new();
    let mut texts = Vec::new();
    let mut attachments = Vec::new();
    for label in labels {
        let Some(input) = inputs.iter().find(|i| i.label == *label) else {
            continue;
        };
        used.extend(input.used.iter().cloned());
        match &input.value {
            CrewInputValue::Text { text } => texts.push((input.label.clone(), cut(text, 1_000))),
            CrewInputValue::Documents { artefacts } => {
                attachments.extend(artefacts.iter().copied())
            }
            CrewInputValue::List { .. } => {}
        }
    }
    (used, texts, attachments)
}

impl Crew {
    /// A step whose run never started counts as a failed attempt with the supervisor's code.
    fn not_started(&self, step: &Step, code: &str) -> StepOutcome {
        crew_log!(warn, crew_run_id = %self.id, label = step.label.as_str(), code = code, "crew_step_not_started");
        StepOutcome {
            member: step.member,
            label: step.label.clone(),
            run_id: RunId::new(),
            state: RunState::Failed,
            summary: Some(format!("Could not start: {code}")),
            artefacts: vec![],
        }
    }

    /// E. Starts the member's run and follows it to its end.
    pub(crate) async fn dispatch(&self, round: &RoundCtx, step: &Step) -> Flow<StepOutcome> {
        let permit = self
            .unless_cancelled(self.slots.clone().acquire_owned())
            .await?
            .map_err(|_| Stop::Gone)?;
        self.check_cancelled()?;
        // E.1
        let target = match step.member {
            AgentRef::AgentInstance { agent_instance_id } => {
                RunTarget::AgentInstance { agent_instance_id }
            }
            AgentRef::RemoteAgent { remote_agent_id } => {
                if self.o.host.remote_route(remote_agent_id).is_none() {
                    return Ok(self.not_started(step, "SUP_NO_ADAPTER"));
                }
                RunTarget::RemoteAgent { remote_agent_id }
            }
        };
        // E.3
        let counters = self.counters();
        let g = &self.charter.guardrails;
        let remaining = g
            .wall_clock_secs
            .saturating_sub(counters.wall_clock_used_secs)
            .clamp(1, MAX_WALL_CLOCK_SECS);
        let limits = RunLimits {
            wall_clock_secs: remaining,
            request_budget: (g.request_budget > 0).then(|| {
                g.request_budget
                    .saturating_sub(counters.requests_used)
                    .max(1)
            }),
            idle_timeout_secs: IDLE_TIMEOUT_SECS,
        };
        // E.2
        let text = build_brief(&self.charter.purpose, step, (remaining / 60).max(1));
        // E.4
        // spec: ordin-desktop/components/CrewOrchestrator#SA-CREW-05
        let request = StartRequest {
            origin: RunOrigin::CrewRound {
                crew_run_id: self.id,
                round_number: round.number,
            },
            workspace_id: self.charter.workspace_id,
            target,
            input: StartInput::FreeText { text },
            attachments: step.attachments.clone(),
            limits: Some(limits),
            work_kind: WorkKind::Documents,
            work_key: format!("crew:{}:{}", self.charter.id, agent_key(&step.member)),
            inputs_used: step.inputs.clone(),
            thread: None,
            started_by_person: false,
            model_override: step.model_override.clone(),
            coding_task_id: None,
        };
        let run_id = match self.o.supervisor.start_run(request).await {
            Ok(id) => id,
            Err(e) => return Ok(self.not_started(step, e.code())),
        };
        // Subscribed before any await: the Supervisor replays only a run's current pause
        // (SessionSupervisor.md SA-SUP-PAUSE-01), so usage and other events reported while the
        // round is being recorded would otherwise be missed.
        let events = self.o.supervisor.subscribe(run_id).ok();
        crate::driver::lock(&self.live.runs).insert(run_id);
        let known = {
            let mut runs = crate::driver::lock(&round.runs);
            runs.push(run_id);
            runs.clone()
        };
        let pool = self.o.pool()?;
        crate::store::set_round_runs(pool, self.id, round.number, &known).await?;
        // The restriction is held by the Rules Engine for the run's start (Rules Engine spec).
        self.o.rules.restrict_run(
            run_id,
            ordin_rules::RunRestriction {
                external_actions_allowed: self
                    .charter
                    .guardrails
                    .members_allowed_external_actions
                    .contains(&step.member),
            },
        );
        crew_log!(
            info,
            crew_run_id = %self.id,
            position = step.label.as_str(),
            run_id = %run_id,
            member = agent_key(&step.member).as_str(),
            "crew_step_dispatched"
        );
        let outcome = self
            .follow_events(run_id, step.member, &step.label, events)
            .await;
        crate::driver::lock(&self.live.runs).remove(&run_id);
        drop(permit);
        outcome
    }

    /// E.5. Follows `run_id` until it ends; a member's question goes to Ordin, an approval pauses
    /// the crew until the Approval Service and the Hook Relay settle it.
    pub(crate) async fn follow_run(
        &self,
        run_id: RunId,
        member: AgentRef,
        label: &str,
    ) -> Flow<StepOutcome> {
        let events = self.o.supervisor.subscribe(run_id).ok();
        self.follow_events(run_id, member, label, events).await
    }

    /// `follow_run` over a subscription the caller already holds.
    pub(crate) async fn follow_events(
        &self,
        run_id: RunId,
        member: AgentRef,
        label: &str,
        events: Option<tokio::sync::broadcast::Receiver<RunEvent>>,
    ) -> Flow<StepOutcome> {
        let mut counted: u32 = 0;
        let mut receiver = events;
        let mut waiting_on_approval = false;
        let ended = loop {
            self.check_cancelled()?;
            let Some(rx) = receiver.as_mut() else {
                // The run is not (or no longer) live: read its stored state.
                if let Some(done) = self.read_end(run_id).await? {
                    break done;
                }
                tokio::time::sleep(POLL).await;
                receiver = self.o.supervisor.subscribe(run_id).ok();
                continue;
            };
            let next = tokio::select! {
                biased;
                _ = self.live.cancel.cancelled() => return Err(Stop::Ended(EndKind::killed())),
                got = tokio::time::timeout(POLL, rx.recv()) => got,
            };
            let event = match next {
                Err(_elapsed) => {
                    // No event for a moment: has the run ended without our seeing it?
                    if let Some(done) = self.read_end(run_id).await? {
                        break done;
                    }
                    continue;
                }
                Ok(Err(RecvError::Lagged(_))) => continue,
                Ok(Err(RecvError::Closed)) => {
                    receiver = None;
                    continue;
                }
                Ok(Ok(event)) => event,
            };
            match event {
                RunEvent::Usage { usage } => {
                    counted = counted.saturating_add(usage.model_turns);
                    self.add_requests(usage.model_turns);
                }
                RunEvent::Paused { reason } => match reason {
                    // spec: ordin-desktop/components/CrewOrchestrator#CO-PQ-1
                    PauseReason::Person { question } => {
                        let question = cut(&question, MAX_MEMBER_QUESTION);
                        self.answer_member(run_id, member, &question).await?;
                    }
                    PauseReason::Approval { .. } if !waiting_on_approval => {
                        waiting_on_approval = true;
                        self.enter_pause(PauseKind::Approval).await?;
                    }
                    _ => {}
                },
                RunEvent::Resumed => {
                    if waiting_on_approval {
                        waiting_on_approval = false;
                        self.leave_pause(PauseKind::Approval).await?;
                    }
                }
                RunEvent::Ended { end } => {
                    break (
                        end_state(end.reason),
                        end.final_message,
                        end.artefacts,
                        None::<u32>,
                    );
                }
                _ => {}
            }
        };
        if waiting_on_approval {
            self.leave_pause(PauseKind::Approval).await?;
        }
        let (state, message, artefacts, stored_turns) = ended;
        // The supervisor broadcasts `Ended` without the final message; the message is kept as the
        // run's end reason. Events the stream did not show (a late subscription) are counted from
        // the stored run.
        let stored = self
            .o
            .supervisor
            .get(run_id)
            .await
            .ok()
            .map(|view| view.run);
        let message = message.or_else(|| stored.as_ref().and_then(|run| run.end_reason.clone()));
        let total = stored_turns.or_else(|| stored.as_ref().map(|run| run.usage.model_turns));
        if let Some(total) = total {
            self.add_requests(total.saturating_sub(counted));
        }
        Ok(StepOutcome {
            member,
            label: label.to_owned(),
            run_id,
            state,
            summary: message.map(|m| cut(&m, MAX_SUMMARY)),
            artefacts,
        })
    }

    /// The stored state of a run, when it has ended.
    async fn read_end(
        &self,
        run_id: RunId,
    ) -> Flow<Option<(RunState, Option<String>, Vec<ArtefactId>, Option<u32>)>> {
        match self.o.supervisor.get(run_id).await {
            Ok(view) => {
                let run = view.run;
                if matches!(
                    run.state,
                    RunState::Queued | RunState::Running | RunState::Paused
                ) {
                    Ok(None)
                } else {
                    let artefacts = self.stored_artefacts(run_id).await?;
                    Ok(Some((
                        run.state,
                        run.end_reason,
                        artefacts,
                        Some(run.usage.model_turns),
                    )))
                }
            }
            Err(e) if e.retryable() => {
                crew_log!(warn, crew_run_id = %self.id, code = e.code(), "crew_run_not_read");
                Err(Stop::Store)
            }
            Err(e) => {
                // The run is not known to the supervisor: it counts as a failed attempt.
                crew_log!(warn, crew_run_id = %self.id, code = e.code(), "crew_run_not_found");
                Ok(Some((RunState::Failed, None, Vec::new(), None)))
            }
        }
    }

    /// The artefacts of a run whose `Ended` was not seen. The supervisor journals `run_result`
    /// before the row says the run ended, so an ended run with no entry has none. A journal that
    /// cannot be read stops the crew: an empty list would read as a step that made nothing.
    async fn stored_artefacts(&self, run_id: RunId) -> Flow<Vec<ArtefactId>> {
        let page = self
            .o
            .journal
            .list_entries(
                EntryFilter {
                    run_id: Some(run_id),
                    kind: Some("run_result".to_owned()),
                },
                None,
            )
            .await
            .map_err(|e| {
                crew_log!(warn, crew_run_id = %self.id, code = ordin_types::CodedError::code(&e), "crew_run_not_read");
                Stop::Store
            })?;
        Ok(page
            .items
            .first()
            .and_then(|entry| serde_json::from_value(entry.data["artefacts"].clone()).ok())
            .unwrap_or_default())
    }

    /// E.5: a member's question. Ordin answers (or puts it to the person); the answer resumes the run.
    async fn answer_member(&self, run_id: RunId, member: AgentRef, question: &str) -> Flow<()> {
        self.enter_pause(PauseKind::Answer).await?;
        let view = self.view();
        let answered = self
            .ask(false, || {
                self.coordinator.answer_member(&view, &member, question)
            })
            .await;
        self.leave_pause(PauseKind::Answer).await?;
        match answered {
            Ok(answer) => {
                self.add_requests(answer.turns);
                let decision = ResumeDecision::Answer { text: answer.text };
                if let Err(e) = self
                    .o
                    .supervisor
                    .resume_with_decision(run_id, decision)
                    .await
                {
                    crew_log!(warn, crew_run_id = %self.id, run_id = %run_id, code = e.code(), "crew_answer_not_delivered");
                }
                Ok(())
            }
            Err(Stop::Unusable(_)) => {
                // Without a usable answer the member cannot go on: its run is cancelled and the
                // step counts as a failed attempt.
                let _ = self.o.supervisor.cancel(run_id).await;
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    /// H. An approval card for a step; `Ok(true)` when the person approved it.
    pub(crate) async fn approve_step(
        &self,
        round: u32,
        label: &str,
        member: &AgentRef,
        line: &str,
    ) -> Flow<bool> {
        let view = self.view();
        let wording = self.o.host.word_step_approval(&view, label, member, line);
        let request = ApprovalRequest {
            workspace_id: self.charter.workspace_id,
            asked_by: ApprovalAsker::CrewRound {
                crew_run_id: self.id,
                round,
            },
            question: wording.question,
            verb: wording.verb,
            where_line: wording.where_line,
            affects: wording.affects,
            changes: wording.changes,
            content: None,
            reject_style: RejectStyle::NotNow,
            show_details: false,
            proposal: wording.proposal,
            asker: None,
            leaves_company: false,
        };
        let card = match self.o.approvals.request(request).await {
            Ok(card) => card,
            Err(e) => {
                crew_log!(error, crew_run_id = %self.id, code = e.code(), "crew_approval_not_requested");
                return Err(Stop::Store);
            }
        };
        self.enter_pause(PauseKind::Approval).await?;
        // No deadline: a card is never decided by timeout.
        let waited = self
            .unless_cancelled(self.o.approvals.wait(card.approval_id, None))
            .await;
        match waited {
            Err(stop) => Err(stop),
            Ok(Err(e)) => {
                crew_log!(error, crew_run_id = %self.id, code = e.code(), "crew_approval_not_awaited");
                Err(Stop::Store)
            }
            Ok(Ok(outcome)) => {
                self.leave_pause(PauseKind::Approval).await?;
                match outcome {
                    WaitOutcome::Decided(ApprovalState::Approved) => Ok(true),
                    WaitOutcome::Decided(ApprovalState::Rejected) => Ok(false),
                    // Expired: the crew is ending by another path.
                    _ => Err(Stop::Gone),
                }
            }
        }
    }

    /// F.2 / D.1: an approval point names this member or step.
    pub(crate) fn needs_approval(&self, position: Option<&str>, member: &AgentRef) -> bool {
        let points = &self.charter.guardrails.approval_points;
        // spec: ordin-desktop/components/CrewOrchestrator#SA-CREW-01
        let member_point = format!("member:{}", agent_key(member));
        points
            .iter()
            .any(|p| *p == member_point || position.is_some_and(|pos| *p == format!("step:{pos}")))
    }

    /// The end of a crew whose person declined a step (SA-CREW-02).
    pub(crate) fn declined(label: &str) -> Stop {
        // spec: ordin-desktop/components/CrewOrchestrator#SA-CREW-02
        Stop::Ended(EndKind::new(
            CrewRunState::Escalated,
            format!("The person chose not to run step {label}."),
        ))
    }

    /// Records a step row for the cursor (D: persisted after every step).
    pub(crate) async fn put_step(&self, row: StepRow) -> Flow<()> {
        let pool = self.o.pool()?;
        crate::store::put_step(pool, self.id, &row).await?;
        self.state().rows.insert(row.position.clone(), row);
        Ok(())
    }
}
