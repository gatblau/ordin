//! Guardrails and validation as pure functions (CrewOrchestrator.md Internal Logic A.4, F, and the
//! 2C rules copied into *Shared Context*). Every decision is a function of the charter, the
//! counters and the call; nothing here reads a clock or a store.

use ordin_organisation_profile::CrewLimits;
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{CrewCharter, CrewPauseReason, CrewRunState, Guardrails, Round};

use crate::types::{CrewInput, LimitLowered};

/// 2D-4: crew members, not counting the coordinator.
pub(crate) const MAX_MEMBERS: usize = 8;
/// 2C: a charter holds 1 to 20 members (LD-47); the application bound of 8 is applied at start.
pub(crate) const MAX_CHARTER_MEMBERS: usize = 20;
/// 2D-4: application maximum of crew rounds.
pub(crate) const MAX_ROUNDS: u32 = 30;
/// 2D-4: application maximum of the crew wall clock, 8 hours.
pub(crate) const MAX_WALL_CLOCK_SECS: u32 = 8 * 3600;
/// 2D-4: application maximum of members running at once.
pub(crate) const MAX_MEMBERS_AT_ONCE: u32 = 8;
/// 2C: `end_reason` holds at most 500 characters.
pub(crate) const MAX_END_REASON: usize = 500;
/// 2C: `coordinator_decision` holds 1 to 4,000 characters.
pub(crate) const MAX_DECISION: usize = 4_000;
/// 2C: a purpose holds 1 to 2,000 characters.
pub(crate) const MAX_PURPOSE: usize = 2_000;
// `ROUND_TOO_MANY_RUNNING` is registered to this crate (2C) and nothing raises it;
// `max_members_running` is enforced by a permit (F.2: the step waits for a free place, it is not
// refused).
/// 2C: a member role holds 1 to 200 characters.
pub(crate) const MAX_ROLE: usize = 200;

/// The three limits of F.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LimitHit {
    Rounds,
    Time,
    Requests,
}

/// The counters a guardrail decision reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counters {
    pub round_counter: u32,
    pub requests_used: u32,
    pub wall_clock_used_secs: u32,
    pub repeated_failures: u32,
}

impl LimitHit {
    /// The end reason stored in English for Diagnostics (the screens resolve `crew-end-*`).
    pub(crate) fn end_reason(self, g: &Guardrails) -> String {
        match self {
            LimitHit::Rounds => format!("Stopped after {} rounds.", g.max_rounds),
            LimitHit::Time if g.wall_clock_secs.is_multiple_of(60) => {
                format!("Stopped after {} minutes.", g.wall_clock_secs / 60)
            }
            LimitHit::Time => format!("Stopped after {} seconds.", g.wall_clock_secs),
            LimitHit::Requests => format!("Stopped after {} requests.", g.request_budget),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            LimitHit::Rounds => "rounds",
            LimitHit::Time => "time",
            LimitHit::Requests => "requests",
        }
    }
}

/// F.1: round, time and request limits, checked before each round or step.
// spec: ordin-desktop/components/CrewOrchestrator#SA-CREW-06
pub(crate) fn check_limits(g: &Guardrails, c: &Counters) -> Option<LimitHit> {
    if c.round_counter >= g.max_rounds {
        return Some(LimitHit::Rounds);
    }
    if c.wall_clock_used_secs >= g.wall_clock_secs {
        return Some(LimitHit::Time);
    }
    if g.request_budget > 0 && c.requests_used >= g.request_budget {
        return Some(LimitHit::Requests);
    }
    None
}

/// Why a `delegate` was refused (F.2); the words are the ones the Error Table gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    NotMember,
    Handoff,
    Input,
}

impl Refusal {
    pub(crate) fn words(self) -> &'static str {
        match self {
            Refusal::NotMember => "This agent is not in the crew.",
            Refusal::Handoff => "This hand-off is not allowed.",
            Refusal::Input => "This input is not part of the crew.",
        }
    }
}

