//! The `memory` MCP server (*Internal Logic* §12 with the ADR-110, ADR-111 and ADR-112 changes).

use std::sync::Arc;

use async_trait::async_trait;
use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolCaller,
    ToolFailure, ToolKey, ToolOutput, ToolProvider, ToolSpec,
};
use ordin_types::RunId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::ProgramKind;
use crate::error::{sentence, MemoryError};
use crate::model::{
    FactSource, FactSourceInput, MemoryCaller, PredicateInput, RecallInput, RecallSearch,
    RememberFactsInput, TripleInput, WiderMode,
};
use crate::service::Inner;

const SERVER: &str = "memory";
const RECALL: &str = "recall";
const REMEMBER: &str = "remember";

/// One line of at most 120 characters on the desktop and personal server (`ToolSpec::description`).
// The amended `recall` and `remember` descriptions run to about 300 characters, and the
// MCP Front refuses more than 120 outside the agent profile. On the desktop and personal server the
// first sentence stands alone, and the amendments' sentences go in the schema's root `description`,
// where the model still reads them, as McpFront.md SA-RECON-10 does for its `report` tool.
const RECALL_LINE: &str = "Recall facts remembered in this workspace that match a question.";
// spec: ordin-desktop/components/MemoryService#MEM-TR-4
const REMEMBER_LINE: &str = "Remember 1–16 facts, one claim each: a subject, a word for how it relates to the value, and a thing or a value.";
const RECALL_MORE: &str = "Name folders to search only those; \"own\" is this team's own knowledge. \
    Use searches for several short keyword (lex) or meaning (vec) queries; the first counts double.";
const WIDER_MORE: &str = "Set wider to look beyond this workspace in the team's knowledge agents \
    even when its own memory answers.";
const REMEMBER_MORE: &str = "A fact is one claim: subject, predicate, object. Use a word from the \
    predicate list, or propose one with its label, meaning, object kind and whether a subject has one \
    value at a time. A link word names how the subject relates to the value (\"target year\", \
    \"budget\", \"supplies\"), never a kind of note (\"key fact\", \"known issue\", \"note\", \
    \"issue\", \"fact\", \"summary\", \"detail\", \"point\", \"info\"). A text value is at most 120 \
    characters; values that disagree are separate facts. Set changed when the earlier value stopped being true, ended when this fact \
    stopped being true, and valid_from to the date it became true when you know it. Without source, \
    the source is this run.";
const REMEMBER_FORM: &str = "remember(source?: {origin_ref, version}, fact_class?, facts: 1–16 × \
    {subject: {name, other_names?, topic?}, predicate: word | {propose: {label, meaning, object, \
    single_valued}}, object: {thing}|{text}|{number, unit?}|{money, currency}|{date}|{yes_no}, \
    changed?, ended?, valid_from?})";
// spec: ordin-desktop/components/MemoryService#MEM-TR-13
/// The schema's description of a text value (MEM-TR-13).
pub const TEXT_VALUE: &str =
    "One claim, at most 120 characters, no claims joined by \";\"; several claims are several facts.";
/// The schema's description of a proposed word's label (MEM-TR-13).
pub const LINK_WORD: &str = "How the subject relates to the value, such as \"target year\", \
    \"budget\" or \"supplies\"; never a kind of note such as \"key fact\" or \"known issue\".";
/// The schema's description of a subject's topic (MEM-TOP-3).
pub const TOPIC_HINT: &str = "A new thing's short topic in plain words, such as Strategy, People, \
    Funding or Suppliers; reuse one already given where it fits.";
const REMEMBER_PLAIN: &str = "Can remember facts for later sessions, each with where it came from.";

/// The arguments of `recall`, as the tool takes them.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecallArguments {
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    searches: Option<Vec<RecallSearch>>,
    #[serde(default)]
    folders: Option<Vec<String>>,
    #[serde(default)]
    result_cap: Option<u8>,
    #[serde(default)]
    wider: Option<bool>,
}

