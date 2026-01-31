//! D. Saved-steps mode: a recursive walk of the workflow outline (ADR-064, ADR-065) with a cursor
//! persisted in `crew_step_states` after every step, so a restart can rebuild where the crew stood.
//! No coordinator turn is spent per step: Ordin is called only at *Ordin checks the result* marks,
//! for conditions written in words, for a member's question, at the end, and when the steps do not
//! fit (then from the position on Ordin coordinates round by round).

use futures::future::BoxFuture;
use ordin_runtime_adapter::AdapterKind;
use ordin_types::domain::runs::{
    AgentStep, CallBlock, Condition, CrewRunState, EachBlock, IfBlock, IfItFails, RunState,
    StepUse, UntilBlock, WorkflowStep,
};
use ordin_types::ArtefactId;
use ordin_usage_planner::{LevelChoice, WorkKind};

use crate::dispatch::{input_lines, is_failure, Step};
use crate::driver::{crew_log, lock, Crew, EndKind, Flow, RoundCtx, Stop};
use crate::guard::cut;
use crate::plan::MAX_CALL_DEPTH;
use crate::store::StepRow;
use crate::types::{
    AgentAvailability, CheckVerdict, CrewInputValue, CrewMode, PersonQuestion, PersonQuestionKind,
    StepOutcome, TakeOverReason,
};

/// ADR-065: the choices when a Repeat until reaches its maximum.
const CARRY_ON: &str = "Carry on with the last result";
const STOP: &str = "Stop";
/// The choices when a step failed twice and *if it fails* says ask.
const TRY_AGAIN: &str = "Try again";
const SKIP_IT: &str = "Skip it";
/// Ordin's answer could not be used (no `TakeOverReason` names this case).
const UNUSABLE_WORDS: &str = "Ordin's answer could not be used.";
/// The paths of a Repeat until's condition.
const YES: &str = "Yes";
const NO: &str = "No";

/// Where in the outline a walk is: the suffix that keeps repeated positions apart ("#2" for item 2
/// of a For each, "@3" for pass 3 of a Repeat until), the For each item, and the call depth.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ctx {
    suffix: String,
    item: Option<String>,
    item_artefact: Option<ArtefactId>,
    depth: usize,
}

impl Ctx {
    pub(crate) fn root() -> Ctx {
        Ctx::default()
    }

    fn key(&self, position: &str) -> String {
        format!("{position}{}", self.suffix)
    }
}

/// D.5: positions of a called workflow are numbered under the block (`3.1`, `3.1.1`).
fn rebase(steps: &[WorkflowStep], prefix: &str) -> Vec<WorkflowStep> {
    let at = |p: &str| format!("{prefix}.{p}");
    let use_ = |u: &StepUse| match u {
        StepUse::Input { label } => StepUse::Input {
            label: label.clone(),
        },
        StepUse::Result { position } => StepUse::Result {
            position: at(position),
        },
    };
    let condition = |c: &Condition| match c {
        Condition::Application {
            condition_kind,
            looks_at_position,
        } => Condition::Application {
            condition_kind: condition_kind.clone(),
            looks_at_position: at(looks_at_position),
        },
        other => other.clone(),
    };
    steps
        .iter()
        .map(|step| match step {
            WorkflowStep::Agent(a) => {
                let mut a = a.clone();
                a.position = at(&a.position);
                a.uses = a.uses.iter().map(use_).collect();
                WorkflowStep::Agent(a)
            }
            WorkflowStep::If(b) => WorkflowStep::If(IfBlock {
                position: at(&b.position),
                condition: condition(&b.condition),
                paths: b
                    .paths
                    .iter()
                    .map(|p| ordin_types::domain::runs::NamedPath {
                        name: p.name.clone(),
                        steps: rebase(&p.steps, prefix),
                    })
                    .collect(),
            }),
            WorkflowStep::Each(b) => WorkflowStep::Each(EachBlock {
                position: at(&b.position),
                list: use_(&b.list),
                maximum: b.maximum,
                steps: rebase(&b.steps, prefix),
            }),
            WorkflowStep::Until(b) => WorkflowStep::Until(UntilBlock {
                position: at(&b.position),
                condition: condition(&b.condition),
                maximum: b.maximum,
                steps: rebase(&b.steps, prefix),
            }),
            WorkflowStep::Call(b) => WorkflowStep::Call(CallBlock {
                position: at(&b.position),
                workflow_id: b.workflow_id,
            }),
        })
        .collect()
}

