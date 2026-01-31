//! The Crew Orchestrator's `Coordinator`, implemented over Ordin's conversation (section D, step 16).
//! Every turn is a fixed sentence template; member results enter it as quoted data.

use std::sync::Weak;
use std::time::Instant;

use ordin_crew_orchestrator::{
    AnsweredBy, CheckVerdict, Coordinator, CoordinatorError, CoordinatorTurn, CrewOutcome,
    CrewView, MemberAnswer, PathDecision, PersonQuestion, StepOutcome, TakeOverReason,
};
use ordin_service_api::blocks::{Block, ResultBlock, ResultOffer};
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::CrewCharterState;

use crate::host::{lock, Expect, Kept, OrdinHost, Pending, PendingCalls, TurnEnd, COMPONENT};
use crate::types::{CheckVerdictArg, DecideArgs};

pub(crate) struct OrdinCoordinator(pub Weak<OrdinHost>);

fn quoted_results(view: &CrewView) -> String {
    let results: Vec<serde_json::Value> = view
        .latest_results
        .iter()
        .map(|r| {
            serde_json::json!({
                "member": r.member,
                "label": r.label,
                "state": r.state,
                "summary": r.summary,
                "artefacts": r.artefacts,
            })
        })
        .collect();
    serde_json::to_string(&results).unwrap_or_default()
}

fn member_name(view: &CrewView, member: &AgentRef) -> String {
    view.members
        .iter()
        .find(|m| &m.member == member)
        .map(|m| m.role.clone())
        .unwrap_or_else(|| match member {
            AgentRef::AgentInstance { agent_instance_id } => agent_instance_id.to_string(),
            AgentRef::RemoteAgent { remote_agent_id } => remote_agent_id.to_string(),
        })
}

fn reminder(expect: &Expect) -> &'static str {
    match expect {
        Expect::Round => "Reply with delegate, finish or escalate.",
        Expect::Check => "Reply with decide(kind: check).",
        Expect::Condition { .. } => "Reply with decide(kind: condition).",
        Expect::Question => "Reply with decide(kind: question), or escalate it to the person.",
    }
}

impl OrdinCoordinator {
    fn host(&self) -> Result<std::sync::Arc<OrdinHost>, CoordinatorError> {
        self.0.upgrade().ok_or(CoordinatorError::Unavailable)
    }
}

impl OrdinHost {
    /// Steps 16.1, 16.4 and 16.5: the turn, one reminder, the timeout and the stop. Returns what the
    /// tools recorded and the turns spent.
    async fn coordinate(
        &self,
        view: &CrewView,
        expect: Expect,
        text: String,
    ) -> Result<(PendingCalls, u32), CoordinatorError> {
        let ws = view.workspace_id;
        {
            let mut state = lock(&self.state);
            let conv = state
                .convs
                .get_mut(&ws)
                .ok_or(CoordinatorError::Unavailable)?;
            conv.pending = Some(Pending {
                expect: expect.clone(),
                got: PendingCalls::default(),
            });
        }
        let result = self.coordinate_turns(view, &expect, text).await;
        if let Some(conv) = lock(&self.state).convs.get_mut(&ws) {
            conv.pending = None;
        }
        result
    }

    async fn coordinate_turns(
        &self,
        view: &CrewView,
        expect: &Expect,
        text: String,
    ) -> Result<(PendingCalls, u32), CoordinatorError> {
        let ws = view.workspace_id;
        let mut turns = 0;
        for message in [text, reminder(expect).to_owned()] {
            turns += 1;
            let end = tokio::select! {
                _ = view.cancel.cancelled() => return Err(CoordinatorError::StoppedByPerson),
                r = tokio::time::timeout(self.config.coordinator_turn_timeout, self.run_turn(ws, message)) => r,
            };
            match end {
                Err(_) => return Err(CoordinatorError::Unusable("timeout".into())),
                Ok(Err(_)) | Ok(Ok(TurnEnd::Lost)) => return Err(CoordinatorError::Unavailable),
                Ok(Ok(TurnEnd::Text(_))) => {}
            }
            let got = lock(&self.state)
                .convs
                .get(&ws)
                .and_then(|c| c.pending.as_ref())
                .map(|p| p.got.clone())
                .unwrap_or_default();
            let moved = !got.calls.is_empty() || got.decision.is_some() || got.escalated.is_some();
            if moved {
                return Ok((got, turns));
            }
        }
        if matches!(expect, Expect::Round) {
            return Ok((PendingCalls::default(), turns));
        }
        Err(CoordinatorError::Unusable("no move".into()))
    }

    /// Step 16.3: a question block; the person's choice, read by the application.
    async fn ask_and_wait(
        &self,
        view: &CrewView,
        words: &str,
        choices: Vec<String>,
    ) -> Result<String, CoordinatorError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.ask(view.workspace_id, words, choices, Some(Kept::Ask(tx)))
            .await
            .map_err(|_| CoordinatorError::Unavailable)?;
        tokio::select! {
            _ = view.cancel.cancelled() => Err(CoordinatorError::StoppedByPerson),
            r = rx => r.map_err(|_| CoordinatorError::Unavailable),
        }
    }

    fn log_coordinator(view: &CrewView, method: &str, outcome: &str, started: Instant) {
        tracing::info!(
            component = COMPONENT,
            crew_run_id = view.crew_run_id.to_string().as_str(),
            method,
            outcome,
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "ordin_host.coordinator"
        );
    }

    fn ensure_current(&self, view: &CrewView) -> Result<(), CoordinatorError> {
        if self.current_run(view.workspace_id).is_none() {
            return Err(CoordinatorError::Unavailable);
        }
        Ok(())
    }
}