/// The arguments of `remember` (WIRE-TR-1): `statement`, `about`, `entities` and `links` are gone,
/// so each is an unknown field here.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TR-1
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RememberArguments {
    #[serde(default)]
    fact_class: Option<String>,
    #[serde(default)]
    source: Option<FactSource>,
    facts: Vec<TripleInput>,
}

fn problem(at: &str, reason: &str) -> ArgumentProblem {
    tracing::debug!(target: "ordin::memory", code = "MEM_INVALID_ARGUMENTS", "memory.arguments_refused");
    ArgumentProblem {
        at: at.to_owned(),
        reason: reason.to_owned(),
    }
}

fn check_recall(arguments: &Value) -> Result<RecallArguments, ArgumentProblem> {
    let parsed: RecallArguments = serde_json::from_value(arguments.clone())
        .map_err(|_| problem("", "the arguments do not fit the form of recall."))?;
    let either = "query: give either query or searches, not both.";
    match (&parsed.query, &parsed.searches) {
        (Some(query), None) => {
            if !(1..=4096).contains(&query.chars().count()) {
                return Err(problem("/query", "query: 1 to 4096 characters."));
            }
        }
        (None, Some(searches)) => {
            if !(1..=4).contains(&searches.len()) {
                return Err(problem("/searches", "searches: 1 to 4 items."));
            }
            for (at, search) in searches.iter().enumerate() {
                if !(1..=500).contains(&search.query.chars().count()) {
                    return Err(problem(
                        &format!("/searches/{at}/query"),
                        "query: 1 to 500 characters.",
                    ));
                }
            }
        }
        _ => return Err(problem("", either)),
    }
    if let Some(folders) = &parsed.folders {
        if !(1..=21).contains(&folders.len()) {
            return Err(problem("/folders", "folders: 1 to 21 names."));
        }
        for (at, folder) in folders.iter().enumerate() {
            if !(1..=300).contains(&folder.chars().count()) {
                return Err(problem(
                    &format!("/folders/{at}"),
                    "folders: each 1 to 300 characters.",
                ));
            }
        }
    }
    if parsed
        .result_cap
        .is_some_and(|cap| !(1..=20).contains(&cap))
    {
        return Err(problem("/result_cap", "result_cap: 1 to 20."));
    }
    Ok(parsed)
}

fn check_remember(arguments: &Value) -> Result<RememberArguments, ArgumentProblem> {
    let parsed: RememberArguments = serde_json::from_value(arguments.clone())
        .map_err(|_| problem("", "the arguments do not fit the form of remember."))?;
    let within = |text: &str, max: usize| (1..=max).contains(&text.chars().count());
    if !(1..=16).contains(&parsed.facts.len()) {
        return Err(problem(
            "/facts",
            "facts: 1 to 16, each a subject, a predicate and an object.",
        ));
    }
    if let Some(class) = parsed.fact_class.as_deref() {
        if class == crate::remember::PASSAGE_CLASS {
            return Err(problem("/fact_class", "fact_class: that name is reserved."));
        }
        if !within(class, 128) {
            return Err(problem("/fact_class", "fact_class: 1 to 128 characters."));
        }
    }
    if let Some(source) = &parsed.source {
        if !within(&source.origin_ref, 2048) {
            return Err(problem(
                "/source/origin_ref",
                "origin_ref: 1 to 2048 characters.",
            ));
        }
        if !within(&source.version, 256) {
            return Err(problem("/source/version", "version: 1 to 256 characters."));
        }
    }
    for (at, fact) in parsed.facts.iter().enumerate() {
        if !within(fact.subject.name.trim(), 200) || fact.subject.other_names.len() > 8 {
            return Err(problem(
                &format!("/facts/{at}/subject"),
                "subject: name 1 to 200 characters, up to 8 other names.",
            ));
        }
        // spec: ordin-desktop/components/MemoryService#MEM-TOP-2
        if fact
            .subject
            .topic
            .as_deref()
            .is_some_and(|t| !crate::topics::topic_ok(t))
        {
            return Err(problem(
                &format!("/facts/{at}/subject/topic"),
                "topic: 1 to 40 characters, a short plain label such as Funding.",
            ));
        }
        if let PredicateInput::Term(key) = &fact.predicate {
            if !within(key, 64) {
                return Err(problem(
                    &format!("/facts/{at}/predicate"),
                    "predicate: a word of 1 to 64 characters, or propose one.",
                ));
            }
        }
        if fact.confidence_permille.is_some_and(|p| p > 1000) {
            return Err(problem(
                &format!("/facts/{at}/confidence_permille"),
                "confidence_permille: 0 to 1000.",
            ));
        }
    }
    Ok(parsed)
}