/// F.2: is the member in the crew, is every hand-off allowed (an empty `allowed_handoffs` allows
/// none between members), and is every input label one of the crew's.
// spec: ordin-desktop/components/CrewOrchestrator#SA-CREW-07
pub(crate) fn check_delegate(
    charter: &CrewCharter,
    inputs: &[CrewInput],
    member: &AgentRef,
    uses_results_of: &[AgentRef],
    uses_inputs: &[String],
) -> Result<(), Refusal> {
    if !charter.members.iter().any(|m| m.member == *member) {
        return Err(Refusal::NotMember);
    }
    for from in uses_results_of {
        let allowed = charter
            .allowed_handoffs
            .iter()
            .any(|h| h.from == *from && h.to == *member);
        if !allowed {
            return Err(Refusal::Handoff);
        }
    }
    for label in uses_inputs {
        if !inputs.iter().any(|i| i.label == *label) {
            return Err(Refusal::Input);
        }
    }
    Ok(())
}

/// A.4: each guardrail above the company maximum (`limits.crew.*`) or the application maximum is
/// lowered to it, and each lowering is listed (07: lowered and said so).
pub(crate) fn lower_guardrails(
    g: &Guardrails,
    company: &CrewLimits,
) -> (Guardrails, Vec<LimitLowered>) {
    let mut out = g.clone();
    let mut lowered = Vec::new();
    let rounds_cap = company.rounds.min(MAX_ROUNDS);
    if g.max_rounds > rounds_cap {
        lowered.push(LimitLowered {
            limit: "limits.crew.rounds".into(),
            asked: g.max_rounds,
            maximum: rounds_cap,
        });
        out.max_rounds = rounds_cap;
    }
    let clock_cap = company
        .wall_clock_minutes
        .saturating_mul(60)
        .min(MAX_WALL_CLOCK_SECS);
    if g.wall_clock_secs > clock_cap {
        lowered.push(LimitLowered {
            limit: "limits.crew.wall_clock_minutes".into(),
            asked: g.wall_clock_secs,
            maximum: clock_cap,
        });
        out.wall_clock_secs = clock_cap;
    }
    let members_cap = company.members_at_once.min(MAX_MEMBERS_AT_ONCE);
    if g.max_members_running > members_cap {
        lowered.push(LimitLowered {
            limit: "limits.crew.members_at_once".into(),
            asked: g.max_members_running,
            maximum: members_cap,
        });
        out.max_members_running = members_cap;
    }
    if let Some(cap) = company.request_budget {
        // A charter budget of 0 means "none", which is above any company maximum.
        if g.request_budget == 0 || g.request_budget > cap {
            lowered.push(LimitLowered {
                limit: "limits.crew.request_budget".into(),
                asked: g.request_budget,
                maximum: cap,
            });
            out.request_budget = cap;
        }
    }
    (out, lowered)
}

/// The 2C charter rules copied into *Shared Context*; the first broken rule's code.
pub(crate) fn validate_charter(charter: &CrewCharter) -> Result<(), &'static str> {
    let chars = |s: &str| s.chars().count();
    if !(1..=MAX_PURPOSE).contains(&chars(&charter.purpose)) {
        return Err("CHRT_PURPOSE_INVALID");
    }
    if charter.members.is_empty() {
        return Err("CHRT_MEMBER_UNKNOWN");
    }
    if charter.members.len() > MAX_CHARTER_MEMBERS {
        return Err("CREW_TOO_MANY_MEMBERS");
    }
    if charter
        .members
        .iter()
        .any(|m| !(1..=MAX_ROLE).contains(&chars(&m.role)))
    {
        return Err("CHRT_ROLE_INVALID");
    }
    let is_member = |a: &AgentRef| charter.members.iter().any(|m| m.member == *a);
    if charter
        .allowed_handoffs
        .iter()
        .any(|h| !is_member(&h.from) || !is_member(&h.to))
    {
        return Err("CHRT_HANDOFF_INVALID");
    }
    let g = &charter.guardrails;
    // 2C names `CHRT_GUARDRAIL_ABOVE_MAX` for "each guardrail >= 1 and at or below the maximum";
    // a guardrail below 1 is reported with it.
    if g.max_rounds < 1
        || g.wall_clock_secs < 1
        || g.max_members_running < 1
        || g.failure_repeat_limit < 1
    {
        return Err("CHRT_GUARDRAIL_ABOVE_MAX");
    }
    if g.members_allowed_external_actions
        .iter()
        .any(|a| !is_member(a))
    {
        return Err("CHRT_EXTERNAL_NOT_MEMBER");
    }
    if charter.workflow_id.is_some() != charter.workflow_version.is_some() {
        return Err("CREW_WORKFLOW_MISMATCH");
    }
    Ok(())
}

