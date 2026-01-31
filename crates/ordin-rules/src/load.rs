//! `load_rule_set`: checks and compiles one instance's controls into an immutable `ControlSet`, and
//! builds its control lines (RulesEngine.md *Internal Logic* A and F; ADR-107 addition to A).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use ordin_types::domain::agent_profile::{CheckPoint, ContentCheck};
use ordin_types::domain::agents::Authority;
use ordin_types::wire::rules::RuleSource;
use ordin_types::AgentInstanceId;
use regex::{Regex, RegexBuilder};

use crate::error::{codes, RulesError};
use crate::policy::{FILE_TOOLS, SHELL_TOOLS};
use crate::types::{ControlLine, ControlSource, ProgramKind};
use crate::vocab::{
    ArgumentRule, CallTarget, CommandBound, Condition, DecidingOutcome, DutyGate, InstanceRuleSet,
    JudgedLoopGate, MessageRule, Point, StepGate,
};
use crate::{MAX_ARGUMENT_RULES, MAX_PATTERN_CHARS};

/// The compiled regex size limit (A.2).
const REGEX_SIZE_LIMIT: usize = 1 << 20;
/// An approval-list entry's longest form (A.2).
const APPROVAL_ENTRY_MAX: usize = 200;
/// A `rule_id` is 1–128 characters (2C).
const RULE_ID_MAX: usize = 128;

/// The tool name a control, a list or a policy is matched by: `mcp__<server>__<tool>` loses its
/// prefix (R9).
// spec: ordin-desktop/components/RulesEngine#SA-RECON-08
pub(crate) fn bare(tool: &str) -> &str {
    tool.strip_prefix("mcp__")
        .and_then(|rest| rest.find("__").map(|at| &rest[at + 2..]))
        .unwrap_or(tool)
}

/// Compiles one pattern as A.2 states.
pub(crate) fn compile(pattern: &str) -> Option<Regex> {
    if pattern.is_empty() || pattern.chars().count() > MAX_PATTERN_CHARS {
        return None;
    }
    RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .ok()
}

fn verb_shape() -> &'static Regex {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    SHAPE.get_or_init(|| {
        Regex::new(r"^[a-z0-9_-]+(\.[a-z0-9_-]+)?$").unwrap_or_else(|_| unreachable!("constant"))
    })
}

fn flag_shape() -> &'static Regex {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    SHAPE.get_or_init(|| {
        Regex::new(r"^--?[A-Za-z0-9][A-Za-z0-9-]*$").unwrap_or_else(|_| unreachable!("constant"))
    })
}

/// One instance's controls, immutable once loaded (A.6). Tool names are kept bare (R9).
pub(crate) struct ControlSet {
    pub id: AgentInstanceId,
    pub package_name: String,
    pub authority: Authority,
    pub folder: PathBuf,
    pub read_only: bool,
    /// Package first, then configuration.
    pub message_rules: Vec<(MessageRule, ControlSource)>,
    pub duties: Vec<DutyGate>,
    /// Command → duty name.
    pub duty_of_command: HashMap<String, String>,
    /// Command → its judged loop.
    pub judged_loops: HashMap<String, JudgedLoopGate>,
    pub bounds: HashMap<String, CommandBound>,
    /// Package first, then configuration; tool names bare.
    pub steps: Vec<(StepGate, ControlSource)>,
    /// Package first, then configuration; tool names bare.
    pub argument_rules: Vec<(ArgumentRule, ControlSource)>,
    pub approval_list: Vec<String>,
    pub package_read_only_tools: Vec<String>,
    pub deny_verbs: Vec<String>,
    pub deny_flags: Vec<String>,
    pub ask_verbs: Vec<String>,
    pub reviewer: Option<String>,
    pub external: Vec<String>,
    /// The bare names of `set.tools`.
    pub bound_tools: Vec<String>,
    /// The MCP server names the agent declares (`AgentToolPolicy::mcp_servers`).
    pub declared_servers: Vec<String>,
    pub commands: Vec<String>,
    pub content_checks: Vec<ContentCheck>,
    /// Every declared control name (J: a setting rule may not take one).
    pub control_names: HashSet<String>,
    pub patterns: HashMap<String, Regex>,
    /// The control lines with the rule source each maps to (F).
    pub lines: Vec<(ControlLine, RuleSource)>,
}

