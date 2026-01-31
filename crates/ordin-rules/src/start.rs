//! The start and the end of a run (RulesEngine.md *Internal Logic* B, E and G.3–G.4).

use std::collections::{BTreeMap, HashMap};

use ordin_approval::ApprovalRequest;
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{ApprovalAsker, RejectStyle};
use ordin_types::{ApprovalId, RunId};

use crate::engine::{lock, now_ms, RulesEngine};
use crate::error::RulesError;
use crate::eval::{EvalError, Evaluator};
use crate::facts::RunFacts;
use crate::load::{bare, ControlSet};
use crate::setting;
use crate::store;
use crate::types::{
    AdoptedRun, ControlSource, EndDecision, Proposal, RunStartDecision, RunStartFacts, StartOutcome,
};
use crate::vocab::{
    AdditiveOutcome, CheckKind, DecidingOutcome, DutyGate, GateOutcome, MessageRule, Point,
};
use crate::{LEASE_SECONDS, MAX_MATCH_CHARS};

/// A duty with no period counts its cycles per day.
const DEFAULT_CYCLE_PERIOD: u32 = 86_400;

/// Where a message rule came from, in the order B.6 ranks sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RuleOrigin {
    Package,
    Configuration,
    Setting,
}

impl RuleOrigin {
    fn label(self) -> &'static str {
        match self {
            RuleOrigin::Package => "package",
            RuleOrigin::Configuration => "configuration",
            RuleOrigin::Setting => "setting",
        }
    }
}

struct Candidate {
    rule: MessageRule,
    origin: RuleOrigin,
    /// `setting/<rule>` for a setting rule; the rule's name otherwise.
    control: String,
}

/// A start that stopped on a fact: the control that could not be evaluated.
struct Unavailable(String);

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// `YYYY-MM-DDTHH:MM:SSZ`.
fn heartbeat_time() -> String {
    let wire = Timestamp::now().to_wire();
    format!("{}Z", wire.chars().take(19).collect::<String>())
}