fn recall_schema(agent_profile: bool) -> Value {
    let mut properties = json!({
        "query": { "type": "string", "minLength": 1, "maxLength": 4096 },
        "searches": {
            "type": "array", "minItems": 1, "maxItems": 4,
            "items": {
                "type": "object", "additionalProperties": false,
                "properties": {
                    "type": { "type": "string", "enum": ["lex", "vec"] },
                    "query": { "type": "string", "minLength": 1, "maxLength": 500 },
                },
                "required": ["type", "query"],
            },
        },
        "folders": {
            "type": "array", "minItems": 1, "maxItems": 21,
            "items": { "type": "string", "minLength": 1, "maxLength": 300 },
        },
        "result_cap": { "type": "integer", "minimum": 1, "maximum": 20 },
    });
    let mut description = format!("Exactly one of query and searches. {RECALL_MORE}");
    if !agent_profile {
        properties["wider"] = json!({ "type": "boolean" });
        description = format!("{description} {WIDER_MORE}");
    }
    // Exactly one of `query` and `searches` is `check_arguments`' rule: McpFront.md SA-RECON-10
    // forbids a `oneOf` at the root, so the spec's `oneOf` over the two `required` sets is not
    // published.
    json!({
        "type": "object",
        "description": description,
        "additionalProperties": false,
        "properties": properties,
    })
}

/// The amended `remember` schema (MemoryService.md amendment ADR-118, *Tool `remember`*).
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TR-1
fn remember_schema() -> Value {
    let thing = json!({
        "type": "object", "additionalProperties": false,
        "properties": {
            "name": { "type": "string", "minLength": 1, "maxLength": 200 },
            "other_names": {
                "type": "array", "maxItems": 8,
                "items": { "type": "string", "minLength": 1, "maxLength": 200 },
            },
        },
        "required": ["name"],
    });
    // MEM-TOP-3: the subject may give a topic for a thing that has none yet; an object's thing never does.
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-3
    let mut subject = thing.clone();
    subject["properties"]["topic"] = json!({
        "type": "string", "minLength": 1, "maxLength": crate::topics::TOPIC_MAX_CHARS,
        "description": TOPIC_HINT,
    });
    let one = |key: &str, value: Value| {
        json!({
            "type": "object", "additionalProperties": false,
            "properties": { key: value }, "required": [key],
        })
    };
    let object = json!({
        "oneOf": [
            one("thing", thing.clone()),
            one("text", json!({
                "type": "string", "minLength": 1, "maxLength": 120,
                "description": TEXT_VALUE,
            })),
            one("number", json!({
                "type": "object", "additionalProperties": false,
                "properties": {
                    "value": { "type": "string" },
                    "unit": { "type": "string", "minLength": 1, "maxLength": 32 },
                },
                "required": ["value"],
            })),
            one("money", json!({
                "type": "object", "additionalProperties": false,
                "properties": {
                    "amount": { "type": "string" },
                    "currency": { "type": "string", "pattern": "^[A-Z]{3}$" },
                },
                "required": ["amount", "currency"],
            })),
            one("date", json!({ "type": "string" })),
            one("yes_no", json!({ "type": "boolean" })),
        ],
    });
    let predicate = json!({
        "description": "A word of this place's vocabulary, by its key, or a proposed new word.",
        "oneOf": [
            { "type": "string", "minLength": 1, "maxLength": 64 },
            one("propose", json!({
                "type": "object", "additionalProperties": false,
                "properties": {
                    "label": {
                        "type": "string", "minLength": 1, "maxLength": 64,
                        "description": LINK_WORD,
                    },
                    "meaning": { "type": "string", "minLength": 1, "maxLength": 300 },
                    "object": {
                        "type": "string",
                        "enum": ["thing", "text", "number", "money", "date", "yes_no"],
                    },
                    "single_valued": { "type": "boolean" },
                },
                "required": ["label", "meaning", "object", "single_valued"],
            })),
        ],
    });
    json!({
        "type": "object",
        "description": REMEMBER_MORE,
        "additionalProperties": false,
        "properties": {
            "fact_class": { "type": "string", "minLength": 1, "maxLength": 128 },
            "source": {
                "type": "object", "additionalProperties": false,
                "properties": {
                    "origin_ref": { "type": "string", "minLength": 1, "maxLength": 2048 },
                    "version": { "type": "string", "minLength": 1, "maxLength": 256 },
                },
                "required": ["origin_ref", "version"],
            },
            "facts": {
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
                        "valid_from": {
                            "type": "string",
                            "pattern": "^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$",
                        },
                    },
                    "required": ["subject", "predicate", "object"],
                },
            },
        },
        "required": ["facts"],
    })
}