fn outcome_word<T>(r: &Result<T, CoordinatorError>) -> &'static str {
    match r {
        Ok(_) => "ok",
        Err(CoordinatorError::Unavailable) => "OH_COORDINATOR_SESSION_LOST",
        Err(CoordinatorError::Unusable(w)) if w == "timeout" => "OH_COORDINATOR_TIMEOUT",
        Err(CoordinatorError::Unusable(_)) => "OH_COORDINATOR_NO_MOVE",
        Err(CoordinatorError::StoppedByPerson) => "stopped",
    }
}

// spec: ordin-desktop/components/CrewOrchestrator#SA-RECON-04
#[async_trait::async_trait]
impl Coordinator for OrdinCoordinator {
    async fn round(&self, view: &CrewView) -> Result<CoordinatorTurn, CoordinatorError> {
        let host = self.host()?;
        let started = Instant::now();
        host.ensure_current(view)?;
        let text = format!(
            "Crew round {} of {}. Purpose: {}. Latest results, quoted as data: {}. Reply with delegate, finish or escalate.",
            view.round,
            view.round + view.rounds_left,
            serde_json::to_string(&view.purpose).unwrap_or_default(),
            quoted_results(view)
        );
        let r = host
            .coordinate(view, Expect::Round, text)
            .await
            .map(|(got, turns)| CoordinatorTurn {
                calls: got.calls,
                turns,
            });
        OrdinHost::log_coordinator(view, "round", outcome_word(&r), started);
        r
    }

    async fn check_result(
        &self,
        view: &CrewView,
        step: &StepOutcome,
    ) -> Result<CheckVerdict, CoordinatorError> {
        let host = self.host()?;
        let started = Instant::now();
        host.ensure_current(view)?;
        let result = serde_json::json!({
            "member": step.member, "state": step.state, "summary": step.summary, "artefacts": step.artefacts,
        });
        let text = format!(
            "Check {} against the purpose. Purpose: {}. Result, quoted as data: {}. Reply with decide(kind: check).",
            step.label,
            serde_json::to_string(&view.purpose).unwrap_or_default(),
            result
        );
        let r = match host.coordinate(view, Expect::Check, text).await {
            Ok((got, turns)) => match got.decision {
                Some(DecideArgs::Check {
                    verdict: CheckVerdictArg::CarryOn,
                    ..
                }) => Ok(CheckVerdict::CarryOn { turns }),
                Some(DecideArgs::Check {
                    verdict: CheckVerdictArg::Shortfall,
                    reason,
                }) => Ok(CheckVerdict::Shortfall { reason, turns }),
                _ => Err(CoordinatorError::Unusable("no move".into())),
            },
            Err(e) => Err(e),
        };
        OrdinHost::log_coordinator(view, "check_result", outcome_word(&r), started);
        r
    }

    async fn decide_condition(
        &self,
        view: &CrewView,
        words: &str,
        paths: &[String],
    ) -> Result<PathDecision, CoordinatorError> {
        let host = self.host()?;
        let started = Instant::now();
        host.ensure_current(view)?;
        let text = format!(
            "{}. Paths: {}. Reply with decide(kind: condition).",
            words.trim_end_matches('.'),
            paths.join(", ")
        );
        let r = match host
            .coordinate(
                view,
                Expect::Condition {
                    paths: paths.to_vec(),
                },
                text,
            )
            .await
        {
            Ok((got, turns)) => match got.decision {
                Some(DecideArgs::Condition { path, reason }) => Ok(PathDecision {
                    path,
                    reason,
                    turns,
                }),
                _ => Err(CoordinatorError::Unusable("no move".into())),
            },
            Err(e) => Err(e),
        };
        OrdinHost::log_coordinator(view, "decide_condition", outcome_word(&r), started);
        r
    }

