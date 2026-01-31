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
    FactSource, LinkInput, MemoryCaller, RecallInput, RecallSearch, RememberInput, ThingInput,
    WiderMode,
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
const REMEMBER_LINE: &str = "Remember one short fact for later sessions in this workspace.";
const RECALL_MORE: &str = "Name folders to search only those; \"own\" is this team's own knowledge. \
    Use searches for several short keyword (lex) or meaning (vec) queries; the first counts double.";
const WIDER_MORE: &str = "Set wider to look beyond this workspace in the team's knowledge agents \
    even when its own memory answers.";
const REMEMBER_MORE: &str = "Name the things the fact is about in entities, and state links \
    between them (\"Borealis\" supplies \"packaging\"); set ended for a link that is no longer true.";

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

/// The arguments of `remember`: `made_from` is not one of them, so it is an unknown field here.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RememberArguments {
    statement: String,
    #[serde(default)]
    about: Option<String>,
    #[serde(default)]
    fact_class: Option<String>,
    #[serde(default)]
    source: Option<FactSource>,
    #[serde(default)]
    entities: Vec<ThingInput>,
    #[serde(default)]
    links: Vec<LinkInput>,
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
    if !within(parsed.statement.trim(), 500) {
        return Err(problem("/statement", "statement: 1 to 500 characters."));
    }
    if parsed.about.as_deref().is_some_and(|a| !within(a, 200)) {
        return Err(problem("/about", "about: 1 to 200 characters."));
    }
    if parsed
        .fact_class
        .as_deref()
        .is_some_and(|c| !within(c, 128))
    {
        return Err(problem("/fact_class", "fact_class: 1 to 128 characters."));
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
    if parsed.entities.len() > 16 {
        return Err(problem("/entities", "entities: up to 16."));
    }
    if parsed.links.len() > 16 {
        return Err(problem("/links", "links: up to 16."));
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

fn remember_schema() -> Value {
    json!({
        "type": "object",
        "description": REMEMBER_MORE,
        "additionalProperties": false,
        "properties": {
            "statement": { "type": "string", "minLength": 1, "maxLength": 500 },
            "about": { "type": "string", "minLength": 1, "maxLength": 200 },
            "fact_class": { "type": "string", "minLength": 1, "maxLength": 128 },
            "source": {
                "type": "object", "additionalProperties": false,
                "properties": {
                    "origin_ref": { "type": "string", "minLength": 1, "maxLength": 2048 },
                    "version": { "type": "string", "minLength": 1, "maxLength": 256 },
                },
                "required": ["origin_ref", "version"],
            },
            "entities": {
                "type": "array", "maxItems": 16,
                "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {
                        "name": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "other_names": {
                            "type": "array", "maxItems": 8,
                            "items": { "type": "string", "minLength": 1, "maxLength": 200 },
                        },
                    },
                    "required": ["name"],
                },
            },
            "links": {
                "type": "array", "maxItems": 16,
                "items": {
                    "type": "object", "additionalProperties": false,
                    "properties": {
                        "from": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "kind": { "type": "string", "minLength": 1, "maxLength": 64 },
                        "to": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "ended": { "type": "boolean" },
                        "confidence_permille": { "type": "integer", "minimum": 0, "maximum": 1000 },
                    },
                    "required": ["from", "kind", "to"],
                },
            },
        },
        "required": ["statement"],
    })
}

fn failure(code: &str, message: String, retryable: bool) -> ToolFailure {
    ToolFailure {
        code: code.to_owned(),
        message,
        retryable,
    }
}

/// `MemoryError` → `ToolFailure`, with the Error Table's sentence.
fn tool_failure(error: &MemoryError) -> ToolFailure {
    failure(
        error.code(),
        sentence(error.code(), error.retryable()).to_owned(),
        error.retryable(),
    )
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

    async fn remember(
        &self,
        ctx: &ToolCallContext,
        arguments: RememberArguments,
    ) -> Result<ToolOutput, ToolFailure> {
        let who = Self::caller(ctx)?;
        let input = RememberInput {
            statement: arguments.statement,
            about: arguments.about,
            fact_class: arguments.fact_class,
            source: arguments.source,
            entities: arguments.entities,
            links: arguments.links,
            made_from: Vec::new(),
        };
        // A hosted agent's `MemoryCaller` names no agent; its session does.
        let agent = match who {
            MemoryCaller::Ordin { .. } => None,
            _ => Some(ctx.agent_instance_id),
        };
        match self
            .inner
            .remember_as(who, agent, input, &ctx.correlation_id)
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
                    argument_form: "remember(statement: 1–500 chars, about?, fact_class?, \
                                    source?: {origin_ref, version}, entities?: [{name, \
                                    other_names?}], links?: [{from, kind, to, ended?, \
                                    confidence_permille?}])"
                        .to_owned(),
                    sandbox_callable: true,
                    plain: "Can remember short facts for later sessions.".to_owned(),
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