impl RulesEngine {
    /// DecisionPoint::RunStart. There is no ask at the start.
    pub async fn begin_run(&self, start: RunStartFacts) -> Result<RunStartDecision, RulesError> {
        // B.1
        let Some(set) = self.set_of(start.agent_instance_id) else {
            tracing::warn!(
                agent_instance_id = start.agent_instance_id.to_string(),
                code = "RULE_RULESET_NOT_LOADED",
                "rules_start_refused"
            );
            return Err(RulesError::RuleSetNotLoaded);
        };
        let restriction = lock(&self.restrictions).get(&start.run_id).copied();
        let mut facts = RunFacts {
            run_id: start.run_id,
            agent_instance_id: start.agent_instance_id,
            workspace_id: start.workspace_id,
            crew_charter_id: start.crew_charter_id,
            commands: start.command.iter().cloned().collect(),
            text_cut: cut(&start.text, MAX_MATCH_CHARS),
            scope: start.scope.clone(),
            asker: start.asker.clone(),
            asker_key: start
                .asker
                .clone()
                .unwrap_or_else(|| start.started_by_id.clone()),
            subset: None,
            single_call: false,
            limit_seconds: None,
            deciding_control: None,
            set_aside: Vec::new(),
            leases: Vec::new(),
            tools_called: BTreeMap::new(),
            tool_calls: Vec::new(),
            changed_work_folder: false,
            ended: false,
            started_at: now_ms(),
            refusals: BTreeMap::new(),
            restriction,
            end_approval: None,
        };
        self.save_run(&facts).await?;
        let mut decision = RunStartDecision {
            outcome: StartOutcome::Proceed,
            command: None,
            tool_subset: None,
            single_call: false,
            limit_seconds: None,
            actions_attached: Vec::new(),
            delivery: None,
            controls_decided: Vec::new(),
        };
        let mut ended_by: Option<(String, &'static str)> = None;
        if let Err(Unavailable(control)) = self
            .start_rounds(&set, &mut facts, &mut decision, &mut ended_by)
            .await
        {
            // B.10 A start is never let through on a fact that could not be read.
            self.release_leases(&mut facts).await;
            let _ = self.save_run(&facts).await;
            tracing::warn!(
                control = control.as_str(),
                error_code = "RULE_FACT_UNAVAILABLE",
                "rules_fact_unavailable"
            );
            return Err(RulesError::FactUnavailable { control });
        }
        if !matches!(decision.outcome, StartOutcome::Proceed) {
            self.release_leases(&mut facts).await;
            if let (Some(sink), Some((control, source))) = (self.metrics(), ended_by.as_ref()) {
                sink.add(
                    "wrightrun_runs_ended_by_control_total",
                    &[
                        ("agent", set.package_name.as_str()),
                        ("control", control.as_str()),
                        ("source", source),
                    ],
                    1,
                );
            }
        }
        // B.9
        self.save_run(&facts).await?;
        for (control, what) in &decision.controls_decided {
            tracing::info!(
                control = control.as_str(),
                what = what.as_str(),
                "rules_start_decided"
            );
        }
        let outcome = match &decision.outcome {
            StartOutcome::Proceed => "proceed",
            StartOutcome::Ended { .. } => "ended",
            StartOutcome::Replied { .. } => "replied",
        };
        self.journal_entry(
            "rules_run_start",
            Some(facts.run_id),
            Some(facts.workspace_id),
            "The rules engine decided the start of a run.",
            serde_json::json!({
                "run_id": facts.run_id.to_string(),
                "outcome": outcome,
                "controls_decided": decision.controls_decided,
            }),
        )
        .await;
        self.start_renewal(facts.run_id, facts.leases.clone());
        Ok(decision)
    }

    /// B.2–B.8, once for the first command and once more for a command a rule started.
    async fn start_rounds(
        &self,
        set: &ControlSet,
        facts: &mut RunFacts,
        decision: &mut RunStartDecision,
        ended_by: &mut Option<(String, &'static str)>,
    ) -> Result<(), Unavailable> {
        let mut command = facts.commands.first().cloned();
        let mut cache: HashMap<String, bool> = HashMap::new();
        let mut counts: HashMap<String, i64> = HashMap::new();
        for round in 0..2 {
            // B.2–B.4 Duty gates.
            if let Some(duty) = command.as_deref().and_then(|c| set.duty_of(c)).cloned() {
                if let Some((gate, ending)) = self.duty_gates(set, facts, &duty).await? {
                    if let Some(form) = &duty.heartbeat {
                        if form.endings.iter().any(|e| e == ending) {
                            self.heartbeat(facts, &duty, &form.entry, ending).await?;
                        }
                    }
                    tracing::info!(
                        control = gate.as_str(),
                        ending,
                        command = command.as_deref().unwrap_or_default(),
                        "rules_start_ended"
                    );
                    decision
                        .controls_decided
                        .push((gate.clone(), format!("ended:{ending}")));
                    decision.actions_attached.clear();
                    decision.outcome = StartOutcome::Ended {
                        control: gate.clone(),
                        ending: ending.to_owned(),
                    };
                    facts.deciding_control = Some(gate.clone());
                    *ended_by = Some((gate, "package"));
                    return Ok(());
                }
                let lock_file = format!("{}lock", duty.folder);
                let key = facts.key();
                match self.host.path_exists(&key, &lock_file).await {
                    Ok(true) => {
                        self.host
                            .remove(&key, &lock_file)
                            .await
                            .map_err(|_| Unavailable(format!("duty:{}:lock", duty.duty)))?;
                        tracing::info!(duty = duty.duty.as_str(), "rules_stale_lock_removed");
                    }
                    Ok(false) => {}
                    Err(_) => return Err(Unavailable(format!("duty:{}:lock", duty.duty))),
                }
            }
            // B.5 Message rules of this command.
            let mut candidates: Vec<Candidate> = set
                .message_rules
                .iter()
                .filter(|(rule, _)| rule.command == command)
                .map(|(rule, source)| Candidate {
                    rule: rule.clone(),
                    origin: if *source == ControlSource::Package {
                        RuleOrigin::Package
                    } else {
                        RuleOrigin::Configuration
                    },
                    control: rule.name.clone(),
                })
                .collect();
            if let Some(scope) = facts.scope.clone() {
                let kept = self
                    .host
                    .setting_rules(&scope)
                    .await
                    .map_err(|_| Unavailable("setting-rules".to_owned()))?;
                for (setting_name, rule) in kept {
                    if let Some(why) = setting::refusal(set, None, &rule) {
                        tracing::info!(
                            rule = format!("{setting_name}/{}", cut(&rule.name, 64)).as_str(),
                            reason = why,
                            "rules_rule_ignored"
                        );
                        continue;
                    }
                    if rule.command == command {
                        candidates.push(Candidate {
                            control: format!("setting/{}", rule.name),
                            rule,
                            origin: RuleOrigin::Setting,
                        });
                    }
                }
            }
            let mut matching = Vec::new();
            {
                let mut evaluator = Evaluator {
                    host: self.host.as_ref(),
                    pool: self.datastore.sqlite(),
                    set,
                    facts,
                    now_ms: now_ms(),
                    cache: std::mem::take(&mut cache),
                    counts: std::mem::take(&mut counts),
                };
                for candidate in candidates {
                    match evaluator
                        .all(&candidate.rule.conditions, command.as_deref())
                        .await
                    {
                        Ok(true) => matching.push(candidate),
                        Ok(false) => {}
                        Err(EvalError::Fact | EvalError::State) => {
                            return Err(Unavailable(candidate.control))
                        }
                    }
                }
                cache = std::mem::take(&mut evaluator.cache);
                counts = std::mem::take(&mut evaluator.counts);
            }
            // B.6 The deciding rule; B.7 additive outcomes in the same order.
            matching.sort_by(|a, b| {
                b.rule
                    .priority
                    .cmp(&a.rule.priority)
                    .then(a.origin.cmp(&b.origin))
                    .then(a.rule.name.cmp(&b.rule.name))
            });
            for candidate in &matching {
                for add in &candidate.rule.adds {
                    match add {
                        AdditiveOutcome::AttachAction { action } => {
                            if !decision.actions_attached.contains(action) {
                                decision.actions_attached.push(action.clone());
                                decision.controls_decided.push((
                                    candidate.control.clone(),
                                    format!("attach-action:{action}"),
                                ));
                            }
                        }
                        AdditiveOutcome::Delivery { to } => {
                            if decision.delivery.is_none() {
                                decision.delivery = Some(*to);
                                decision
                                    .controls_decided
                                    .push((candidate.control.clone(), "delivery".to_owned()));
                            }
                        }
                    }
                }
            }
            let Some(decider) = matching.iter().find(|c| c.rule.decides.is_some()) else {
                return Ok(());
            };
            let control = decider.control.clone();
            // B.8 Apply.
            match decider.rule.decides.clone() {
                Some(DecidingOutcome::End { ending }) => {
                    decision.actions_attached.clear();
                    decision
                        .controls_decided
                        .push((control.clone(), format!("ended:{ending}")));
                    decision.outcome = StartOutcome::Ended {
                        control: control.clone(),
                        ending,
                    };
                    facts.deciding_control = Some(control.clone());
                    *ended_by = Some((control, decider.origin.label()));
                    return Ok(());
                }
                Some(DecidingOutcome::Reply { text }) => {
                    decision
                        .controls_decided
                        .push((control.clone(), "reply".to_owned()));
                    decision.outcome = StartOutcome::Replied {
                        control: control.clone(),
                        text,
                    };
                    facts.deciding_control = Some(control.clone());
                    *ended_by = Some((control, decider.origin.label()));
                    return Ok(());
                }
                Some(DecidingOutcome::Command { command: next }) => {
                    if round == 1 {
                        tracing::info!(
                            rule = control.as_str(),
                            reason = "second-command",
                            "rules_rule_ignored"
                        );
                        return Ok(());
                    }
                    decision
                        .controls_decided
                        .push((control.clone(), format!("command:{next}")));
                    decision.command = Some(next.clone());
                    facts.commands.push(next.clone());
                    if let Some(minutes) = set.bounds.get(&next).and_then(|b| b.wall_clock_minutes)
                    {
                        let seconds = minutes.saturating_mul(60);
                        decision.limit_seconds =
                            Some(decision.limit_seconds.map_or(seconds, |l| l.min(seconds)));
                        // Kept for a second instance (SA-RECON-12).
                        facts.limit_seconds = decision.limit_seconds;
                    }
                    command = Some(next);
                }
                Some(DecidingOutcome::Tools { tools }) => {
                    let tools: Vec<String> = tools.iter().map(|t| bare(t).to_owned()).collect();
                    decision
                        .controls_decided
                        .push((control.clone(), "tools".to_owned()));
                    decision.tool_subset = Some(tools.clone());
                    facts.subset = Some(tools);
                    facts.deciding_control = Some(control);
                    return Ok(());
                }
                Some(DecidingOutcome::SingleCall { command: single }) => {
                    decision
                        .controls_decided
                        .push((control.clone(), format!("single-call:{single}")));
                    decision.command = Some(single.clone());
                    decision.single_call = true;
                    facts.single_call = true;
                    facts.commands.push(single);
                    facts.deciding_control = Some(control);
                    return Ok(());
                }
                None => return Ok(()),
            }
        }
        Ok(())
    }

    /// B.2: the first unmet duty gate and its ending, or None when every gate is met.
    async fn duty_gates(
        &self,
        set: &ControlSet,
        facts: &mut RunFacts,
        duty: &DutyGate,
    ) -> Result<Option<(String, &'static str)>, Unavailable> {
        let d = &duty.duty;
        let key = facts.key();
        let gate = |part: &str| format!("duty:{d}:{part}");
        // B.2.1, B.2.2
        for (part, file, ending) in [
            ("stop", "STOP", "stop-present"),
            ("escalation", "escalation.md", "escalation-present"),
        ] {
            let present = self
                .host
                .path_exists(&key, &format!("{}{file}", duty.folder))
                .await
                .map_err(|_| Unavailable(gate(part)))?;
            if present {
                return Ok(Some((gate(part), ending)));
            }
        }
        // B.2.3
        if let Some(check) = &duty.work_check {
            if check.kind == CheckKind::File {
                if let Some(condition) = &check.condition {
                    let mut evaluator = Evaluator {
                        host: self.host.as_ref(),
                        pool: self.datastore.sqlite(),
                        set,
                        facts,
                        now_ms: now_ms(),
                        cache: HashMap::new(),
                        counts: HashMap::new(),
                    };
                    let holds = evaluator
                        .holds(condition, Some(duty.command.as_str()))
                        .await
                        .map_err(|_| Unavailable(gate("work-check")))?;
                    if !holds {
                        return Ok(Some((gate("work-check"), "nothing-to-do")));
                    }
                }
            }
        }
        // B.2.4
        let pool = self.pool().map_err(|_| Unavailable(gate("lock")))?;
        let lock_key = crate::duty_lock_key(d);
        let taken = store::take_lock(
            pool,
            &lock_key,
            &facts.run_id.to_string(),
            now_ms(),
            i64::from(LEASE_SECONDS) * 1_000,
        )
        .await
        .map_err(|_| Unavailable(gate("lock")))?;
        if !taken {
            return Ok(Some((gate("lock"), "overlap")));
        }
        if !facts.leases.contains(&lock_key) {
            facts.leases.push(lock_key);
        }
        let commands = set.commands_of_duty(d);
        let mut in_progress: u32 = 0;
        for command in &commands {
            in_progress = in_progress.saturating_add(
                self.host
                    .runs_in_progress(command)
                    .await
                    .map_err(|_| Unavailable(gate("lock")))?,
            );
        }
        if in_progress > 1
            && self
                .host
                .paused_run_with_command(&commands, facts.run_id)
                .await
                .map_err(|_| Unavailable(gate("lock")))?
        {
            return Ok(Some((gate("lock"), "overlap")));
        }
        // B.2.5
        if let Some(max) = duty.max_cycles {
            // A duty with `max-cycles` and no `period-seconds` counts its cycles per day.
            let period = duty.period_seconds.unwrap_or(DEFAULT_CYCLE_PERIOD);
            let count = store::count_start(pool, "command", &lock_key_of(d), period, now_ms())
                .await
                .map_err(|_| Unavailable(gate("cycle-limit")))?;
            if count > i64::from(max) {
                return Ok(Some((gate("cycle-limit"), "cycle-limit")));
            }
        }
        Ok(None)
    }

    /// B.3: appends the heartbeat line.
    async fn heartbeat(
        &self,
        facts: &RunFacts,
        duty: &DutyGate,
        entry: &str,
        ending: &str,
    ) -> Result<(), Unavailable> {
        let key = facts.key();
        let path = format!("{}heartbeat.md", duty.folder);
        let control = || Unavailable(format!("duty:{}:heartbeat", duty.duty));
        let existing = self
            .host
            .read_text(&key, &path)
            .await
            .map_err(|_| control())?;
        let cycle = existing.as_deref().map_or(0, |text| {
            text.lines().filter(|l| !l.trim().is_empty()).count()
        }) + 1;
        let line = entry
            .replace("{time}", &heartbeat_time())
            .replace("{ending}", ending)
            .replace("{cycle}", &cycle.to_string());
        self.host
            .append_line(&key, &path, &line)
            .await
            .map_err(|_| control())
    }

    /// G.4. Some(gate) = refused, overlap.
    // spec: ordin-desktop/components/RulesEngine#SA-RECON-12
    pub async fn adopted_run(&self, run_id: RunId) -> Result<Option<AdoptedRun>, RulesError> {
        Ok(self.load_run(run_id).await?.map(|facts| AdoptedRun {
            agent_instance_id: facts.agent_instance_id,
            workspace_id: facts.workspace_id,
            scope: facts.scope,
            tool_subset: facts.subset,
            single_call: facts.single_call,
            limit_seconds: facts.limit_seconds,
        }))
    }

    pub async fn resume_run(&self, run_id: RunId) -> Result<Option<String>, RulesError> {
        let mut facts = self.load_run(run_id).await?.ok_or(RulesError::RunUnknown)?;
        let duty = self.set_of(facts.agent_instance_id).and_then(|set| {
            facts
                .commands
                .first()
                .and_then(|c| set.duty_of(c).map(|d| d.duty.clone()))
        });
        if let Some(duty) = duty {
            let key = crate::duty_lock_key(&duty);
            let taken = store::take_lock(
                self.pool()?,
                &key,
                &run_id.to_string(),
                now_ms(),
                i64::from(LEASE_SECONDS) * 1_000,
            )
            .await?;
            if !taken {
                let gate = format!("duty:{duty}:lock");
                tracing::info!(
                    control = gate.as_str(),
                    ending = "overlap",
                    "rules_resume_refused"
                );
                return Ok(Some(gate));
            }
            if !facts.leases.contains(&key) {
                facts.leases.push(key);
            }
        }
        facts.started_at = now_ms();
        self.save_run(&facts).await?;
        self.start_renewal(run_id, facts.leases.clone());
        Ok(None)
    }

    /// The run ended: releases leases and drops its facts.
    pub async fn end_run(&self, run_id: RunId) {
        if let Ok(Some(mut facts)) = self.load_run(run_id).await {
            self.release_leases(&mut facts).await;
        } else if let Some(task) = lock(&self.renewals).remove(&run_id) {
            task.abort();
        }
        lock(&self.runs).remove(&run_id);
        lock(&self.restrictions).remove(&run_id);
        if let Ok(pool) = self.pool() {
            if let Err(error) = store::delete_run(pool, &run_id.to_string()).await {
                tracing::warn!(
                    run_id = run_id.to_string(),
                    code = error.code(),
                    "rules_run_rows_kept"
                );
            }
        }
    }

    /// E.2: the unmet end gates, in order, without those set aside.
    async fn unmet_end_gates(&self, set: &ControlSet, facts: &RunFacts) -> Vec<(String, EndGate)> {
        let mut out = Vec::new();
        for command in &facts.commands {
            if let Some(judged) = set.judged_loops.get(command) {
                let reviewer_calls = facts.called(&format!("task:{}", judged.reviewer));
                if facts.changed_work_folder && reviewer_calls == 0 {
                    out.push((
                        format!("judged-loop:{command}"),
                        EndGate::Judged {
                            reviewer: judged.reviewer.clone(),
                            pause_after: judged.pause_after,
                        },
                    ));
                }
            }
        }
        let mut evaluator = Evaluator {
            host: self.host.as_ref(),
            pool: self.datastore.sqlite(),
            set,
            facts,
            now_ms: now_ms(),
            cache: HashMap::new(),
            counts: HashMap::new(),
        };
        for (gate, source) in set.steps.iter().filter(|(g, _)| g.point == Point::End) {
            let applies = match &gate.command {
                Some(c) => facts.commands.contains(c),
                None => facts.commands.is_empty(),
            };
            if !applies {
                continue;
            }
            // A condition that cannot be read keeps the gate unmet (fail closed).
            let holds = evaluator
                .all(&gate.conditions, facts.commands.first().map(String::as_str))
                .await
                .unwrap_or(true);
            if holds {
                let pause_after = match gate.outcome {
                    GateOutcome::RefuseEnding { pause_after } => pause_after,
                    GateOutcome::Refuse => 3,
                };
                out.push((
                    gate.name.clone(),
                    EndGate::Step {
                        pause_after,
                        source: *source,
                    },
                ));
            }
        }
        out.retain(|(name, _)| !facts.set_aside.contains(name));
        out
    }

    /// E. DecisionPoint::RunEnd.
    pub async fn decide_run_end(&self, run_id: RunId) -> EndDecision {
        let mut facts = match self.load_run(run_id).await {
            Ok(Some(facts)) => facts,
            _ => {
                tracing::info!(run_id = run_id.to_string(), "rules_run_unknown");
                return EndDecision::End;
            }
        };
        if facts.ended || facts.single_call {
            return self.finish(&mut facts).await;
        }
        let Some(set) = self.set_of(facts.agent_instance_id) else {
            return self.finish(&mut facts).await;
        };
        let gates = self.unmet_end_gates(&set, &facts).await;
        let Some((first, gate)) = gates.first().cloned() else {
            return self.finish(&mut facts).await;
        };
        let refusals = facts.refusals.get(&first).copied().unwrap_or(0);
        let limit = gate.pause_after();
        if refusals < limit {
            facts.refusals.insert(first.clone(), refusals + 1);
            let _ = self.save_run(&facts).await;
            tracing::info!(
                control = first.as_str(),
                refusals = u64::from(refusals + 1),
                "rules_ending_refused"
            );
            self.metric(
                &set,
                "wrightrun_run_endings_refused_total",
                &first,
                gate.source_label(),
            );
            return EndDecision::Continue {
                message: gate.message(&first),
                control: first,
            };
        }
        // E.5
        let names: Vec<String> = gates.iter().map(|(n, _)| n.clone()).collect();
        match self.open_end_approval(&facts, &names).await {
            Some(approval_id) => {
                facts.end_approval = Some(approval_id);
                let _ = self.save_run(&facts).await;
                EndDecision::Ask {
                    gates: names,
                    approval_id,
                }
            }
            // The approval cannot be opened: the ending stays refused (the gate holds, fail closed)
            // and the run goes on.
            None => EndDecision::Continue {
                message: gate.message(&first),
                control: first,
            },
        }
    }

    async fn open_end_approval(&self, facts: &RunFacts, gates: &[String]) -> Option<ApprovalId> {
        let proposal = Proposal::EndGatesSetAside {
            gates: gates.to_vec(),
        };
        let wording = self.host.word(&facts.key(), &proposal);
        let request = ApprovalRequest {
            workspace_id: facts.workspace_id,
            asked_by: ApprovalAsker::Run {
                run_id: facts.run_id,
            },
            question: wording.question,
            verb: wording.verb,
            where_line: wording.where_line,
            affects: wording.affects,
            changes: wording.changes,
            content: None,
            reject_style: RejectStyle::Reject,
            show_details: false,
            proposal: wording.proposal,
            asker: facts.asker.clone(),
            leaves_company: false,
        };
        match self.approvals.request(request).await {
            Ok(card) => Some(card.approval_id),
            Err(error) => {
                tracing::warn!(
                    run_id = facts.run_id.to_string(),
                    code = ordin_types::CodedError::code(&error),
                    "rules_end_approval_unavailable"
                );
                None
            }
        }
    }

    /// After an `EndDecision::Ask` is decided: approved sets the gates aside.
    pub async fn settle_end_gates(&self, run_id: RunId, approved: bool) -> EndDecision {
        let Ok(Some(mut facts)) = self.load_run(run_id).await else {
            return EndDecision::End;
        };
        let Some(set) = self.set_of(facts.agent_instance_id) else {
            return self.finish(&mut facts).await;
        };
        let gates = self.unmet_end_gates(&set, &facts).await;
        facts.end_approval = None;
        if approved {
            for (name, gate) in &gates {
                tracing::info!(control = name.as_str(), "rules_set_aside");
                self.metric(
                    &set,
                    "wrightrun_controls_set_aside_total",
                    name,
                    gate.source_label(),
                );
                facts.set_aside.push(name.clone());
            }
            return self.finish(&mut facts).await;
        }
        let Some((first, gate)) = gates.first().cloned() else {
            return self.finish(&mut facts).await;
        };
        facts.refusals.insert(first.clone(), 0);
        let _ = self.save_run(&facts).await;
        EndDecision::Continue {
            message: gate.message(&first),
            control: first,
        }
    }

    /// Releases the leases and lets the run end.
    async fn finish(&self, facts: &mut RunFacts) -> EndDecision {
        self.release_leases(facts).await;
        let _ = self.save_run(facts).await;
        EndDecision::End
    }

    fn metric(&self, set: &ControlSet, family: &str, control: &str, source: &str) {
        if let Some(sink) = self.metrics() {
            sink.add(
                family,
                &[
                    ("agent", set.package_name.as_str()),
                    ("control", control),
                    ("source", source),
                ],
                1,
            );
        }
    }
}

/// The cycle counter key of a duty (`duty:<d>`, B.2.5).
fn lock_key_of(duty: &str) -> String {
    crate::duty_lock_key(duty)
}

#[derive(Debug, Clone)]
enum EndGate {
    Judged {
        reviewer: String,
        pause_after: u8,
    },
    Step {
        pause_after: u8,
        source: ControlSource,
    },
}

impl EndGate {
    fn pause_after(&self) -> u8 {
        match self {
            EndGate::Judged { pause_after, .. } | EndGate::Step { pause_after, .. } => *pause_after,
        }
    }

    fn source_label(&self) -> &'static str {
        match self {
            EndGate::Step {
                source: ControlSource::Configuration,
                ..
            } => "configuration",
            _ => "package",
        }
    }

    /// E.4's fixed texts.
    fn message(&self, gate: &str) -> String {
        match self {
            EndGate::Judged { reviewer, .. } => format!(
                "The run cannot end yet. The control {gate} requires a call to the sub-agent {reviewer} before the run ends."
            ),
            EndGate::Step { .. } => format!("The run cannot end yet. The control {gate} is not met."),
        }
    }
}
