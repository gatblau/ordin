//! The tools of server `ordin` (OrdinHost.md *ToolProvider* and section C), offered to Ordin's
//! current run only. Every call is already decided by the Rules Engine in the MCP Front.

use std::sync::{Arc, Weak};
use std::time::Instant;

use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolCaller,
    ToolFailure, ToolKey, ToolOutput, ToolProvider, ToolSpec,
};
use ordin_memory_service::{
    FactSource, FactSourceInput, MemoryCaller, RecallInput, RecallSearch, RecallSearchKind,
    RememberFactsInput, RememberInput, ThingInput, WiderMode,
};
use ordin_package_catalogue::{ChangeChoiceInput, PackChoice, PendingChoice};
use ordin_runtime_adapter::{PauseReason, RunEndReason, RunEvent};
use ordin_service_api::blocks::{
    Block, BlockCheckContext, FormBlock, FormField, FormInput, MessageBlock,
};

/// OH-RC-1: the longest `produced` summary, the block check's bound.
const PRODUCED_SUMMARY_MAX_CHARS: usize = 2_000;
/// OH-ACC-1: the most folders one access card offers.
const ACCESS_PLACES_MAX: usize = 20;
/// OH-TW-1: the argument that names a team workspace's results folder.
const RESULTS_FOLDER_ARGUMENT: &str = "results_folder";
/// 2C `Task`: at most 50 arguments (LD-40).
const TASK_ARGUMENTS_MAX: usize = 50;
/// 2C `Task`: free text 1–8,000 characters (LD-40).
const TASK_FREE_TEXT_MAX_CHARS: usize = 8_000;
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{RunOrigin, RunState, Task, TaskArgument, TaskTarget};
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::UnansweredReason;
use ordin_types::{AgentInstanceId, MemoryFactId, RunId, TaskId, WorkspaceId};
use ordin_usage_planner::{AnswerToIndex, EstimateRequest, ReuseQuery, WorkKind, WorkShape};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::host::{cut_chars, lock, Expect, Kept, OrdinHost, COMPONENT};
use crate::ports::{DirectoryAgent, LocMessage};
use crate::types::*;

pub(crate) const SERVER: &str = "ordin";

/// OH-TR-2: the version of a source that is not a working copy.
const AS_READ: &str = "as read";

/// Step 15.3: the Memory Service's codes that go back to the model as a failure it can correct.
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
const REMEMBER_REFUSALS: [&str; 15] = [
    "MEM_FACTS_INVALID",
    "MEM_TRIPLE_INVALID",
    "MEM_TERM_UNKNOWN",
    "MEM_TERM_PROPOSAL_INVALID",
    "MEM_TERM_PROPOSAL_REFUSED",
    "MEM_TERM_OBJECT_MISMATCH",
    "MEM_FLAGS_INVALID",
    "MEM_TOO_MANY_TERMS",
    "MEM_SOURCE_REQUIRED",
    "MEM_SOURCE_INVALID",
    "MEM_NOT_A_FACT",
    "MEM_MADE_FROM_NOT_ALLOWED",
    "MEM_MADE_FROM_INVALID",
    // spec: ordin-desktop/components/OrdinHost#OH-TOP-1
    "MEM_TOPIC_INVALID",
    "MEM_TOPIC_REFUSED",
];

/// The published schema of `remember`'s `facts`: the Memory Service's `TripleInput` (MemoryService.md
/// amendment ADR-118), whose serde shape it describes.
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
pub(crate) fn triple_inputs_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let thing = json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "name": { "type": "string", "minLength": 1, "maxLength": 200 },
            "other_names": {
                "type": "array", "maxItems": 8,
                "items": { "type": "string", "minLength": 1, "maxLength": 200 }
            }
        },
        "required": ["name"]
    });
    let one = |key: &str, value: Value| {
        json!({
            "type": "object", "additionalProperties": false,
            "properties": { key: value }, "required": [key]
        })
    };
    let object = json!({ "oneOf": [
        one("thing", thing.clone()),
        // spec: ordin-desktop/components/MemoryService#MEM-TR-13
        one("text", json!({
            "type": "string", "minLength": 1, "maxLength": 120,
            "description": ordin_memory_service::REMEMBER_TEXT_VALUE
        })),
        one("number", json!({
            "type": "object", "additionalProperties": false,
            "properties": { "value": { "type": "string" }, "unit": { "type": "string", "minLength": 1, "maxLength": 32 } },
            "required": ["value"]
        })),
        one("money", json!({
            "type": "object", "additionalProperties": false,
            "properties": { "amount": { "type": "string" }, "currency": { "type": "string", "pattern": "^[A-Z]{3}$" } },
            "required": ["amount", "currency"]
        })),
        one("date", json!({ "type": "string", "pattern": "^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$" })),
        one("yes_no", json!({ "type": "boolean" })),
    ]});
    let predicate = json!({ "oneOf": [
        { "type": "string", "minLength": 1, "maxLength": 64 },
        one("propose", json!({
            "type": "object", "additionalProperties": false,
            "properties": {
                "label": {
                    "type": "string", "minLength": 1, "maxLength": 64,
                    "description": ordin_memory_service::REMEMBER_LINK_WORD
                },
                "meaning": { "type": "string", "minLength": 1, "maxLength": 300 },
                "object": { "type": "string", "enum": ["thing", "text", "number", "money", "date", "yes_no"] },
                "single_valued": { "type": "boolean" }
            },
            "required": ["label", "meaning", "object", "single_valued"]
        })),
    ]});
    // OH-TOP-1: the subject may carry a topic for a thing that has none yet (MemoryService.md
    // MEM-TOP-3); an object's thing never does.
    // spec: ordin-desktop/components/OrdinHost#OH-TOP-1
    let mut subject = thing.clone();
    subject["properties"]["topic"] = json!({
        "type": "string", "minLength": 1,
        "maxLength": ordin_memory_service::TOPIC_MAX_CHARS,
        "description": "A new thing's short topic in plain words, such as Strategy, People, Funding or Suppliers; reuse one already given where it fits."
    });
    let schema = json!({
        "type": "array", "minItems": 1, "maxItems": 16,
        "items": {
            "type": "object", "additionalProperties": false,
            "properties": {
                "subject": subject,
                "predicate": predicate,
                "object": object,
                "changed": { "type": "boolean" },
                "ended": { "type": "boolean" },
                "confidence_permille": { "type": "integer", "minimum": 0, "maximum": 1000 },
                "valid_from": { "type": "string", "pattern": "^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$" }
            },
            "required": ["subject", "predicate", "object"]
        }
    });
    schemars::Schema::try_from(schema).unwrap_or_default()
}

/// OH-DLG-2: what is known of a delegated run that did not complete.
struct TaskFailure {
    /// The run went from `queued` to its end without reaching `running`.
    never_ran: bool,
    /// The run's `end_reason`, without a closing full stop; None when it has none.
    reason: Option<String>,
}

pub(crate) fn fail(code: &str, message: &str, retryable: bool) -> ToolFailure {
    ToolFailure {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable,
    }
}

pub(crate) fn dep_fail(e: &crate::ports::DepError) -> ToolFailure {
    // OH-DEP-1: the model is told only that it did not work; the dependency's own code goes to the log,
    // or a failure in the field cannot be told apart from any other (found on Windows: `remember`
    // failed with nothing in the log but `OH_DEPENDENCY_FAILED`).
    // spec: ordin-desktop/components/OrdinHost#OH-DEP-1
    tracing::warn!(
        component = COMPONENT,
        code = e.code.as_str(),
        retryable = e.retryable,
        "ordin_host.dependency_failed"
    );
    // OH-DEP-2: memory whose one-time download has not been made is not an error to report. Only the
    // permanent form of the code means that; the retryable form is a model that answered too late.
    // spec: ordin-desktop/components/OrdinHost#OH-DEP-2
    if e.code == "MEM_MODEL_UNAVAILABLE" && !e.retryable {
        return fail("OH_DEPENDENCY_FAILED", MEMORY_NOT_READY, false);
    }
    fail(
        "OH_DEPENDENCY_FAILED",
        "That did not work; try again.",
        e.retryable,
    )
}

/// OH-DEP-2: what Ordin is told when memory's models are not downloaded yet.
pub(crate) const MEMORY_NOT_READY: &str = "Memory is not ready yet: its one-time download has not been made. Tell the person they can start it from the card ordin shows; do not call it an error.";

fn own(e: &crate::ports::DepError) -> ToolFailure {
    let message = if e.message.is_empty() {
        e.code.clone()
    } else {
        e.message.clone()
    };
    fail(&e.code, &message, e.retryable)
}

pub(crate) fn out(text: impl Into<String>, structured: Option<Value>) -> ToolOutput {
    ToolOutput {
        text: text.into(),
        structured,
    }
}

/// Typed deserialisation with unknown fields refused; the serde path becomes the JSON Pointer.
pub(crate) fn typed<T: DeserializeOwned>(value: &Value) -> Result<T, ArgumentProblem> {
    serde_path_to_error::deserialize::<_, T>(value).map_err(|error| {
        let mut at = String::new();
        for segment in error.path().iter() {
            match segment {
                serde_path_to_error::Segment::Seq { index } => at.push_str(&format!("/{index}")),
                serde_path_to_error::Segment::Map { key } => {
                    at.push('/');
                    at.push_str(&key.replace('~', "~0").replace('/', "~1"));
                }
                serde_path_to_error::Segment::Enum { .. }
                | serde_path_to_error::Segment::Unknown => {}
            }
        }
        let message = error.inner().to_string();
        for prefix in ["unknown field `", "missing field `"] {
            if let Some(rest) = message.strip_prefix(prefix) {
                if let Some(field) = rest.split('`').next() {
                    let suffix = format!("/{field}");
                    if !at.ends_with(&suffix) {
                        at.push_str(&suffix);
                    }
                }
            }
        }
        ArgumentProblem {
            at,
            reason: message.chars().take(300).collect(),
        }
    })
}

pub(crate) fn len(at: &str, value: &str, min: usize, max: usize) -> Result<(), ArgumentProblem> {
    let n = value.chars().count();
    if n < min || n > max {
        return Err(ArgumentProblem {
            at: at.to_owned(),
            reason: format!("must be {min} to {max} characters"),
        });
    }
    Ok(())
}

pub(crate) fn schema<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or_else(|_| json!({}))
}

/// The published schema of `decide`: one object, the variants of `DecideArgs` merged as McpFront.md
/// SA-RECON-10 merges the `work` tools' variants. The arguments are still parsed into the enum by
/// `check_arguments`; only the published description differs from the generated one.
// spec: ordin-desktop/components/OrdinHost#OH-REC-4
pub(crate) fn decide_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "description": "kind = check needs kind, verdict, reason. kind = condition needs kind, path, reason. kind = question needs kind, answer.",
        "properties": {
            "kind": {"type": "string", "enum": ["check", "condition", "question"]},
            "verdict": {"type": "string", "enum": ["carry_on", "shortfall"]},
            "reason": {"type": "string"},
            "path": {"type": "string"},
            "answer": {"type": "string"}
        },
        "required": ["kind"]
    })
}

/// The published schema of `change_setting`: one object, the variants of `ChangeSettingArgs`
/// merged; `value` differs per form, so it is an `anyOf` of each form's schema in variant order.
// spec: ordin-desktop/components/OrdinHost#OH-REC-4
pub(crate) fn change_setting_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "description": "setting = tone needs setting, value. setting = register needs setting, value. setting = length needs setting, value. setting = name_to_use needs setting; no value, null or an empty value clears the name. setting = interface_language needs setting, locale, name.",
        "properties": {
            "setting": {
                "type": "string",
                "enum": ["tone", "register", "length", "name_to_use", "interface_language"]
            },
            "value": {"anyOf": [
                {"type": "string", "enum": ["friendly", "neutral", "warm", "direct"]},
                {"type": "string", "enum": ["everyday", "formal"]},
                {"type": "string", "enum": ["brief", "shortest"]},
                {"type": ["string", "null"]}
            ]},
            "locale": {"type": "string"},
            "name": {"type": "string"}
        },
        "required": ["setting"]
    })
}

