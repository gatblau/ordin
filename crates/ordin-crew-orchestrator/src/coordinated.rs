//! C. Coordinated mode: a loop of coordinator turns, each checked against the guardrails (F).

use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{CrewRunState, RunState};
use ordin_types::RunId;

use crate::dispatch::{input_lines, is_failure, Step};
use crate::driver::{crew_log, Crew, EndKind, Flow, RoundCtx, Stop};
use crate::guard::{check_delegate, cut, Refusal};
use crate::store::StepRow;
use crate::types::{CrewToolCall, StepOutcome};

/// C.3: a delegate's brief is cut to 200 characters in the round's one-line decision.
const BRIEF_IN_DECISION: usize = 200;
/// An empty list of calls is treated as `escalate` (C): the question the person sees.
const NO_INSTRUCTION: &str = "Ordin did not say what to do next.";

/// C.3: the one-line rendering of the calls.
fn render(crew: &Crew, calls: &[CrewToolCall]) -> String {
    let parts: Vec<String> = calls
        .iter()
        .map(|call| match call {
            CrewToolCall::Delegate { member, brief, .. } => {
                format!(
                    "delegate {}: {}",
                    crew.role_of(member),
                    cut(brief, BRIEF_IN_DECISION)
                )
            }
            CrewToolCall::Finish { .. } => "finish".to_owned(),
            CrewToolCall::Escalate { .. } => "escalate".to_owned(),
        })
        .collect();
    parts.join("; ")
}

impl Crew {
    /// C. Rounds until the crew stops.
    pub(crate) async fn coordinated(&self) -> Stop {
        loop {
            if let Err(stop) = self.coordinated_round().await {
                return stop;
            }
        }
    }

    async fn coordinated_round(&self) -> Flow<()> {
        // C.1
        self.check_cancelled()?;
        self.stop_if_limit()?;
        // C.2
        let view = self.view();
        let turn = self.ask(true, || self.coordinator.round(&view)).await?;
        self.add_requests(turn.turns);
        let calls = if turn.calls.is_empty() {
            vec![CrewToolCall::Escalate {
                question: NO_INSTRUCTION.to_owned(),
                recommendation: None,
            }]
        } else {
            turn.calls
        };
        // C.3
        let round = self.open_round(&render(self, &calls)).await?;
        self.state().step_seq = 0;
        // C.4: decide each call in order; the dispatches run together.
        let mut dispatches = Vec::new();
        let mut ending: Option<EndKind> = None;
        for call in calls {
            match call {
                CrewToolCall::Finish { summary } => {
                    round.note("allowed: finish".to_owned(), None);
                    ending = Some(EndKind::new(CrewRunState::Finished, summary));
                    break;
                }
                CrewToolCall::Escalate {
                    question,
                    recommendation,
                } => {
                    round.note("allowed: escalate".to_owned(), None);
                    let mut kind = EndKind::new(CrewRunState::Escalated, question);
                    kind.recommendation = recommendation.clone();
                    self.state().recommendation = recommendation;
                    ending = Some(kind);
                    break;
                }
                CrewToolCall::Delegate {
                    member,
                    brief,
                    uses_results_of,
                    uses_inputs,
                } => {
                    match check_delegate(
                        &self.charter,
                        &self.inputs,
                        &member,
                        &uses_results_of,
                        &uses_inputs,
                    ) {
                        Err(refusal) => self.refuse(&round, &member, refusal).await,
                        Ok(()) => {
                            round.note(format!("allowed: {}", self.role_of(&member)), None);
                            let k = {
                                let mut st = self.state();
                                st.step_seq += 1;
                                st.step_seq
                            };
                            dispatches.push(self.delegate(
                                &round,
                                k,
                                member,
                                brief,
                                uses_results_of,
                                uses_inputs,
                            ));
                        }
                    }
                }
            }
        }
        // C.5: wait for every dispatched step, in call order.
        let results = futures::future::join_all(dispatches).await;
        let mut stop: Option<Stop> = None;
        let mut failure: Option<EndKind> = None;
        for result in results {
            match result {
                Ok(outcome) => {
                    if let Some(kind) = self.record_outcome(&outcome) {
                        failure.get_or_insert(kind);
                    }
                }
                Err(s) => {
                    stop.get_or_insert(s);
                }
            }
        }
        self.close_round(&round).await?;
        if let Some(stop) = stop {
            return Err(stop);
        }
        if let Some(kind) = ending {
            return Err(Stop::Ended(kind));
        }
        if let Some(kind) = failure {
            return Err(Stop::Ended(kind));
        }
        Ok(())
    }

    /// F.2: a refusal is recorded with the round and shown to the coordinator in the next view as a
    /// result with state `failed`; no member run starts and no model is called for the decision.
    async fn refuse(&self, round: &RoundCtx, member: &AgentRef, refusal: Refusal) {
        let words = refusal.words();
        round.note(format!("refused: {words}"), Some("CREW_GUARDRAIL_REFUSED"));
        crew_log!(
            info,
            crew_run_id = self.id.to_string().as_str(),
            round = round.number,
            code = "CREW_GUARDRAIL_REFUSED",
            decision = words,
            "crew_guardrail"
        );
        let outcome = StepOutcome {
            member: *member,
            label: format!("round {}", round.number),
            run_id: RunId::new(),
            state: RunState::Failed,
            summary: Some(format!("Refused: {words}")),
            artefacts: vec![],
        };
        self.remember(&outcome);
    }