fn failure(code: &str, message: String, retryable: bool) -> ToolFailure {
    ToolFailure {
        code: code.to_owned(),
        message,
        retryable,
    }
}

/// `MemoryError` → `ToolFailure`, with the Error Table's sentence; a refused fact names
/// `facts[i].<field>`.
fn tool_failure(error: &MemoryError) -> ToolFailure {
    failure(error.code(), error.tool_text(), error.retryable())
}

/// The Error Table's tool text for one refused fact (MemoryService.md amendment ADR-118).
pub(crate) fn fact_sentence(code: &str, index: usize, field: &str) -> String {
    let at = format!("facts[{index}].{field}");
    match (code, field) {
        ("MEM_TRIPLE_INVALID", "valid_from") => format!(
            "{at}: the date it became true, as 2026, 2026-04 or 2026-04-15, not later than today."
        ),
        ("MEM_TRIPLE_INVALID", "object") => format!(
            "{at}: the value does not read as its kind; text 1 to 120 characters, a number like 240, \
             a money amount like 12.50 with a three-letter currency such as GBP, a date as 2026, \
             2026-04 or 2026-04-15, or a thing's name of 1 to 200 characters."
        ),
        // MEM-TR-13: the refusal says what to do instead.
        // spec: ordin-desktop/components/MemoryService#MEM-TR-13
        ("MEM_TRIPLE_INVALID", "object.text") => format!(
            "{at}: a text value is one claim of at most 120 characters, with no claims joined by \";\". \
             Split it into separate facts, one claim each, with a link word for how the thing relates \
             to each value; values that disagree are separate facts."
        ),
        ("MEM_TERM_PROPOSAL_INVALID", "predicate.propose.label") => format!(
            "{at}: a link word names how the thing relates to the value, such as \"target year\", \
             \"budget\" or \"supplies\", never a kind of note such as \"key fact\" or \"known issue\". \
             Split the note into separate facts, each with such a word."
        ),
        // MEM-TR-13 (d): a word used by key whose label names a kind of note.
        // spec: ordin-desktop/components/MemoryService#MEM-TR-13
        ("MEM_TERM_PROPOSAL_INVALID", "predicate") => format!(
            "{at}: this word names a kind of note, not how the thing relates to the value. Use a word \
             for how the thing relates to the value, such as \"target year\", \"budget\" or \
             \"supplies\", or propose one, and split the note into separate facts."
        ),
        ("MEM_TRIPLE_INVALID", "subject") => {
            format!("{at}: name 1 to 200 characters, up to 8 other names.")
        }
        _ => format!("{at}: {}", sentence(code, false)),
    }
}

// spec: ordin-desktop/components/MemoryService#SA-RECON-02
/// §12: provider `memory-service`, server `memory`.
pub(crate) struct MemoryToolProvider {
    inner: Arc<Inner>,
}