/// The tool table: name, description, argument form, plain words and input schema.
fn specs(desk: Option<Value>) -> Vec<ToolSpec> {
    // spec: ordin-desktop/components/OrdinHost#OH-REC-1
    // `decide` and `change_setting` take an enum; each publishes one object schema (OH-REC-4).
    let mut list = vec![
        // spec: ordin-desktop/components/OrdinHost#OH-TF-3
        // spec: ordin-desktop/components/OrdinHost#OH-TF-5
        ("delegate", "Give work to one agent as a brief, or a step to a crew member. Pass a task's inputs only when the person named it.", "delegate(agent, brief, command?, arguments?, inputs?: {name: value}, artefacts?, code_in?)", "Can ask your agents to do work.", schema::<DelegateArgs>()),
        ("finish", "End the crew run with a summary.", "finish(summary: text)", "Can end a crew's work.", schema::<FinishArgs>()),
        ("escalate", "Ask the person one question when the crew cannot carry on.", "escalate(question: text, recommendation?: text)", "Can ask you a question for the crew.", schema::<EscalateArgs>()),
        ("decide", "Answer the crew's request for a check, a condition or a member's question.", "decide(kind: \"check\", verdict: \"carry_on\"|\"shortfall\", reason) | decide(kind: \"condition\", path, reason) | decide(kind: \"question\", answer)", "Checks results while a crew works.", decide_schema()),
        ("create_crew", "Propose a crew: purpose, agents with roles, and limits.", "create_crew(purpose, members: [{agent, role}] 1–8, limits?: {rounds, wall_clock_minutes, members_at_once, request_budget}, inputs?: [labels])", "Can propose a crew of agents.", schema::<CreateCrewArgs>()),
        ("keep_crew", "Draft a workflow from the crew run that just finished.", "keep_crew(crew_run_id, name)", "Can save a crew's steps as a workflow.", schema::<KeepCrewArgs>()),
        ("save_workflow", "Draft a workflow for the person to save.", "save_workflow(name, purpose, inputs, agents: [{key, agent, role}], steps: [format 3 steps], limits?, crew_run_id?)", "Can draft workflows for you to save.", schema::<SaveWorkflowArgs>()),
        ("propose_workflow_change", "Propose a changed version of a workflow for the person to save.", "propose_workflow_change(workflow_id, base_version, steps, change_note, purpose?, inputs?, limits?)", "Can suggest changes to a workflow.", schema::<ProposeWorkflowChangeArgs>()),
        ("take_over", "Ask the crew to let you coordinate the rest of a workflow run from a step; the crew decides.", "take_over(crew_run_id, from_position: \"3\", reason)", "Can ask to take over a workflow that does not fit.", schema::<TakeOverArgs>()),
        ("show", "Show the person a block from the catalogue.", "show(block: {kind: \"message\"|\"question\"|\"form\"|…, …})", "Shows you messages, forms and results.", schema::<ShowArgs>()),
        // spec: ordin-desktop/components/OrdinHost#OH-TR-2
        ("remember", "Keep 1–16 facts, one claim each: subject, a word for how it relates to the value, thing or value. Or summary+made_from.", "remember(source: \"conversation\" | where read, facts?: 1–16 × {subject: {name, other_names?, topic?}, predicate: word | {propose: {label, meaning, object, single_valued}}, object: {thing}|{text}|{number, unit?}|{money, currency}|{date}|{yes_no}, changed?, ended?, valid_from?}, summary?, made_from?)", "Remembers facts about this workspace, each with where it came from.", schema::<RememberArgs>()),
        ("recall", "Recall kept facts. folders: only those (\"own\" is this team's); searches: lex or vec queries; wider: company knowledge.", "recall(query?: text | searches?: 1–4 × {type: lex|vec, query}, folders?: names, result_cap?: 1–20, wider?: bool)", "Recalls what it remembered, and can look in your company's knowledge agents.", schema::<RecallArgs>()),
        ("write_page", "Write or update a knowledge page in this workspace, with a one-sentence description of what it explains and its type.", "write_page(title, body: markdown, sources: [{name: place|sharepoint|confluence, location, version_read}], description: text, type?: Explanation|How-to|Decision|Reference|Glossary, page_id?: text to change a page you wrote)", "Keeps knowledge pages for the workspace.", schema::<WritePageArgs>()),
        ("find_agent", "Find agents in the library for a need.", "find_agent(need: text)", "Finds agents in your library.", schema::<FindAgentArgs>()),
        ("propose_agent", "Suggest the person adds an agent from the library.", "propose_agent(agent_repository_id, agent: \"urn:ordin:…\", reason)", "Suggests agents to add.", schema::<ProposeAgentArgs>()),
        ("propose_push", "Propose sending changed documents back where they live.", "propose_push(request: text, items: [{item: working copy item id, summary: text}])", "Can propose sending changes back, which you approve.", schema::<ProposePushArgs>()),
        ("propose_workflow_run", "Propose running a saved workflow; the person starts it from the crew plan.", "propose_workflow_run(workflow_id, request: text)", "Can suggest running one of your saved workflows.", schema::<ProposeWorkflowRunArgs>()),
        ("keep_dashboard", "Keep a report as a dashboard the person can refresh: the one titled report, else the most recent.", "keep_dashboard(name: text, report?: \"<report title>\")", "Can keep a report as a dashboard when you ask.", schema::<KeepDashboardArgs>()),
        ("change_setting", "Change one of the person's settings when they ask for it in words.", "change_setting(setting: \"tone\"|\"register\"|\"length\"|\"name_to_use\"|\"interface_language\", value | locale + name)", "Can change how it talks to you and your language when you ask.", change_setting_schema()),
    ];
    // spec: ordin-desktop/components/OrdinHost#OH-MIN-2
    #[cfg(feature = "minutes")]
    list.push((
        "keep_minutes",
        "Keep the meeting's minutes: a summary, the decisions, the actions with their owners, and the open questions.",
        "keep_minutes(title, summary, decisions[], actions[{text, owner?}], open_questions[])",
        "Keeps the minutes of a meeting you ask it to write up.",
        schema::<crate::minutes::KeepMinutesArgs>(),
    ));
    // OH-MIN-7: Ordin's minutes and transcription tools; offered only with a minutes port (OH-MIN-5).
    // spec: ordin-desktop/components/OrdinHost#OH-MIN-7
    #[cfg(feature = "minutes")]
    list.extend(crate::minutes_tools::specs());
    if let Some(schema) = desk {
        list.push((
            "propose_trigger",
            "Propose a trigger that runs a saved workflow when a chat file, an event or a message in a linked Slack channel arrives.",
            "propose_trigger(name, listens_for, filter, input_mapping, workflow_id, workflow_version, …)",
            "Can propose triggers, which you turn on.",
            schema,
        ));
    }
    list.into_iter()
        .map(|(name, description, form, plain, schema)| ToolSpec {
            name: name.to_owned(),
            description: description.to_owned(),
            input_schema: schema,
            argument_form: form.to_owned(),
            sandbox_callable: false,
            plain: plain.to_owned(),
        })
        .collect()
}

/// One of Ordin's eleven minutes tools (OH-MIN-7); always false without the feature.
fn is_minutes_tool(name: &str) -> bool {
    #[cfg(feature = "minutes")]
    {
        crate::minutes_tools::MINUTES_TOOLS.contains(&name)
    }
    #[cfg(not(feature = "minutes"))]
    {
        let _ = name;
        false
    }
}

/// The `ToolProvider` of server `ordin` (`provider_id = "ordin-host"`).
pub(crate) struct OrdinTools(pub Weak<OrdinHost>);