/// The 2C crew-run rules, checked before a `crew_runs` write.
pub(crate) fn check_crew_run(
    g: &Guardrails,
    c: &Counters,
    state: CrewRunState,
    pause_reason: Option<CrewPauseReason>,
    end_reason: Option<&str>,
    workflow: (bool, bool),
) -> Result<(), &'static str> {
    if workflow.0 != workflow.1 {
        return Err("CREW_WORKFLOW_MISMATCH");
    }
    if c.round_counter > g.max_rounds {
        return Err("CREW_ROUNDS_EXCEEDED");
    }
    if g.request_budget > 0 && c.requests_used > g.request_budget {
        return Err("CREW_BUDGET_EXCEEDED");
    }
    if (state == CrewRunState::Paused) != pause_reason.is_some() {
        return Err("CREW_PAUSE_REASON_MISMATCH");
    }
    if end_reason.is_some_and(|r| r.chars().count() > MAX_END_REASON) {
        return Err("CREW_END_REASON_TOO_LONG");
    }
    Ok(())
}

/// The 2C round rules: numbers run from 1 with no gaps, and the decision holds 1 to 4,000
/// characters. `previous` is the number of the last round already stored.
pub(crate) fn check_round(previous: u32, round: &Round) -> Result<(), &'static str> {
    if round.number != previous + 1 {
        return Err("ROUND_NUMBER_GAP");
    }
    if !(1..=MAX_DECISION).contains(&round.coordinator_decision.chars().count()) {
        return Err("ROUND_DECISION_INVALID");
    }
    Ok(())
}

/// The registry code for a step-rule code carried as text in a [`crate::PlanProblem`].
pub(crate) fn step_code(code: &str) -> &'static str {
    crate::error::ALL_CODES
        .iter()
        .copied()
        .find(|known| *known == code)
        .unwrap_or("CREW_CHARTER_INVALID")
}

/// A stable text for an agent, used in work keys and position-free messages.
pub(crate) fn agent_key(agent: &AgentRef) -> String {
    match agent {
        AgentRef::AgentInstance { agent_instance_id } => agent_instance_id.to_string(),
        AgentRef::RemoteAgent { remote_agent_id } => remote_agent_id.to_string(),
    }
}