fn settled(state: &str) -> bool {
    matches!(state, "done" | "skipped" | "changed_by_ordin")
}

impl Crew {
    /// The whole mode: the outline, then a take-over into the coordinated loop when the steps do
    /// not fit. Returns why the crew stops.
    pub(crate) async fn run(&self, settle_follow: Option<bool>) -> Stop {
        if let Some(follow) = settle_follow {
            if let Err(stop) = self.settle_open_round(follow).await {
                return stop;
            }
        }
        let saved = self.state().mode == CrewMode::SavedSteps;
        if saved {
            match self.walk(&self.charter.steps, &Ctx::root()).await {
                Ok(()) => {
                    return Stop::Ended(EndKind::new(CrewRunState::Finished, "all steps done"));
                }
                Err(Stop::TakeOver { position, reason }) => {
                    if let Err(stop) = self.take_over(position, reason).await {
                        return stop;
                    }
                }
                Err(other) => return other,
            }
        }
        self.coordinated().await
    }

    /// D.6. Ordin takes over from `position` and coordinates round by round from now on.
    async fn take_over(&self, position: String, reason: TakeOverReason) -> Flow<()> {
        crew_log!(
            info,
            crew_run_id = %self.id,
            position = position.as_str(),
            reason = crate::store::enum_text(&reason_kind(&reason)).as_str(),
            "crew_taken_over"
        );
        let pool = self.o.pool()?;
        crate::store::set_taken_over(pool, self.id, &position).await?;
        {
            let mut st = self.state();
            st.mode = CrewMode::Coordinated;
            st.taken_over = true;
            st.position = None;
        }
        let view = self.view();
        self.ask(true, || {
            self.coordinator
                .take_over(&view, position.clone(), reason.clone())
        })
        .await
    }

    /// A step boundary: the crew may be cancelled, and Ordin may have asked to take over (D.8).
    fn boundary(&self) -> Flow<()> {
        self.check_cancelled()?;
        let pending = lock(&self.live.pending_take_over).take();
        if let Some((position, reason)) = pending {
            return Err(Stop::TakeOver {
                position: position.clone(),
                reason: TakeOverReason::OrdinAsked { position, reason },
            });
        }
        Ok(())
    }

    /// An answer of Ordin that could not be used: journalled with the position, then a take-over.
    async fn unusable(&self, position: &str) -> Stop {
        self.journal(
            "crew_coordinator_unusable",
            "Ordin's answer could not be used.",
            serde_json::json!({ "crew_run_id": self.id, "position": position, "code": "CREW_COORDINATOR_UNUSABLE" }),
        )
        .await;
        Stop::TakeOver {
            position: position.to_owned(),
            reason: TakeOverReason::CheckFoundShortfall {
                position: position.to_owned(),
                reason: UNUSABLE_WORDS.to_owned(),
            },
        }
    }