#[async_trait::async_trait]
impl ToolProvider for OrdinTools {
    fn provider_id(&self) -> &'static str {
        "ordin-host"
    }

    fn servers(&self) -> Vec<ServerSpec> {
        let desk = self
            .0
            .upgrade()
            .and_then(|h| h.trigger_desk())
            .map(|d| d.proposal_schema());
        match ServerName::new(SERVER) {
            Ok(name) => vec![ServerSpec {
                name,
                tools: specs(desk),
            }],
            Err(_) => vec![],
        }
    }

    // spec: ordin-desktop/components/OrdinHost#SA-RECON-08
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        let Some(host) = self.0.upgrade() else {
            return Ok(vec![]);
        };
        let Ok(server) = ServerName::new(SERVER) else {
            return Ok(vec![]);
        };
        // OH-MIN-2: a minutes run is offered `keep_minutes` and nothing else; no other run is
        // offered it.
        // spec: ordin-desktop/components/OrdinHost#OH-MIN-2
        #[cfg(feature = "minutes")]
        if host.is_minutes_run(grant.run_id) {
            if grant.caller != ToolCaller::Ordin {
                return Ok(vec![]);
            }
            return Ok(vec![ToolKey {
                server,
                tool: "keep_minutes".to_owned(),
            }]);
        }
        if grant.caller != ToolCaller::Ordin
            || host.current_run(grant.workspace_id) != Some(grant.run_id)
        {
            return Ok(vec![]);
        }
        let desk = host.trigger_desk().map(|d| d.proposal_schema());
        // OH-MIN-4, OH-MIN-5: the eleven minutes tools only when the program gave the port.
        // spec: ordin-desktop/components/OrdinHost#OH-MIN-4
        #[cfg(feature = "minutes")]
        let minutes_offered = host.minutes_port().is_some();
        #[cfg(not(feature = "minutes"))]
        let minutes_offered = false;
        Ok(specs(desk)
            .into_iter()
            .filter(|s| s.name != "keep_minutes")
            .filter(|s| minutes_offered || !is_minutes_tool(&s.name))
            .map(|s| ToolKey {
                server: server.clone(),
                tool: s.name,
            })
            .collect())
    }

    fn check_arguments(&self, key: &ToolKey, a: &Value) -> Result<(), ArgumentProblem> {
        #[cfg(feature = "minutes")]
        if let Some(result) = crate::minutes_tools::check_arguments(&key.tool, a) {
            return result;
        }
        match key.tool.as_str() {
            "delegate" => {
                let d: DelegateArgs = typed(a)?;
                len("/agent", &d.agent, 1, 36)?;
                len("/brief", &d.brief, 1, 8_000)?;
                if let Some(c) = &d.command {
                    len("/command", c, 1, 100)?;
                }
                if let Some(x) = &d.arguments {
                    len("/arguments", x, 0, 16_384)?;
                }
                if d.artefacts.len() > 50 {
                    return Err(ArgumentProblem {
                        at: "/artefacts".into(),
                        reason: "at most 50".into(),
                    });
                }
                if let Some(c) = &d.code_in {
                    len("/code_in", c, 1, 200)?;
                }
                // OH-TF-3: at most 8 inputs; each text 1–4,000 characters, a list at most 20.
                if let Some(inputs) = &d.inputs {
                    if inputs.len() > 8 {
                        return Err(ArgumentProblem {
                            at: "/inputs".into(),
                            reason: "at most 8".into(),
                        });
                    }
                    for (name, value) in inputs {
                        let at = format!("/inputs/{name}");
                        match value {
                            TaskInputText::One(text) => len(&at, text, 1, 4_000)?,
                            TaskInputText::Many(list) => {
                                if list.len() > 20 {
                                    return Err(ArgumentProblem {
                                        at,
                                        reason: "at most 20".into(),
                                    });
                                }
                                for (i, text) in list.iter().enumerate() {
                                    len(&format!("{at}/{i}"), text, 1, 4_000)?;
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            "finish" => len("/summary", &typed::<FinishArgs>(a)?.summary, 1, 4_000),
            "escalate" => {
                let e: EscalateArgs = typed(a)?;
                len("/question", &e.question, 1, 500)?;
                len(
                    "/recommendation",
                    e.recommendation.as_deref().unwrap_or(""),
                    0,
                    500,
                )
            }
            "decide" => match typed::<DecideArgs>(a)? {
                DecideArgs::Check { reason, .. } => len("/reason", &reason, 1, 500),
                DecideArgs::Condition { path, reason } => {
                    len("/path", &path, 1, 40)?;
                    len("/reason", &reason, 1, 500)
                }
                DecideArgs::Question { answer } => len("/answer", &answer, 1, 4_000),
            },
            "create_crew" => {
                let c: CreateCrewArgs = typed(a)?;
                len("/purpose", &c.purpose, 1, 2_000)?;
                for (i, m) in c.members.iter().enumerate() {
                    len(&format!("/members/{i}/role"), &m.role, 1, 200)?;
                }
                if c.inputs.len() > 20 {
                    return Err(ArgumentProblem {
                        at: "/inputs".into(),
                        reason: "at most 20".into(),
                    });
                }
                for (i, l) in c.inputs.iter().enumerate() {
                    len(&format!("/inputs/{i}"), l, 1, 60)?;
                }
                Ok(())
            }
            "keep_crew" => len("/name", &typed::<KeepCrewArgs>(a)?.name, 1, 80),
            "keep_dashboard" => {
                let k: KeepDashboardArgs = typed(a)?;
                len("/name", k.name.trim(), 1, 80)?;
                len("/report", k.report.as_deref().unwrap_or(""), 0, 200)
            }
            "save_workflow" => typed::<SaveWorkflowArgs>(a).map(|_| ()),
            "propose_workflow_change" => len(
                "/change_note",
                &typed::<ProposeWorkflowChangeArgs>(a)?.change_note,
                1,
                200,
            ),
            "take_over" => {
                let t: TakeOverArgs = typed(a)?;
                len("/from_position", &t.from_position, 1, 20)?;
                len("/reason", &t.reason, 1, 500)
            }
            // The block itself is checked by the catalogue check when called (step 14).
            "show" => match a.get("block") {
                Some(Value::Object(_)) => Ok(()),
                _ => Err(ArgumentProblem {
                    at: "/block".into(),
                    reason: "a block object is required".into(),
                }),
            },
            // spec: ordin-desktop/components/OrdinHost#OH-TR-2
            "remember" => {
                let r: RememberArgs = typed(a)?;
                len("/source", &r.source, 1, 2_048)?;
                let shape = ArgumentProblem {
                    at: "/facts".into(),
                    reason: "remember takes facts, or a summary with made_from.".into(),
                };
                match (&r.summary, r.facts.is_empty()) {
                    (Some(summary), true) => {
                        let n = summary.chars().count();
                        if !(1..=500).contains(&n) || r.made_from.is_empty() {
                            return Err(shape);
                        }
                        if !(2..=64).contains(&r.made_from.len()) {
                            return Err(ArgumentProblem {
                                at: "/made_from".into(),
                                reason: "2 to 64 fact ids".into(),
                            });
                        }
                    }
                    (None, false) => {
                        if !r.made_from.is_empty() || r.facts.len() > 16 {
                            return Err(shape);
                        }
                    }
                    _ => return Err(shape),
                }
                Ok(())
            }
            "recall" => {
                if a.get("wider").is_some_and(|w| !w.is_boolean()) {
                    return Err(ArgumentProblem {
                        at: "/wider".into(),
                        reason: "must be true or false".into(),
                    });
                }
                let r: RecallArgs = typed(a)?;
                match (&r.query, &r.searches) {
                    (Some(q), None) => len("/query", q, 1, 4_096)?,
                    (None, Some(s)) => {
                        if !(1..=4).contains(&s.len()) {
                            return Err(ArgumentProblem {
                                at: "/searches".into(),
                                reason: "1 to 4 searches".into(),
                            });
                        }
                        for (i, x) in s.iter().enumerate() {
                            len(&format!("/searches/{i}/query"), &x.query, 1, 500)?;
                        }
                    }
                    _ => {
                        return Err(ArgumentProblem {
                            at: "/query".into(),
                            reason: "exactly one of query and searches".into(),
                        })
                    }
                }
                if let Some(f) = &r.folders {
                    if !(1..=21).contains(&f.len()) {
                        return Err(ArgumentProblem {
                            at: "/folders".into(),
                            reason: "1 to 21 folders".into(),
                        });
                    }
                }
                if r.result_cap.is_some_and(|c| !(1..=20).contains(&c)) {
                    return Err(ArgumentProblem {
                        at: "/result_cap".into(),
                        reason: "1 to 20".into(),
                    });
                }
                Ok(())
            }
            "write_page" => {
                let d = a.get("description").and_then(Value::as_str);
                match d {
                    Some(d)
                        if !d.is_empty()
                            && d.chars().count() <= 200
                            && !d.contains(['\n', '\r']) => {}
                    _ => {
                        return Err(ArgumentProblem {
                            at: "/description".into(),
                            reason: "a one-sentence description of 1 to 200 characters is required"
                                .into(),
                        })
                    }
                }
                if let Some(t) = a.get("type") {
                    let ok = t.as_str().is_some_and(|t| PAGE_TYPES.contains(&t));
                    if !ok {
                        return Err(ArgumentProblem {
                            at: "/type".into(),
                            reason:
                                "must be one of Explanation, How-to, Decision, Reference, Glossary"
                                    .into(),
                        });
                    }
                }
                let w: WritePageArgs = typed(a)?;
                len("/title", &w.title, 1, 200)?;
                if w.body.len() > 1_048_576 {
                    return Err(ArgumentProblem {
                        at: "/body".into(),
                        reason: "at most 1 MiB".into(),
                    });
                }
                if w.sources.len() > 100 {
                    return Err(ArgumentProblem {
                        at: "/sources".into(),
                        reason: "at most 100".into(),
                    });
                }
                Ok(())
            }
            "find_agent" => len("/need", &typed::<FindAgentArgs>(a)?.need, 1, 500),
            "propose_agent" => {
                let p: ProposeAgentArgs = typed(a)?;
                len("/agent", &p.agent, 1, 256)?;
                len("/reason", &p.reason, 1, 300)
            }
            "propose_push" => {
                let p: ProposePushArgs = typed(a)?;
                len("/request", &p.request, 1, 4_000)?;
                if !(1..=200).contains(&p.items.len()) {
                    return Err(ArgumentProblem {
                        at: "/items".into(),
                        reason: "1 to 200 items".into(),
                    });
                }
                for (i, item) in p.items.iter().enumerate() {
                    len(&format!("/items/{i}/summary"), &item.summary, 1, 120)?;
                    if item.summary.contains('\n') {
                        return Err(ArgumentProblem {
                            at: format!("/items/{i}/summary"),
                            reason: "one line".into(),
                        });
                    }
                }
                Ok(())
            }
            "propose_workflow_run" => typed::<ProposeWorkflowRunArgs>(a).map(|_| ()),
            "change_setting" => typed::<ChangeSettingArgs>(a).map(|_| ()),
            // OH-MIN-2: the shape only; a value outside its rules answers OH_MINUTES_INVALID from
            // the call, so the agent can call again.
            #[cfg(feature = "minutes")]
            "keep_minutes" => typed::<crate::minutes::KeepMinutesArgs>(a).map(|_| ()),
            "propose_trigger" => match a {
                Value::Object(_) => Ok(()),
                _ => Err(ArgumentProblem {
                    at: String::new(),
                    reason: "an object is required".into(),
                }),
            },
            _ => Err(ArgumentProblem {
                at: String::new(),
                reason: "unknown tool".into(),
            }),
        }
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        let host = self.0.upgrade().ok_or_else(|| {
            fail(
                "OH_NOT_ORDIN_SESSION",
                "These tools are for Ordin's conversation only.",
                false,
            )
        })?;
        let started = Instant::now();
        let result = host.tool(&ctx, &key.tool, arguments).await;
        let (outcome, code) = match &result {
            Ok(_) => ("ok", String::new()),
            Err(f) => ("failed", f.code.clone()),
        };
        tracing::info!(
            component = COMPONENT,
            workspace_id = ctx.workspace_id.to_string().as_str(),
            run_id = ctx.run_id.to_string().as_str(),
            tool = key.tool.to_string().as_str(),
            outcome,
            code = code.as_str(),
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "ordin_host.tool"
        );
        result
    }

    async fn session_closed(&self, _run_id: RunId) {}
}

pub(crate) const PAGE_TYPES: [&str; 5] =
    ["Explanation", "How-to", "Decision", "Reference", "Glossary"];

/// A delegation kept in memory while a question about it waits (step 8.3).
#[derive(Debug, Clone)]
pub(crate) struct Delegation {
    pub workspace_id: WorkspaceId,
    pub agent: AgentInstanceId,
    pub name: String,
    pub args: Delegated,
    pub stage: Stage,
}

/// The arguments of a `delegate` call, kept.
#[derive(Debug, Clone)]
pub(crate) struct Delegated {
    pub brief: String,
    pub command: Option<String>,
    pub arguments: Option<String>,
    pub artefacts: Vec<ordin_types::ArtefactId>,
    pub code_in: Option<String>,
    /// OH-TA-1: the task this one tries again, from **Try again** on its run; such a delegation is
    /// never answered from an earlier run (step 8.3 (a)), and the person started it.
    pub run_again_of: Option<TaskId>,
    /// OH-TF-1: a task started from its form (or with its named inputs, OH-TF-3): its arguments,
    /// documents and what its start does; never answered from an earlier run.
    pub form: Option<Box<crate::task_form::FormStart>>,
    /// OH-ACC-3: the work changes a linked document; its working copy is asked for read and change.
    pub changes_documents: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum Stage {
    Pack(PendingChoice),
    Reuse,
    Estimate,
    /// OH-TF-1 step 7 (c): which linked repository a software-engineering task works in.
    Repository,
    /// OH-ACC-2: the folders, `(place, link name)`, the agent needs to read; **Allow** grants them.
    Folders(Vec<(ordin_types::PlaceId, String)>),
}

/// OH-TA-1: a task's arguments as the one `arguments` text of step 8.3 (d): the value of the one
/// argument named `arguments`, else each argument as a line `<name>: <value>`; `None` when there are
/// none.
fn arguments_text(arguments: &[TaskArgument]) -> Option<String> {
    match arguments {
        [] => None,
        [only] if only.name == "arguments" => Some(only.value.clone()),
        many => Some(
            many.iter()
                .map(|a| format!("{}: {}", a.name, a.value))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
}

/// Where a delegation stands after step 8.3 ran as far as it could.
pub(crate) enum Delegated8 {
    Started { run_id: RunId, name: String },
    Waiting(ToolOutput),
    Failed(ToolFailure),
}

impl OrdinHost {
    /// Step 7, then the tool.
    pub(crate) async fn tool(
        &self,
        ctx: &ToolCallContext,
        tool: &str,
        a: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        // spec: ordin-desktop/components/OrdinHost#SA-RECON-08
        let not_ordin = || {
            fail(
                "OH_NOT_ORDIN_SESSION",
                "These tools are for Ordin's conversation only.",
                false,
            )
        };
        // OH-MIN-2: a minutes run can call `keep_minutes` and nothing else.
        // spec: ordin-desktop/components/OrdinHost#OH-MIN-2
        #[cfg(feature = "minutes")]
        if self.is_minutes_run(ctx.run_id) {
            if ctx.caller != ToolCaller::Ordin || tool != "keep_minutes" {
                return Err(crate::minutes::not_a_minutes_run());
            }
            let args = typed(&a).map_err(|p| {
                fail(
                    "MCP_ARGUMENTS_INVALID",
                    &format!("Wrong arguments for {tool} at {}: {}", p.at, p.reason),
                    false,
                )
            })?;
            return self.keep_minutes_tool(ctx.run_id, args).await;
        }
        if ctx.caller != ToolCaller::Ordin || self.current_run(ctx.workspace_id) != Some(ctx.run_id)
        {
            return Err(not_ordin());
        }
        let ws = ctx.workspace_id;
        // OH-WK-1: the working line says what Ordin is doing; `delegate` says it once it knows the
        // agent's name.
        if tool != "delegate" {
            let doing = match tool {
                "find_agent" => Some(LocMessage::DoingFindAgent),
                "recall" => Some(LocMessage::DoingRecall),
                "remember" => Some(LocMessage::DoingRemember),
                "write_page" => Some(LocMessage::DoingWritePage),
                "show" => Some(LocMessage::DoingShow),
                // spec: ordin-desktop/components/OrdinHost#OH-MIN-11
                #[cfg(feature = "minutes")]
                "transcribe_recording" => Some(LocMessage::DoingTranscribeRecording),
                #[cfg(feature = "minutes")]
                "import_minutes" => Some(LocMessage::DoingImportMinutes),
                #[cfg(feature = "minutes")]
                "export_minutes" => Some(LocMessage::DoingExportMinutes),
                _ => None,
            };
            self.working_on(ws, doing).await;
        }
        let bad = |p: ArgumentProblem| {
            fail(
                "MCP_ARGUMENTS_INVALID",
                &format!("Wrong arguments for {tool} at {}: {}", p.at, p.reason),
                false,
            )
        };
        #[cfg(feature = "minutes")]
        if is_minutes_tool(tool) {
            return self.minutes_tool(ws, tool, a).await;
        }
        match tool {
            "delegate" => self.delegate(ws, typed(&a).map_err(bad)?).await,
            "finish" => {
                let f: FinishArgs = typed(&a).map_err(bad)?;
                self.crew_call(
                    ws,
                    ordin_crew_orchestrator::CrewToolCall::Finish { summary: f.summary },
                    "Finished.",
                )
            }
            "escalate" => {
                let e: EscalateArgs = typed(&a).map_err(bad)?;
                self.escalate(ws, e)
            }
            "decide" => self.decide(ws, typed(&a).map_err(bad)?),
            "create_crew" => self.create_crew(ws, typed(&a).map_err(bad)?).await,
            "keep_crew" => {
                self.keep_crew(ws, ctx.run_id, typed(&a).map_err(bad)?)
                    .await
            }
            "keep_dashboard" => self.keep_dashboard_tool(ws, typed(&a).map_err(bad)?).await,
            "save_workflow" => {
                self.save_workflow_draft(ws, ctx.run_id, typed(&a).map_err(bad)?)
                    .await
            }
            "propose_workflow_change" => {
                self.propose_workflow_change(ws, typed(&a).map_err(bad)?)
                    .await
            }
            "take_over" => {
                let t: TakeOverArgs = typed(&a).map_err(bad)?;
                // Step 13: a crew of another workspace is answered as an unknown one, and the
                // orchestrator is not asked (FU-120).
                let owner = self
                    .ports
                    .orchestrator
                    .workspace_of(t.crew_run_id)
                    .await
                    .map_err(|e| own(&e))?;
                if owner != Some(ws) {
                    return Err(fail(
                        "CREW_NOT_FOUND",
                        "This crew run no longer exists.",
                        false,
                    ));
                }
                self.ports
                    .orchestrator
                    .request_take_over(t.crew_run_id, t.from_position.clone(), t.reason)
                    .await
                    .map_err(|e| own(&e))?;
                Ok(out(
                    format!("Taken over from step {}.", t.from_position),
                    None,
                ))
            }
            "show" => self.show_tool(ws, a).await,
            "remember" => {
                self.remember_tool(ws, ctx.run_id, typed(&a).map_err(bad)?)
                    .await
            }
            "recall" => {
                self.recall_tool(ws, ctx.run_id, typed(&a).map_err(bad)?)
                    .await
            }
            "write_page" => {
                self.write_page_tool(ws, ctx.run_id, typed(&a).map_err(bad)?)
                    .await
            }
            "find_agent" => {
                let f: FindAgentArgs = typed(&a).map_err(bad)?;
                let found = self
                    .ports
                    .catalogue
                    .find_agents(ws, &f.need)
                    .await
                    .map_err(|e| dep_fail(&e))?;
                // OH-FA-1: each line carries what `propose_agent` takes, or the key `delegate` takes when
                // the agent is already in the workspace; the name, summary and source alone left Ordin
                // nothing to propose it with.
                // spec: ordin-desktop/components/OrdinHost#OH-FA-1
                let lines: Vec<String> = found
                    .iter()
                    .take(10)
                    .map(|a| match a.in_workspace {
                        Some(key) => format!(
                            "{} — {} ({}; already in this workspace, key {key})",
                            a.name, a.summary, a.from
                        ),
                        None => format!(
                            "{} — {} ({}; agent_repository_id {}, agent {})",
                            a.name, a.summary, a.from, a.agent_repository_id, a.identifier
                        ),
                    })
                    .collect();
                Ok(out(
                    if lines.is_empty() {
                        "No agent in the library fits.".to_owned()
                    } else {
                        lines.join("\n")
                    },
                    None,
                ))
            }
            "propose_agent" => {
                let p: ProposeAgentArgs = typed(&a).map_err(bad)?;
                self.ports
                    .catalogue
                    .check_proposal(p.agent_repository_id, &p.agent)
                    .await
                    .map_err(|e| own(&e))?;
                let block = Block::AgentProposal(ordin_service_api::blocks::AgentProposalBlock {
                    agent_repository_id: p.agent_repository_id,
                    agent: p.agent,
                    reason: p.reason,
                });
                self.show_block(ws, MessageAuthor::Ordin, block, false)
                    .await
                    .map_err(|e| fail(&e.code(), "That did not work; try again.", e.retryable()))?;
                Ok(out("Shown.", None))
            }
            "propose_push" => {
                let p: ProposePushArgs = typed(&a).map_err(bad)?;
                let id = self
                    .ports
                    .push
                    .propose(ordin_push::PushProposal {
                        workspace_id: ws,
                        run_id: Some(ctx.run_id),
                        request: p.request,
                        items: p
                            .items
                            .into_iter()
                            .map(|i| ordin_push::ProposedItem {
                                working_copy_item_id: i.item,
                                summary: i.summary,
                                change_view: None,
                            })
                            .collect(),
                    })
                    .await
                    .map_err(|e| dep_fail(&e))?;
                Ok(out(
                    "Proposed; the person approves before anything is sent.",
                    Some(json!({ "pending_push_id": id })),
                ))
            }
            "propose_trigger" => {
                let desk = self.trigger_desk().ok_or_else(not_ordin)?;
                let id = desk
                    .propose(ws, a)
                    .await
                    .map_err(|e| fail(&e.code, &e.message, e.retryable))?;
                Ok(out(
                    "Proposed; the person turns it on.",
                    Some(json!({ "trigger_id": id })),
                ))
            }
            "propose_workflow_run" => self.propose_workflow_run(ws, typed(&a).map_err(bad)?).await,
            "change_setting" => self.change_setting(ws, typed(&a).map_err(bad)?).await,
            _ => Err(fail(
                "MCP_ARGUMENTS_INVALID",
                "No such tool on server ordin.",
                false,
            )),
        }
    }

    // ---------------------------------------------------------------- step 9 (crew tools)

    fn crew_call(
        &self,
        ws: WorkspaceId,
        call: ordin_crew_orchestrator::CrewToolCall,
        answer: &str,
    ) -> Result<ToolOutput, ToolFailure> {
        let mut state = lock(&self.state);
        let pending = state
            .convs
            .get_mut(&ws)
            .and_then(|c| c.pending.as_mut())
            .ok_or_else(|| fail("OH_NOT_COORDINATING", "No crew is waiting for this.", false))?;
        match pending.expect {
            Expect::Round => {
                pending.got.calls.push(call);
                Ok(out(answer, None))
            }
            _ => Err(fail(
                "OH_DECISION_KIND_MISMATCH",
                &format!("The crew is waiting for {}.", expected(&pending.expect)),
                false,
            )),
        }
    }

    fn escalate(&self, ws: WorkspaceId, e: EscalateArgs) -> Result<ToolOutput, ToolFailure> {
        let mut state = lock(&self.state);
        let pending = state
            .convs
            .get_mut(&ws)
            .and_then(|c| c.pending.as_mut())
            .ok_or_else(|| fail("OH_NOT_COORDINATING", "No crew is waiting for this.", false))?;
        match pending.expect {
            Expect::Round => {
                pending
                    .got
                    .calls
                    .push(ordin_crew_orchestrator::CrewToolCall::Escalate {
                        question: e.question,
                        recommendation: e.recommendation,
                    });
                Ok(out("Escalated to the person.", None))
            }
            Expect::Question => {
                pending.got.escalated = Some((e.question, e.recommendation));
                Ok(out("Escalated to the person.", None))
            }
            _ => Err(fail(
                "OH_DECISION_KIND_MISMATCH",
                &format!("The crew is waiting for {}.", expected(&pending.expect)),
                false,
            )),
        }
    }

    fn decide(&self, ws: WorkspaceId, d: DecideArgs) -> Result<ToolOutput, ToolFailure> {
        let mut state = lock(&self.state);
        let pending = state
            .convs
            .get_mut(&ws)
            .and_then(|c| c.pending.as_mut())
            .ok_or_else(|| fail("OH_NOT_COORDINATING", "No crew is waiting for this.", false))?;
        let fits = match (&pending.expect, &d) {
            (Expect::Check, DecideArgs::Check { .. })
            | (Expect::Question, DecideArgs::Question { .. }) => true,
            (Expect::Condition { paths }, DecideArgs::Condition { path, .. }) => {
                if !paths.contains(path) {
                    return Err(fail(
                        "OH_PATH_UNKNOWN",
                        &format!("Choose one of these paths: {}.", paths.join(", ")),
                        false,
                    ));
                }
                true
            }
            _ => false,
        };
        if !fits {
            return Err(fail(
                "OH_DECISION_KIND_MISMATCH",
                &format!("The crew is waiting for {}.", expected(&pending.expect)),
                false,
            ));
        }
        pending.got.decision = Some(d);
        Ok(out("Decided.", None))
    }

    // ---------------------------------------------------------------- step 8 (delegate)

    pub(crate) async fn directory_hit(
        &self,
        ws: WorkspaceId,
        key: &str,
    ) -> Result<Option<DirectoryAgent>, ToolFailure> {
        let agents = self
            .ports
            .inspector
            .directory(ws)
            .await
            .map_err(|e| dep_fail(&e))?;
        Ok(agents
            .into_iter()
            .find(|a| a.agent_instance_id.to_string() == key))
    }

    /// OH-DLG-1: a `command` must be one the agent's directory entry lists, by its name or by the name
    /// the directory shows it under (step 6); otherwise no task starts. Found in the field: Ordin passed
    /// a library use case's title as a command, the run was refused before it started, and Ordin had
    /// already been told "Started".
    // spec: ordin-desktop/components/OrdinHost#OH-DLG-1
    fn check_command(&self, agent: &DirectoryAgent, command: &str) -> Result<(), ToolFailure> {
        let claude = self.config.harness == ordin_types::domain::HarnessKind::ClaudeCode;
        let listed: Vec<String> = agent
            .commands
            .iter()
            .map(|c| crate::seed::listed_command_name(agent, c, claude))
            .collect();
        let offered = agent.commands.iter().zip(&listed).any(|(c, shown)| {
            c.name == command
                || c.session_name.as_deref() == Some(command)
                || shown == command
                || crate::task_form::run_command_name(c) == command
        });
        if offered {
            return Ok(());
        }
        let its = if listed.is_empty() {
            "It has no commands.".to_owned()
        } else {
            format!("Its commands: {}.", listed.join(", "))
        };
        Err(fail(
            "OH_COMMAND_UNKNOWN",
            &format!(
                "{} has no command \"{command}\". {its} Give the work as a brief, without a command.",
                agent.display_name
            ),
            false,
        ))
    }

    /// The command of `agent` that `command` names, as `check_command` matches it.
    fn offered_command(
        &self,
        agent: &DirectoryAgent,
        command: &str,
    ) -> Option<crate::ports::DirectoryCommand> {
        let claude = self.config.harness == ordin_types::domain::HarnessKind::ClaudeCode;
        agent
            .commands
            .iter()
            .find(|c| {
                c.name == command
                    || c.session_name.as_deref() == Some(command)
                    || crate::seed::listed_command_name(agent, c, claude) == command
                    || crate::task_form::run_command_name(c) == command
            })
            .cloned()
    }

    async fn delegate(&self, ws: WorkspaceId, d: DelegateArgs) -> Result<ToolOutput, ToolFailure> {
        // spec: ordin-desktop/components/OrdinHost#SA-CLOSE-10
        let unknown = || {
            fail(
                "OH_AGENT_UNKNOWN",
                "No agent with that key is in this workspace; use a key from the agent directory.",
                false,
            )
        };
        let agent = self
            .directory_hit(ws, &d.agent)
            .await?
            .ok_or_else(unknown)?;
        // spec: ordin-desktop/components/OrdinHost#OH-WK-1
        self.working_on(
            ws,
            Some(LocMessage::DoingDelegate {
                agent: agent.display_name.clone(),
            }),
        )
        .await;
        let member = AgentRef::AgentInstance {
            agent_instance_id: agent.agent_instance_id,
        };
        let in_round = {
            let state = lock(&self.state);
            state
                .convs
                .get(&ws)
                .and_then(|c| c.pending.as_ref())
                .is_some_and(|p| matches!(p.expect, Expect::Round))
        };
        if in_round {
            return self.crew_call(
                ws,
                ordin_crew_orchestrator::CrewToolCall::Delegate {
                    member,
                    brief: d.brief,
                    uses_results_of: vec![],
                    uses_inputs: vec![],
                },
                &format!("Delegated to {}.", agent.display_name),
            );
        }
        if let Some(command) = &d.command {
            self.check_command(&agent, command)?;
            // OH-TF-3: a task with named inputs is given them, or its form opens.
            // spec: ordin-desktop/components/OrdinHost#OH-TF-3
            if let Some(task) = self.offered_command(&agent, command) {
                if task.inputs.is_some() {
                    // OH-TF-5: a task's command and inputs, and so its form, only when the person's
                    // latest message names the task; a request in their own words is a plain brief.
                    // spec: ordin-desktop/components/OrdinHost#OH-TF-5
                    if !self.person_named_task(ws, &agent, &task) {
                        return Err(fail(
                            "OH_INPUT_INVALID",
                            &format!(
                                "The person did not ask for {} by name. Give the work as a brief, without a command or inputs.",
                                crate::task_form::task_title(&task)
                            ),
                            false,
                        ));
                    }
                    if d.arguments.is_some() {
                        return Err(fail(
                            "OH_INPUT_INVALID",
                            &format!(
                                "{} takes named inputs; pass them as inputs, not arguments.",
                                crate::task_form::task_title(&task)
                            ),
                            false,
                        ));
                    }
                    let given = d.inputs.clone().unwrap_or_default();
                    return match self.delegate_with_inputs(ws, &agent, &task, &given).await {
                        Ok(Delegated8::Started { run_id, name }) => Ok(out(
                            format!("Started: {name} is working on it."),
                            Some(json!({ "run_id": run_id })),
                        )),
                        Ok(Delegated8::Waiting(o)) => Ok(o),
                        Ok(Delegated8::Failed(f)) => Err(f),
                        Err(e) => {
                            let message = match e.message() {
                                "" => "That did not work; try again.",
                                m => m,
                            };
                            Err(fail(&e.code(), message, e.retryable()))
                        }
                    };
                }
                if task.form_unreadable && d.inputs.is_some() {
                    return Err(fail(
                        "OH_TASK_UNREADABLE",
                        "This task can't be started from here. Ask Ordin in the conversation instead.",
                        false,
                    ));
                }
            }
        }
        self.ports
            .catalogue
            .ensure_ready(agent.agent_instance_id)
            .await
            .map_err(|e| own(&e))?;
        // OH-DLG-3: the task runs the command under its bare stem, whichever name Ordin used.
        // spec: ordin-desktop/components/OrdinHost#OH-DLG-3
        let command = d.command.map(|c| match self.offered_command(&agent, &c) {
            Some(listed) => crate::task_form::run_command_name(&listed),
            None => c,
        });
        let delegation = Delegation {
            workspace_id: ws,
            agent: agent.agent_instance_id,
            name: agent.display_name.clone(),
            args: Delegated {
                brief: d.brief,
                command,
                arguments: d.arguments,
                artefacts: d.artefacts,
                code_in: d.code_in,
                run_again_of: None,
                form: None,
                changes_documents: d.changes_documents,
            },
            stage: Stage::Reuse,
        };
        // OH-ACC-2: a file the person attached in a folder the agent cannot read asks first.
        if let Some(asked) = self.ask_for_folders(&delegation).await {
            return Ok(asked);
        }
        if let Some(asked) = self
            .ask_for_access(ws, agent.agent_instance_id, &agent.display_name)
            .await
        {
            return Ok(asked);
        }
        match self.proceed(delegation, true).await {
            Delegated8::Started { run_id, name } => Ok(out(
                format!("Started: {name} is working on it."),
                Some(json!({ "run_id": run_id })),
            )),
            Delegated8::Waiting(o) => Ok(o),
            Delegated8::Failed(f) => Err(f),
        }
    }

    /// OH-TA-1: **Try again** on a run that ended `interrupted` or `failed`. The run's task is given to
    /// the same agent again through step 8.3 (folder access, pack question, estimate), never answered
    /// from an earlier run, as a new task whose `run_again_of` is the old one.
    // spec: ordin-desktop/components/OrdinHost#OH-TA-1
    pub async fn try_again_run(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
    ) -> Result<TryAgainOutcome, crate::OhError> {
        use crate::OhError;
        let failed = |f: ToolFailure| OhError::DependencyFailed {
            dependency: "delegation",
            code: f.code,
            retryable: f.retryable,
        };
        let (run, owner, task) = self
            .ports
            .supervisor
            .run_task(run_id)
            .await
            .map_err(|e| OhError::dependency("supervisor", &e))?
            .ok_or(OhError::RunUnknown)?;
        if owner != workspace_id {
            return Err(OhError::RunUnknown);
        }
        if !matches!(run.state, RunState::Interrupted | RunState::Failed) {
            return Err(OhError::RunNotInterrupted);
        }
        let Some(task) = task else {
            return Err(OhError::RunNotRepeatable);
        };
        let TaskTarget::AgentInstance { agent_instance_id } = task.target else {
            return Err(OhError::RunNotRepeatable);
        };
        // A coding task works in its own worktree, which a plain task would not have.
        let coding = self
            .ports
            .projects
            .is_coding_task(task.id)
            .await
            .map_err(|e| OhError::dependency("projects", &e))?;
        if coding {
            return Err(OhError::RunNotRepeatable);
        }
        let agent = self
            .directory_hit(workspace_id, &agent_instance_id.to_string())
            .await
            .map_err(failed)?
            .ok_or(OhError::RunNotRepeatable)?;
        self.ports
            .catalogue
            .ensure_ready(agent.agent_instance_id)
            .await
            .map_err(|e| OhError::dependency("catalogue", &e))?;
        let waiting = |o: ToolOutput| {
            tracing::info!(
                component = COMPONENT,
                run_id = run_id.to_string().as_str(),
                "ordin_host.try_again_waiting"
            );
            Ok(TryAgainOutcome::Waiting { text: o.text })
        };
        if let Some(asked) = self
            .ask_for_access(workspace_id, agent.agent_instance_id, &agent.display_name)
            .await
        {
            return waiting(asked);
        }
        let delegation = Delegation {
            workspace_id,
            agent: agent.agent_instance_id,
            name: agent.display_name.clone(),
            args: Delegated {
                brief: task
                    .free_text
                    .clone()
                    .or_else(|| task.command.clone())
                    .unwrap_or_default(),
                command: task.command.clone(),
                arguments: arguments_text(&task.arguments),
                artefacts: task.attached_artefacts.clone(),
                code_in: None,
                run_again_of: Some(task.id),
                form: None,
                changes_documents: false,
            },
            stage: Stage::Estimate,
        };
        match self.proceed(delegation, true).await {
            Delegated8::Started {
                run_id: new_run_id, ..
            } => {
                tracing::info!(
                    component = COMPONENT,
                    run_id = run_id.to_string().as_str(),
                    new_run_id = new_run_id.to_string().as_str(),
                    "ordin_host.try_again"
                );
                Ok(TryAgainOutcome::Started { run_id: new_run_id })
            }
            Delegated8::Waiting(o) => waiting(o),
            Delegated8::Failed(f) => Err(failed(f)),
        }
    }

    /// OH-ACC-1: before a task starts on `agent`, when the workspace links folders and the agent may
    /// use none of them, no task starts: the conversation shows a tick box per folder and Ordin is told
    /// to delegate again once the person has chosen. Asked once per agent per conversation, so a
    /// person who ticks nothing is not asked again; a failed read asks nothing.
    // spec: ordin-desktop/components/OrdinHost#OH-ACC-1
    pub(crate) async fn ask_for_access(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        name: &str,
    ) -> Option<ToolOutput> {
        let asked_before = lock(&self.state)
            .convs
            .get(&ws)
            .is_some_and(|c| c.access_asked.contains(&agent));
        if asked_before {
            return None;
        }
        let folders = match self.ports.workspaces.agent_folders(ws, agent).await {
            Ok(folders) => folders,
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.access_not_read"
                );
                return None;
            }
        };
        if folders.is_empty() || folders.iter().any(|f| f.granted) {
            return None;
        }
        if let Some(conv) = lock(&self.state).convs.get_mut(&ws) {
            conv.access_asked.insert(agent);
        }
        let block = Block::Access(ordin_service_api::blocks::AccessBlock {
            agent_instance_id: agent,
            agent_name: name.to_owned(),
            text: format!(
                "{name} can't use this workspace's folders yet. Tick the ones it may use, then ask me to go ahead."
            ),
            places: folders
                .into_iter()
                .take(ACCESS_PLACES_MAX)
                .map(|f| ordin_service_api::blocks::AccessPlace {
                    place_id: f.place_id,
                    name: f.name,
                    read_only: f.read_only,
                })
                .collect(),
        });
        if let Err(e) = self
            .show_block(ws, MessageAuthor::Ordin, block, false)
            .await
        {
            tracing::warn!(
                component = COMPONENT,
                code = e.code().as_str(),
                "ordin_host.access_not_shown"
            );
            return None;
        }
        Some(out(
            format!("{name} can't use any folder of this workspace yet. I asked the person which ones it may use; delegate again once they say so."),
            None,
        ))
    }

    /// OH-TF-5: whether the person's latest message names `task`: its title, or its command as the
    /// agent lists it or as it is named, ignoring case.
    // spec: ordin-desktop/components/OrdinHost#OH-TF-5
    fn person_named_task(
        &self,
        ws: WorkspaceId,
        agent: &DirectoryAgent,
        task: &crate::ports::DirectoryCommand,
    ) -> bool {
        let said = lock(&self.state)
            .convs
            .get(&ws)
            .map(|c| c.persons_text.to_lowercase())
            .unwrap_or_default();
        if said.trim().is_empty() {
            return false;
        }
        let claude = self.config.harness == ordin_types::domain::HarnessKind::ClaudeCode;
        let mut names: Vec<String> = vec![
            task.name.clone(),
            crate::seed::listed_command_name(agent, task, claude),
        ];
        names.extend(task.title.clone());
        names.extend(task.session_name.clone());
        names
            .iter()
            .map(|n| n.trim().to_lowercase())
            .any(|n| !n.is_empty() && said.contains(&n))
    }

    /// OH-ACC-2: before a plain delegation starts, when the person's latest message attached a file
    /// in a folder the agent holds no grant on, no task starts: one waiting question asks the person
    /// to let the agent read those folders, with **Allow** and **Not now**, and the delegation is kept
    /// under it. Asked once per agent and folder for each message of the person; a failed read or show
    /// asks nothing. Nothing is granted here: the person's **Allow** grants (OH-ACC-1).
    // spec: ordin-desktop/components/OrdinHost#OH-ACC-2
    pub(crate) async fn ask_for_folders(&self, d: &Delegation) -> Option<ToolOutput> {
        let ws = d.workspace_id;
        let needed: Vec<ordin_types::PlaceId> = {
            let state = lock(&self.state);
            let conv = state.convs.get(&ws)?;
            conv.attached_places
                .iter()
                .filter(|p| !conv.folder_asked.contains(&(d.agent, **p)))
                .copied()
                .collect()
        };
        if needed.is_empty() {
            return None;
        }
        let folders = match self.ports.workspaces.agent_folders(ws, d.agent).await {
            Ok(folders) => folders,
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.access_not_read"
                );
                return None;
            }
        };
        // OH-ACC-3: a change to a linked document also asks when the agent may only read its working copy.
        // spec: ordin-desktop/components/OrdinHost#OH-ACC-3
        let changing = if d.args.changes_documents {
            self.change_needed(ws, d.agent, &needed).await
        } else {
            Vec::new()
        };
        let missing: Vec<(ordin_types::PlaceId, String)> = needed
            .iter()
            .filter_map(|p| {
                folders
                    .iter()
                    .find(|f| f.place_id == *p && (!f.granted || changing.contains(p)))
                    .map(|f| (f.place_id, f.name.clone()))
            })
            .collect();
        if missing.is_empty() {
            return None;
        }
        if let Some(conv) = lock(&self.state).convs.get_mut(&ws) {
            for (place, _) in &missing {
                conv.folder_asked.insert((d.agent, *place));
            }
        }
        let names = folder_names(&missing);
        let name = &d.name;
        let kept = Delegation {
            stage: Stage::Folders(missing),
            ..d.clone()
        };
        if let Err(e) = self
            .ask(
                ws,
                &cut_chars(
                    &if changing.is_empty() {
                        format!("{name} needs to read {names} to answer.")
                    } else {
                        format!(
                            "{name} needs to change {names}. Changes go back only after you approve them."
                        )
                    },
                    FOLDER_QUESTION_MAX_CHARS,
                ),
                vec![FOLDER_ALLOW.to_owned(), FOLDER_NOT_NOW.to_owned()],
                Some(Kept::Delegation(Box::new(kept))),
            )
            .await
        {
            tracing::warn!(
                component = COMPONENT,
                code = e.code().as_str(),
                "ordin_host.access_not_shown"
            );
            return None;
        }
        tracing::info!(
            component = COMPONENT,
            workspace_id = ws.to_string().as_str(),
            agent_instance_id = d.agent.to_string().as_str(),
            "ordin_host.access_asked"
        );
        Some(out(
            format!(
                "Nothing has started yet. I asked the person to let {name} read {names}; the task starts when they press Allow. Tell them in one sentence what {name} will do."
            ),
            None,
        ))
    }

    /// OH-ACC-3: of `places`, the working copies of linked documents on which `agent` does not hold read
    /// and change. Any other kind of place is never asked for change here.
    // spec: ordin-desktop/components/OrdinHost#OH-ACC-3
    async fn change_needed(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        places: &[ordin_types::PlaceId],
    ) -> Vec<ordin_types::PlaceId> {
        use ordin_types::domain::places::{FolderAccess, Grantee, PlaceKind};
        let grants = self.ports.workspaces.grants(ws).await.unwrap_or_default();
        let mut out = Vec::new();
        for place in places {
            let Ok(found) = self.ports.files.place(*place).await else {
                continue;
            };
            if found.kind != PlaceKind::WorkingCopy {
                continue;
            }
            let held = grants.iter().any(|g| {
                g.place_id == *place
                    && g.access == FolderAccess::ReadAndChange
                    && matches!(g.grantee, Grantee::AgentInstance { agent_instance_id } if agent_instance_id == agent)
            });
            if !held {
                out.push(*place);
            }
        }
        out
    }

    /// OH-ACC-2: the person's answer to the folder question. **Allow** grants read access to each
    /// folder asked about and goes on with the delegation; **Not now** grants nothing and starts
    /// nothing. Ordin is told either way.
    // spec: ordin-desktop/components/OrdinHost#OH-ACC-2
    async fn folders_answered(
        &self,
        d: &Delegation,
        places: &[(ordin_types::PlaceId, String)],
        text: &str,
    ) -> Result<Option<Delegated8>, crate::OhError> {
        let ws = d.workspace_id;
        let name = &d.name;
        let names = folder_names(places);
        if text != FOLDER_ALLOW {
            tracing::info!(
                component = COMPONENT,
                workspace_id = ws.to_string().as_str(),
                agent_instance_id = d.agent.to_string().as_str(),
                "ordin_host.access_declined"
            );
            self.send_turn(
                ws,
                crate::host::Turn::fixed(format!(
                    "The person chose Not now: {name} may not read {names}, so nothing started. Tell the person in one sentence that {name} can't read it without access."
                )),
            )
            .await?;
            return Ok(None);
        }
        // OH-ACC-3: read and change on a linked document's working copy when the work changes it.
        let changing = if d.args.changes_documents {
            let ids: Vec<ordin_types::PlaceId> = places.iter().map(|(p, _)| *p).collect();
            self.change_needed(ws, d.agent, &ids).await
        } else {
            Vec::new()
        };
        let mut made: Vec<ordin_types::PlaceId> = Vec::new();
        for (place, _) in places {
            if let Err(e) = self
                .ports
                .workspaces
                .set_agent_grant(
                    ws,
                    d.agent,
                    *place,
                    Some(if changing.contains(place) {
                        ordin_types::domain::places::FolderAccess::ReadAndChange
                    } else {
                        ordin_types::domain::places::FolderAccess::Read
                    }),
                )
                .await
            {
                for undo in made {
                    if let Err(u) = self
                        .ports
                        .workspaces
                        .set_agent_grant(ws, d.agent, undo, None)
                        .await
                    {
                        tracing::warn!(
                            component = COMPONENT,
                            code = u.code.as_str(),
                            "ordin_host.access_not_removed"
                        );
                    }
                }
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.access_not_given"
                );
                self.send_turn(
                    ws,
                    crate::host::Turn::fixed(format!(
                        "{name} could not be given access to {names} ({}), so nothing started. Tell the person in one sentence and that they can ask again.",
                        e.code
                    )),
                )
                .await?;
                return Ok(None);
            }
            made.push(*place);
        }
        tracing::info!(
            component = COMPONENT,
            workspace_id = ws.to_string().as_str(),
            agent_instance_id = d.agent.to_string().as_str(),
            folders = made.len(),
            "ordin_host.access_allowed"
        );
        Ok(Some(self.proceed(d.clone(), true).await))
    }

    /// Step 8.3 from (a0) (`from_pack`) or from (c).
    pub(crate) async fn proceed(&self, d: Delegation, from_pack: bool) -> Delegated8 {
        let ws = d.workspace_id;
        if from_pack {
            let choices = match self.ports.catalogue.pending_choices(d.agent).await {
                Ok(c) => c,
                Err(e) => return Delegated8::Failed(own(&e)),
            };
            if let Some(first) = choices.into_iter().next() {
                return self.ask_pack(d, first).await;
            }
            // (a) reuse; never for Try again, which asks for the work to be done again (OH-TA-1).
            // OH-TF-1 step 7: never for a task from its form either; the person asked for it now.
            if d.args.artefacts.is_empty()
                && d.args.code_in.is_none()
                && d.args.run_again_of.is_none()
                && d.args.form.is_none()
            {
                let query = ReuseQuery {
                    request: d.args.brief.clone(),
                    agent: AgentRef::AgentInstance {
                        agent_instance_id: d.agent,
                    },
                    workspace_id: ws,
                    input_versions: vec![],
                };
                match self.ports.usage.find_reusable(query).await {
                    Ok(Some(r)) => {
                        let _ = self.show_message(ws, &r.text).await;
                        let when = local(&r.answered_at);
                        let kept = Delegation {
                            stage: Stage::Reuse,
                            ..d
                        };
                        if let Err(e) = self
                            .ask(
                                ws,
                                &format!(
                                    "This is the answer from {when}; nothing it used has changed."
                                ),
                                vec!["Ask again".into(), "That's fine".into()],
                                Some(Kept::Delegation(Box::new(kept))),
                            )
                            .await
                        {
                            return Delegated8::Failed(fail(
                                &e.code(),
                                "That did not work; try again.",
                                e.retryable(),
                            ));
                        }
                        if let Err(e) = self.ports.usage.record_reuse(r.id, None).await {
                            tracing::warn!(
                                component = COMPONENT,
                                code = e.code.as_str(),
                                "ordin_host.reuse_not_recorded"
                            );
                        }
                        return Delegated8::Waiting(out(
                            "Answered from an earlier run; the person can ask again.",
                            Some(json!({ "reused_answer_id": r.id })),
                        ));
                    }
                    Ok(None) => {}
                    Err(e) => tracing::warn!(
                        component = COMPONENT,
                        code = e.code.as_str(),
                        "ordin_host.reuse_unavailable"
                    ),
                }
            }
            // (b) estimate.
            if d.args.artefacts.len() > 20 {
                let req = EstimateRequest {
                    work_key: format!(
                        "task:{}:{}",
                        d.agent,
                        d.args.command.as_deref().unwrap_or("free_text")
                    ),
                    shape: WorkShape::Task {
                        documents: u32::try_from(d.args.artefacts.len()).unwrap_or(u32::MAX),
                        total_bytes: 0,
                    },
                };
                match self.ports.usage.estimate(req).await {
                    Ok(e) if e.ask_first => {
                        let question = format!(
                            "Estimated use: {}, about {} to {} model turns. Start?",
                            crate::store::word(&e.band),
                            e.min_turns,
                            e.max_turns
                        );
                        let kept = Delegation {
                            stage: Stage::Estimate,
                            ..d
                        };
                        if let Err(e) = self
                            .ask(
                                ws,
                                &question,
                                vec!["Start".into(), "Not now".into()],
                                Some(Kept::Delegation(Box::new(kept))),
                            )
                            .await
                        {
                            return Delegated8::Failed(fail(
                                &e.code(),
                                "That did not work; try again.",
                                e.retryable(),
                            ));
                        }
                        return Delegated8::Waiting(out(
                            "Waiting for the person to start it.",
                            None,
                        ));
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(
                        component = COMPONENT,
                        code = e.code.as_str(),
                        "ordin_host.estimate_unavailable"
                    ),
                }
            }
        }
        self.start_task(d).await
    }

    async fn ask_pack(&self, d: Delegation, choice: PendingChoice) -> Delegated8 {
        let ws = d.workspace_id;
        let name = d.name.clone();
        let class = choice.class_label.clone();
        let count = choice.choices.len();
        if choice.at_least == 1 && (1..=PACK_FORM_OPTIONS_MAX).contains(&count) {
            let labels: Vec<String> = choice.choices.iter().map(|c| c.label.clone()).collect();
            let agent = d.agent;
            let kept = Some(Kept::Delegation(Box::new(Delegation {
                stage: Stage::Pack(choice),
                ..d
            })));
            // OH-PACK-1: one candidate is confirmed, 2–4 are a question, 5–20 a form with a list.
            // spec: ordin-desktop/components/OrdinHost#OH-PACK-1
            let asked = match labels.as_slice() {
                [only] => {
                    self.ask(
                        ws,
                        &format!("{name} needs a {class} to work with. It can use {only}."),
                        vec![format!("Use {only}"), PACK_NOT_NOW.to_owned()],
                        kept,
                    )
                    .await
                }
                _ if count <= 4 => {
                    self.ask(
                        ws,
                        &format!("{name} needs a {class} to work with. Which one?"),
                        labels,
                        kept,
                    )
                    .await
                }
                _ => {
                    self.ask_pack_form(ws, &name, &class, agent, labels, kept)
                        .await
                }
            };
            if let Err(e) = asked {
                return Delegated8::Failed(fail(
                    &e.code(),
                    "That did not work; try again.",
                    e.retryable(),
                ));
            }
            return Delegated8::Waiting(out(
                format!("Waiting for the person to choose a {class} for {name}."),
                None,
            ));
        }
        let _ = self
            .show_message(
                ws,
                &format!("{name} needs you to choose its {class} with Change on its agent page."),
            )
            .await;
        Delegated8::Waiting(out(
            format!("{name} needs a pack choice on its agent page first."),
            None,
        ))
    }

    /// OH-PACK-1: a pack question with 5–20 candidates is a form with one list, waiting.
    async fn ask_pack_form(
        &self,
        ws: WorkspaceId,
        name: &str,
        class: &str,
        agent: AgentInstanceId,
        labels: Vec<String>,
        kept: Option<Kept>,
    ) -> Result<ordin_types::BlockId, crate::OhError> {
        let block = Block::Form(FormBlock {
            title: format!("{name} needs a {class} to work with"),
            verb: PACK_FORM_VERB.to_owned(),
            target: ordin_types::wire::run::RunTarget::AgentInstance {
                agent_instance_id: agent,
            },
            command: None,
            fields: vec![FormField {
                name: PACK_FORM_FIELD.to_owned(),
                label: format!("Which {class}?"),
                required: true,
                input: FormInput::Choice {
                    options: labels,
                    value: None,
                },
                filled_by_ordin: false,
            }],
            task: None,
        });
        let id = self
            .show_block(ws, MessageAuthor::Ordin, block, true)
            .await?;
        if let Some(kept) = kept {
            lock(&self.state).kept.insert(id, (ws, kept));
        }
        Ok(id)
    }

    /// Steps 8.3 (c) and (d).
    pub(crate) async fn start_task(&self, mut d: Delegation) -> Delegated8 {
        let ws = d.workspace_id;
        let task_id = TaskId::new();
        let mut coding_task_id = None;
        // OH-TF-1 step 7 (c): a software-engineering task from its form runs in its own worktree.
        // spec: ordin-desktop/components/OrdinHost#OH-TF-1
        if let Some(form) = d.args.form.clone() {
            if form.software_engineering && d.args.code_in.is_none() {
                match self.task_repository(&d, &form).await {
                    Ok(Some(repository)) => d.args.code_in = Some(repository),
                    Ok(None) => {
                        return Delegated8::Waiting(out(
                            format!(
                                "Waiting for the person to choose the repository {} works in.",
                                d.name
                            ),
                            None,
                        ))
                    }
                    Err(failed) => return failed,
                }
            }
        }
        if let Some(code_in) = &d.args.code_in {
            let repos = match self.ports.workspaces.linked_repositories(ws).await {
                Ok(r) => r,
                Err(e) => return Delegated8::Failed(dep_fail(&e)),
            };
            let Some(repo) = repos.into_iter().find(|r| &r.repository == code_in) else {
                return Delegated8::Failed(fail(
                    "OH_INPUT_INVALID",
                    "That repository is not linked to this workspace.",
                    false,
                ));
            };
            let project = match self.ports.projects.project_for_place(repo.place_id).await {
                Ok(Some((id, true))) => id,
                Ok(_) => {
                    return Delegated8::Failed(fail(
                        "OH_INPUT_INVALID",
                        "That repository is not open as a project here.",
                        false,
                    ))
                }
                Err(e) => return Delegated8::Failed(own(&e)),
            };
            let task_name = d
                .args
                .command
                .clone()
                .unwrap_or_else(|| d.args.brief.chars().take(100).collect());
            match self
                .ports
                .projects
                .start_coding_task(ordin_projects::StartCodingTask {
                    project_id: project,
                    origin: RunOrigin::Task { task_id },
                    agent_instance_id: d.agent,
                    request: d.args.brief.clone(),
                    task_name,
                    attempt_group_id: None,
                })
                .await
            {
                Ok(id) => coding_task_id = Some(id),
                Err(e) => return Delegated8::Failed(own(&e)),
            }
        }
        // OrdinHost.md Internal Logic step 8.3 (d): the one `TaskArgument` is named `arguments`, which
        // the Supervisor renders as the line `arguments: <text>`.
        let (command, mut free_text, mut arguments) = match &d.args.command {
            Some(c) => (
                Some(c.clone()),
                None,
                d.args
                    .arguments
                    .clone()
                    .map(|v| {
                        vec![TaskArgument {
                            name: "arguments".into(),
                            value: v,
                        }]
                    })
                    .unwrap_or_default(),
            ),
            None => (None, Some(d.args.brief.clone()), vec![]),
        };
        // OH-TF-1 step 7, at the start and only then: the folders granted and the documents recorded;
        // the task's arguments are one per input (step 6).
        let mut attached = d.args.artefacts.clone();
        let mut granted = Vec::new();
        if let Some(form) = &d.args.form {
            arguments = form.arguments.clone();
            match self.grant_and_record(ws, d.agent, form).await {
                Ok((made, artefacts)) => {
                    granted = made;
                    attached.extend(artefacts);
                }
                Err(e) => return Delegated8::Failed(dep_fail(&e)),
            }
        } else {
            // OH-ATT-2: the files the person attached reach the agent as its run's attachments.
            attached.extend(self.attached_files_as_artefacts(ws, d.agent).await);
        }
        let kind = if coding_task_id.is_some() {
            WorkKind::Code
        } else {
            WorkKind::Documents
        };
        // OH-TW-1: a coding task gets no results folder; its files reach the team through the
        // repository itself.
        if coding_task_id.is_none() {
            self.name_results_folder(ws, d.agent, &mut arguments, &mut free_text)
                .await;
        }
        let task = Task {
            id: task_id,
            workspace_id: ws,
            target: TaskTarget::AgentInstance {
                agent_instance_id: d.agent,
            },
            command,
            free_text,
            arguments,
            attached_artefacts: attached,
            run_again_of: d.args.run_again_of,
            requested_at: Timestamp::now(),
        };
        // OH-TA-1: Try again is the person's own press, so the run is started by the person; so is
        // a task from its form (OH-TF-1 step 7 (d)).
        // spec: ordin-desktop/components/OrdinHost#OH-TA-1
        let by_person =
            d.args.run_again_of.is_some() || d.args.form.as_ref().is_some_and(|f| f.by_person);
        let run_id = match self
            .ports
            .supervisor
            .submit_task(task, kind, by_person, coding_task_id)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                self.remove_grants(ws, d.agent, &granted).await;
                return Delegated8::Failed(own(&e));
            }
        };
        if let Some(form) = &d.args.form {
            // OH-TF-1 step 8: the form Ordin opened reads "Form sent"; nothing goes to Ordin.
            if let Some(block_id) = form.block_id {
                if let Err(e) = self.task_form_sent(ws, block_id).await {
                    tracing::warn!(
                        component = COMPONENT,
                        code = e.code().as_str(),
                        "ordin_host.task_form_not_marked"
                    );
                }
            }
            self.log_task_started(ws, d.agent, run_id, form);
        }
        let eligible =
            d.args.artefacts.is_empty() && d.args.code_in.is_none() && d.args.form.is_none();
        self.follow_task(ws, run_id, d.agent, d.args.brief.clone(), eligible);
        Delegated8::Started {
            run_id,
            name: d.name,
        }
    }

    /// OH-TW-1: in a team workspace whose results folder the agent may read and change, the task
    /// names that folder: a command task as its last argument `results_folder`, a free-text task as
    /// a sentence after its text and one blank line. Where it cannot be added (the inputs already
    /// name `results_folder`, 50 arguments, or a text that would pass 8,000 characters) nothing is
    /// added and `ordin_host.results_folder_not_named` is logged. OH-TW-2: a failure to read the
    /// folder, the grants or the place leaves it out and is logged; the task starts as before.
    // spec: ordin-desktop/components/OrdinHost#OH-TW-1
    // spec: ordin-desktop/components/OrdinHost#OH-TW-2
    pub(crate) async fn name_results_folder(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        arguments: &mut Vec<TaskArgument>,
        free_text: &mut Option<String>,
    ) {
        let sentence = match self.results_folder_sentence(ws, agent).await {
            Ok(Some(sentence)) => sentence,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    workspace_id = ws.to_string().as_str(),
                    code = e.code.as_str(),
                    "ordin_host.results_folder_unread"
                );
                return;
            }
        };
        let named = match free_text {
            Some(text) => {
                let longer = format!("{text}\n\n{sentence}");
                let fits = longer.chars().count() <= TASK_FREE_TEXT_MAX_CHARS;
                if fits {
                    *text = longer;
                }
                fits
            }
            None => {
                let fits = arguments.len() < TASK_ARGUMENTS_MAX
                    && !arguments.iter().any(|a| a.name == RESULTS_FOLDER_ARGUMENT);
                if fits {
                    arguments.push(TaskArgument {
                        name: RESULTS_FOLDER_ARGUMENT.to_owned(),
                        value: sentence,
                    });
                }
                fits
            }
        };
        if !named {
            tracing::info!(
                component = COMPONENT,
                workspace_id = ws.to_string().as_str(),
                "ordin_host.results_folder_not_named"
            );
        }
    }

    /// OH-TW-1: "Save the files this task makes in <path>." when the workspace has a results folder
    /// and `agent` holds read and change on it; None otherwise (the agent is told nothing).
    async fn results_folder_sentence(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<Option<String>, crate::ports::DepError> {
        use ordin_types::domain::places::{FolderAccess, Grantee};
        let Some(place_id) = self.ports.workspaces.results_folder(ws).await? else {
            return Ok(None);
        };
        let grants = self.ports.workspaces.grants(ws).await?;
        let held = grants.iter().any(|g| {
            g.place_id == place_id
                && g.access == FolderAccess::ReadAndChange
                && matches!(g.grantee, Grantee::AgentInstance { agent_instance_id } if agent_instance_id == agent)
        });
        if !held {
            return Ok(None);
        }
        let path = self.ports.files.place(place_id).await?.path;
        Ok(Some(format!("Save the files this task makes in {path}.")))
    }

    /// OH-RC-1: when a delegated task ends, one card in the conversation says so. A completed task shows
    /// what it produced (its final message and its files) as a `produced` result, whose screen is the
    /// run's; any other end is one fixed sentence naming the agent.
    // spec: ordin-desktop/components/OrdinHost#OH-RC-1
    pub(crate) async fn show_task_end(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        agent: AgentInstanceId,
        end: &ordin_runtime_adapter::RunEnd,
    ) {
        let name = match self.directory_hit(ws, &agent.to_string()).await {
            Ok(Some(a)) => a.display_name,
            _ => "The agent".to_owned(),
        };
        let shown = if end.reason == RunEndReason::Completed {
            let summary = end
                .final_message
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(|m| cut_chars(m, PRODUCED_SUMMARY_MAX_CHARS))
                .unwrap_or_else(|| format!("{name} finished."));
            // OH-RC-2: Ordin's next turn carries what the task said.
            self.note_finished_task(ws, &name, &summary);
            let block = Block::Result(ordin_service_api::blocks::ResultBlock::Produced {
                run_id,
                summary,
                items: end
                    .artefacts
                    .iter()
                    .map(|a| ordin_types::wire::approval::ContentRef::Artefact { artefact_id: *a })
                    .collect(),
            });
            self.show_block(ws, MessageAuthor::Ordin, block, false)
                .await
        } else if end.reason == RunEndReason::Cancelled {
            self.show_message(ws, &format!("{name} was stopped.")).await
        } else {
            let failed = self.task_failure(run_id).await;
            let text = match (end.reason, failed.never_ran, &failed.reason) {
                (RunEndReason::StoppedAtLimit, ..) => {
                    format!("{name} stopped at its limit before it finished.")
                }
                (_, true, Some(reason)) => format!("{name} couldn't start: {reason}."),
                (_, true, None) => format!("{name} couldn't start."),
                (_, false, Some(reason)) => {
                    format!("{name} stopped before it finished: {reason}.")
                }
                (_, false, None) => format!("{name} stopped before it finished."),
            };
            let shown = self.show_message(ws, &text).await;
            self.tell_ordin_task_ended(ws, run_id, agent, &name, &failed)
                .await;
            shown
        };
        if let Err(e) = shown {
            tracing::warn!(
                component = COMPONENT,
                code = e.code().as_str(),
                "ordin_host.task_end_not_shown"
            );
        }
    }

    /// OH-DLG-2.1: whether a run that did not complete ever reached `running`, and its end reason, from
    /// `supervisor.run_task` (X-1). When the call fails or the run is unknown, the run is taken to have
    /// started and to have no reason, which keeps OH-RC-1's sentence.
    // spec: ordin-desktop/components/OrdinHost#OH-DLG-2
    async fn task_failure(&self, run_id: RunId) -> TaskFailure {
        let run = match self.ports.supervisor.run_task(run_id).await {
            Ok(Some((run, _, _))) => Some(run),
            _ => None,
        };
        TaskFailure {
            never_ran: run.as_ref().is_some_and(|r| r.started_at.is_none()),
            reason: run
                .and_then(|r| r.end_reason)
                .map(|r| r.trim().trim_end_matches('.').to_owned())
                .filter(|r| !r.is_empty()),
        }
    }

    /// OH-DLG-2.2: Ordin takes one fixed turn for a task that did not complete and was not stopped by
    /// the person, so it never goes on saying the agent is working. Found in the field: after a refused
    /// start Ordin, asked "who are you?", said it was still waiting on the agent. The run's code is read
    /// as SessionSupervisor.md SA-SUP-END-1 says and left out when there is none or the read fails. The
    /// turn is queued like a message while Ordin's turn is running (`send_turn`).
    // spec: ordin-desktop/components/OrdinHost#OH-DLG-2
    async fn tell_ordin_task_ended(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        agent: AgentInstanceId,
        name: &str,
        failed: &TaskFailure,
    ) {
        let how = if failed.never_ran {
            "couldn't start"
        } else {
            "stopped before it finished"
        };
        let reason = failed
            .reason
            .as_deref()
            .map(|r| format!(": {r}"))
            .unwrap_or_default();
        let code = match self.ports.journal.run_code(run_id).await {
            Ok(Some(code)) => format!(" (code {code})"),
            _ => String::new(),
        };
        let text = format!(
            "The task for {name} ({agent}) ended: {how}{reason}{code}. It is not running. Tell the person in one sentence what happened and what you will do next."
        );
        if let Err(e) = self.send_turn(ws, crate::host::Turn::fixed(text)).await {
            tracing::warn!(
                component = COMPONENT,
                code = e.code().as_str(),
                "ordin_host.task_end_not_shown"
            );
        }
    }

    /// Indexing the answer (ADR-062) and counting what the run wrote (OH-AM-3.3).
    fn follow_task(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        agent: AgentInstanceId,
        brief: String,
        eligible: bool,
    ) {
        let Ok(mut events) = self.ports.supervisor.subscribe(run_id) else {
            return;
        };
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut approvals = 0u32;
            loop {
                let event = match events.recv().await {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                };
                let Some(host) = me.upgrade() else { return };
                match event {
                    RunEvent::Paused {
                        reason: PauseReason::Approval { .. },
                    } => approvals += 1,
                    RunEvent::Ended { end } => {
                        if eligible && end.reason == RunEndReason::Completed {
                            if let Some(text) = end.final_message.clone() {
                                let actions = host
                                    .ports
                                    .journal
                                    .external_actions_for_run(run_id)
                                    .await
                                    .unwrap_or(0);
                                let a = AnswerToIndex {
                                    query: ReuseQuery {
                                        request: brief.clone(),
                                        agent: AgentRef::AgentInstance {
                                            agent_instance_id: agent,
                                        },
                                        workspace_id: ws,
                                        input_versions: vec![],
                                    },
                                    source_run_id: run_id,
                                    answered_at: Timestamp::now(),
                                    text,
                                    external_actions: actions,
                                    files_changed: u32::try_from(end.artefacts.len())
                                        .unwrap_or(u32::MAX),
                                    approvals,
                                };
                                match host.ports.usage.index_answer(a).await {
                                    Ok(_) => {}
                                    Err(e) if e.code == "USAGE_NOT_ANSWER_ONLY" => {
                                        tracing::debug!(
                                            component = COMPONENT,
                                            code = e.code.as_str(),
                                            "ordin_host.not_indexed"
                                        );
                                    }
                                    Err(e) => tracing::warn!(
                                        component = COMPONENT,
                                        code = e.code.as_str(),
                                        "ordin_host.not_indexed"
                                    ),
                                }
                            }
                        }
                        host.show_task_end(ws, run_id, agent, &end).await;
                        host.count_run(ws, run_id).await;
                        host.consolidate(ws, run_id).await;
                        return;
                    }
                    _ => {}
                }
            }
        });
    }

    /// An answer to a question kept with a delegation (step 8.3, through step 38).
    pub(crate) async fn delegation_answered(
        &self,
        ws: WorkspaceId,
        d: Delegation,
        text: &str,
    ) -> Result<(), crate::OhError> {
        let next = match &d.stage {
            // OH-PACK-1: "Not now" starts nothing; "Use <label>" is the one candidate.
            Stage::Pack(_) if text == PACK_NOT_NOW => None,
            Stage::Pack(choice) => {
                let pack = choice
                    .choices
                    .iter()
                    .find(|c| c.label == text || format!("Use {}", c.label) == text)
                    .map(|c| c.pack.clone())
                    .unwrap_or_else(|| text.to_owned());
                let input = ChangeChoiceInput {
                    agent_instance_id: d.agent,
                    choice: PackChoice {
                        rule_index: choice.rule_index,
                        packs: vec![pack],
                    },
                };
                match self.ports.catalogue.record_answer(input).await {
                    Ok(()) => Some(self.proceed(d.clone(), true).await),
                    Err(e) => {
                        self.send_turn(
                            ws,
                            crate::host::Turn::fixed(format!(
                                "The choice could not be saved ({}).",
                                e.code
                            )),
                        )
                        .await?;
                        None
                    }
                }
            }
            Stage::Reuse if text == "Ask again" => Some(self.start_task(d.clone()).await),
            Stage::Estimate if text == "Start" => Some(self.start_task(d.clone()).await),
            Stage::Reuse | Stage::Estimate => None,
            // OH-TF-1 step 7 (c): the start continues from (c) with the repository chosen.
            Stage::Repository => {
                let mut d = d.clone();
                d.args.code_in = Some(text.to_owned());
                Some(self.start_task(d).await)
            }
            Stage::Folders(places) => self.folders_answered(&d, places, text).await?,
        };
        match next {
            Some(Delegated8::Started { name, .. }) => {
                self.send_turn(
                    ws,
                    crate::host::Turn::fixed(format!("Started: {name} is working on it.")),
                )
                .await?;
            }
            Some(Delegated8::Failed(f)) => {
                self.send_turn(
                    ws,
                    crate::host::Turn::fixed(format!("The task could not start ({}).", f.code)),
                )
                .await?;
            }
            Some(Delegated8::Waiting(_)) | None => {}
        }
        Ok(())
    }

    // ---------------------------------------------------------------- step 14 (show)

    async fn show_tool(&self, ws: WorkspaceId, a: Value) -> Result<ToolOutput, ToolFailure> {
        let raw = a.get("block").cloned().unwrap_or(Value::Null);
        let ctx = BlockCheckContext {
            workspace_id: ws,
            run_id: None,
            now: Timestamp::now(),
        };
        let block = match self.ports.blocks.check_block(&raw, &ctx).await {
            Ok(b) => b,
            Err(rejections) => {
                let first = rejections
                    .first()
                    .map(|r| r.reason.clone())
                    .unwrap_or_default();
                let listed: Vec<&ordin_service_api::blocks::BlockRejection> =
                    rejections.iter().take(20).collect();
                let list = serde_json::to_string(&listed).unwrap_or_default();
                return Err(fail("BLOCK_REFUSED", &format!("{first}\n{list}"), false));
            }
        };
        match &block {
            Block::CrewPlan(p) => {
                let mut c = self.pool.acquire().await.map_err(|_| {
                    fail(
                        "OH_DATASTORE_UNAVAILABLE",
                        "Something went wrong; try again.",
                        true,
                    )
                })?;
                let ok = crate::store::charter(&mut c, p.crew_charter_id)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|r| r.charter.workspace_id == ws);
                if !ok {
                    return Err(fail(
                        "OH_CHARTER_UNKNOWN",
                        "No crew plan with that id is in this workspace.",
                        false,
                    ));
                }
            }
            Block::Form(f) => {
                let key = match &f.target {
                    ordin_types::wire::run::RunTarget::AgentInstance { agent_instance_id } => {
                        agent_instance_id.to_string()
                    }
                    ordin_types::wire::run::RunTarget::RemoteAgent { remote_agent_id } => {
                        remote_agent_id.to_string()
                    }
                };
                if self.directory_hit(ws, &key).await?.is_none() {
                    return Err(fail("OH_AGENT_UNKNOWN", "No agent with that key is in this workspace; use a key from the agent directory.", false));
                }
            }
            Block::AgentProposal(p) => {
                self.ports
                    .catalogue
                    .check_proposal(p.agent_repository_id, &p.agent)
                    .await
                    .map_err(|e| own(&e))?;
            }
            _ => {}
        }
        let waiting = matches!(
            block,
            Block::Question(_)
                | Block::Form(_)
                | Block::Result(ordin_service_api::blocks::ResultBlock::Offer { .. })
        );
        let is_form = matches!(block, Block::Form(_));
        let id = self
            .show_block(ws, MessageAuthor::Ordin, block, waiting)
            .await
            .map_err(|e| fail(&e.code(), "That did not work; try again.", e.retryable()))?;
        if is_form {
            self.mark_key_points_form(ws, id);
        }
        Ok(out("Shown.", Some(json!({ "block_id": id }))))
    }

    // ---------------------------------------------------------------- step 15

    // spec: ordin-desktop/components/OrdinHost#OH-TR-2
    async fn remember_tool(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        r: RememberArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        let who = MemoryCaller::Ordin {
            workspace_id: ws,
            run_id,
        };
        // Step 15.3: the Memory Service's refusals go back to the model so it can correct the call.
        let refused = |e: crate::ports::DepError| {
            if REMEMBER_REFUSALS.contains(&e.code.as_str()) {
                fail(&e.code, &e.message, false)
            } else {
                dep_fail(&e)
            }
        };
        // Step 15.2: a summary, written by the Memory Service as a passage (MEM-TR-6).
        if let Some(summary) = r.summary {
            let source = if r.source == "conversation" {
                None
            } else {
                Some(FactSource {
                    version: self.source_version(ws, &r.source).await,
                    origin_ref: r.source,
                })
            };
            let entities = self
                .consolidating_thing(ws, &r.made_from)
                .map(|name| ThingInput {
                    name,
                    other_names: vec![],
                })
                .into_iter()
                .collect();
            let input = RememberInput {
                statement: summary,
                about: None,
                fact_class: None,
                source,
                entities,
                links: vec![],
                made_from: r.made_from,
            };
            let receipt = self
                .ports
                .memory
                .remember(who, input)
                .await
                .map_err(refused)?;
            return Ok(out("Kept.", Some(json!({ "fact_id": receipt.id }))));
        }
        // Step 15.1: the facts, with their source.
        let source = if r.source == "conversation" {
            FactSourceInput::Conversation
        } else {
            FactSourceInput::Document(FactSource {
                version: self.source_version(ws, &r.source).await,
                origin_ref: r.source,
            })
        };
        let receipt = self
            .ports
            .memory
            .remember_facts(
                who,
                RememberFactsInput {
                    source: Some(source),
                    fact_class: None,
                    facts: r.facts,
                },
            )
            .await
            .map_err(refused)?;
        let ids: Vec<String> = receipt.items.iter().map(|i| i.id.to_string()).collect();
        if let Some(conv) = lock(&self.state).convs.get_mut(&ws) {
            conv.own_facts.extend(
                receipt
                    .items
                    .iter()
                    .filter(|i| i.state == ordin_memory_service::FactState::Kept)
                    .map(|i| i.id),
            );
        }
        Ok(out(
            format!("Kept {}.", receipt.items.len()),
            Some(json!({ "fact_ids": ids })),
        ))
    }

    /// OH-AM step 3: the thing of the consolidation turn whose quoted facts `made_from` names.
    fn consolidating_thing(&self, ws: WorkspaceId, made_from: &[MemoryFactId]) -> Option<String> {
        let state = lock(&self.state);
        let conv = state.convs.get(&ws)?;
        made_from
            .iter()
            .find_map(|id| conv.consolidating.get(id).cloned())
    }

    /// OH-TR-2: the version of the document `source` names — `"<origin_version_read>/<local_version>"`
    /// of the first working-copy item whose `origin_path` equals it, cut at 256 characters; `"as read"`
    /// when it is not a working copy or the working copy cannot be read. One read per call, so the
    /// 16 facts of one source read the links once.
    // spec: ordin-desktop/components/OrdinHost#OH-TR-2
    pub(crate) async fn source_version(&self, ws: WorkspaceId, source: &str) -> String {
        match self.ports.workspaces.source_documents(ws).await {
            Ok(items) => items
                .into_iter()
                .find(|d| d.origin_path == source)
                .map(|d| {
                    cut_chars(
                        &format!("{}/{}", d.origin_version_read, d.local_version),
                        256,
                    )
                })
                .unwrap_or_else(|| AS_READ.to_owned()),
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.source_version_unknown"
                );
                AS_READ.to_owned()
            }
        }
    }

    async fn recall_tool(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        r: RecallArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        let input = RecallInput {
            query: r.query,
            searches: r.searches.map(|s| {
                s.into_iter()
                    .map(|x| RecallSearch {
                        kind: match x.kind {
                            SearchKindArg::Lex => RecallSearchKind::Lex,
                            SearchKindArg::Vec => RecallSearchKind::Vec,
                        },
                        query: x.query,
                    })
                    .collect()
            }),
            folders: r.folders,
            result_cap: r.result_cap.unwrap_or(10),
            wider: if r.wider == Some(true) {
                WiderMode::Always
            } else {
                WiderMode::Auto
            },
        };
        let result = self
            .ports
            .memory
            .recall(
                MemoryCaller::Ordin {
                    workspace_id: ws,
                    run_id,
                },
                input,
            )
            .await
            .map_err(|e| match e.code.as_str() {
                "MEM_SEARCHES_INVALID" | "MEM_FOLDER_UNKNOWN" => fail(&e.code, &e.message, false),
                _ => dep_fail(&e),
            })?;
        if let Some(conv) = lock(&self.state).convs.get_mut(&ws) {
            conv.recalls.push(result.recall_id);
        }
        Ok(out(recall_answer(&result), None))
    }

    async fn write_page_tool(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        w: WritePageArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        // OH-KP-1: a page Ordin wrote is changed by giving its id; without one a new page is written.
        let page_id = match w.page_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => match ordin_types::KnowledgePageId::parse(raw) {
                Ok(id) => Some(id),
                Err(_) => {
                    return Err(fail(
                        "MCP_ARGUMENTS_INVALID",
                        "Wrong arguments for write_page at /page_id: it is not a page id; give the page_id an earlier write_page answered, or leave it out for a new page.",
                        false,
                    ))
                }
            },
            None => None,
        };
        let req = ordin_knowledge::WritePage {
            workspace_id: ws,
            page_id,
            title: w.title,
            body_markdown: w.body,
            sources: w
                .sources
                .into_iter()
                .map(|s| ordin_types::domain::places::KnowledgeSource {
                    origin: s.name,
                    location: s.location,
                    version_read: s.version_read,
                })
                .collect(),
            run_id: Some(run_id),
            tags: None,
            aliases: None,
            page_type: Some(w.page_type.unwrap_or_else(|| "Explanation".to_owned())),
            description: Some(w.description),
        };
        let id = self
            .ports
            .knowledge
            .write_page(req)
            .await
            .map_err(|e| match e.code.as_str() {
                "KNOW_DESCRIPTION_INVALID" | "KNOW_TYPE_INVALID" => {
                    fail(&e.code, &e.message, false)
                }
                // OH-KP-1: the two refusals Ordin can put right say what to change (found on the 0.1.1 Mac
                // build: three tries of the same call, each answered "That did not work; try again.").
                "KNOW_SOURCE_INVALID" | "KNOW_TITLE_DUPLICATE" => {
                    tracing::warn!(
                        component = COMPONENT,
                        code = e.code.as_str(),
                        retryable = e.retryable,
                        "ordin_host.dependency_failed"
                    );
                    fail(&e.code, write_page_help(&e.code), false)
                }
                _ => dep_fail(&e),
            })?;
        Ok(out("Written.", Some(json!({ "page_id": id }))))
    }
}