impl ControlSet {
    pub(crate) fn duty(&self, name: &str) -> Option<&DutyGate> {
        self.duties.iter().find(|d| d.duty == name)
    }

    pub(crate) fn duty_of(&self, command: &str) -> Option<&DutyGate> {
        self.duty_of_command
            .get(command)
            .and_then(|duty| self.duty(duty))
    }

    /// Every command mapped to a duty.
    pub(crate) fn commands_of_duty(&self, duty: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .duty_of_command
            .iter()
            .filter(|(_, d)| d.as_str() == duty)
            .map(|(c, _)| c.clone())
            .collect();
        out.sort();
        out
    }

    pub(crate) fn is_bound(&self, tool: &str) -> bool {
        self.bound_tools.iter().any(|t| t == tool)
    }
}

struct Loader {
    usable: HashSet<String>,
    subagents: HashSet<String>,
    unmapped: HashSet<String>,
    arguments: HashMap<String, Vec<String>>,
    patterns: HashMap<String, Regex>,
    skipped: Vec<(String, String, RuleSource)>,
}

impl Loader {
    fn unbound(&self, control: &str, tool: &str, place: &str) -> RulesError {
        RulesError::load(
            codes::UNBOUND_TOOL,
            format!("the control {control} names the tool {tool}, which is not bound"),
            place,
        )
    }

    fn check_tool(&self, control: &str, tool: &str, place: &str) -> Result<(), RulesError> {
        if self.usable.contains(bare(tool)) {
            Ok(())
        } else {
            Err(self.unbound(control, tool, place))
        }
    }

    fn pattern(&mut self, control: &str, pattern: &str, place: &str) -> Result<(), RulesError> {
        match compile(pattern) {
            Some(regex) => {
                self.patterns.insert(pattern.to_owned(), regex);
                Ok(())
            }
            None => Err(RulesError::load(
                codes::BAD_PATTERN,
                format!("the control {control} holds a pattern that cannot be used"),
                place,
            )),
        }
    }

    fn conditions(
        &mut self,
        control: &str,
        conditions: &[Condition],
        place: &str,
    ) -> Result<(), RulesError> {
        for condition in conditions {
            match condition {
                Condition::Pattern { patterns, .. } => {
                    for pattern in patterns {
                        self.pattern(control, pattern, place)?;
                    }
                }
                Condition::Called { target, name, .. }
                | Condition::NotCalled { target, name, .. } => match target {
                    CallTarget::Tool => self.check_tool(control, name, place)?,
                    CallTarget::Subagent => {
                        if !self.subagents.contains(name) {
                            return Err(self.unbound(control, &format!("task:{name}"), place));
                        }
                    }
                },
                _ => {}
            }
        }
        Ok(())
    }

    fn step(&mut self, gate: &StepGate, place: &str) -> Result<bool, RulesError> {
        if let Some(tool) = &gate.tool {
            if gate.point == Point::BeforeTool && self.unmapped.contains(bare(tool)) {
                return Ok(false);
            }
            self.check_tool(&gate.name, tool, place)?;
        }
        self.conditions(&gate.name, &gate.conditions, place)?;
        Ok(true)
    }

    fn argument_rule(&mut self, rule: &ArgumentRule, place: &str) -> Result<bool, RulesError> {
        let tool = bare(&rule.tool);
        if self.unmapped.contains(tool) {
            return Ok(false);
        }
        self.check_tool(&rule.name, &rule.tool, place)?;
        let places = self.arguments.get(tool).cloned().unwrap_or_default();
        let described = places.iter().any(|p| {
            p == &rule.argument
                || p.strip_suffix(".*")
                    .is_some_and(|stem| rule.argument.starts_with(&format!("{stem}.")))
        });
        if !described {
            return Err(RulesError::load(
                codes::UNKNOWN_ARGUMENT,
                format!(
                    "the control {} names the argument {} of the tool {}, which the tool does not describe",
                    rule.name, rule.argument, rule.tool
                ),
                place,
            ));
        }
        for pattern in &rule.patterns {
            self.pattern(&rule.name, pattern, place)?;
        }
        Ok(true)
    }