    pub(crate) fn walk<'a>(
        &'a self,
        steps: &'a [WorkflowStep],
        ctx: &'a Ctx,
    ) -> BoxFuture<'a, Flow<()>> {
        Box::pin(async move {
            let mut i = 0;
            while i < steps.len() {
                self.boundary()?;
                match &steps[i] {
                    WorkflowStep::Agent(first) => {
                        // Consecutive steps sharing a group start together (D.1).
                        let mut j = i + 1;
                        if let Some(group) = first.group {
                            while j < steps.len()
                                && matches!(&steps[j], WorkflowStep::Agent(b) if b.group == Some(group))
                            {
                                j += 1;
                            }
                        }
                        let members: Vec<&AgentStep> = steps[i..j]
                            .iter()
                            .filter_map(|s| match s {
                                WorkflowStep::Agent(a) => Some(a),
                                _ => None,
                            })
                            .collect();
                        self.run_group(&members, ctx).await?;
                        i = j;
                        continue;
                    }
                    WorkflowStep::If(block) => self.run_if(block, ctx).await?,
                    WorkflowStep::Each(block) => self.run_each(block, ctx).await?,
                    WorkflowStep::Until(block) => self.run_until(block, ctx).await?,
                    WorkflowStep::Call(block) => self.run_call(block, ctx).await?,
                }
                i += 1;
            }
            Ok(())
        })
    }

    // ---- D.1 agent steps

    /// Restores a settled step's outcome from its row.
    fn restore(&self, key: &str, position: &str) {
        let row = self.state().rows.get(key).cloned();
        if let Some(row) = row {
            if row.state == "skipped" {
                self.state().skipped.insert(position.to_owned());
            }
            if let Some(outcome) = row.outcome {
                self.remember(&outcome);
                let mut st = self.state();
                st.by_position.insert(key.to_owned(), outcome.clone());
                st.by_position.insert(position.to_owned(), outcome);
            }
        }
    }

    fn is_settled(&self, key: &str) -> bool {
        self.state()
            .rows
            .get(key)
            .is_some_and(|r| settled(&r.state))
    }

    /// One step or a group of steps that start together: one round (F.1 is checked before it).
    async fn run_group(&self, group: &[&AgentStep], ctx: &Ctx) -> Flow<()> {
        if group.iter().all(|s| self.is_settled(&ctx.key(&s.position))) {
            for step in group {
                self.restore(&ctx.key(&step.position), &step.position);
            }
            return Ok(());
        }
        self.stop_if_limit()?;
        let Some(first) = group.first() else {
            return Ok(());
        };
        self.state().position = Some(first.position.clone());
        let names: Vec<&str> = group.iter().map(|s| s.position.as_str()).collect();
        let round = self
            .open_round(&format!(
                "step {}: {}",
                names.join(", "),
                cut(&first.line, 200)
            ))
            .await?;
        let results =
            futures::future::join_all(group.iter().map(|s| self.run_step(s, ctx, &round))).await;
        self.close_round(&round).await?;
        results.into_iter().collect()
    }

    /// `Ok` with the labels of what a step uses, or the label that is missing.
    #[allow(clippy::type_complexity)]
    fn resolve(&self, step: &AgentStep, ctx: &Ctx) -> Result<Step, String> {
        let mut labels = Vec::new();
        let mut earlier = Vec::new();
        for used in &step.uses {
            match used {
                StepUse::Input { label } => {
                    if !self.inputs.iter().any(|i| i.label == *label) {
                        return Err(label.clone());
                    }
                    labels.push(label.clone());
                }
                StepUse::Result { position } => {
                    let st = self.state();
                    let found = st
                        .by_position
                        .get(&ctx.key(position))
                        .or_else(|| st.by_position.get(position));
                    match found {
                        Some(o) => {
                            earlier.push((position.clone(), o.summary.clone(), o.artefacts.clone()))
                        }
                        None => return Err(position.clone()),
                    }
                }
            }
        }
        let (inputs, mut texts, mut attachments) = input_lines(&self.inputs, &labels);
        if let Some(item) = &ctx.item {
            texts.push(("item".to_owned(), cut(item, 1_000)));
        }
        if let Some(artefact) = ctx.item_artefact {
            attachments.push(artefact);
        }
        Ok(Step {
            member: step.agent,
            label: step.position.clone(),
            line: step.line.clone(),
            inputs,
            texts,
            earlier,
            attachments,
            model_override: None,
        })
    }

    /// The agent is present, at the version the workflow was saved with (D.1).
    async fn check_agent(&self, step: &AgentStep) -> Flow<()> {
        match self
            .o
            .host
            .agent_status(self.workspace(), &step.agent)
            .await
        {
            AgentAvailability::Missing => Err(Stop::TakeOver {
                position: step.position.clone(),
                reason: TakeOverReason::AgentMissing { member: step.agent },
            }),
            AgentAvailability::Present { version } => {
                let saved = self.saved_versions.iter().find(|(a, _)| *a == step.agent);
                match saved {
                    Some((_, was)) if *was != version => Err(Stop::TakeOver {
                        position: step.position.clone(),
                        reason: TakeOverReason::AgentChanged { member: step.agent },
                    }),
                    _ => Ok(()),
                }
            }
        }
    }

    /// ADR-062: a step that falls short is tried once on the next level.
    async fn stepped_up(&self, failed: &StepOutcome) -> Option<LevelChoice> {
        let view = self.o.supervisor.get(failed.run_id).await.ok()?;
        let adapter: AdapterKind = view.run.adapter;
        // The level the first run used is not kept; the planner's own `choose` gives it again,
        // and `step_up` returns the next level from it.
        let previous = self.o.usage.choose(WorkKind::Documents, adapter);
        self.o
            .usage
            .step_up(&previous, WorkKind::Documents, adapter)
    }

    async fn run_step(&self, step: &AgentStep, ctx: &Ctx, round: &RoundCtx) -> Flow<()> {
        let key = ctx.key(&step.position);
        let position = step.position.clone();
        self.state()
            .agent_steps
            .insert(position.clone(), step.clone());
        let prior = self.state().rows.get(&key).cloned();
        if prior.as_ref().is_some_and(|r| settled(&r.state)) {
            self.restore(&key, &position);
            return Ok(());
        }
        let mut brief = match self.resolve(step, ctx) {
            Ok(brief) => brief,
            Err(label) => {
                return Err(Stop::TakeOver {
                    position,
                    reason: TakeOverReason::InputMissing { label },
                });
            }
        };
        self.check_agent(step).await?;
        if (step.asks_first || self.needs_approval(Some(&position), &step.agent))
            && !self
                .approve_step(round.number, &position, &step.agent, &step.line)
                .await?
        {
            return Err(Crew::declined(&position));
        }
        let mut attempts = prior.as_ref().map(|r| r.attempts).unwrap_or(0);
        let mut follow = prior
            .as_ref()
            .and_then(|r| r.run_id.filter(|_| r.state == "running"));
        let mut asked = false;
        let outcome = loop {
            let result = match follow.take() {
                Some(run_id) => self.follow_run(run_id, step.agent, &position).await?,
                None => {
                    attempts += 1;
                    self.put_step(StepRow {
                        position: key.clone(),
                        state: "running".to_owned(),
                        attempts,
                        run_id: None,
                        outcome: None,
                        decided_path: None,
                        passes: None,
                    })
                    .await?;
                    self.dispatch(round, &brief).await?
                }
            };
            self.check_cancelled()?;
            let failed = is_failure(result.state) || result.state == RunState::Cancelled;
            if !failed {
                break result;
            }
            crew_log!(info, crew_run_id = %self.id, position = position.as_str(), attempts = attempts, "crew_step_failed");
            brief.model_override = None;
            if attempts < 2 {
                // The retry asks for the next level when the first run was on a lower one.
                brief.model_override = self.stepped_up(&result).await;
                continue;
            }
            match step.if_it_fails {
                IfItFails::OrdinTakesOver => {
                    return Err(Stop::TakeOver {
                        position: position.clone(),
                        reason: TakeOverReason::StepFailedTwice { position },
                    });
                }
                IfItFails::SkipIt => return self.skip(&key, &position, attempts, result).await,
                IfItFails::AskMe => {
                    if asked {
                        return Err(Stop::TakeOver {
                            position: position.clone(),
                            reason: TakeOverReason::StepFailedTwice { position },
                        });
                    }
                    let question = PersonQuestion {
                        kind: PersonQuestionKind::StepFailedTwice,
                        words: format!("Step {position} failed twice."),
                        choices: vec![TRY_AGAIN.to_owned(), SKIP_IT.to_owned(), STOP.to_owned()],
                    };
                    let view = self.view();
                    let choice = match self
                        .ask(false, || {
                            self.coordinator.ask_person(&view, question.clone())
                        })
                        .await
                    {
                        Ok(choice) => choice,
                        Err(Stop::Unusable(_)) => return Err(self.unusable(&position).await),
                        Err(other) => return Err(other),
                    };
                    match choice.as_str() {
                        TRY_AGAIN => {
                            asked = true;
                            brief.model_override = self.stepped_up(&result).await;
                        }
                        SKIP_IT => return self.skip(&key, &position, attempts, result).await,
                        STOP => {
                            return Err(Stop::Ended(EndKind::new(
                                CrewRunState::Escalated,
                                format!("The person chose to stop at step {position}."),
                            )));
                        }
                        _ => return Err(self.unusable(&position).await),
                    }
                }
            }
        };
        // Settled: the cursor row, the results.
        self.settle(&key, &position, attempts, "done", &outcome)
            .await?;
        if step.ordin_checks_result {
            let view = self.view();
            let verdict = match self
                .ask(false, || self.coordinator.check_result(&view, &outcome))
                .await
            {
                Ok(verdict) => verdict,
                Err(Stop::Unusable(_)) => return Err(self.unusable(&position).await),
                Err(other) => return Err(other),
            };
            match verdict {
                CheckVerdict::CarryOn { turns } => self.add_requests(turns),
                CheckVerdict::Shortfall { reason, turns } => {
                    self.add_requests(turns);
                    self.state().shortfalls.insert(position.clone());
                    return Err(Stop::TakeOver {
                        position: position.clone(),
                        reason: TakeOverReason::CheckFoundShortfall { position, reason },
                    });
                }
            }
        }
        Ok(())
    }

    async fn settle(
        &self,
        key: &str,
        position: &str,
        attempts: u32,
        state: &str,
        outcome: &StepOutcome,
    ) -> Flow<()> {
        {
            let mut st = self.state();
            st.by_position.insert(key.to_owned(), outcome.clone());
            st.by_position.insert(position.to_owned(), outcome.clone());
        }
        self.remember(outcome);
        self.put_step(StepRow {
            position: key.to_owned(),
            state: state.to_owned(),
            attempts,
            run_id: Some(outcome.run_id),
            outcome: Some(outcome.clone()),
            decided_path: None,
            passes: None,
        })
        .await
    }

    /// A step skipped after it failed (`skip_it`, or the person's "Skip it").
    async fn skip(&self, key: &str, position: &str, attempts: u32, last: StepOutcome) -> Flow<()> {
        self.state().skipped.insert(position.to_owned());
        self.settle(key, position, attempts, "skipped", &last).await
    }

    // ---- D.2 If

    /// A condition decided by the application: the first path holds, the second does not
    /// (SA-CREW-04).
    async fn application(&self, kind: &str, looks_at: &str, ctx: &Ctx) -> Flow<bool> {
        // spec: ordin-desktop/components/CrewOrchestrator#SA-CREW-03
        let outcome = {
            let st = self.state();
            st.by_position
                .get(&ctx.key(looks_at))
                .or_else(|| st.by_position.get(looks_at))
                .cloned()
        };
        Ok(match kind {
            "list_has_items" => outcome.is_some_and(|o| !o.artefacts.is_empty()),
            "step_failed" => {
                self.state().skipped.contains(looks_at)
                    || outcome
                        .is_some_and(|o| is_failure(o.state) || o.state == RunState::Cancelled)
            }
            "check_found_shortfall" => self.state().shortfalls.contains(looks_at),
            "document_changed" => {
                let step = self.state().agent_steps.get(looks_at).cloned();
                let label = step.and_then(|s| {
                    s.uses.iter().find_map(|u| match u {
                        StepUse::Input { label } => Some(label.clone()),
                        _ => None,
                    })
                });
                match label.and_then(|l| self.inputs.iter().find(|i| i.label == l)) {
                    Some(input) => self.o.host.document_changed(self.workspace(), input).await,
                    None => false,
                }
            }
            other => return Err(Stop::Unusable(format!("unknown condition kind {other}"))),
        })
    }

    /// Decides a condition with named paths; returns the chosen index, the reason and who decided.
    async fn decide(
        &self,
        position: &str,
        condition: &Condition,
        names: &[String],
        ctx: &Ctx,
        person_kind: PersonQuestionKind,
    ) -> Flow<(usize, String, &'static str)> {
        match condition {
            Condition::Application {
                condition_kind,
                looks_at_position,
            } => {
                let holds = self
                    .application(condition_kind, looks_at_position, ctx)
                    .await;
                let holds = match holds {
                    Ok(holds) => holds,
                    Err(Stop::Unusable(_)) => return Err(self.unusable(position).await),
                    Err(other) => return Err(other),
                };
                Ok((
                    if holds { 0 } else { 1 },
                    condition_kind.clone(),
                    "application",
                ))
            }
            Condition::Ordin { words } => {
                self.stop_if_limit()?;
                let round = self
                    .open_round(&format!("condition at {position}: {}", cut(words, 200)))
                    .await?;
                let view = self.view();
                let answer = self
                    .ask(false, || {
                        self.coordinator.decide_condition(&view, words, names)
                    })
                    .await;
                let answer = match answer {
                    Ok(answer) => answer,
                    Err(Stop::Unusable(_)) => {
                        self.close_round(&round).await?;
                        return Err(self.unusable(position).await);
                    }
                    Err(other) => return Err(other),
                };
                self.add_requests(answer.turns);
                self.close_round(&round).await?;
                match names.iter().position(|n| *n == answer.path) {
                    Some(index) => Ok((index, answer.reason, "ordin")),
                    None => Err(self.unusable(position).await),
                }
            }
            Condition::Person { words } => {
                self.stop_if_limit()?;
                let round = self
                    .open_round(&format!("condition at {position}: {}", cut(words, 200)))
                    .await?;
                let question = PersonQuestion {
                    kind: person_kind,
                    words: words.clone(),
                    choices: names.to_vec(),
                };
                let view = self.view();
                let chosen = self
                    .ask(false, || {
                        self.coordinator.ask_person(&view, question.clone())
                    })
                    .await;
                self.close_round(&round).await?;
                let chosen = match chosen {
                    Ok(chosen) => chosen,
                    Err(Stop::Unusable(_)) => return Err(self.unusable(position).await),
                    Err(other) => return Err(other),
                };
                match names.iter().position(|n| *n == chosen) {
                    Some(index) => Ok((index, "The person chose.".to_owned(), "person")),
                    None => Err(self.unusable(position).await),
                }
            }
        }
    }

    async fn run_if(&self, block: &IfBlock, ctx: &Ctx) -> Flow<()> {
        let key = ctx.key(&block.position);
        self.state().position = Some(block.position.clone());
        let names: Vec<String> = block.paths.iter().map(|p| p.name.clone()).collect();
        let stored = self
            .state()
            .rows
            .get(&key)
            .and_then(|r| r.decided_path.clone());
        let index = match stored.and_then(|p| names.iter().position(|n| *n == p)) {
            Some(index) => index,
            None => {
                let (index, reason, by) = self
                    .decide(
                        &block.position,
                        &block.condition,
                        &names,
                        ctx,
                        PersonQuestionKind::AskMe,
                    )
                    .await?;
                let path = names.get(index).cloned().unwrap_or_default();
                crew_log!(
                    info,
                    crew_run_id = %self.id,
                    position = block.position.as_str(),
                    path = path.as_str(),
                    decided_by = by,
                    "crew_path_taken"
                );
                self.journal(
                    "crew_path_taken",
                    "A path was taken.",
                    serde_json::json!({
                        "crew_run_id": self.id,
                        "position": block.position,
                        "path": path,
                        "reason": reason,
                        "decided_by": by,
                    }),
                )
                .await;
                self.put_step(StepRow {
                    position: key.clone(),
                    state: "running".to_owned(),
                    attempts: 0,
                    run_id: None,
                    outcome: None,
                    decided_path: Some(path),
                    passes: None,
                })
                .await?;
                index
            }
        };
        if let Some(path) = block.paths.get(index) {
            self.walk(&path.steps, ctx).await?;
        }
        Ok(())
    }

    // ---- D.3 For each

    fn items_of(&self, list: &StepUse, ctx: &Ctx) -> Vec<String> {
        match list {
            StepUse::Input { label } => match self.inputs.iter().find(|i| i.label == *label) {
                Some(input) => match &input.value {
                    CrewInputValue::List { items } => items.clone(),
                    CrewInputValue::Documents { artefacts } => {
                        artefacts.iter().map(|a| a.to_string()).collect()
                    }
                    CrewInputValue::Text { text } => vec![text.clone()],
                },
                None => Vec::new(),
            },
            StepUse::Result { position } => {
                let st = self.state();
                st.by_position
                    .get(&ctx.key(position))
                    .or_else(|| st.by_position.get(position))
                    .map(|o| o.artefacts.iter().map(|a| a.to_string()).collect())
                    .unwrap_or_default()
            }
        }
    }

    async fn run_each(&self, block: &EachBlock, ctx: &Ctx) -> Flow<()> {
        self.state().position = Some(block.position.clone());
        let mut items = self.items_of(&block.list, ctx);
        let maximum = block.maximum as usize;
        if items.len() > maximum {
            self.journal(
                "crew_items_cut",
                "A For each list was cut to its maximum.",
                serde_json::json!({
                    "crew_run_id": self.id,
                    "position": block.position,
                    "items": items.len(),
                    "maximum": block.maximum,
                }),
            )
            .await;
            items.truncate(maximum);
        }
        // Items run together; the members-at-once permits bound how many member runs go at once.
        let contexts: Vec<Ctx> = items
            .iter()
            .enumerate()
            .map(|(n, item)| Ctx {
                suffix: format!("{}#{}", ctx.suffix, n + 1),
                item: Some(item.clone()),
                item_artefact: ArtefactId::parse(item).ok(),
                depth: ctx.depth,
            })
            .collect();
        let results =
            futures::future::join_all(contexts.iter().map(|c| self.walk(&block.steps, c))).await;
        results.into_iter().collect()
    }

    // ---- D.4 Repeat until

    async fn run_until(&self, block: &UntilBlock, ctx: &Ctx) -> Flow<()> {
        let key = ctx.key(&block.position);
        self.state().position = Some(block.position.clone());
        let made = self
            .state()
            .rows
            .get(&key)
            .and_then(|r| r.passes)
            .unwrap_or(0);
        let names = vec![YES.to_owned(), NO.to_owned()];
        for pass in (made + 1)..=block.maximum {
            let sub = Ctx {
                suffix: format!("{}@{pass}", ctx.suffix),
                ..ctx.clone()
            };
            self.walk(&block.steps, &sub).await?;
            let (index, _, _) = self
                .decide(
                    &block.position,
                    &block.condition,
                    &names,
                    &sub,
                    PersonQuestionKind::RepeatUntil,
                )
                .await?;
            if index == 0 {
                return Ok(());
            }
            self.put_step(StepRow {
                position: key.clone(),
                state: "running".to_owned(),
                attempts: 0,
                run_id: None,
                outcome: None,
                decided_path: None,
                passes: Some(pass),
            })
            .await?;
        }
        // The maximum is reached without the condition holding (ADR-065).
        let question = PersonQuestion {
            kind: PersonQuestionKind::RepeatAtMaximum,
            words: format!(
                "Step {} ran {} times and the condition did not hold.",
                block.position, block.maximum
            ),
            choices: vec![CARRY_ON.to_owned(), STOP.to_owned()],
        };
        let view = self.view();
        let choice = match self
            .ask(false, || {
                self.coordinator.ask_person(&view, question.clone())
            })
            .await
        {
            Ok(choice) => choice,
            Err(Stop::Unusable(_)) => return Err(self.unusable(&block.position).await),
            Err(other) => return Err(other),
        };
        if choice == STOP {
            return Err(Stop::Ended(EndKind::new(
                CrewRunState::Escalated,
                format!("The person chose to stop at step {}.", block.position),
            )));
        }
        Ok(())
    }

    // ---- D.5 Run another workflow

    async fn run_call(&self, block: &CallBlock, ctx: &Ctx) -> Flow<()> {
        self.state().position = Some(block.position.clone());
        if ctx.depth >= MAX_CALL_DEPTH {
            return Err(Stop::Ended(
                EndKind::new(CrewRunState::Failed, "STEP_CALL_INVALID")
                    .with_code("STEP_CALL_INVALID"),
            ));
        }
        let Some(steps) = self
            .o
            .host
            .workflow_steps(self.workspace(), block.workflow_id)
            .await
        else {
            return Err(Stop::TakeOver {
                position: block.position.clone(),
                reason: TakeOverReason::InputMissing {
                    label: "workflow".to_owned(),
                },
            });
        };
        let rebased = rebase(&steps, &block.position);
        let sub = Ctx {
            depth: ctx.depth + 1,
            ..ctx.clone()
        };
        self.walk(&rebased, &sub).await
    }
}

/// The kind name of a reason, for the log line (never the free words).
fn reason_kind(reason: &TakeOverReason) -> String {
    match reason {
        TakeOverReason::StepFailedTwice { .. } => "step_failed_twice",
        TakeOverReason::CheckFoundShortfall { .. } => "check_found_shortfall",
        TakeOverReason::AgentMissing { .. } => "agent_missing",
        TakeOverReason::AgentChanged { .. } => "agent_changed",
        TakeOverReason::InputMissing { .. } => "input_missing",
        TakeOverReason::OrdinAsked { .. } => "ordin_asked",
    }
    .to_owned()
}