/// OH-KP-1: what Ordin is told when `write_page` is refused for a source or a title it can change.
fn write_page_help(code: &str) -> &'static str {
    if code == "KNOW_SOURCE_INVALID" {
        "A source was refused. A source's name is place, sharepoint or confluence; for a place, its location is place:<place id>:<path>, copied from the \"as a page's source:\" part of the Attached line. If you do not have that, write the page with no sources."
    } else {
        "A page with this title already exists. To change that page, call write_page again with its page_id (the one write_page answered); to write another page, use a different title."
    }
}

/// OH-ACC-2: the folder question's two answers, and the longest question it shows.
pub(crate) const FOLDER_ALLOW: &str = "Allow";
pub(crate) const FOLDER_NOT_NOW: &str = "Not now";
const FOLDER_QUESTION_MAX_CHARS: usize = 500;

/// OH-ACC-2: link names as one phrase: "A", "A and B", "A, B and C".
fn folder_names(places: &[(ordin_types::PlaceId, String)]) -> String {
    let names: Vec<&str> = places.iter().map(|(_, n)| n.as_str()).collect();
    match names.as_slice() {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// OH-PACK-1: the most candidates a pack form lists (2C wire: a choice field holds 2–20 options).
const PACK_FORM_OPTIONS_MAX: usize = 20;
/// OH-PACK-1: the answer that starts nothing.
const PACK_NOT_NOW: &str = "Not now";
/// OH-PACK-1: the pack form's button and its one field.
const PACK_FORM_VERB: &str = "Choose";
pub(crate) const PACK_FORM_FIELD: &str = "pack";

fn expected(e: &Expect) -> &'static str {
    match e {
        Expect::Round => "delegate, finish or escalate",
        Expect::Check => "decide(kind: check)",
        Expect::Condition { .. } => "decide(kind: condition)",
        Expect::Question => "decide(kind: question)",
    }
}

fn local(t: &Timestamp) -> String {
    let local: chrono::DateTime<chrono::Local> = t.0.into();
    local.format("%Y-%m-%d %H:%M").to_string()
}

fn date(t: &Timestamp) -> String {
    let local: chrono::DateTime<chrono::Local> = t.0.into();
    local.format("%Y-%m-%d").to_string()
}

/// Step 15 (`recall`, as the ADR-110, ADR-111 and ADR-112/113 amendments state).
// spec: ordin-desktop/components/OrdinHost#SA-OH-KA-02
pub(crate) fn recall_answer(r: &ordin_memory_service::RecallResult) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut described: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (i, f) in r.facts.iter().enumerate() {
        let mut line = format!("{}. {} (kept {})", i + 1, f.statement, date(&f.written_at));
        let own = f.folder.as_deref().is_none_or(|folder| folder == "own");
        if let (false, Some(folder)) = (own, &f.folder) {
            line.push_str(&format!(" — from {folder}"));
        }
        if f.set_by_company {
            line.push_str(" — set by your company");
        }
        if let Some(from) = &f.adopted_from {
            line.push_str(&format!(" — adopted from {from}"));
        }
        lines.push(line);
        if let (false, Some(folder), Some(description)) = (own, &f.folder, &f.folder_description) {
            if described.insert(folder.clone()) {
                lines.push(format!("   {folder}: {description}"));
            }
        }
    }
    for l in r.links.iter().take(20) {
        lines.push(format!(
            "Link: {} {} {} (since {})",
            l.from.name,
            l.kind,
            l.to.name,
            date(&l.since)
        ));
    }
    for w in r.wider.iter().take(20) {
        let mut line = format!(
            "{} — from {}: {}/{}",
            w.statement, w.agent, w.repository, w.path
        );
        if let Some(title) = &w.page_title {
            line.push_str(&format!(" — {title}"));
        }
        if let Some(section) = &w.section {
            line.push_str(&format!(" § {section}"));
        }
        line.push_str(&format!(" (as of {})", date(&w.as_of)));
        match crate::store::word(&w.page_status).as_str() {
            "draft" => line.push_str(" — draft"),
            "deprecated" => line.push_str(" — deprecated"),
            _ => {}
        }
        lines.push(line);
    }
    // Step 15.3: "Nothing kept matches." only when the local facts and the wider facts are empty;
    // the lines of step 15.2 still follow it.
    if (r.facts.is_empty() || r.abstained) && r.wider.is_empty() {
        lines = vec!["Nothing kept matches.".to_owned()];
    }
    let late: Vec<&str> = r
        .wider_status
        .unanswered
        .iter()
        .filter(|u| matches!(u.reason, UnansweredReason::NotInTime))
        .map(|u| u.agent.as_str())
        .collect();
    if !late.is_empty() {
        lines.push(format!("Did not answer in time: {}.", late.join(", ")));
    }
    let unreachable: Vec<&str> = r
        .wider_status
        .unanswered
        .iter()
        .filter(|u| !matches!(u.reason, UnansweredReason::NotInTime))
        .map(|u| u.agent.as_str())
        .collect();
    if !unreachable.is_empty() {
        lines.push(format!("Could not be reached: {}.", unreachable.join(", ")));
    }
    if r.wider_status.asked {
        tracing::info!(
            component = COMPONENT,
            asked = true,
            wider_facts = r.wider.len(),
            unanswered = r.wider_status.unanswered.len(),
            "ordin_host.recall_wider"
        );
    }
    lines.join("\n")
}

/// A fixed message block of the given text (no model turn).
#[allow(dead_code)]
pub(crate) fn message_block(text: &str) -> Block {
    Block::Message(MessageBlock {
        text: text.to_owned(),
        attachments: vec![],
        memory_used: vec![],
    })
}

/// Keeps `Arc` in scope for the provider's construction.
#[allow(dead_code)]
type Shared = Arc<OrdinHost>;
