//! A. `check_plan`: does the workflow or crew fit (03 *Run a saved workflow*), and the 2C step
//! rules re-run on a charter's saved steps.

use std::collections::HashSet;

use futures::future::BoxFuture;
use ordin_organisation_profile::CrewLimits;
use ordin_types::domain::runs::{Condition, CrewCharter, StepUse, WorkflowStep};
use ordin_types::{WorkflowId, WorkspaceId};

use crate::guard::{lower_guardrails, MAX_MEMBERS};
use crate::types::{
    AgentAvailability, CrewHost, CrewInput, CrewInputValue, PlanCheck, PlanProblem,
};

/// 2C: steps and blocks in one outline.
const MAX_STEPS: usize = 50;
/// 2C: nesting of blocks inside blocks.
const MAX_NESTING: usize = 4;
/// 2C: a For each runs 1 to 50 items.
const EACH_MAX: u32 = 50;
/// 2C: a Repeat until runs 1 to 10 passes.
const UNTIL_MAX: u32 = 10;
/// 2C: an If has 2 to 4 paths.
const IF_PATHS: std::ops::RangeInclusive<usize> = 2..=4;
/// 2D-4: a workflow calls another to a depth of 3.
pub(crate) const MAX_CALL_DEPTH: usize = 3;
/// A group of steps that start at the same time holds at most the application maximum of members
/// at once (2D-4: 8). Assumed here; 2C names `STEP_GROUP_TOO_LARGE` without its bound.
const MAX_GROUP: usize = 8;
/// The application condition kinds (D.2).
pub(crate) const CONDITION_KINDS: [&str; 4] = [
    "list_has_items",
    "step_failed",
    "check_found_shortfall",
    "document_changed",
];

/// The code and position of the first broken step rule.
pub(crate) type Invalid = (&'static str, String);

struct Validator<'a> {
    inputs: &'a [CrewInput],
    count: usize,
    positions: HashSet<String>,
}

fn position_ok(position: &str) -> bool {
    !position.is_empty()
        && position.len() <= 32
        && position
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
}