/// Cuts `text` to at most `max` characters.
pub(crate) fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::domain::runs::{CoordinatorKind, CrewCharterState, CrewMember, Handoff};
    use ordin_types::time::Timestamp;
    use ordin_types::{AgentInstanceId, CrewCharterId, WorkspaceId};

    fn agent() -> AgentRef {
        AgentRef::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        }
    }

    pub(crate) fn guardrails() -> Guardrails {
        Guardrails {
            max_rounds: 10,
            wall_clock_secs: 3600,
            request_budget: 0,
            max_members_running: 4,
            members_allowed_external_actions: vec![],
            approval_points: vec![],
            failure_repeat_limit: 2,
        }
    }

    fn charter(a: AgentRef, b: AgentRef) -> CrewCharter {
        CrewCharter {
            id: CrewCharterId::new(),
            workspace_id: WorkspaceId::new(),
            name: None,
            purpose: "Compare quotes".into(),
            charter_version: 1,
            coordinator: CoordinatorKind::Ordin,
            members: vec![
                CrewMember {
                    member: a,
                    role: "analyst".into(),
                },
                CrewMember {
                    member: b,
                    role: "writer".into(),
                },
            ],
            allowed_handoffs: vec![Handoff { from: a, to: b }],
            guardrails: guardrails(),
            input_fields: vec![],
            workflow_id: None,
            workflow_version: None,
            steps: vec![],
            state: CrewCharterState::Valid,
            written_at: Timestamp::now(),
        }
    }

    #[test]
    fn limits_are_checked_in_the_order_rounds_time_requests() {
        let mut g = guardrails();
        g.request_budget = 5;
        let mut c = Counters::default();
        assert_eq!(check_limits(&g, &c), None);
        c.requests_used = 5;
        assert_eq!(check_limits(&g, &c), Some(LimitHit::Requests));
        c.wall_clock_used_secs = 3600;
        assert_eq!(check_limits(&g, &c), Some(LimitHit::Time));
        c.round_counter = 10;
        assert_eq!(check_limits(&g, &c), Some(LimitHit::Rounds));
        assert_eq!(LimitHit::Rounds.end_reason(&g), "Stopped after 10 rounds.");
        assert_eq!(LimitHit::Time.end_reason(&g), "Stopped after 60 minutes.");
        assert_eq!(
            LimitHit::Requests.end_reason(&g),
            "Stopped after 5 requests."
        );
        assert_eq!(LimitHit::Time.name(), "time");
    }

    #[test]
    fn no_request_budget_never_stops_on_requests() {
        let g = guardrails();
        let c = Counters {
            requests_used: 9_999,
            ..Counters::default()
        };
        assert_eq!(check_limits(&g, &c), None);
    }

    #[test]
    fn delegate_checks_follow_the_charter() {
        let (a, b) = (agent(), agent());
        let c = charter(a, b);
        assert_eq!(check_delegate(&c, &[], &a, &[], &[]), Ok(()));
        assert_eq!(check_delegate(&c, &[], &b, &[a], &[]), Ok(()));
        assert_eq!(
            check_delegate(&c, &[], &a, &[b], &[]),
            Err(Refusal::Handoff)
        );
        assert_eq!(
            check_delegate(&c, &[], &agent(), &[], &[]),
            Err(Refusal::NotMember)
        );
        assert_eq!(
            check_delegate(&c, &[], &a, &[], &["Quotes".into()]),
            Err(Refusal::Input)
        );
        assert_eq!(Refusal::NotMember.words(), "This agent is not in the crew.");
        assert_eq!(Refusal::Handoff.words(), "This hand-off is not allowed.");
        assert_eq!(
            Refusal::Input.words(),
            "This input is not part of the crew."
        );
    }

    #[test]
    fn an_empty_handoff_list_allows_no_handoff() {
        let (a, b) = (agent(), agent());
        let mut c = charter(a, b);
        c.allowed_handoffs.clear();
        assert_eq!(
            check_delegate(&c, &[], &b, &[a], &[]),
            Err(Refusal::Handoff)
        );
    }

    #[test]
    fn guardrails_above_a_maximum_are_lowered_and_listed() {
        let mut g = guardrails();
        g.max_rounds = 30;
        g.wall_clock_secs = 8 * 3600;
        g.max_members_running = 8;
        let company = CrewLimits {
            rounds: 10,
            wall_clock_minutes: 60,
            members_at_once: 4,
            request_budget: Some(100),
        };
        let (lowered, list) = lower_guardrails(&g, &company);
        assert_eq!(lowered.max_rounds, 10);
        assert_eq!(lowered.wall_clock_secs, 3600);
        assert_eq!(lowered.max_members_running, 4);
        assert_eq!(lowered.request_budget, 100);
        assert_eq!(list.len(), 4);
        let (same, none) = lower_guardrails(&lowered, &company);
        assert_eq!(same, lowered);
        assert!(none.is_empty());
    }

    #[test]
    fn the_application_maximum_applies_above_a_generous_company_maximum() {
        let mut g = guardrails();
        g.max_rounds = 99;
        let company = CrewLimits {
            rounds: 30,
            wall_clock_minutes: 480,
            members_at_once: 8,
            request_budget: None,
        };
        let (lowered, list) = lower_guardrails(&g, &company);
        assert_eq!(lowered.max_rounds, 30);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn charter_rules_report_the_first_broken_rule() {
        let (a, b) = (agent(), agent());
        let good = charter(a, b);
        assert_eq!(validate_charter(&good), Ok(()));
        let mut c = good.clone();
        c.purpose.clear();
        assert_eq!(validate_charter(&c), Err("CHRT_PURPOSE_INVALID"));
        let mut c = good.clone();
        c.members.clear();
        assert_eq!(validate_charter(&c), Err("CHRT_MEMBER_UNKNOWN"));
        let mut c = good.clone();
        c.members[0].role = "r".repeat(201);
        assert_eq!(validate_charter(&c), Err("CHRT_ROLE_INVALID"));
        let mut c = good.clone();
        c.allowed_handoffs.push(Handoff {
            from: a,
            to: agent(),
        });
        assert_eq!(validate_charter(&c), Err("CHRT_HANDOFF_INVALID"));
        let mut c = good.clone();
        c.guardrails.max_rounds = 0;
        assert_eq!(validate_charter(&c), Err("CHRT_GUARDRAIL_ABOVE_MAX"));
        let mut c = good.clone();
        c.guardrails.members_allowed_external_actions = vec![agent()];
        assert_eq!(validate_charter(&c), Err("CHRT_EXTERNAL_NOT_MEMBER"));
        let mut c = good.clone();
        c.workflow_version = Some(1);
        assert_eq!(validate_charter(&c), Err("CREW_WORKFLOW_MISMATCH"));
        let mut c = good;
        c.members = (0..21)
            .map(|_| CrewMember {
                member: agent(),
                role: "x".into(),
            })
            .collect();
        assert_eq!(validate_charter(&c), Err("CREW_TOO_MANY_MEMBERS"));
    }

    #[test]
    fn crew_run_rules_are_checked_before_a_write() {
        let g = guardrails();
        let c = Counters::default();
        let ok = |s, p, e: Option<&str>, w| check_crew_run(&g, &c, s, p, e, w);
        assert_eq!(
            ok(CrewRunState::Running, None, None, (false, false)),
            Ok(())
        );
        assert_eq!(
            ok(CrewRunState::Running, None, None, (true, false)),
            Err("CREW_WORKFLOW_MISMATCH")
        );
        assert_eq!(ok(CrewRunState::Running, None, None, (true, true)), Ok(()));
        assert_eq!(
            ok(CrewRunState::Paused, None, None, (false, false)),
            Err("CREW_PAUSE_REASON_MISMATCH")
        );
        assert_eq!(
            ok(
                CrewRunState::Running,
                Some(CrewPauseReason::Restart),
                None,
                (false, false)
            ),
            Err("CREW_PAUSE_REASON_MISMATCH")
        );
        let long = "x".repeat(501);
        assert_eq!(
            ok(CrewRunState::Failed, None, Some(&long), (false, false)),
            Err("CREW_END_REASON_TOO_LONG")
        );
        let over = Counters {
            round_counter: 11,
            ..Counters::default()
        };
        assert_eq!(
            check_crew_run(&g, &over, CrewRunState::Running, None, None, (false, false)),
            Err("CREW_ROUNDS_EXCEEDED")
        );
        let mut budgeted = g.clone();
        budgeted.request_budget = 3;
        let spent = Counters {
            requests_used: 4,
            ..Counters::default()
        };
        assert_eq!(
            check_crew_run(
                &budgeted,
                &spent,
                CrewRunState::Running,
                None,
                None,
                (false, false)
            ),
            Err("CREW_BUDGET_EXCEEDED")
        );
    }

    #[test]
    fn round_rules_check_the_number_and_the_decision() {
        let round = |number, decision: &str| Round {
            crew_run_id: ordin_types::CrewRunId::new(),
            number,
            coordinator_decision: decision.into(),
            guardrail_decisions: vec![],
            member_runs: vec![],
            opened_at: Timestamp::now(),
            closed_at: None,
        };
        assert_eq!(check_round(0, &round(1, "finish")), Ok(()));
        assert_eq!(check_round(1, &round(3, "finish")), Err("ROUND_NUMBER_GAP"));
        assert_eq!(check_round(0, &round(1, "")), Err("ROUND_DECISION_INVALID"));
        assert_eq!(
            check_round(0, &round(1, &"d".repeat(4_001))),
            Err("ROUND_DECISION_INVALID")
        );
    }

    #[test]
    fn cut_keeps_whole_characters() {
        assert_eq!(cut("héllo", 2), "hé");
        assert_eq!(cut("abc", 10), "abc");
    }
}
