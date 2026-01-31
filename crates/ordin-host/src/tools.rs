//! The tools of server `ordin` (OrdinHost.md *ToolProvider* and section C), offered to Ordin's
//! current run only. Every call is already decided by the Rules Engine in the MCP Front.

use std::sync::{Arc, Weak};
use std::time::Instant;

use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolCaller,
    ToolFailure, ToolKey, ToolOutput, ToolProvider, ToolSpec,
};
use ordin_memory_service::{
    LinkInput, MemoryCaller, RecallInput, RecallSearch, RecallSearchKind, RememberInput,
    ThingInput, WiderMode,
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
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{RunOrigin, RunState, Task, TaskArgument, TaskTarget};
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::UnansweredReason;
use ordin_types::{AgentInstanceId, RunId, TaskId, WorkspaceId};
use ordin_usage_planner::{AnswerToIndex, EstimateRequest, ReuseQuery, WorkKind, WorkShape};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::host::{cut_chars, lock, Expect, Kept, OrdinHost, COMPONENT};
use crate::ports::{DirectoryAgent, LocMessage};
use crate::types::*;

pub(crate) const SERVER: &str = "ordin";

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

fn out(text: impl Into<String>, structured: Option<Value>) -> ToolOutput {
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

fn len(at: &str, value: &str, min: usize, max: usize) -> Result<(), ArgumentProblem> {
    let n = value.chars().count();
    if n < min || n > max {
        return Err(ArgumentProblem {
            at: at.to_owned(),
            reason: format!("must be {min} to {max} characters"),
        });
    }
    Ok(())
}

fn schema<T: schemars::JsonSchema>() -> Value {
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
        ("delegate", "Give a task to one agent, or a step to a crew member.", "delegate(agent: \"<directory key>\", brief: text, command?: \"<name>\", arguments?: text, artefacts?: [ids], code_in?: \"<owner/name>\")", "Can ask your agents to do work.", schema::<DelegateArgs>()),
        ("finish", "End the crew run with a summary.", "finish(summary: text)", "Can end a crew's work.", schema::<FinishArgs>()),
        ("escalate", "Ask the person one question when the crew cannot carry on.", "escalate(question: text, recommendation?: text)", "Can ask you a question for the crew.", schema::<EscalateArgs>()),
        ("decide", "Answer the crew's request for a check, a condition or a member's question.", "decide(kind: \"check\", verdict: \"carry_on\"|\"shortfall\", reason) | decide(kind: \"condition\", path, reason) | decide(kind: \"question\", answer)", "Checks results while a crew works.", decide_schema()),
        ("create_crew", "Propose a crew: purpose, agents with roles, and limits.", "create_crew(purpose, members: [{agent, role}] 1–8, limits?: {rounds, wall_clock_minutes, members_at_once, request_budget}, inputs?: [labels])", "Can propose a crew of agents.", schema::<CreateCrewArgs>()),
        ("keep_crew", "Draft a workflow from the crew run that just finished.", "keep_crew(crew_run_id, name)", "Can save a crew's steps as a workflow.", schema::<KeepCrewArgs>()),
        ("save_workflow", "Draft a workflow for the person to save.", "save_workflow(name, purpose, inputs, agents: [{key, agent, role}], steps: [format 3 steps], limits?, crew_run_id?)", "Can draft workflows for you to save.", schema::<SaveWorkflowArgs>()),
        ("propose_workflow_change", "Propose a changed version of a workflow for the person to save.", "propose_workflow_change(workflow_id, base_version, steps, change_note, purpose?, inputs?, limits?)", "Can suggest changes to a workflow.", schema::<ProposeWorkflowChangeArgs>()),
        ("take_over", "Ask the crew to let you coordinate the rest of a workflow run from a step; the crew decides.", "take_over(crew_run_id, from_position: \"3\", reason)", "Can ask to take over a workflow that does not fit.", schema::<TakeOverArgs>()),
        ("show", "Show the person a block from the catalogue.", "show(block: {kind: \"message\"|\"question\"|\"form\"|…, …})", "Shows you messages, forms and results.", schema::<ShowArgs>()),
        ("remember", "Keep a fact about this workspace. Name its things in entities and links between them in links; ended: no longer true.", "remember(fact: text, source?: text, entities?: [{name, other_names?}], links?: [{from, kind, to, ended?, confidence_permille?}], made_from?: [fact ids])", "Remembers what matters in this workspace, and how things relate.", schema::<RememberArgs>()),
        ("recall", "Recall kept facts. folders: only those (\"own\" is this team's); searches: lex or vec queries; wider: company knowledge.", "recall(query?: text | searches?: 1–4 × {type: lex|vec, query}, folders?: names, result_cap?: 1–20, wider?: bool)", "Recalls what it remembered, and can look in your company's knowledge agents.", schema::<RecallArgs>()),
        ("write_page", "Write or update a knowledge page in this workspace, with a one-sentence description of what it explains and its type.", "write_page(title, body: markdown, sources: [{name, location, version_read}], description: text, type?: Explanation|How-to|Decision|Reference|Glossary)", "Keeps knowledge pages for the workspace.", schema::<WritePageArgs>()),
        ("find_agent", "Find agents in the library for a need.", "find_agent(need: text)", "Finds agents in your library.", schema::<FindAgentArgs>()),
        ("propose_agent", "Suggest the person adds an agent from the library.", "propose_agent(agent_repository_id, agent: \"urn:ordin:…\", reason)", "Suggests agents to add.", schema::<ProposeAgentArgs>()),
        ("propose_push", "Propose sending changed documents back where they live.", "propose_push(request: text, items: [{item: working copy item id, summary: text}])", "Can propose sending changes back, which you approve.", schema::<ProposePushArgs>()),
        ("propose_workflow_run", "Propose running a saved workflow; the person starts it from the crew plan.", "propose_workflow_run(workflow_id, request: text)", "Can suggest running one of your saved workflows.", schema::<ProposeWorkflowRunArgs>()),
        ("change_setting", "Change one of the person's settings when they ask for it in words.", "change_setting(setting: \"tone\"|\"register\"|\"length\"|\"name_to_use\"|\"interface_language\", value | locale + name)", "Can change how it talks to you and your language when you ask.", change_setting_schema()),
    ];
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
        if grant.caller != ToolCaller::Ordin
            || host.current_run(grant.workspace_id) != Some(grant.run_id)
        {
            return Ok(vec![]);
        }
        let Ok(server) = ServerName::new(SERVER) else {
            return Ok(vec![]);
        };
        let desk = host.trigger_desk().map(|d| d.proposal_schema());
        Ok(specs(desk)
            .into_iter()
            .map(|s| ToolKey {
                server: server.clone(),
                tool: s.name,
            })
            .collect())
    }

    fn check_arguments(&self, key: &ToolKey, a: &Value) -> Result<(), ArgumentProblem> {
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
            "remember" => {
                let r: RememberArgs = typed(a)?;
                len("/fact", &r.fact, 1, 500)?;
                len("/source", r.source.as_deref().unwrap_or(""), 0, 2_048)?;
                if r.entities.len() > 16 || r.links.len() > 16 {
                    return Err(ArgumentProblem {
                        at: "/entities".into(),
                        reason: "at most 16 things and 16 links".into(),
                    });
                }
                for (i, t) in r.entities.iter().enumerate() {
                    len(&format!("/entities/{i}/name"), &t.name, 1, 200)?;
                    if t.other_names.len() > 8 {
                        return Err(ArgumentProblem {
                            at: format!("/entities/{i}/other_names"),
                            reason: "at most 8".into(),
                        });
                    }
                }
                for (i, l) in r.links.iter().enumerate() {
                    len(&format!("/links/{i}/from"), &l.from, 1, 200)?;
                    len(&format!("/links/{i}/kind"), &l.kind, 1, 64)?;
                    len(&format!("/links/{i}/to"), &l.to, 1, 200)?;
                    if l.confidence_permille.is_some_and(|c| c > 1000) {
                        return Err(ArgumentProblem {
                            at: format!("/links/{i}/confidence_permille"),
                            reason: "0 to 1000".into(),
                        });
                    }
                }
                if !r.made_from.is_empty() && !(2..=64).contains(&r.made_from.len()) {
                    return Err(ArgumentProblem {
                        at: "/made_from".into(),
                        reason: "2 to 64 fact ids".into(),
                    });
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
}

#[derive(Debug, Clone)]
pub(crate) enum Stage {
    Pack(PendingChoice),
    Reuse,
    Estimate,
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
            c.name == command || c.session_name.as_deref() == Some(command) || shown == command
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
        }
        self.ports
            .catalogue
            .ensure_ready(agent.agent_instance_id)
            .await
            .map_err(|e| own(&e))?;
        if let Some(asked) = self
            .ask_for_access(ws, agent.agent_instance_id, &agent.display_name)
            .await
        {
            return Ok(asked);
        }
        let delegation = Delegation {
            workspace_id: ws,
            agent: agent.agent_instance_id,
            name: agent.display_name.clone(),
            args: Delegated {
                brief: d.brief,
                command: d.command,
                arguments: d.arguments,
                artefacts: d.artefacts,
                code_in: d.code_in,
                run_again_of: None,
            },
            stage: Stage::Reuse,
        };
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
            if d.args.artefacts.is_empty()
                && d.args.code_in.is_none()
                && d.args.run_again_of.is_none()
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
    async fn start_task(&self, d: Delegation) -> Delegated8 {
        let ws = d.workspace_id;
        let task_id = TaskId::new();
        let mut coding_task_id = None;
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
        let (command, free_text, arguments) = match &d.args.command {
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
        let kind = if coding_task_id.is_some() {
            WorkKind::Code
        } else {
            WorkKind::Documents
        };
        let task = Task {
            id: task_id,
            workspace_id: ws,
            target: TaskTarget::AgentInstance {
                agent_instance_id: d.agent,
            },
            command,
            free_text,
            arguments,
            attached_artefacts: d.args.artefacts.clone(),
            run_again_of: d.args.run_again_of,
            requested_at: Timestamp::now(),
        };
        // OH-TA-1: Try again is the person's own press, so the run is started by the person.
        // spec: ordin-desktop/components/OrdinHost#OH-TA-1
        let by_person = d.args.run_again_of.is_some();
        let run_id = match self
            .ports
            .supervisor
            .submit_task(task, kind, by_person, coding_task_id)
            .await
        {
            Ok(r) => r,
            Err(e) => return Delegated8::Failed(own(&e)),
        };
        let eligible = d.args.artefacts.is_empty() && d.args.code_in.is_none();
        self.follow_task(ws, run_id, d.agent, d.args.brief.clone(), eligible);
        Delegated8::Started {
            run_id,
            name: d.name,
        }
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

    async fn remember_tool(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        r: RememberArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        let consolidating = !r.made_from.is_empty();
        let input = RememberInput {
            statement: r.fact,
            about: None,
            fact_class: None,
            source: Self::fact_source(r.source),
            entities: r
                .entities
                .into_iter()
                .map(|t| ThingInput {
                    name: t.name,
                    other_names: t.other_names,
                })
                .collect(),
            links: r
                .links
                .into_iter()
                .map(|l| LinkInput {
                    from: l.from,
                    kind: l.kind,
                    to: l.to,
                    ended: l.ended,
                    confidence_permille: l.confidence_permille,
                })
                .collect(),
            made_from: r.made_from,
        };
        let receipt = self
            .ports
            .memory
            .remember(
                MemoryCaller::Ordin {
                    workspace_id: ws,
                    run_id,
                },
                input,
            )
            .await
            .map_err(|e| match e.code.as_str() {
                "MEM_ENTITIES_INVALID"
                | "MEM_LINKS_INVALID"
                | "MEM_MADE_FROM_NOT_ALLOWED"
                | "MEM_MADE_FROM_INVALID" => fail(&e.code, &e.message, false),
                _ => dep_fail(&e),
            })?;
        if !consolidating && receipt.state == ordin_memory_service::FactState::Kept {
            if let Some(conv) = lock(&self.state).convs.get_mut(&ws) {
                conv.own_facts.push(receipt.id);
            }
        }
        Ok(out("Kept.", Some(json!({ "fact_id": receipt.id }))))
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
        let req = ordin_knowledge::WritePage {
            workspace_id: ws,
            page_id: None,
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
                _ => dep_fail(&e),
            })?;
        Ok(out("Written.", Some(json!({ "page_id": id }))))
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