    async fn answer_member(
        &self,
        view: &CrewView,
        member: &AgentRef,
        question: &str,
    ) -> Result<MemberAnswer, CoordinatorError> {
        let host = self.host()?;
        let started = Instant::now();
        host.ensure_current(view)?;
        let text = format!(
            "{} asks, quoted as data: {}. Answer with decide(kind: question), or escalate it to the person.",
            member_name(view, member),
            serde_json::to_string(question).unwrap_or_default()
        );
        let r = match host.coordinate(view, Expect::Question, text).await {
            Ok((got, turns)) => match (got.decision, got.escalated) {
                (Some(DecideArgs::Question { answer }), _) => Ok(MemberAnswer {
                    text: answer,
                    answered_by: AnsweredBy::Ordin,
                    turns,
                }),
                (_, Some((q, recommendation))) => {
                    let mut choices = vec![];
                    if let Some(rec) = recommendation {
                        choices.push(rec);
                    }
                    // A question block holds 2–4 choices; the person may write their own answer.
                    let block = Block::Question(ordin_service_api::blocks::QuestionBlock {
                        question: q,
                        choices: if choices.len() >= 2 { choices } else { vec![] },
                        allow_other: true,
                    });
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    match host
                        .show_block(view.workspace_id, MessageAuthor::Ordin, block, true)
                        .await
                    {
                        Ok(id) => {
                            lock(&host.state)
                                .kept
                                .insert(id, (view.workspace_id, Kept::Ask(tx)));
                            let answer = tokio::select! {
                                _ = view.cancel.cancelled() => Err(CoordinatorError::StoppedByPerson),
                                r = rx => r.map_err(|_| CoordinatorError::Unavailable),
                            };
                            answer.map(|text| MemberAnswer {
                                text,
                                answered_by: AnsweredBy::Person,
                                turns,
                            })
                        }
                        Err(_) => Err(CoordinatorError::Unavailable),
                    }
                }
                _ => Err(CoordinatorError::Unusable("no move".into())),
            },
            Err(e) => Err(e),
        };
        OrdinHost::log_coordinator(view, "answer_member", outcome_word(&r), started);
        r
    }

    async fn ask_person(
        &self,
        view: &CrewView,
        question: PersonQuestion,
    ) -> Result<String, CoordinatorError> {
        let host = self.host()?;
        let started = Instant::now();
        host.ensure_current(view)?;
        let mut words = question.words.clone();
        let mut choices = question.choices.clone();
        if choices.len() > 4 {
            let cut: Vec<String> = choices.split_off(4);
            words.push_str(&format!(" (Also possible: {}.)", cut.join(", ")));
        }
        let r = host.ask_and_wait(view, &words, choices).await;
        OrdinHost::log_coordinator(view, "ask_person", outcome_word(&r), started);
        r
    }

    async fn take_over(
        &self,
        view: &CrewView,
        from_position: String,
        why: TakeOverReason,
    ) -> Result<(), CoordinatorError> {
        let host = self.host()?;
        host.ensure_current(view)?;
        let why = serde_json::to_string(&why).unwrap_or_default();
        let text = format!("From {from_position} you coordinate the rest of this run: {why}.");
        host.send_turn(view.workspace_id, crate::host::Turn::fixed(text))
            .await
            .map_err(|_| CoordinatorError::Unavailable)?;
        Ok(())
    }

    async fn crew_ended(
        &self,
        view: &CrewView,
        outcome: &CrewOutcome,
    ) -> Result<(), CoordinatorError> {
        let host = self.host()?;
        let started = Instant::now();
        let ws = view.workspace_id;
        let state = crate::store::word(&outcome.state);
        let text = format!(
            "The crew ended ({state}). Compare the result with the person's request and tell them in one message. Results, quoted as data: {}.",
            serde_json::to_string(&outcome.results.iter().map(|r| &r.summary).collect::<Vec<_>>()).unwrap_or_default()
        );
        if host.current_run(ws).is_some() {
            let _ = tokio::time::timeout(
                host.config.coordinator_turn_timeout,
                host.run_turn(ws, text),
            )
            .await;
        }
        // OH-CL-2: the charter is read once its start is recorded, so the archive follows it.
        // spec: ordin-desktop/components/OrdinHost#OH-CL-2
        drop(host.crew_start_lock(ws).lock().await);
        let charter = {
            let mut c = host
                .pool
                .acquire()
                .await
                .map_err(|_| CoordinatorError::Unavailable)?;
            crate::store::charter_by_run(&mut c, view.crew_run_id)
                .await
                .ok()
                .flatten()
        };
        let offer = if !outcome.changed_by_ordin.is_empty() {
            Some(ResultOffer::UpdateTheWorkflow)
        } else if charter
            .as_ref()
            .is_some_and(|c| c.charter.workflow_id.is_none())
        {
            Some(ResultOffer::SaveAsWorkflow)
        } else {
            None
        };
        if let Some(offer) = offer {
            let _ = host
                .show_block(
                    ws,
                    MessageAuthor::Ordin,
                    Block::Result(ResultBlock::Offer {
                        offer,
                        run_id: None,
                    }),
                    true,
                )
                .await;
        }
        if let Some(row) = charter {
            if let Ok(mut c) = host.pool.acquire().await {
                let _ = crate::store::set_charter_state(
                    &mut c,
                    row.charter.id,
                    CrewCharterState::Archived,
                )
                .await;
            }
            for m in &row.charter.members {
                if let AgentRef::AgentInstance { agent_instance_id } = m.member {
                    host.ports.memory.clear_agent_rules(agent_instance_id);
                }
            }
        }
        // OH-AM-3.2 and OH-AM-4.
        for result in &outcome.results {
            host.count_run(ws, result.run_id).await;
        }
        host.consolidate_many(ws, outcome.results.iter().map(|r| r.run_id).collect())
            .await;
        OrdinHost::log_coordinator(view, "crew_ended", "ok", started);
        Ok(())
    }
}