impl MemoryToolProvider {
    pub(crate) fn new(inner: Arc<Inner>) -> MemoryToolProvider {
        MemoryToolProvider { inner }
    }

    fn agent_profile(&self) -> bool {
        matches!(
            self.inner.program,
            ProgramKind::AgentSingle | ProgramKind::AgentScaled
        )
    }

    fn server_name() -> Option<ServerName> {
        ServerName::new(SERVER).ok()
    }

    /// `MemoryCaller` from `ctx` only.
    fn caller(ctx: &ToolCallContext) -> Result<MemoryCaller, ToolFailure> {
        let run_id: RunId = ctx.run_id;
        let workspace_id = ctx.workspace_id;
        match ctx.caller {
            ToolCaller::CrewMember { crew_run_id } => Ok(MemoryCaller::CrewMember {
                workspace_id,
                run_id,
                crew_run_id,
                agent_instance_id: ctx.agent_instance_id,
            }),
            ToolCaller::Agent => Ok(MemoryCaller::Agent {
                workspace_id,
                run_id,
                agent_instance_id: ctx.agent_instance_id,
            }),
            ToolCaller::HostedAgent => match ctx.scope_id {
                Some(scope_id) => Ok(MemoryCaller::HostedAgent { scope_id, run_id }),
                None => {
                    let error = MemoryError::Refused {
                        code: "MEM_SCOPE_MISSING",
                    };
                    Err(tool_failure(&error))
                }
            },
            ToolCaller::Ordin => Ok(MemoryCaller::Ordin {
                workspace_id,
                run_id,
            }),
        }
    }

    async fn folder_message(&self, ctx: &ToolCallContext, folders: &[String]) -> String {
        let named = match self.inner.folders() {
            Some(known) => known.collections_for(ctx.workspace_id, folders).await.err(),
            None => None,
        };
        let name = named
            .or_else(|| folders.first().cloned())
            .unwrap_or_default();
        format!("No folder named \"{name}\" here; use a folder name recall returned, or \"own\".")
    }

    async fn recall(
        &self,
        ctx: &ToolCallContext,
        arguments: RecallArguments,
    ) -> Result<ToolOutput, ToolFailure> {
        let who = Self::caller(ctx)?;
        let folders = arguments.folders.clone();
        let input = RecallInput {
            query: arguments.query,
            searches: arguments.searches,
            folders: arguments.folders,
            result_cap: arguments.result_cap.unwrap_or(10),
            wider: match arguments.wider {
                Some(true) => WiderMode::Always,
                _ => WiderMode::Auto,
            },
        };
        match self.inner.recall_as(who, input, &ctx.correlation_id).await {
            Ok(result) => output(&result),
            Err(error) if error.code() == "MEM_FOLDER_UNKNOWN" => {
                let message = self
                    .folder_message(ctx, folders.as_deref().unwrap_or(&[]))
                    .await;
                Err(failure(error.code(), message, false))
            }
            Err(error) => Err(tool_failure(&error)),
        }
    }

    /// §12 (ADR-118): `remember` builds `RememberFactsInput` and calls `remember_facts`.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-4
    async fn remember(
        &self,
        ctx: &ToolCallContext,
        arguments: RememberArguments,
    ) -> Result<ToolOutput, ToolFailure> {
        let who = Self::caller(ctx)?;
        let input = RememberFactsInput {
            source: arguments.source.map(FactSourceInput::Document),
            fact_class: arguments.fact_class,
            facts: arguments.facts,
        };
        // A hosted agent's `MemoryCaller` names no agent; its session does.
        let agent = match who {
            MemoryCaller::Ordin { .. } => None,
            _ => Some(ctx.agent_instance_id),
        };
        match self
            .inner
            .remember_facts_as(who, agent, input, &ctx.correlation_id)
            .await
        {
            Ok(receipt) => output(&receipt),
            Err(error) if error.code() == "MEM_CLASS_REQUIRED" => {
                let list = self
                    .inner
                    .rules_of(ctx.agent_instance_id)
                    .map(|rules| rules.retains.join(", "))
                    .unwrap_or_default();
                Err(failure(
                    error.code(),
                    format!("Name the fact_class; this agent keeps only: {list}."),
                    false,
                ))
            }
            Err(error) => Err(tool_failure(&error)),
        }
    }
}