    /// Puts `outcome` first among the latest results, replacing the same member's earlier one.
    pub(crate) fn remember(&self, outcome: &StepOutcome) {
        let mut st = self.state();
        st.latest.retain(|o| o.member != outcome.member);
        st.latest.insert(0, outcome.clone());
    }

    /// F.3: counts consecutive failed steps of one member. Reaching `failure_repeat_limit` in
    /// coordinated mode escalates the crew.
    pub(crate) fn record_outcome(&self, outcome: &StepOutcome) -> Option<EndKind> {
        self.remember(outcome);
        let limit = self.charter.guardrails.failure_repeat_limit;
        let mut st = self.state();
        if is_failure(outcome.state) {
            if st.last_failed == Some(outcome.member) {
                st.counters.repeated_failures += 1;
            } else {
                st.last_failed = Some(outcome.member);
                st.counters.repeated_failures = 1;
            }
            if st.counters.repeated_failures >= limit {
                let who = self.role_of(&outcome.member);
                return Some(
                    EndKind::new(
                        CrewRunState::Escalated,
                        format!("{who} failed twice in a row."),
                    )
                    .with_code("CREW_MEMBER_FAILED_TWICE"),
                );
            }
        } else if outcome.state == RunState::Completed && st.last_failed == Some(outcome.member) {
            st.last_failed = None;
            st.counters.repeated_failures = 0;
        }
        None
    }

    /// One allowed `delegate`: an approval point first (F.2), then E.
    async fn delegate(
        &self,
        round: &RoundCtx,
        k: u32,
        member: AgentRef,
        brief: String,
        uses_results_of: Vec<AgentRef>,
        uses_inputs: Vec<String>,
    ) -> Flow<StepOutcome> {
        let label = format!("round {}", round.number);
        if self.needs_approval(None, &member)
            && !self
                .approve_step(round.number, &label, &member, &brief)
                .await?
        {
            return Err(Crew::declined(&label));
        }
        let (inputs, texts, attachments) = input_lines(&self.inputs, &uses_inputs);
        let earlier = {
            let st = self.state();
            uses_results_of
                .iter()
                .filter_map(|from| st.latest.iter().find(|o| o.member == *from))
                .map(|o| {
                    (
                        self.role_of(&o.member),
                        o.summary.clone(),
                        o.artefacts.clone(),
                    )
                })
                .collect()
        };
        let step = Step {
            member,
            label: label.clone(),
            line: brief,
            inputs,
            texts,
            earlier,
            attachments,
            model_override: None,
        };
        if self.state().taken_over {
            // ADR-064: each step Ordin dispatches after a take-over is journalled as changed by Ordin.
            self.state().changed_by_ordin.push(label.clone());
            self.journal(
                "crew_step_changed_by_ordin",
                "Ordin changed a step after taking over.",
                serde_json::json!({ "crew_run_id": self.id, "position": label }),
            )
            .await;
        }
        let outcome = self.dispatch(round, &step).await?;
        // The cursor row lets a restart rebuild the latest results (J.2).
        self.put_step(StepRow {
            position: format!("r{}.{}", round.number, k),
            state: if is_failure(outcome.state) {
                "failed"
            } else {
                "done"
            }
            .to_owned(),
            attempts: 1,
            run_id: Some(outcome.run_id),
            outcome: Some(outcome.clone()),
            decided_path: None,
            passes: None,
        })
        .await?;
        Ok(outcome)
    }

    /// J.2: the round left open by a restart is settled before the coordinator is asked again. Member
    /// runs still in progress are followed again; a run the supervisor marked `interrupted` counts as
    /// a failed attempt.
    pub(crate) async fn settle_open_round(&self, follow: bool) -> Flow<()> {
        let pool = self.o.pool()?;
        let rounds = crate::store::list_rounds(pool, self.id).await?;
        let Some(open) = rounds.into_iter().find(|r| r.closed_at.is_none()) else {
            return Ok(());
        };
        let round = RoundCtx {
            number: open.number,
            decisions: std::sync::Mutex::new(open.guardrail_decisions.clone()),
            runs: std::sync::Mutex::new(open.member_runs.clone()),
            codes: std::sync::Mutex::new(Vec::new()),
        };
        let mut failure = None;
        if follow {
            for run_id in &open.member_runs {
                let Ok(view) = self.o.supervisor.get(*run_id).await else {
                    continue;
                };
                let label = format!("round {}", open.number);
                let outcome = self.follow_run(*run_id, view.run.agent, &label).await?;
                if let Some(kind) = self.record_outcome(&outcome) {
                    failure.get_or_insert(kind);
                }
            }
        }
        self.close_round(&round).await?;
        if let Some(kind) = failure {
            return Err(Stop::Ended(kind));
        }
        Ok(())
    }
}