    fn message_rule(&mut self, rule: &MessageRule, place: &str) -> Result<(), RulesError> {
        self.conditions(&rule.name, &rule.conditions, place)?;
        if let Some(DecidingOutcome::Tools { tools }) = &rule.decides {
            for tool in tools {
                self.check_tool(&rule.name, tool, place)?;
            }
        }
        Ok(())
    }

    fn skip(&mut self, name: &str, tool: &str, source: RuleSource) {
        self.skipped.push((
            name.to_owned(),
            format!("the operation {tool} is not mapped, so no call of it can be made"),
            source,
        ));
    }
}

/// The names the generated duty, judged-loop and bound controls take (A.3).
fn generated_names(
    duties: &[DutyGate],
    loops: &[JudgedLoopGate],
    bounds: &[CommandBound],
) -> Vec<String> {
    let mut out = Vec::new();
    for duty in duties {
        for part in [
            "stop",
            "escalation",
            "work-check",
            "lock",
            "cycle-limit",
            "done-check",
            "heartbeat",
            "failed-cycles",
        ] {
            out.push(format!("duty:{}:{part}", duty.duty));
        }
    }
    for gate in loops {
        for command in &gate.commands {
            out.push(format!("judged-loop:{command}"));
        }
    }
    for bound in bounds {
        out.push(format!("bound:{}", bound.command));
        out.push(format!("bound:{}:rounds", bound.command));
        out.push(format!("bound:{}:repeated-failures", bound.command));
    }
    out
}

fn override_error(name: &str, place: &str) -> RulesError {
    RulesError::load(
        codes::GATE_OVERRIDE,
        format!("the configured control {name} takes the name of a package control"),
        place,
    )
}

fn policy_error(entry: &str, place: String) -> RulesError {
    let shown: String = entry.chars().take(64).collect();
    RulesError::load(
        codes::POLICY_INVALID,
        format!("the policy entry {shown} is not well formed"),
        place,
    )
}