fn output<T: Serialize>(result: &T) -> Result<ToolOutput, ToolFailure> {
    let structured =
        serde_json::to_value(result).map_err(|_| tool_failure(&MemoryError::internal()))?;
    Ok(ToolOutput {
        text: structured.to_string(),
        structured: Some(structured),
    })
}

#[async_trait]
impl ToolProvider for MemoryToolProvider {
    fn provider_id(&self) -> &'static str {
        "memory-service"
    }

    fn servers(&self) -> Vec<ServerSpec> {
        let Some(name) = Self::server_name() else {
            return Vec::new();
        };
        let agent_profile = self.agent_profile();
        let line = |first: &str, more: &str| {
            if agent_profile {
                format!("{first} {more}")
            } else {
                first.to_owned()
            }
        };
        vec![ServerSpec {
            name,
            tools: vec![
                ToolSpec {
                    name: RECALL.to_owned(),
                    description: line(RECALL_LINE, RECALL_MORE),
                    input_schema: recall_schema(agent_profile),
                    argument_form: "recall(query?: 1–4096 chars | searches?: 1–4 × {type: \
                                    lex|vec, query}, folders?: names, result_cap?: 1–20)"
                        .to_owned(),
                    sandbox_callable: true,
                    plain: "Can recall facts it remembered earlier.".to_owned(),
                },
                ToolSpec {
                    name: REMEMBER.to_owned(),
                    description: line(REMEMBER_LINE, REMEMBER_MORE),
                    input_schema: remember_schema(),
                    argument_form: REMEMBER_FORM.to_owned(),
                    sandbox_callable: true,
                    plain: REMEMBER_PLAIN.to_owned(),
                },
            ],
        }]
    }

    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        let offered = match grant.caller {
            ToolCaller::CrewMember { .. } => true,
            ToolCaller::HostedAgent => grant.scope_id.is_some(),
            ToolCaller::Agent => self.inner.rules_of(grant.agent_instance_id).is_some(),
            ToolCaller::Ordin => false,
        };
        let Some(server) = Self::server_name().filter(|_| offered) else {
            return Ok(Vec::new());
        };
        Ok([RECALL, REMEMBER]
            .iter()
            .map(|tool| ToolKey {
                server: server.clone(),
                tool: (*tool).to_owned(),
            })
            .collect())
    }

    fn check_arguments(&self, key: &ToolKey, arguments: &Value) -> Result<(), ArgumentProblem> {
        match key.tool.as_str() {
            RECALL => check_recall(arguments).map(|_| ()),
            REMEMBER => check_remember(arguments).map(|_| ()),
            // `call` answers an unknown tool with its own code.
            _ => Ok(()),
        }
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        let run = async {
            match key.tool.as_str() {
                RECALL => {
                    let parsed = check_recall(&arguments)
                        .map_err(|p| failure("MEM_INVALID_ARGUMENTS", p.reason, false))?;
                    self.recall(&ctx, parsed).await
                }
                REMEMBER => {
                    let parsed = check_remember(&arguments)
                        .map_err(|p| failure("MEM_INVALID_ARGUMENTS", p.reason, false))?;
                    self.remember(&ctx, parsed).await
                }
                _ => Err(tool_failure(&MemoryError::Refused {
                    code: "MEM_TOOL_UNKNOWN",
                })),
            }
        };
        let late = || {
            tool_failure(&MemoryError::Unavailable {
                code: "MEM_MODEL_UNAVAILABLE",
                retryable: true,
            })
        };
        let deadline = tokio::time::Instant::from_std(ctx.deadline);
        tokio::select! {
            result = run => result,
            _ = ctx.cancel.cancelled() => Err(late()),
            _ = tokio::time::sleep_until(deadline) => Err(late()),
        }
    }

    /// Nothing is held per session.
    async fn session_closed(&self, _run_id: RunId) {}
}