impl Validator<'_> {
    fn node(&mut self, position: &str) -> Result<(), Invalid> {
        self.count += 1;
        if self.count > MAX_STEPS {
            return Err(("STEP_TOO_MANY", position.to_owned()));
        }
        if !position_ok(position) || !self.positions.insert(position.to_owned()) {
            return Err(("STEP_POSITION_INVALID", position.to_owned()));
        }
        Ok(())
    }

    fn condition(
        &self,
        position: &str,
        condition: &Condition,
        visible: &HashSet<String>,
    ) -> Result<(), Invalid> {
        let bad = || ("STEP_CONDITION_INVALID", position.to_owned());
        match condition {
            Condition::Application {
                condition_kind,
                looks_at_position,
            } => {
                if !CONDITION_KINDS.contains(&condition_kind.as_str())
                    || !visible.contains(looks_at_position)
                {
                    return Err(bad());
                }
            }
            Condition::Ordin { words } | Condition::Person { words } => {
                if words.trim().is_empty() {
                    return Err(bad());
                }
            }
        }
        Ok(())
    }

    fn list(
        &self,
        position: &str,
        list: &StepUse,
        visible: &HashSet<String>,
    ) -> Result<(), Invalid> {
        match list {
            StepUse::Input { label } => {
                let text_input = self
                    .inputs
                    .iter()
                    .any(|i| i.label == *label && matches!(i.value, CrewInputValue::Text { .. }));
                if text_input {
                    return Err(("STEP_EACH_NOT_LIST", position.to_owned()));
                }
            }
            StepUse::Result { position: earlier } => {
                if !visible.contains(earlier) {
                    return Err(("STEP_USES_LATER_OR_ABSENT", position.to_owned()));
                }
            }
        }
        Ok(())
    }

    fn steps(
        &mut self,
        steps: &[WorkflowStep],
        nesting: usize,
        visible: &mut HashSet<String>,
    ) -> Result<(), Invalid> {
        let mut group: Option<(u32, usize)> = None;
        for step in steps {
            match step {
                WorkflowStep::Agent(a) => {
                    self.node(&a.position)?;
                    if a.line.trim().is_empty() {
                        return Err(("STEP_LINE_INVALID", a.position.clone()));
                    }
                    for used in &a.uses {
                        if let StepUse::Result { position } = used {
                            if !visible.contains(position) {
                                return Err(("STEP_USES_LATER_OR_ABSENT", a.position.clone()));
                            }
                        }
                    }
                    group = match (a.group, group) {
                        (Some(g), Some((current, n))) if g == current => Some((g, n + 1)),
                        (Some(g), _) => Some((g, 1)),
                        (None, _) => None,
                    };
                    if group.is_some_and(|(_, n)| n > MAX_GROUP) {
                        return Err(("STEP_GROUP_TOO_LARGE", a.position.clone()));
                    }
                    visible.insert(a.position.clone());
                }
                WorkflowStep::If(b) => {
                    group = None;
                    self.node(&b.position)?;
                    self.block_depth(&b.position, nesting)?;
                    if !IF_PATHS.contains(&b.paths.len()) {
                        return Err(("STEP_IF_PATHS_OUT_OF_RANGE", b.position.clone()));
                    }
                    self.condition(&b.position, &b.condition, visible)?;
                    for path in &b.paths {
                        // A path's results are not certain after the block: they do not merge back.
                        let mut inner = visible.clone();
                        self.steps(&path.steps, nesting + 1, &mut inner)?;
                    }
                }
                WorkflowStep::Each(b) => {
                    group = None;
                    self.node(&b.position)?;
                    self.block_depth(&b.position, nesting)?;
                    if !(1..=EACH_MAX).contains(&b.maximum) {
                        return Err(("STEP_EACH_MAX_OUT_OF_RANGE", b.position.clone()));
                    }
                    self.list(&b.position, &b.list, visible)?;
                    self.steps(&b.steps, nesting + 1, visible)?;
                }
                WorkflowStep::Until(b) => {
                    group = None;
                    self.node(&b.position)?;
                    self.block_depth(&b.position, nesting)?;
                    if !(1..=UNTIL_MAX).contains(&b.maximum) {
                        return Err(("STEP_UNTIL_MAX_OUT_OF_RANGE", b.position.clone()));
                    }
                    self.steps(&b.steps, nesting + 1, visible)?;
                    self.condition(&b.position, &b.condition, visible)?;
                }
                WorkflowStep::Call(b) => {
                    group = None;
                    self.node(&b.position)?;
                    self.block_depth(&b.position, nesting)?;
                }
            }
        }
        Ok(())
    }

    fn block_depth(&self, position: &str, nesting: usize) -> Result<(), Invalid> {
        if nesting + 1 > MAX_NESTING {
            return Err(("STEP_NESTING_TOO_DEEP", position.to_owned()));
        }
        Ok(())
    }
}

/// The structural 2C step checks (everything except calls, which need the host).
pub(crate) fn validate_steps(steps: &[WorkflowStep], inputs: &[CrewInput]) -> Result<(), Invalid> {
    let mut v = Validator {
        inputs,
        count: 0,
        positions: HashSet::new(),
    };
    v.steps(steps, 0, &mut HashSet::new())
}

