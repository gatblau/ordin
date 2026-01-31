//! Workflow files (OrdinHost.md sections E and F): the fixed-code check (step 19), the reader of
//! formats 1, 2 and 3 (step 26.2, SA-CLOSE-05), the format 3 YAML writer (step 25) and the
//! conversion of a file's steps to 2C `WorkflowStep`s for a charter. Nothing here calls a model.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ordin_service_api::commands::workflows::{CheckProblem, WorkflowCheckReport};
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{
    AgentStep, CallBlock, Condition, EachBlock, IfBlock, NamedPath as DomainPath,
    StepUse as DomainUse, UntilBlock, WorkflowStep,
};
use ordin_types::wire::workflow_file::{
    AgentRepositoryRef, Decider, ListSource, StepUse, WorkflowFileAgent, WorkflowFileStep,
    WorkflowInputKind,
};
use ordin_types::wire::WorkflowFile;
use ordin_types::{AgentInstanceId, WorkflowId};
use serde_json::Value;

use crate::types::CrewLimitPolicy;

/// 2D-4 constants: the file size, nesting, steps and blocks, and call depth (Configuration).
pub(crate) const FILE_MAX_BYTES: usize = 262_144;
const MAX_STEPS: usize = 50;
const MAX_DEPTH: usize = 4;
const MAX_CALL_DEPTH: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckMode {
    Save,
    Import,
    Duplicate,
    RepositoryRead,
    Draft,
}

/// The presence of one agent of the file in the workspace (step 19.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Presence {
    Present(AgentInstanceId),
    OtherVersion(AgentInstanceId, String),
    Missing,
}

/// What the pure check needs from outside: gathered by the host before it runs.
#[derive(Debug, Clone, Default)]
pub(crate) struct CheckFacts {
    pub maxima: Option<CrewLimitPolicy>,
    pub profile_keys: Vec<String>,
    /// The workspace's other saved workflows at their latest versions, by name.
    pub workflows: HashMap<String, WorkflowFile>,
    /// Per `agents[i]`, in order.
    pub presence: Vec<Presence>,
    /// Instances that can send email (step 19.5).
    pub sends_email: BTreeSet<AgentInstanceId>,
    /// `agents[i]` the legacy reader could not match (WFF_AGENT_UNMATCHED already reported).
    pub unmatched: BTreeSet<usize>,
}