/// A. Checks and compiles the set. Pure: no store or Journal access.
pub(crate) fn compile_set(
    set: &InstanceRuleSet,
    program: ProgramKind,
) -> Result<ControlSet, RulesError> {
    // A.1 Names a control may use.
    let mut usable: HashSet<String> = set.tools.iter().map(|t| bare(&t.name).to_owned()).collect();
    for (tool, _) in FILE_TOOLS {
        usable.insert(tool.to_owned());
    }
    usable.insert("task".to_owned());
    let mut subagents: HashSet<String> = HashSet::from(["general-purpose".to_owned()]);
    if let Some(reviewer) = &set.reviewer {
        subagents.insert(reviewer.clone());
    }
    for sub in &subagents {
        usable.insert(format!("task:{sub}"));
    }
    let mut arguments: HashMap<String, Vec<String>> = set
        .tools
        .iter()
        .map(|t| (bare(&t.name).to_owned(), t.arguments.clone()))
        .collect();
    for (tool, argument) in FILE_TOOLS {
        arguments
            .entry(tool.to_owned())
            .or_insert_with(|| vec![argument.to_owned()]);
    }
    for tool in SHELL_TOOLS {
        arguments
            .entry(tool.to_owned())
            .or_insert_with(|| vec!["command".to_owned()]);
    }
    let mut loader = Loader {
        usable,
        subagents,
        unmapped: set
            .unmapped_operations
            .iter()
            .map(|o| bare(o).to_owned())
            .collect(),
        arguments,
        patterns: HashMap::new(),
        skipped: Vec::new(),
    };

    // A.2 The package's declaration, in declared order.
    const DECL: &str = "wrightrun-declaration.yaml: ";
    let declaration = set.declaration.clone().unwrap_or_default();
    let gates = &declaration.gates;
    for (i, duty) in gates.duties.iter().enumerate() {
        if let Some(condition) = duty.work_check.as_ref().and_then(|c| c.condition.as_ref()) {
            let name = format!("duty:{}:work-check", duty.duty);
            loader.conditions(
                &name,
                std::slice::from_ref(condition),
                &format!("{DECL}gates.duties[{i}]"),
            )?;
        }
    }
    for (i, gate) in gates.judged_loops.iter().enumerate() {
        if !loader.subagents.contains(&gate.reviewer) {
            let name = gate
                .commands
                .first()
                .map_or_else(|| "judged-loop".to_owned(), |c| format!("judged-loop:{c}"));
            return Err(loader.unbound(
                &name,
                &format!("task:{}", gate.reviewer),
                &format!("{DECL}gates.judged-loops[{i}]"),
            ));
        }
    }
    let mut steps = Vec::new();
    for (i, gate) in gates.steps.iter().enumerate() {
        if loader.step(gate, &format!("{DECL}gates.steps[{i}]"))? {
            steps.push((normalised_step(gate), ControlSource::Package));
        } else {
            let tool = gate.tool.clone().unwrap_or_default();
            loader.skip(&gate.name, &tool, RuleSource::DeclarationGate);
        }
    }
    let mut argument_rules = Vec::new();
    for (i, rule) in gates.argument_rules.iter().enumerate() {
        if loader.argument_rule(rule, &format!("{DECL}gates.argument-rules[{i}]"))? {
            argument_rules.push((normalised_rule(rule), ControlSource::Package));
        } else {
            loader.skip(&rule.name, &rule.tool, RuleSource::DeclarationArgumentRule);
        }
    }
    let mut message_rules = Vec::new();
    for (i, rule) in declaration.message_rules.iter().enumerate() {
        loader.message_rule(rule, &format!("{DECL}message-rules[{i}]"))?;
        message_rules.push((rule.clone(), ControlSource::Package));
    }

    // A.2 then the configuration.
    let configured = set.configured.clone().unwrap_or_default();
    let mut configured_names: Vec<(String, String)> = Vec::new();
    for (i, gate) in configured.steps.iter().enumerate() {
        let place = format!("controls.steps[{i}]");
        loader.step(gate, &place)?;
        steps.push((normalised_step(gate), ControlSource::Configuration));
        configured_names.push((gate.name.clone(), place));
    }
    for (i, rule) in configured.argument_rules.iter().enumerate() {
        let place = format!("controls.argument-rules[{i}]");
        loader.argument_rule(rule, &place)?;
        argument_rules.push((normalised_rule(rule), ControlSource::Configuration));
        configured_names.push((rule.name.clone(), place));
    }
    for (i, rule) in configured.message_rules.iter().enumerate() {
        let place = format!("controls.message-rules[{i}]");
        loader.message_rule(rule, &place)?;
        message_rules.push((rule.clone(), ControlSource::Configuration));
        configured_names.push((rule.name.clone(), place));
    }

    // A.2 Approval-list entries (SA-CLOSE-01).
    let mut approval_list = Vec::new();
    for (i, entry) in set.approval_list.iter().enumerate() {
        let entry = entry.trim();
        if entry.is_empty() || entry.chars().count() > APPROVAL_ENTRY_MAX {
            return Err(policy_error(entry, format!("approval-required[{i}]")));
        }
        approval_list.push(entry.to_owned());
    }

    // A.2 At most 200 argument rules.
    if argument_rules.len() > MAX_ARGUMENT_RULES {
        return Err(RulesError::load(
            codes::TOO_MANY_RULES,
            format!(
                "the instance declares {} argument rules; at most {MAX_ARGUMENT_RULES} are allowed",
                argument_rules.len()
            ),
            "gates.argument-rules",
        ));
    }

    // A.3 A configured control may not take a package control's name.
    let mut package_names: HashSet<String> =
        generated_names(&gates.duties, &gates.judged_loops, &gates.bounds)
            .into_iter()
            .collect();
    package_names.extend(gates.steps.iter().map(|g| g.name.clone()));
    package_names.extend(gates.argument_rules.iter().map(|r| r.name.clone()));
    package_names.extend(declaration.message_rules.iter().map(|r| r.name.clone()));
    for (name, place) in &configured_names {
        if package_names.contains(name) {
            return Err(override_error(name, place));
        }
    }
    let mut all_names = package_names.clone();
    all_names.extend(configured_names.iter().map(|(n, _)| n.clone()));
    // ADR-107: content-check names are unique across every control.
    for (i, check) in configured.content_checks.iter().enumerate() {
        let place = format!("controls.content-checks[{i}]");
        if !all_names.insert(check.name.clone()) {
            return Err(override_error(&check.name, &place));
        }
    }

    // A.4 The agent's tool policy: deny and ask lists only, so it cannot widen.
    let policy = set.tool_policy.clone().unwrap_or_default();
    for (i, verb) in policy.prohibited_verbs.iter().enumerate() {
        if !verb_shape().is_match(verb) {
            return Err(policy_error(
                verb,
                format!("tool-policy.prohibited_verbs[{i}]"),
            ));
        }
    }
    for (i, verb) in policy.requires_approval.iter().enumerate() {
        if !verb_shape().is_match(verb) {
            return Err(policy_error(
                verb,
                format!("tool-policy.requires_approval[{i}]"),
            ));
        }
    }
    for (i, flag) in policy.forbidden_flags.iter().enumerate() {
        if !flag_shape().is_match(flag) {
            return Err(policy_error(
                flag,
                format!("tool-policy.forbidden_flags[{i}]"),
            ));
        }
    }
    let deny_verbs = policy.prohibited_verbs.clone();
    let ask_verbs: Vec<String> = policy
        .requires_approval
        .iter()
        .filter(|v| !deny_verbs.contains(v))
        .cloned()
        .collect();

    // A.5 The duty of each command.
    let mut duty_of_command = HashMap::new();
    for duty in &gates.duties {
        duty_of_command.insert(duty.command.clone(), duty.duty.clone());
    }
    for (duty, command) in &configured.schedule_entries {
        if gates.duties.iter().any(|d| &d.duty == duty) && !duty_of_command.contains_key(command) {
            duty_of_command.insert(command.clone(), duty.clone());
        }
    }
    let mut judged_loops = HashMap::new();
    for gate in &gates.judged_loops {
        for command in &gate.commands {
            judged_loops.insert(command.clone(), gate.clone());
        }
    }
    let bounds = gates
        .bounds
        .iter()
        .map(|b| (b.command.clone(), b.clone()))
        .collect();

    let mut control_set = ControlSet {
        id: set.agent_instance_id,
        package_name: set.package_name.clone(),
        authority: set.authority,
        folder: set.agent_folder.clone(),
        read_only: set.agent_folder_read_only,
        message_rules,
        duties: gates.duties.clone(),
        duty_of_command,
        judged_loops,
        bounds,
        steps,
        argument_rules,
        approval_list,
        package_read_only_tools: set
            .package_read_only_tools
            .iter()
            .map(|t| bare(t).to_owned())
            .collect(),
        deny_verbs,
        deny_flags: policy.forbidden_flags.clone(),
        ask_verbs,
        reviewer: set.reviewer.clone(),
        external: set.external.iter().map(|t| bare(t).to_owned()).collect(),
        bound_tools: set.tools.iter().map(|t| bare(&t.name).to_owned()).collect(),
        declared_servers: policy.mcp_servers.clone(),
        commands: set.commands.clone(),
        content_checks: configured.content_checks.clone(),
        control_names: all_names,
        patterns: std::mem::take(&mut loader.patterns),
        lines: Vec::new(),
    };
    // A.7 / F The control lines.
    control_set.lines = lines(&control_set, set, &gates.bounds, &loader.skipped, program);
    Ok(control_set)
}