/// Calls: at most 3 levels deep and no cycle, reading each called workflow through the host.
pub(crate) fn validate_calls<'a>(
    host: &'a dyn CrewHost,
    workspace: WorkspaceId,
    steps: &'a [WorkflowStep],
    chain: &'a [WorkflowId],
    depth: usize,
) -> BoxFuture<'a, Result<(), Invalid>> {
    Box::pin(async move {
        for step in steps {
            match step {
                WorkflowStep::Call(call) => {
                    if chain.contains(&call.workflow_id) {
                        return Err(("STEP_CALL_CYCLE", call.position.clone()));
                    }
                    if depth >= MAX_CALL_DEPTH {
                        return Err(("STEP_CALL_INVALID", call.position.clone()));
                    }
                    let Some(inner) = host.workflow_steps(workspace, call.workflow_id).await else {
                        return Err(("STEP_CALL_INVALID", call.position.clone()));
                    };
                    let mut next = chain.to_vec();
                    next.push(call.workflow_id);
                    validate_calls(host, workspace, &inner, &next, depth + 1).await?;
                }
                WorkflowStep::If(b) => {
                    for path in &b.paths {
                        validate_calls(host, workspace, &path.steps, chain, depth).await?;
                    }
                }
                WorkflowStep::Each(b) => {
                    validate_calls(host, workspace, &b.steps, chain, depth).await?
                }
                WorkflowStep::Until(b) => {
                    validate_calls(host, workspace, &b.steps, chain, depth).await?
                }
                WorkflowStep::Agent(_) => {}
            }
        }
        Ok(())
    })
}

/// A. `check_plan(charter, inputs)`.
pub(crate) async fn check_plan(
    host: &dyn CrewHost,
    company: &CrewLimits,
    charter: &CrewCharter,
    inputs: &[CrewInput],
) -> PlanCheck {
    let mut problems = Vec::new();
    // A.1
    if charter.members.len() > MAX_MEMBERS {
        problems.push(PlanProblem::TooManyMembers {
            count: u32::try_from(charter.members.len()).unwrap_or(u32::MAX),
        });
    }
    let saved = match (charter.workflow_id, charter.workflow_version) {
        (Some(id), Some(version)) => host.saved_agent_versions(id, version).await,
        _ => Vec::new(),
    };
    for member in &charter.members {
        match host
            .agent_status(charter.workspace_id, &member.member)
            .await
        {
            AgentAvailability::Missing => problems.push(PlanProblem::AgentMissing {
                member: member.member,
            }),
            AgentAvailability::Present { version } => {
                // ADR-064: a changed version still fits; the run starts and Ordin takes over there.
                if let Some((_, was)) = saved.iter().find(|(a, _)| *a == member.member) {
                    if *was != version {
                        problems.push(PlanProblem::AgentChanged {
                            member: member.member,
                            saved: was.clone(),
                            present: version,
                        });
                    }
                }
            }
        }
    }
    // A.2
    for label in &charter.input_fields {
        if !inputs.iter().any(|i| i.label == *label) {
            problems.push(PlanProblem::InputMissing {
                label: label.clone(),
            });
        }
    }
    // A.3
    if !charter.steps.is_empty() {
        let mut checked = validate_steps(&charter.steps, inputs);
        if checked.is_ok() {
            let chain: Vec<WorkflowId> = charter.workflow_id.into_iter().collect();
            checked = validate_calls(host, charter.workspace_id, &charter.steps, &chain, 0).await;
        }
        if let Err((code, position)) = checked {
            problems.push(PlanProblem::StepsInvalid {
                code: code.to_owned(),
                position,
            });
        }
    }
    // A.4
    let (_, lowered) = lower_guardrails(&charter.guardrails, company);
    // A.5
    let fits = !problems.iter().any(|p| {
        matches!(
            p,
            PlanProblem::AgentMissing { .. }
                | PlanProblem::InputMissing { .. }
                | PlanProblem::TooManyMembers { .. }
                | PlanProblem::StepsInvalid { .. }
        )
    });
    PlanCheck {
        fits,
        problems,
        lowered,
    }
}

/// B.2: the code `start` reports for a plan that does not fit.
pub(crate) fn problem_code(problem: &PlanProblem) -> &'static str {
    match problem {
        PlanProblem::TooManyMembers { .. } => "CREW_TOO_MANY_MEMBERS",
        PlanProblem::StepsInvalid { code, .. } => crate::guard::step_code(code),
        // `AgentMissing` and `InputMissing` have no 2C code of their own; the first is reported as
        // an unknown member and the second as the generic code.
        PlanProblem::AgentMissing { .. } => "CHRT_MEMBER_UNKNOWN",
        PlanProblem::InputMissing { .. } | PlanProblem::AgentChanged { .. } => {
            "CREW_CHARTER_INVALID"
        }
    }
}