pub(crate) fn problem(code: &str, at: impl Into<String>, args: &[(&str, String)]) -> CheckProblem {
    let message_id = match code {
        "WFF_LIMIT_LOWERED" => "workflow-import-limit-lowered".to_owned(),
        "OH_AGENT_MISSING" => "workflow-import-agent-missing".to_owned(),
        _ => ordin_types::error_registry::lookup(code)
            .map(|e| e.sentence_id.to_owned())
            .unwrap_or_else(|| code.to_ascii_lowercase().replace('_', "-")),
    };
    CheckProblem {
        code: code.to_owned(),
        at: at.into(),
        message_id,
        args: args
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn within(s: &str, min: usize, max: usize) -> bool {
    let n = chars(s);
    n >= min && n <= max
}

/// `^[0-9]{1,2}[a-z]?(\.[0-9]{1,2}[a-z]?){0,3}$`, parsed into (number, letter) segments.
pub(crate) fn parse_position(p: &str) -> Option<Vec<(u32, Option<char>)>> {
    let segments: Vec<&str> = p.split('.').collect();
    if segments.is_empty() || segments.len() > 4 {
        return None;
    }
    let mut out = Vec::with_capacity(segments.len());
    for s in segments {
        let (digits, letter) = match s.chars().last() {
            Some(c) if c.is_ascii_lowercase() => (&s[..s.len() - 1], Some(c)),
            _ => (s, None),
        };
        if digits.is_empty() || digits.len() > 2 || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out.push((digits.parse().ok()?, letter));
    }
    Some(out)
}

fn key_valid(key: &str) -> bool {
    let mut it = key.chars();
    matches!(it.next(), Some(c) if c.is_ascii_lowercase())
        && key.len() <= 32
        && it.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// One step or block in outline (depth-first) order.
#[derive(Debug, Clone)]
struct Entry<'a> {
    position: &'a str,
    step: &'a WorkflowFileStep,
    depth: usize,
    /// The `if` blocks this entry lies inside, with the path index.
    ifs: Vec<(&'a str, usize)>,
    in_each: bool,
}

fn position_of(step: &WorkflowFileStep) -> &str {
    match step {
        WorkflowFileStep::Agent { position, .. }
        | WorkflowFileStep::If { position, .. }
        | WorkflowFileStep::Each { position, .. }
        | WorkflowFileStep::Until { position, .. }
        | WorkflowFileStep::Call { position, .. } => position,
    }
}

fn walk<'a>(
    steps: &'a [WorkflowFileStep],
    depth: usize,
    ifs: &[(&'a str, usize)],
    in_each: bool,
    out: &mut Vec<Entry<'a>>,
) {
    for step in steps {
        out.push(Entry {
            position: position_of(step),
            step,
            depth,
            ifs: ifs.to_vec(),
            in_each,
        });
        match step {
            WorkflowFileStep::If {
                position, paths, ..
            } => {
                for (i, path) in paths.iter().enumerate() {
                    let mut inner = ifs.to_vec();
                    inner.push((position.as_str(), i));
                    walk(&path.steps, depth + 1, &inner, in_each, out);
                }
            }
            WorkflowFileStep::Each { steps, .. } => walk(steps, depth + 1, ifs, true, out),
            WorkflowFileStep::Until { steps, .. } => walk(steps, depth + 1, ifs, in_each, out),
            WorkflowFileStep::Agent { .. } | WorkflowFileStep::Call { .. } => {}
        }
    }
}

fn calls_of(file: &WorkflowFile) -> Vec<String> {
    let mut entries = Vec::new();
    walk(&file.steps, 1, &[], false, &mut entries);
    entries
        .iter()
        .filter_map(|e| match e.step {
            WorkflowFileStep::Call { workflow, .. } => Some(workflow.clone()),
            _ => None,
        })
        .collect()
}

/// The deepest chain of calls from `name` (1 = calls nothing), and whether `root` is reached.
fn call_depth(
    name: &str,
    root: &str,
    workflows: &HashMap<String, WorkflowFile>,
    seen: &mut Vec<String>,
) -> (usize, bool) {
    if name == root {
        return (1, true);
    }
    let Some(file) = workflows.get(name) else {
        return (1, false);
    };
    if seen.iter().any(|s| s == name) {
        return (1, false);
    }
    seen.push(name.to_owned());
    let mut deepest = 1;
    let mut cycle = false;
    for called in calls_of(file) {
        let (d, c) = call_depth(&called, root, workflows, seen);
        deepest = deepest.max(d + 1);
        cycle |= c;
    }
    seen.pop();
    (deepest, cycle)
}

fn lower(
    report: &mut WorkflowCheckReport,
    value: &mut u32,
    range: (u32, u32),
    company: u32,
    at: &str,
    limit: &str,
) {
    if *value < range.0 {
        report.problems.push(problem("WFF_LIMIT_INVALID", at, &[]));
        return;
    }
    let maximum = company.min(range.1);
    if *value > maximum {
        report.notices.push(problem(
            "WFF_LIMIT_LOWERED",
            at,
            &[
                ("asked", value.to_string()),
                ("maximum", maximum.to_string()),
                ("limit", limit.to_owned()),
            ],
        ));
        *value = maximum;
    }
}

/// Step 19: every row in document order, all problems collected; limits above the maxima are
/// lowered in `file` and said in a notice. Pure: `facts` was gathered by the caller.
// spec: ordin-desktop/components/OrdinHost#SA-CLOSE-05
pub(crate) fn check(
    file: &mut WorkflowFile,
    facts: &CheckFacts,
    mode: CheckMode,
) -> WorkflowCheckReport {
    let mut r = WorkflowCheckReport {
        problems: Vec::new(),
        notices: Vec::new(),
    };
    if file.format == 0 {
        r.problems
            .push(problem("WFF_FORMAT_UNSUPPORTED", "/format", &[]));
    } else if file.format > 3 {
        r.problems.push(problem("WFF_FORMAT_NEWER", "/format", &[]));
    }
    if !within(&file.name, 1, 80) {
        r.problems.push(problem("WFF_NAME_INVALID", "/name", &[]));
    }
    if !within(&file.purpose, 1, 200) {
        r.problems
            .push(problem("WFF_PURPOSE_INVALID", "/purpose", &[]));
    }
    if file.inputs.len() > 20 {
        r.problems
            .push(problem("WFF_INPUT_INVALID", "/inputs", &[]));
    }
    let mut labels = BTreeSet::new();
    for (i, input) in file.inputs.iter().enumerate() {
        if !within(&input.label, 1, 60) || !labels.insert(input.label.clone()) {
            r.problems
                .push(problem("WFF_INPUT_INVALID", format!("/inputs/{i}"), &[]));
        }
    }
    if file.agents.is_empty() || file.agents.len() > 8 {
        r.problems
            .push(problem("WFF_AGENTS_INVALID", "/agents", &[]));
    }
    let mut keys = BTreeSet::new();
    for (i, agent) in file.agents.iter().enumerate() {
        let at = format!("/agents/{i}");
        if !key_valid(&agent.key) || !keys.insert(agent.key.clone()) {
            r.problems
                .push(problem("WFF_AGENT_KEY_INVALID", format!("{at}/key"), &[]));
        }
        if facts.unmatched.contains(&i) {
            continue;
        }
        if !within(&agent.id, 1, 256) || !agent.id.starts_with("urn:ordin:") {
            r.problems
                .push(problem("WFF_AGENT_ID_INVALID", format!("{at}/id"), &[]));
        }
        if semver::VersionReq::parse(&agent.version).is_err() {
            r.problems.push(problem(
                "WFF_AGENT_VERSION_INVALID",
                format!("{at}/version"),
                &[],
            ));
        }
        let repository_ok = match &agent.repository {
            AgentRepositoryRef::ProfileKey(k) => facts.profile_keys.iter().any(|p| p == k),
            AgentRepositoryRef::Address(a) => a.starts_with("https://") && a.len() > 8,
        };
        if !repository_ok {
            r.problems.push(problem(
                "WFF_REPOSITORY_INVALID",
                format!("{at}/repository"),
                &[],
            ));
        }
        if !within(&agent.role, 1, 120) {
            r.problems
                .push(problem("WFF_AGENT_ROLE_INVALID", format!("{at}/role"), &[]));
        }
    }

    let mut entries = Vec::new();
    walk(&file.steps, 1, &[], false, &mut entries);
    if entries.len() > MAX_STEPS {
        r.problems
            .push(problem("WFF_TOO_MANY_STEPS", "/steps", &[]));
    }
    if entries.iter().any(|e| e.depth > MAX_DEPTH) {
        r.problems.push(problem("WFF_TOO_DEEP", "/steps", &[]));
    }

    // Limits (19.3).
    let maxima = facts.maxima.unwrap_or(CrewLimitPolicy {
        rounds: 30,
        wall_clock_minutes: 480,
        members_at_once: 8,
        request_budget: None,
        failure_repeat_limit: 5,
    });
    let mut rounds = u32::from(file.limits.max_rounds);
    lower(
        &mut r,
        &mut rounds,
        (1, 30),
        u32::from(maxima.rounds),
        "/limits/max_rounds",
        "rounds",
    );
    file.limits.max_rounds = u16::try_from(rounds).unwrap_or(30);
    let mut minutes = u32::from(file.limits.wall_clock_minutes);
    lower(
        &mut r,
        &mut minutes,
        (1, 480),
        u32::from(maxima.wall_clock_minutes),
        "/limits/wall_clock_minutes",
        "minutes",
    );
    file.limits.wall_clock_minutes = u16::try_from(minutes).unwrap_or(480);
    let mut at_once = u32::from(file.limits.agents_at_once);
    lower(
        &mut r,
        &mut at_once,
        (1, 8),
        u32::from(maxima.members_at_once),
        "/limits/agents_at_once",
        "agents at once",
    );
    file.limits.agents_at_once = u8::try_from(at_once).unwrap_or(8);
    if !(1..=5).contains(&file.limits.failure_repeat_limit) {
        r.problems.push(problem(
            "WFF_LIMIT_INVALID",
            "/limits/failure_repeat_limit",
            &[],
        ));
    }
    for (i, key) in file.limits.external_action_agents.iter().enumerate() {
        if !keys.contains(key) {
            r.problems.push(problem(
                "WFF_AGENT_KEY_UNKNOWN",
                format!("/limits/external_action_agents/{i}"),
                &[],
            ));
        }
    }
    if chars(&file.origin) > 200 {
        r.problems
            .push(problem("WFF_ORIGIN_INVALID", "/origin", &[]));
    }

    // Positions: valid, unique, in outline order.
    let mut seen_positions = BTreeSet::new();
    let mut previous: Option<Vec<(u32, Option<char>)>> = None;
    for e in &entries {
        match parse_position(e.position) {
            Some(parsed) => {
                let ordered = previous.as_ref().is_none_or(|p| parsed > *p);
                if !seen_positions.insert(e.position.to_owned()) || !ordered {
                    r.problems
                        .push(problem("WFF_POSITION_INVALID", e.position, &[]));
                }
                previous = Some(parsed);
            }
            None => r
                .problems
                .push(problem("WFF_POSITION_INVALID", e.position, &[])),
        }
    }

    let index_of = |p: &str| entries.iter().position(|e| e.position == p);
    let input_kind = |label: &str| {
        file.inputs
            .iter()
            .find(|i| i.label == label)
            .map(|i| i.kind)
    };
    let mut groups: BTreeMap<String, u32> = BTreeMap::new();

    for (n, e) in entries.iter().enumerate() {
        let earlier = |p: &str| index_of(p).is_some_and(|i| i < n);
        match e.step {
            WorkflowFileStep::Agent {
                agent, line, uses, ..
            } => {
                if !keys.contains(agent) {
                    r.problems
                        .push(problem("WFF_AGENT_KEY_UNKNOWN", e.position, &[]));
                }
                if !within(line, 1, 200) {
                    r.problems
                        .push(problem("WFF_LINE_INVALID", e.position, &[]));
                }
                let on_path = uses.iter().all(|u| match u {
                    StepUse::Input(label) => input_kind(label).is_some(),
                    StepUse::Item(_) => e.in_each,
                    StepUse::Step(p) => match index_of(p) {
                        Some(i) if i < n => entries[i].ifs.iter().all(|f| e.ifs.contains(f)),
                        _ => false,
                    },
                });
                if !on_path {
                    r.problems
                        .push(problem("WFF_USE_NOT_ON_PATH", e.position, &[]));
                }
                if let Some(parsed) = parse_position(e.position) {
                    if parsed.last().is_some_and(|(_, l)| l.is_some()) {
                        let group: Vec<String> = parsed
                            .iter()
                            .enumerate()
                            .map(|(i, (num, l))| {
                                if i + 1 == parsed.len() {
                                    num.to_string()
                                } else {
                                    format!("{num}{}", l.map(String::from).unwrap_or_default())
                                }
                            })
                            .collect();
                        *groups.entry(group.join(".")).or_default() += 1;
                    }
                }
            }
            WorkflowFileStep::If {
                decided_by, paths, ..
            } => {
                let names: BTreeSet<&str> = paths.iter().map(|p| p.name.as_str()).collect();
                if !(2..=4).contains(&paths.len())
                    || names.len() != paths.len()
                    || paths.iter().any(|p| !within(&p.name, 1, 40))
                    || paths.iter().all(|p| p.steps.is_empty())
                {
                    r.problems
                        .push(problem("WFF_IF_PATHS_INVALID", e.position, &[]));
                }
                check_decider(&mut r, decided_by, e.position, &earlier);
            }
            WorkflowFileStep::Each {
                list, max_items, ..
            } => {
                if !(1..=50).contains(max_items) {
                    r.problems
                        .push(problem("WFF_LOOP_MAX_INVALID", e.position, &[]));
                }
                let ok = match list {
                    ListSource::Input(label) => input_kind(label) == Some(WorkflowInputKind::List),
                    ListSource::Step(p) => earlier(p),
                };
                if !ok {
                    r.problems
                        .push(problem("WFF_LOOP_LIST_INVALID", e.position, &[]));
                }
            }
            WorkflowFileStep::Until {
                decided_by,
                max_times,
                ..
            } => {
                if !(1..=10).contains(max_times) {
                    r.problems
                        .push(problem("WFF_LOOP_MAX_INVALID", e.position, &[]));
                }
                check_decider(&mut r, decided_by, e.position, &earlier);
            }
            WorkflowFileStep::Call { workflow, .. } => {
                if workflow == &file.name {
                    r.problems.push(problem(
                        "WFF_CALL_CYCLE",
                        e.position,
                        &[("name", workflow.clone())],
                    ));
                } else if !facts.workflows.contains_key(workflow) {
                    r.problems.push(problem(
                        "WFF_CALL_UNKNOWN",
                        e.position,
                        &[("name", workflow.clone())],
                    ));
                } else {
                    let (depth, cycle) =
                        call_depth(workflow, &file.name, &facts.workflows, &mut Vec::new());
                    if cycle {
                        r.problems.push(problem(
                            "WFF_CALL_CYCLE",
                            e.position,
                            &[("name", workflow.clone())],
                        ));
                    } else if depth + 1 > MAX_CALL_DEPTH {
                        r.problems.push(problem(
                            "WFF_CALL_TOO_DEEP",
                            e.position,
                            &[("name", workflow.clone())],
                        ));
                    }
                }
            }
        }
    }
    for (group, count) in groups {
        if count > u32::from(file.limits.agents_at_once) {
            r.problems.push(problem("WFF_GROUP_TOO_LARGE", group, &[]));
        }
    }

    // 19.4 agents in this workspace, 19.5 sending steps.
    let strict = mode == CheckMode::Save;
    for (i, agent) in file.agents.iter().enumerate() {
        if facts.unmatched.contains(&i) {
            continue;
        }
        match facts.presence.get(i) {
            Some(Presence::Present(_)) | None => {}
            Some(Presence::OtherVersion(..)) | Some(Presence::Missing) => {
                let p = problem(
                    "OH_AGENT_MISSING",
                    format!("/agents/{i}"),
                    &[("name", agent.name.clone())],
                );
                if strict {
                    r.problems.push(p);
                } else {
                    r.notices.push(p);
                }
            }
        }
    }
    for e in &entries {
        if let WorkflowFileStep::Agent {
            agent, asks_first, ..
        } = e.step
        {
            let instance = file
                .agents
                .iter()
                .position(|a| &a.key == agent)
                .and_then(|i| facts.presence.get(i))
                .and_then(|p| match p {
                    Presence::Present(id) => Some(*id),
                    _ => None,
                });
            if instance.is_some_and(|id| facts.sends_email.contains(&id)) && !asks_first {
                r.problems
                    .push(problem("WFF_SEND_WITHOUT_ASK", e.position, &[]));
            }
        }
    }
    r
}

fn check_decider(
    r: &mut WorkflowCheckReport,
    decider: &Decider,
    at: &str,
    earlier: &dyn Fn(&str) -> bool,
) {
    let ok = match decider {
        Decider::Ordin { words } | Decider::Person { words } => within(words, 1, 300),
        Decider::Application { step, .. } => earlier(step),
    };
    if !ok {
        r.problems.push(problem("WFF_CONDITION_INVALID", at, &[]));
    }
}

/// Whether the workflow's state is `needs_agents` after saving (19.4, other modes).
pub(crate) fn needs_agents(facts: &CheckFacts) -> bool {
    facts
        .presence
        .iter()
        .any(|p| !matches!(p, Presence::Present(_)))
}

// ---------------------------------------------------------------- reading (step 26.2)

/// A refusal of the whole file (no workflow row is added).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refused(pub &'static str);

/// An agent of a format 1 or 2 file (`{ name, version, role }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyAgent {
    pub name: String,
    pub version: String,
    pub role: String,
}

/// The file read as far as it can be without the workspace.
#[derive(Debug, Clone)]
pub(crate) enum Read {
    Current(WorkflowFile),
    Legacy {
        format: u8,
        value: Value,
        agents: Vec<LegacyAgent>,
    },
}

fn rename_kinds(steps: &mut Value) {
    let Some(list) = steps.as_array_mut() else {
        return;
    };
    for step in list {
        if let Some(obj) = step.as_object_mut() {
            if let Some(kind) = obj.get("kind").and_then(Value::as_str) {
                let renamed = match kind {
                    "each" => Some("for_each"),
                    "until" => Some("repeat_until"),
                    "call" => Some("run_workflow"),
                    _ => None,
                };
                if let Some(r) = renamed {
                    obj.insert("kind".into(), Value::String(r.into()));
                }
            }
            if let Some(inner) = obj.get_mut("steps") {
                rename_kinds(inner);
            }
            if let Some(paths) = obj.get_mut("paths").and_then(Value::as_array_mut) {
                for path in paths {
                    if let Some(inner) = path.get_mut("steps") {
                        rename_kinds(inner);
                    }
                }
            }
        }
    }
}

/// Step 26.2 up to the agents: size, the restricted YAML reader, the format.
pub(crate) fn read(yaml: &str) -> Result<Read, Refused> {
    if yaml.len() > FILE_MAX_BYTES {
        return Err(Refused("WFF_FILE_TOO_LARGE"));
    }
    let mut value: Value =
        ordin_types::yaml::read_restricted(yaml, FILE_MAX_BYTES).map_err(|e| match e.code() {
            "YAML_TOO_LARGE" => Refused("WFF_FILE_TOO_LARGE"),
            _ => Refused("WFF_FILE_MALFORMED"),
        })?;
    let Some(obj) = value.as_object() else {
        return Err(Refused("WFF_FILE_MALFORMED"));
    };
    let format = obj.get("format").or_else(|| obj.get("format_version"));
    let format = match format.and_then(Value::as_i64) {
        Some(f) if f > 3 => return Err(Refused("WFF_FORMAT_NEWER")),
        Some(f) if f >= 1 => f as u8,
        _ => return Err(Refused("WFF_FORMAT_UNSUPPORTED")),
    };
    if let Some(steps) = value.get_mut("steps") {
        rename_kinds(steps);
    }
    if format == 3 {
        return serde_json::from_value::<WorkflowFile>(value)
            .map(Read::Current)
            .map_err(|_| Refused("WFF_FILE_MALFORMED"));
    }
    let agents = value
        .get("agents")
        .and_then(Value::as_array)
        .ok_or(Refused("WFF_FILE_MALFORMED"))?
        .iter()
        .map(|a| {
            let text = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_owned);
            Ok(LegacyAgent {
                name: text("name").ok_or(Refused("WFF_FILE_MALFORMED"))?,
                version: text("version").unwrap_or_default(),
                role: text("role").unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>, Refused>>()?;
    Ok(Read::Legacy {
        format,
        value,
        agents,
    })
}

/// The key of a legacy agent: the name lower-cased, every run outside `[a-z0-9]` as `-`, cut to 32,
/// suffixed `-2`, `-3` … when taken.
pub(crate) fn legacy_key(name: &str, taken: &BTreeSet<String>) -> String {
    let mut key = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            key.push(c);
            dash = false;
        } else if !dash {
            key.push('-');
            dash = true;
        }
    }
    let base: String = key.chars().take(32).collect();
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|n| {
            let suffix = format!("-{n}");
            let cut: String = base.chars().take(32 - suffix.len()).collect();
            format!("{cut}{suffix}")
        })
        .find(|k| !taken.contains(k))
        .unwrap_or(base)
}

/// The identity of a matched legacy agent (from `agent_identity`).
#[derive(Debug, Clone)]
pub(crate) struct Identity {
    pub repository: AgentRepositoryRef,
    pub id: String,
    pub name: String,
}

/// Step 26.2.1–26.2.3: a format 1 or 2 file into the format-3 model; the problems of its agents.
pub(crate) fn build_legacy(
    format: u8,
    value: &Value,
    agents: &[LegacyAgent],
    identities: &[Option<Identity>],
) -> Result<(WorkflowFile, Vec<CheckProblem>, BTreeSet<usize>), Refused> {
    let mut problems = Vec::new();
    let mut unmatched = BTreeSet::new();
    let mut taken = BTreeSet::new();
    let mut by_name: HashMap<String, String> = HashMap::new();
    let mut file_agents = Vec::new();
    for (i, a) in agents.iter().enumerate() {
        let at = format!("/agents/{i}");
        if !within(&a.name, 1, 120) {
            problems.push(problem("WFF_AGENT_NAME_INVALID", format!("{at}/name"), &[]));
        }
        if semver::VersionReq::parse(&a.version).is_err() {
            problems.push(problem(
                "WFF_AGENT_VERSION_INVALID",
                format!("{at}/version"),
                &[],
            ));
        }
        if !within(&a.role, 1, 120) {
            problems.push(problem("WFF_AGENT_ROLE_INVALID", format!("{at}/role"), &[]));
        }
        let key = legacy_key(&a.name, &taken);
        taken.insert(key.clone());
        by_name.insert(a.name.clone(), key.clone());
        let entry = match identities.get(i).and_then(Option::as_ref) {
            Some(id) => WorkflowFileAgent {
                key,
                repository: id.repository.clone(),
                id: id.id.clone(),
                version: a.version.clone(),
                name: id.name.clone(),
                role: a.role.clone(),
            },
            None => {
                unmatched.insert(i);
                problems.push(problem(
                    "WFF_AGENT_UNMATCHED",
                    at,
                    &[("name", a.name.clone())],
                ));
                WorkflowFileAgent {
                    key,
                    repository: AgentRepositoryRef::Address(String::new()),
                    id: String::new(),
                    version: a.version.clone(),
                    name: a.name.clone(),
                    role: a.role.clone(),
                }
            }
        };
        file_agents.push(entry);
    }
    let mut v = value.clone();
    let obj = v.as_object_mut().ok_or(Refused("WFF_FILE_MALFORMED"))?;
    obj.remove("format_version");
    obj.insert("format".into(), Value::from(3));
    obj.insert(
        "agents".into(),
        serde_json::to_value(&file_agents).map_err(|_| Refused("WFF_FILE_MALFORMED"))?,
    );
    if let Some(steps) = obj.get_mut("steps") {
        legacy_steps(steps, format, &by_name);
    }
    let file: WorkflowFile =
        serde_json::from_value(v).map_err(|_| Refused("WFF_FILE_MALFORMED"))?;
    Ok((file, problems, unmatched))
}

fn legacy_steps(steps: &mut Value, format: u8, by_name: &HashMap<String, String>) {
    let Some(list) = steps.as_array_mut() else {
        return;
    };
    for step in list {
        let Some(obj) = step.as_object_mut() else {
            continue;
        };
        if format == 1 {
            obj.insert("kind".into(), Value::String("agent".into()));
            for key in [
                "decided_by",
                "list",
                "max_items",
                "max_times",
                "workflow",
                "paths",
                "if_it_fails",
                "steps",
            ] {
                obj.remove(key);
            }
        }
        if let Some(name) = obj.get("agent").and_then(Value::as_str) {
            if let Some(key) = by_name.get(name) {
                obj.insert("agent".into(), Value::String(key.clone()));
            }
        }
        if let Some(inner) = obj.get_mut("steps") {
            legacy_steps(inner, format, by_name);
        }
        if let Some(paths) = obj.get_mut("paths").and_then(Value::as_array_mut) {
            for path in paths {
                if let Some(inner) = path.get_mut("steps") {
                    legacy_steps(inner, format, by_name);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- writing (step 25)

/// The key order of format 3's structs (the order of their fields), for the writer.
const KEY_ORDER: &[&str] = &[
    "format",
    "kind",
    "position",
    "name",
    "purpose",
    "inputs",
    "agents",
    "steps",
    "limits",
    "origin",
    "label",
    "required",
    "key",
    "repository",
    "id",
    "version",
    "role",
    "agent",
    "line",
    "uses",
    "asks_first",
    "ordin_checks",
    "if_it_fails",
    "decided_by",
    "by",
    "check",
    "step",
    "words",
    "paths",
    "list",
    "max_items",
    "max_times",
    "workflow",
    "max_rounds",
    "wall_clock_minutes",
    "request_budget",
    "agents_at_once",
    "failure_repeat_limit",
    "external_action_agents",
];

fn rank(key: &str) -> usize {
    KEY_ORDER
        .iter()
        .position(|k| *k == key)
        .unwrap_or(KEY_ORDER.len())
}

fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::Null => Some("null".into()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        // A JSON string is a YAML 1.2 double-quoted scalar.
        Value::String(s) => serde_json::to_string(s).ok(),
        Value::Array(a) if a.is_empty() => Some("[]".into()),
        Value::Object(o) if o.is_empty() => Some("{}".into()),
        _ => None,
    }
}

fn emit(v: &Value, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.cmp(b)));
            for k in keys {
                let value = &map[k.as_str()];
                match scalar(value) {
                    Some(s) => out.push_str(&format!("{pad}{k}: {s}\n")),
                    None => {
                        out.push_str(&format!("{pad}{k}:\n"));
                        emit(value, indent + 2, out);
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                match scalar(item) {
                    Some(s) => out.push_str(&format!("{pad}- {s}\n")),
                    None => {
                        let mut inner = String::new();
                        emit(item, indent + 2, &mut inner);
                        // The first line of the item takes the dash.
                        let body = inner.trim_start_matches(' ');
                        out.push_str(&format!("{pad}- {body}"));
                    }
                }
            }
        }
        other => {
            if let Some(s) = scalar(other) {
                out.push_str(&format!("{pad}{s}\n"));
            }
        }
    }
}

/// The version's content as format 3 YAML: keys in the order of the struct, step tags spelled out,
/// no anchors, every string double-quoted.
pub(crate) fn to_yaml(file: &WorkflowFile) -> String {
    let value = serde_json::to_value(file).unwrap_or(Value::Null);
    let mut out = String::new();
    emit(&value, 0, &mut out);
    out
}

// ---------------------------------------------------------------- charters (C-24)

/// A file's steps as 2C `WorkflowStep`s; agents by key → instance, called workflows by name → id.
pub(crate) fn to_domain(
    steps: &[WorkflowFileStep],
    agents: &HashMap<String, AgentInstanceId>,
    called: &HashMap<String, WorkflowId>,
) -> Vec<WorkflowStep> {
    steps
        .iter()
        .filter_map(|step| step_to_domain(step, agents, called))
        .collect()
}

// R1 limit (step 17): 2C domain `StepUse` has no form for a `for_each` item (`StepUse::Item`);
// the Crew Orchestrator gives each item's step the item as an input, so it is left out here.
// spec: ordin-desktop/components/OrdinHost#OH-R1-1
fn uses_to_domain(uses: &[StepUse]) -> Vec<DomainUse> {
    uses.iter()
        .filter_map(|u| match u {
            StepUse::Input(label) => Some(DomainUse::Input {
                label: label.clone(),
            }),
            StepUse::Step(position) => Some(DomainUse::Result {
                position: position.clone(),
            }),
            StepUse::Item(_) => None,
        })
        .collect()
}

fn condition(d: &Decider) -> Condition {
    match d {
        Decider::Application { check, step } => Condition::Application {
            condition_kind: crate::store::word(check),
            looks_at_position: step.clone(),
        },
        Decider::Ordin { words } => Condition::Ordin {
            words: words.clone(),
        },
        Decider::Person { words } => Condition::Person {
            words: words.clone(),
        },
    }
}

fn step_to_domain(
    step: &WorkflowFileStep,
    agents: &HashMap<String, AgentInstanceId>,
    called: &HashMap<String, WorkflowId>,
) -> Option<WorkflowStep> {
    Some(match step {
        WorkflowFileStep::Agent {
            position,
            agent,
            line,
            uses,
            asks_first,
            ordin_checks,
            if_it_fails,
        } => {
            let group = parse_position(position)
                .and_then(|p| p.last().copied())
                .and_then(|(n, l)| l.map(|_| n));
            WorkflowStep::Agent(AgentStep {
                position: position.clone(),
                group,
                agent: AgentRef::AgentInstance {
                    agent_instance_id: *agents.get(agent)?,
                },
                line: line.clone(),
                uses: uses_to_domain(uses),
                asks_first: *asks_first,
                ordin_checks_result: *ordin_checks,
                if_it_fails: *if_it_fails,
            })
        }
        WorkflowFileStep::If {
            position,
            decided_by,
            paths,
        } => WorkflowStep::If(IfBlock {
            position: position.clone(),
            condition: condition(decided_by),
            paths: paths
                .iter()
                .map(|p| DomainPath {
                    name: p.name.clone(),
                    steps: to_domain(&p.steps, agents, called),
                })
                .collect(),
        }),
        WorkflowFileStep::Each {
            position,
            list,
            max_items,
            steps,
        } => WorkflowStep::Each(EachBlock {
            position: position.clone(),
            list: match list {
                ListSource::Input(label) => DomainUse::Input {
                    label: label.clone(),
                },
                ListSource::Step(position) => DomainUse::Result {
                    position: position.clone(),
                },
            },
            maximum: u32::from(*max_items),
            steps: to_domain(steps, agents, called),
        }),
        WorkflowFileStep::Until {
            position,
            decided_by,
            max_times,
            steps,
        } => WorkflowStep::Until(UntilBlock {
            position: position.clone(),
            condition: condition(decided_by),
            maximum: u32::from(*max_times),
            steps: to_domain(steps, agents, called),
        }),
        WorkflowFileStep::Call { position, workflow } => WorkflowStep::Call(CallBlock {
            position: position.clone(),
            workflow_id: *called.get(workflow)?,
        }),
    })
}

/// Counts for the Usage Planner's workflow estimate (step 18): agent steps at each block's maximum,
/// and `if` steps decided by Ordin.
pub(crate) fn estimate_counts(steps: &[WorkflowFileStep]) -> (u32, u32) {
    let mut agent_steps = 0u32;
    let mut ordin_turns = 0u32;
    for step in steps {
        match step {
            WorkflowFileStep::Agent { .. } => agent_steps += 1,
            WorkflowFileStep::If {
                decided_by, paths, ..
            } => {
                if matches!(decided_by, Decider::Ordin { .. }) {
                    ordin_turns += 1;
                }
                let (a, o) = paths
                    .iter()
                    .map(|p| estimate_counts(&p.steps))
                    .fold((0, 0), |acc, x| (acc.0.max(x.0), acc.1.max(x.1)));
                agent_steps += a;
                ordin_turns += o;
            }
            WorkflowFileStep::Each {
                max_items, steps, ..
            } => {
                let (a, o) = estimate_counts(steps);
                agent_steps += a * u32::from(*max_items);
                ordin_turns += o * u32::from(*max_items);
            }
            WorkflowFileStep::Until {
                max_times,
                steps,
                decided_by,
                ..
            } => {
                let (a, o) = estimate_counts(steps);
                let ordin = u32::from(matches!(decided_by, Decider::Ordin { .. }));
                agent_steps += a * u32::from(*max_times);
                ordin_turns += (o + ordin) * u32::from(*max_times);
            }
            WorkflowFileStep::Call { .. } => {}
        }
    }
    (agent_steps, ordin_turns)
}