fn normalised_step(gate: &StepGate) -> StepGate {
    let mut gate = gate.clone();
    gate.tool = gate.tool.as_deref().map(|t| bare(t).to_owned());
    gate
}

fn normalised_rule(rule: &ArgumentRule) -> ArgumentRule {
    let mut rule = rule.clone();
    rule.tool = bare(&rule.tool).to_owned();
    rule
}

fn cut_id(name: &str) -> String {
    name.chars().take(RULE_ID_MAX).collect()
}

struct LineBuilder {
    out: Vec<(ControlLine, RuleSource)>,
}

impl LineBuilder {
    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        name: String,
        source: ControlSource,
        point: Point,
        command: Option<String>,
        enforced: bool,
        reason: &str,
        detail: String,
        rule: RuleSource,
    ) {
        self.out.push((
            ControlLine {
                name: cut_id(&name),
                source,
                point,
                command,
                enforced,
                reason: reason.to_owned(),
                detail,
            },
            rule,
        ));
    }
}

const NO_SHELL: &str = "no shell";
const NO_DONE_CHECK: &str = "a done-check has no enforcement point in this release";
const NO_ROUND_RESULT: &str = "the runtime does not observe the result of a round";
const NO_COMMANDS: &str = "this agent may not run commands";

/// F. The lines, in the order the start-up report lists them.
fn lines(
    cs: &ControlSet,
    set: &InstanceRuleSet,
    bounds: &[CommandBound],
    skipped: &[(String, String, RuleSource)],
    program: ProgramKind,
) -> Vec<(ControlLine, RuleSource)> {
    use ControlSource::{AgentPolicy, Application, Configuration, Package};
    let gate = RuleSource::DeclarationGate;
    let mut b = LineBuilder { out: Vec::new() };
    let served_by = |tool: &str| -> String {
        set.tools
            .iter()
            .find(|t| bare(&t.name) == tool)
            .and_then(|t| t.served_by.clone())
            .unwrap_or_default()
    };
    for duty in &cs.duties {
        let d = &duty.duty;
        let command = Some(duty.command.clone());
        for part in ["stop", "escalation"] {
            b.push(
                format!("duty:{d}:{part}"),
                Package,
                Point::Start,
                command.clone(),
                true,
                "",
                String::new(),
                gate,
            );
        }
        if let Some(check) = &duty.work_check {
            let file = check.kind == crate::vocab::CheckKind::File;
            b.push(
                format!("duty:{d}:work-check"),
                Package,
                Point::Start,
                command.clone(),
                file,
                if file { "" } else { NO_SHELL },
                String::new(),
                gate,
            );
        }
        b.push(
            format!("duty:{d}:lock"),
            Package,
            Point::Start,
            command.clone(),
            true,
            "",
            String::new(),
            gate,
        );
        if duty.max_cycles.is_some() {
            b.push(
                format!("duty:{d}:cycle-limit"),
                Package,
                Point::Start,
                command.clone(),
                true,
                "",
                String::new(),
                gate,
            );
        }
        if duty.done_check.is_some() {
            b.push(
                format!("duty:{d}:done-check"),
                Package,
                Point::End,
                command.clone(),
                false,
                NO_DONE_CHECK,
                String::new(),
                gate,
            );
        }
        if duty.heartbeat.is_some() {
            b.push(
                format!("duty:{d}:heartbeat"),
                Package,
                Point::Start,
                command.clone(),
                true,
                "",
                String::new(),
                gate,
            );
        }
        if duty.max_failed_cycles.is_some() {
            b.push(
                format!("duty:{d}:failed-cycles"),
                Package,
                Point::End,
                command.clone(),
                false,
                NO_ROUND_RESULT,
                String::new(),
                gate,
            );
        }
    }
    let mut loops: Vec<(&String, &JudgedLoopGate)> = cs.judged_loops.iter().collect();
    loops.sort_by(|a, b| a.0.cmp(b.0));
    for (command, judged) in loops {
        b.push(
            format!("judged-loop:{command}"),
            Package,
            Point::End,
            Some(command.clone()),
            true,
            "",
            format!("reviewer {}", judged.reviewer),
            gate,
        );
    }
    for bound in bounds {
        let c = &bound.command;
        b.push(
            format!("bound:{c}"),
            Package,
            Point::Start,
            Some(c.clone()),
            bound.wall_clock_minutes.is_some(),
            "",
            String::new(),
            gate,
        );
        if bound.max_rounds.is_some() {
            b.push(
                format!("bound:{c}:rounds"),
                Package,
                Point::End,
                Some(c.clone()),
                false,
                NO_ROUND_RESULT,
                String::new(),
                gate,
            );
        }
        if bound.max_repeated_failures.is_some() {
            b.push(
                format!("bound:{c}:repeated-failures"),
                Package,
                Point::End,
                Some(c.clone()),
                false,
                NO_ROUND_RESULT,
                String::new(),
                gate,
            );
        }
    }
    let push_steps = |b: &mut LineBuilder, source: ControlSource| {
        for (step, s) in cs.steps.iter().filter(|(_, s)| *s == source) {
            let detail = step.tool.as_deref().map(served_by).unwrap_or_default();
            b.push(
                step.name.clone(),
                *s,
                step.point,
                step.command.clone(),
                true,
                "",
                detail,
                gate,
            );
        }
    };
    let push_rules = |b: &mut LineBuilder, source: ControlSource| {
        for (rule, s) in cs.argument_rules.iter().filter(|(_, s)| *s == source) {
            b.push(
                rule.name.clone(),
                *s,
                Point::BeforeTool,
                None,
                true,
                "",
                served_by(&rule.tool),
                RuleSource::DeclarationArgumentRule,
            );
        }
    };
    let push_messages = |b: &mut LineBuilder, source: ControlSource| {
        for (rule, s) in cs.message_rules.iter().filter(|(_, s)| *s == source) {
            b.push(
                rule.name.clone(),
                *s,
                Point::Start,
                rule.command.clone(),
                true,
                "",
                String::new(),
                gate,
            );
        }
    };
    push_steps(&mut b, Package);
    push_rules(&mut b, Package);
    for (name, reason, rule) in skipped {
        b.push(
            name.clone(),
            Package,
            Point::BeforeTool,
            None,
            false,
            reason,
            String::new(),
            *rule,
        );
    }
    push_messages(&mut b, Package);
    push_steps(&mut b, Configuration);
    push_rules(&mut b, Configuration);
    push_messages(&mut b, Configuration);
    for check in &cs.content_checks {
        for point in &check.points {
            let (at, what) = match point {
                CheckPoint::Input => (Point::Start, "question"),
                CheckPoint::Output => (Point::End, "answer"),
            };
            b.push(
                check.name.clone(),
                Configuration,
                at,
                None,
                true,
                "",
                format!(
                    "checked by {}/{}; every {what} is sent there",
                    check.connection, check.operation
                ),
                gate,
            );
        }
    }
    // The application lines, all enforced.
    b.push(
        "app.folder_grant".to_owned(),
        Application,
        Point::BeforeTool,
        None,
        true,
        "",
        String::new(),
        RuleSource::AppFolderPolicy,
    );
    let shell_detail = if cs.authority == Authority::SystemChanging {
        String::new()
    } else {
        NO_COMMANDS.to_owned()
    };
    b.push(
        "app.shell".to_owned(),
        Application,
        Point::BeforeTool,
        None,
        true,
        "",
        shell_detail,
        RuleSource::AppShellPolicy,
    );
    b.push(
        "app.external_action".to_owned(),
        Application,
        Point::BeforeTool,
        None,
        true,
        "",
        String::new(),
        RuleSource::AppApprovalPolicy,
    );
    b.push(
        "app.package_tool".to_owned(),
        Application,
        Point::BeforeTool,
        None,
        true,
        "",
        String::new(),
        RuleSource::AppApprovalPolicy,
    );
    // SPEC-GAP-4: F lists the reviewer line only under C-78's fallback; whether the hook names the
    // sub-agent is not known at load. In the agent profile with a reviewer the line is listed as
    // enforced (the C-78-confirmed reading of the "Reviewer kept to reading" scenario).
    // spec: ordin-desktop/components/RulesEngine#C-78
    if program.is_agent() && cs.reviewer.is_some() {
        b.push(
            "app.reviewer_read_only".to_owned(),
            Application,
            Point::BeforeTool,
            None,
            true,
            "",
            String::new(),
            RuleSource::DeclarationToolRule,
        );
    }
    for verb in &cs.deny_verbs {
        b.push(
            verb.clone(),
            AgentPolicy,
            Point::BeforeTool,
            None,
            true,
            "",
            String::new(),
            RuleSource::AgentToolPolicy,
        );
    }
    for flag in &cs.deny_flags {
        b.push(
            flag.clone(),
            AgentPolicy,
            Point::BeforeTool,
            None,
            true,
            "",
            String::new(),
            RuleSource::AgentToolPolicy,
        );
    }
    b.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_names_lose_the_mcp_prefix() {
        assert_eq!(bare("mcp__supplier-db__fetch"), "fetch");
        assert_eq!(bare("mcp__craft_slides__render"), "render");
        assert_eq!(bare("Read"), "Read");
        assert_eq!(bare("mcp__broken"), "mcp__broken");
    }

    #[test]
    fn patterns_compile_within_limits() {
        assert!(compile("^a+$").is_some());
        assert!(compile("").is_none());
        assert!(compile(&"a".repeat(201)).is_none());
        assert!(compile("(").is_none());
    }
}
