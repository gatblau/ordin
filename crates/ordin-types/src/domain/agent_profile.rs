//! 2C part 1 § Agent profile (ordin server, ADR-097, ADR-098).

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- RunRecord ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RunRecord {
    pub id: RunRecordId,
    pub run_id: String,
    pub thread_id: String,
    pub started_by: RunStarter,
    pub scope_id: ScopeId,
    pub request: RunRequest,
    /// Gathered context; its shape is wrightrun's (LD-38).
    pub gathered_context: serde_json::Value,
    pub state: RunRecordState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_kind: Option<RunEndKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    pub stretch_summaries: Vec<String>,
    pub queued_at: Timestamp,
    /// amended January 2026, decision 80: C-28.
    pub created_at: Timestamp,
    /// The first time an instance took the run; a resumed run keeps it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunStarter {
    Caller { caller_id: String },
    ScheduleEntry { entry: String },
    Intake { intake: String },
    ChatConnection { connection: String, asker: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunRequest {
    Command {
        command: String,
        arguments: Vec<TaskArgument>,
    },
    Message {
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RunRecordState {
    Queued,
    Executing,
    Paused,
    Ended,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// `Control`: the run was ended by a control (wire state `ended-by-control`); amended January 2026, decision 80: C-28.
pub enum RunEndKind {
    Completed,
    Control,
    Failed,
    RunLimit,
}

// ---- Scope ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Scope {
    pub id: ScopeId,
    pub kind: ScopeKind,
    /// The chat system's identifier; None for the agent-wide scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_identifier: Option<String>,
    /// The chat connection the scope belongs to; None for the agent-wide scope (amended January 2026, decision 80: C-28;
    /// name *chosen here* from Retention and Erasure's `ScopeEntry.connection`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    pub state: ScopeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub erased_at: Option<Timestamp>,
    pub enabled_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Channel,
    DirectChat,
    AgentWide,
    Knowledge,
} // Knowledge: amended January 2026 (ADR-113)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ScopeState {
    Enabled,
    Disabled,
    Erased,
}

// =====================================================================================================
// The agent-configuration types (AgentConfiguration.md *Types*, placed in `domain::agent_profile`;
// SharedTypes.md Internal Logic 11). Every struct is `kebab-case` with `deny_unknown_fields` unless shown
// otherwise; a `Vec` defaults to empty and a `bool` to false unless a default function is named.
// `f64` fields are the configuration format's own exception to 2C's no-float convention (SA-RECON-03).
// They carry no validation function here; the Agent Configuration crate validates a whole configuration.
// =====================================================================================================

// ---------- the file ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AgentConfig {
    pub package: String,
    pub model: ModelSetting,
    #[serde(default)]
    pub work_namespace: Option<String>,
    pub run_limit_minutes: u32,
    #[serde(default)]
    pub writable_paths: Option<Vec<String>>, // None: whole work folder; Some([]): none (ADR-055 of wrightrun)
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub schedules: Vec<ScheduleEntry>,
    #[serde(default)]
    pub intakes: Vec<IntakeEntry>,
    #[serde(default)]
    pub chat: Vec<ChatConnection>,
    #[serde(default)]
    pub knowledge: Vec<KnowledgeSourceConfig>,
    #[serde(default)]
    pub retention: RetentionConfig,
    #[serde(default)]
    pub controls: ControlsConfig,
    #[serde(default)]
    pub settings: Vec<SettingDeclaration>,
    #[serde(default)]
    pub contract: ContractConfig,
    #[serde(default)]
    pub mappings: Vec<OperationMapping>,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    // ---- keys added by ordin (all optional; absent = wrightrun's behaviour, ADR-098 item 2) ----
    #[serde(default)]
    pub harness: Option<HarnessChoice>, // chosen here; default Claude
    #[serde(default)]
    pub traces: bool, // chosen here; ADR-098 item 6, `07:16`
    #[serde(default)]
    pub reach: Option<ReachConfig>, // ADR-097 item 3
    #[serde(default)]
    pub approvers: Option<ApproversConfig>, // ADR-097 item 4
    #[serde(default)]
    pub knowledge_agent: Option<KnowledgeAgentConfig>, // YAML key `knowledge-agent` (ADR-113)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum HarnessChoice {
    Claude,
    Copilot,
}

/// `model: anthropic:claude-sonnet-4-6`, or the mapping form. Named `ModelSetting` so as not to clash with
/// 2C wire `ModelChoice` (the run-level level and model of `StartRun`; reconciliation row 278). Deserialised by a hand-written
/// `Deserialize` that accepts a YAML string or a mapping and reports the mapping's own key errors.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(untagged))]
pub enum ModelSetting {
    Short(String),
    Policy(ModelPolicyConfig),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ModelPolicyConfig {
    pub primary: String, // MODEL_ID
    #[serde(default)]
    pub fallback: Option<String>,
    pub temperature: f64, // 0.0..=2.0; accepted, reported as not carried (C-77)
    #[serde(default)]
    pub gateway_url: Option<String>,
    pub safe_reply: String, // 1..=2000 characters
    /// Amended January 2026 (ADR-107): settings the harness's own provider mode needs, set in the harness
    /// child's environment only (ClaudeLocalAdapter.md step 8). Name -> value; at most MAX_MODEL_ENVIRONMENT.
    #[serde(default)]
    pub environment: std::collections::BTreeMap<String, EnvValue>,
}

/// One `model.environment` value: exactly one of the two keys (V-4 row `model.environment`).
/// `{literal: "eu-1"}` for a setting that is not a secret; `{secret: MODEL_TOKEN}` names a variable whose
/// value Model Policy resolves from `ORDIN_SECRETS_DIR` or the server's secret store (2D-1d).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct EnvValue {
    #[serde(default)]
    pub literal: Option<String>, // 1..=MAX_LITERAL_CHARS, no control character
    #[serde(default)]
    pub secret: Option<String>, // the variable-name rule (ENV_NAME)
}

// ---------- connections ----------
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ToolsConfig {
    #[serde(default)]
    pub mcp: Vec<McpBinding>,
    #[serde(default)]
    pub http: Vec<HttpConnection>,
    #[serde(default)]
    pub publish: Vec<PublishConnection>,
    /// wrightrun's Python plug-in contract is not kept (ADR-098 item 2); any item is refused (V-21).
    #[serde(default)]
    pub connectors: Vec<serde_json::Value>,
    #[serde(default)]
    pub approve_before: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct McpBinding {
    pub server: String, // 1..=128 characters
    pub expose: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>, // ENV_NAME each
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>, // header -> ENV_NAME
    #[serde(default)]
    pub transport: Option<McpTransport>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub serves: Vec<String>,
    #[serde(default)]
    pub external: bool, // ADR-097 item 4
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum McpTransport {
    Stdio,
    Sse,
    Http,
    StreamableHttp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HttpOperation {
    pub name: String, // TOOL_NAME
    pub method: HttpMethod,
    pub path: String,        // starts with "/"
    pub description: String, // 1..=1024
    #[serde(default)]
    pub query: Vec<String>,
    #[serde(default)]
    pub body: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HttpConnection {
    pub name: String, // SLUG
    pub base_url: String,
    #[serde(default)]
    pub openapi: Option<String>, // relative path beside the configuration
    #[serde(default)]
    pub operations: Vec<HttpOperation>,
    pub expose: Vec<String>,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default = "thirty")]
    pub timeout_seconds: u32, // 1..=300
    #[serde(default)]
    pub serves: Vec<String>,
    #[serde(default)]
    pub external: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PublishConnection {
    pub name: String,
    pub tool: String,        // TOOL_NAME
    pub description: String, // 1..=1024
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub url_env: Option<String>,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default = "message_field")]
    pub field: String, // default "message"
    #[serde(default = "thirty")]
    pub timeout_seconds: u32,
    #[serde(default)]
    pub serves: Vec<String>,
    #[serde(default)]
    pub external: bool,
}

// ---------- intakes, schedules, auth ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct IntakeEntry {
    pub name: String,
    pub command: String,
    #[serde(default = "yes")]
    pub accept_token: bool,
    #[serde(default)]
    pub secret_env: Option<String>,
    #[serde(default = "signature_header")]
    pub signature_header: String, // "X-Wrightrun-Signature"
    #[serde(default)]
    pub timestamp_header: Option<String>,
    #[serde(default = "body_64k")]
    pub max_body_bytes: u32, // 65_536; 1..=1_048_576
    #[serde(default = "ten")]
    pub max_runs_in_progress: u32, // 1..=1000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ScheduleEntry {
    pub duty: String,
    pub command: String,
    #[serde(default)]
    pub arguments: String, // at most 4096 characters
    pub cron: String, // five fields, UTC
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AuthConfig {
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
    pub role_claim: String,
    pub operator_value: String,
    pub caller_value: String,
    #[serde(default = "rs256")]
    pub algorithms: Vec<String>, // default ["RS256"]
}

// ---------- chat ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ChatTrigger {
    pub kind: TriggerKind,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub starts: Option<String>,
    #[serde(default)]
    pub delivery: Delivery, // default Asker
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TriggerKind {
    Mention,
    Command,
    ThreadMention,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Delivery {
    #[default]
    Asker,
    Thread,
}

/// Amended January 2026 (ADR-107): `chat[].context` is either the former mapping of five keys (`Fixed`) or a
/// YAML sequence of context steps (`Steps`). Deserialised by a hand-written `Deserialize` that takes a mapping
/// as `Fixed` and a sequence as `Steps` and reports each form's own key errors. `context_steps` widens `Fixed`
/// to the first three steps, so a configuration written before ADR-107 loads and behaves unchanged.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(untagged))]
pub enum ChatContext {
    Fixed(ChatContextFixed),
    Steps(Vec<ContextStep>),
}
impl Default for ChatContext {
    fn default() -> Self {
        ChatContext::Fixed(ChatContextFixed::default())
    }
}

impl ChatContext {
    /// The ordered steps, `Fixed` widened (V-18). In `ordin-types`, so the Channel Gateway reads the same rule.
    ///
    /// `Fixed` gives the transcript step, then the images step when `images`, then the channel-documents
    /// step when `channel_documents`. V-4 requires `channel_documents_max_bytes` whenever
    /// `channel_documents` is set; a value that has not passed V-4 and lacks it gives no such step.
    pub fn steps(&self) -> Vec<ContextStep> {
        match self {
            ChatContext::Steps(steps) => steps.clone(),
            ChatContext::Fixed(f) => {
                let mut steps = vec![ContextStep::Transcript {
                    max_messages: f.transcript_messages,
                }];
                if f.images {
                    steps.push(ContextStep::Images {
                        max_bytes: f.image_max_bytes,
                        max_total_bytes: IMAGES_TOTAL_BYTES,
                    });
                }
                if f.channel_documents {
                    if let Some(max_bytes) = f.channel_documents_max_bytes {
                        steps.push(ContextStep::ChannelDocuments { max_bytes });
                    }
                }
                steps
            }
        }
    }
}

/// The former keys, unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ChatContextFixed {
    #[serde(default = "two_hundred")]
    pub transcript_messages: u32, // 0..=200
    #[serde(default = "yes")]
    pub images: bool,
    #[serde(default = "ten_mib")]
    pub image_max_bytes: u32, // 1..=10_485_760
    #[serde(default)]
    pub channel_documents: bool,
    #[serde(default)]
    pub channel_documents_max_bytes: Option<u32>, // 1..=10_485_760
}

/// One step ordin runs before the model, in list order (Channel Gateway K.3.7). Each step's result is one
/// context piece, or a `LeftOut` item when it fails or finds nothing; a failing step never stops the others.
// The YAML keys of a variant's fields are kebab-case too (`max-bytes`, `period-seconds`; AgentConfiguration.md
// *Rules of the keys*), which `rename_all` alone does not give a tagged enum's fields: `rename_all_fields`
// added so the stated keys read (SPEC-GAP-12).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    deny_unknown_fields
)]
pub enum ContextStep {
    /// The thread so far, oldest messages dropped beyond `max_messages`.
    Transcript {
        #[serde(default = "two_hundred")]
        max_messages: u32,
    }, // 0..=200
    /// Images of the question and the thread, newest first, at most 10.
    Images {
        #[serde(default = "ten_mib")]
        max_bytes: u32, // each, 1..=10_485_760
        #[serde(default = "twenty_mib")]
        max_total_bytes: u32,
    }, // 1..=IMAGES_TOTAL_BYTES
    /// The documents attached to the channel.
    ChannelDocuments { max_bytes: u32 }, // 1..=10_485_760
    /// Search of one knowledge source for the asker's words (Knowledge Service `lookup`).
    KnowledgeLookup {
        source: String,
        #[serde(default = "five")]
        limit: u32, // 1..=20
        max_bytes: u32,
    }, // 1..=1_048_576; required, no default (HLD OQ-71)
    /// The newest items of one knowledge source, run only when the question holds one of `when`.
    RecentMessages {
        source: String,
        #[serde(default = "ten")]
        limit: u32, // 1..=50
        max_bytes: u32, // 1..=1_048_576
        #[serde(default)]
        when: Vec<String>,
    }, // keywords; empty = always
    /// A fixed note, added only when a `channel-documents` and a `knowledge-lookup` step both gave a piece.
    PrecedenceNote { text: String }, // 1..=1_000 characters
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StatusSignals {
    #[serde(default = "eyes")]
    pub received: String,
    /// Set on the question while its run is queued or running, removed at delivery (ADR-107).
    #[serde(default = "hourglass_flowing_sand")]
    pub working: String,
    #[serde(default = "white_check_mark")]
    pub done: String,
    #[serde(default = "warning")]
    pub failed: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ReplyAction {
    pub id: String,
    pub kind: ReplyActionKind,
    pub label: String, // 1..=75
    #[serde(default)]
    pub url_setting: Option<String>,
    #[serde(default)]
    pub notify_setting: Option<String>,
    // hand-off only (ADR-107): where the escalation goes, whom it mentions, and a switch per scope
    #[serde(default)]
    pub target_channel_setting: Option<String>, // a text setting: one channel id
    #[serde(default)]
    pub mention_users_setting: Option<String>, // a text setting: user ids
    #[serde(default)]
    pub mention_groups_setting: Option<String>, // a text setting: group ids, `here`, `channel`
    #[serde(default)]
    pub enabled_setting: Option<String>, // a flag setting declared with default true
    // link only (ADR-107): a list-backed link — one button per list entry whose keywords match the question
    #[serde(default)]
    pub list_setting: Option<String>, // a list setting
    #[serde(default)]
    pub label_field: Option<String>,
    #[serde(default)]
    pub url_field: Option<String>,
    #[serde(default)]
    pub keywords_field: Option<String>,
    #[serde(default)]
    pub style: LinkStyle, // default Url
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ReplyActionKind {
    Share,
    HandOff,
    Link,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum LinkStyle {
    #[default]
    Url,
    WorkflowLink,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct EntryCommand {
    pub command: String,
    pub setting: String,
    pub action: EntryAction,
    #[serde(default)]
    pub fill: std::collections::BTreeMap<String, FillSource>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum EntryAction {
    Add,
    Remove,
    Show,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FillSource {
    LatestQuestion,
    LatestReply,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct JobCommand {
    pub command: String,
    pub does: JobCommandKind,
    pub source: String,
    #[serde(default)]
    pub who: Who, // default Members
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum JobCommandKind {
    Ingest,
    Progress,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Who {
    #[default]
    Members,
    OperatorsOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ChatConnection {
    pub name: String,
    pub kind: ChatKind,
    pub bot_token_env: String,
    pub app_token_env: String,
    #[serde(default)]
    pub triggers: Vec<ChatTrigger>,
    #[serde(default)]
    pub context: ChatContext,
    #[serde(default)]
    pub status: StatusSignals,
    #[serde(default)]
    pub reply_actions: Vec<ReplyAction>,
    #[serde(default)]
    pub expose_tools: Vec<RuntimeTool>,
    pub failure_text: String,  // 1..=2000
    pub approval_text: String, // 1..=2000
    #[serde(default)]
    pub entry_commands: Vec<EntryCommand>,
    #[serde(default)]
    pub job_commands: Vec<JobCommand>,
    #[serde(default)]
    pub version_command: Option<String>,
    #[serde(default)]
    pub settings_command: Option<String>,
    #[serde(default)]
    pub serves: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ChatKind {
    Slack,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTool {
    ReplyAction,
    ChannelDocuments,
}

// ---------- knowledge, retention ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct QuestionType {
    pub name: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub r#match: MatchMode, // YAML key `match`; default Any
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum MatchMode {
    #[default]
    Any,
    All,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Ranking {
    #[serde(default)]
    pub thread_replies: bool,
    #[serde(default)]
    pub one_per_text: bool,
    #[serde(default)]
    pub favour_links_and_steps: Vec<QuestionType>,
}

/// One `knowledge[]` source of the configuration. Named `KnowledgeSourceConfig` so as not to clash with
/// 2C domain `KnowledgeSource { origin, location, version_read }` (reconciliation row 322).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct KnowledgeSourceConfig {
    pub name: String,
    pub kind: KnowledgeKind,
    #[serde(default)]
    pub connection: Option<String>,
    #[serde(default = "english")]
    pub language: String, // ^[a-z_]{1,32}$
    #[serde(default)]
    pub ingest_cron: Option<String>,
    #[serde(default)]
    pub ranking: Ranking,
    #[serde(default = "five")]
    pub default_results: u32, // 1..=50
    #[serde(default)]
    pub results_setting: Option<String>,
    #[serde(default)]
    pub own_messages_setting: Option<String>,
    #[serde(default)]
    pub hidden_senders_setting: Option<String>,
    #[serde(default)]
    pub expose: Vec<KnowledgeOperation>,
    #[serde(default)]
    pub max_notes_per_scope: Option<u32>, // 1..=10_000, kind Notes only
    #[serde(default)]
    pub max_note_chars: Option<u32>, // 1..=20_000, kind Notes only
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeKind {
    ChatHistory,
    Entries,
    Notes,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeOperation {
    Search,
    Recent,
    Open,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RetentionConfig {
    #[serde(default)]
    pub chat_history_days: Option<u32>, // each 1..=36_500
    #[serde(default)]
    pub entries_days: Option<u32>,
    #[serde(default)]
    pub notes_days: Option<u32>,
    #[serde(default)]
    pub chat_replies_days: Option<u32>,
    #[serde(default)]
    pub runs_days: Option<u32>,
    #[serde(default)]
    pub jobs_days: Option<u32>,
    #[serde(default)]
    pub cron: Option<String>,
}

// ---------- settings, contract, mappings ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ListField {
    pub name: String,
    pub max_chars: u32,
    #[serde(default = "yes")]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SettingDeclaration {
    pub name: String,
    pub kind: SettingKind,
    pub label: String, // 1..=75
    #[serde(default)]
    pub who: Who,
    #[serde(default)]
    pub default: Option<SettingDefault>,
    #[serde(default)]
    pub minimum: Option<f64>,
    #[serde(default)]
    pub maximum: Option<f64>,
    #[serde(default)]
    pub max_chars: Option<u32>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub lookup: Option<SettingLookup>,
    #[serde(default)]
    pub lookup_source: Option<String>,
    #[serde(default)]
    pub max_options: Option<u32>,
    #[serde(default)]
    pub always_listed: Vec<String>,
    #[serde(default)]
    pub max_items: Option<u32>,
    #[serde(default)]
    pub fields: Vec<ListField>,
    #[serde(default)]
    pub line_form: Option<String>,
    #[serde(default)]
    pub allowed_outcomes: Vec<RuleListOutcome>,
    #[serde(default)]
    pub keyword_share: Option<f64>,
    #[serde(default = "one_hundred")]
    pub rule_priority: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SettingKind {
    Flag,
    Number,
    Text,
    Selection,
    List,
    RuleList,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum SettingLookup {
    KnowledgeSenders,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RuleListOutcome {
    Reply,
    Command,
    SingleCall,
    AttachAction,
    Delivery,
}
/// A YAML boolean, number, string or list of strings; deserialised by `#[serde(untagged)]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum SettingDefault {
    Flag(bool),
    Number(f64),
    Text(String),
    Items(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Notes,
    Events,
    RunFacts,
}
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ContractConfig {
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub requires: Vec<Capability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OperationMapping {
    pub operation: String,
    pub connection: String,
    pub exposed: String,
    #[serde(default)]
    pub arguments: std::collections::BTreeMap<String, String>,
}

// ---------- ADR-097 keys (chosen here) ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ReachConfig {
    /// Chat identifiers of the channels the agent answers in; absent = every enabled channel (wrightrun).
    #[serde(default)]
    pub channels: Option<Vec<String>>,
    /// Chat identifiers of the people the agent answers in those channels; absent = every member.
    #[serde(default)]
    pub users: Option<Vec<String>>,
    /// Chat identifiers of the people whose direct messages are answered; absent or empty = none (ADR-097 item 3).
    #[serde(default)]
    pub direct_message_users: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ApproversConfig {
    #[serde(default)]
    pub users: Vec<ApproverUser>,
    /// Identity-provider group names; checked only where a token is presented (a desktop, the resume route).
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default = "groups_claim")]
    pub group_claim: String, // default "groups"
    #[serde(default)]
    pub asker_may_approve_internal: bool, // default false
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ApproverUser {
    #[serde(default)]
    pub chat_user: Option<String>, // CHAT_ID
    #[serde(default)]
    pub subject: Option<String>, // the identity provider's `sub`, 1..=256
}
// Who reads which approver field (reconciliation rows 589, 598):
// - `users[].subject`, `groups`, `group_claim`: Caller Authentication sets `Principal.approver` from a token's
//   `sub` and from the values of the claim `group_claim` names; the Approval Service matches a desktop
//   approver (ADR-097 item 9) by the same `subject` and groups.
// - `users[].chat_user`: the Channel Gateway, which sends approval cards in an approver's direct chat and
//   passes the chat user to the Approval Service as the deciding approver.
// - `asker_may_approve_internal`: the Approval Service's asker rule.

/// The control vocabulary and the package's declaration and manifest (AgentConfiguration.md, *Types*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Point {
    Start,
    BeforeTool,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    deny_unknown_fields
)]
pub enum Condition {
    PathExists {
        path: String,
    },
    PathAbsent {
        path: String,
    },
    PathContains {
        path: String,
        text: String,
    }, // text 1..=200
    RunInProgress,
    StartCount {
        of: CountOf,
        more_than: u32,
        period_seconds: u32,
    }, // 0..=100_000; 1..=2_592_000
    RepeatedMessage {
        period_seconds: u32,
    }, // 1..=86_400
    Keywords {
        words: Vec<String>,
        #[serde(default)]
        r#match: MatchMode,
    },
    KeywordShare {
        words: Vec<String>,
        share: f64,
    }, // 0.5..=1.0
    Pattern {
        patterns: Vec<String>,
        #[serde(default)]
        r#match: MatchMode,
    },
    Setting {
        name: String,
        equals: ScalarValue,
    },
    Called {
        target: CallTarget,
        name: String,
        #[serde(default = "one")]
        count: u32,
    },
    NotCalled {
        target: CallTarget,
        name: String,
        #[serde(default = "one")]
        count: u32,
    },
    Elapsed {
        at_least_seconds: u32,
    }, // 1..=86_400
    ChangedWorkFolder,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CountOf {
    Command,
    Scope,
    Asker,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CallTarget {
    Tool,
    Subagent,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ScalarValue {
    Flag(bool),
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    deny_unknown_fields
)]
pub enum DecidingOutcome {
    End { ending: String },
    Reply { text: String }, // 1..=4000
    Command { command: String },
    Tools { tools: Vec<String> },
    SingleCall { command: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    deny_unknown_fields
)]
pub enum AdditiveOutcome {
    AttachAction { action: String },
    Delivery { to: Delivery },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MessageRule {
    pub name: String,
    #[serde(default)]
    pub priority: i32, // -1000..=1000
    #[serde(default)]
    pub command: Option<String>,
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub decides: Option<DecidingOutcome>,
    #[serde(default)]
    pub adds: Vec<AdditiveOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    deny_unknown_fields
)]
pub enum StepOutcome {
    Refuse,
    RefuseEnding {
        #[serde(default = "three")]
        pause_after: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StepGate {
    pub name: String,
    pub point: Point, // BeforeTool or End only
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    pub conditions: Vec<Condition>,
    pub outcome: StepOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AddressCondition {
    #[serde(default)]
    pub schemes: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub with_subdomains: bool,
    #[serde(default)]
    pub host_is_ip: Option<bool>,
    #[serde(default)]
    pub host_is_single_label: Option<bool>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub path_prefixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ArgumentRule {
    pub name: String,
    pub tool: String,
    pub argument: String,
    pub form: ArgumentForm,
    #[serde(default)]
    pub outcome: ArgumentOutcome, // default Refuse
    #[serde(default)]
    pub present: Option<bool>,
    #[serde(default)]
    pub equals_any: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub max_length: Option<u32>,
    #[serde(default)]
    pub address: Option<AddressCondition>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ArgumentForm {
    RefuseWhen,
    RefuseUnless,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ArgumentOutcome {
    #[default]
    Refuse,
    Pause,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ControlsConfig {
    #[serde(default)]
    pub message_rules: Vec<MessageRule>,
    #[serde(default)]
    pub steps: Vec<StepGate>,
    #[serde(default)]
    pub argument_rules: Vec<ArgumentRule>, // at most 200
    /// Amended January 2026 (ADR-107): configuration only; a package declaration has no content checks,
    /// because the checker is a connection of the deployment. At most MAX_CONTENT_CHECKS.
    #[serde(default)]
    pub content_checks: Vec<ContentCheck>,
}

/// A `content-check` control (ADR-107; chosen here, SA-HOST-05). ordin sends the text to `operation` of the
/// HTTP connection `connection` and acts on the verdict; the operation is never offered to the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ContentCheck {
    pub name: String,            // SLUG; unique across all controls of the file
    pub connection: String,      // a `tools.http[].name`
    pub operation: String,       // an item of that connection's `expose`
    pub points: Vec<CheckPoint>, // `input`, `output`, or both; no duplicate
    #[serde(default)]
    pub on_block: OnBlock, // default End
    #[serde(default)]
    pub on_unreachable: OnUnreachable, // default Block
    #[serde(default)]
    pub categories: Vec<String>, // empty = any block counts; each 1..=64 characters
    #[serde(default = "five")]
    pub timeout_seconds: u32, // 1..=30
    #[serde(default = "twelve_thousand")]
    pub max_chars: u32, // 1..=100_000; the text is cut to this length
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CheckPoint {
    Input,
    Output,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum OnBlock {
    #[default]
    End,
    RetryWithoutContext,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum OnUnreachable {
    #[default]
    Block,
    Allow,
}

// ---------- the package declaration (written by the package; read by the Release Builder) ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Check {
    pub kind: CheckKind,
    #[serde(default)]
    pub condition: Option<Condition>,
    #[serde(default)]
    pub command: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CheckKind {
    File,
    Command,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HeartbeatForm {
    pub entry: String,
    pub endings: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DutyGate {
    pub duty: String,
    pub command: String,
    pub folder: String,
    #[serde(default)]
    pub work_check: Option<Check>,
    #[serde(default)]
    pub done_check: Option<Check>,
    #[serde(default)]
    pub max_cycles: Option<u32>,
    #[serde(default)]
    pub period_seconds: Option<u32>,
    #[serde(default)]
    pub heartbeat: Option<HeartbeatForm>,
    #[serde(default)]
    pub max_failed_cycles: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct JudgedLoopGate {
    pub commands: Vec<String>,
    pub reviewer: String,
    #[serde(default = "three")]
    pub pause_after: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CommandBound {
    pub command: String,
    #[serde(default)]
    pub wall_clock_minutes: Option<u32>,
    #[serde(default)]
    pub max_rounds: Option<u32>,
    #[serde(default)]
    pub max_repeated_failures: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Gates {
    #[serde(default)]
    pub duties: Vec<DutyGate>,
    #[serde(default)]
    pub judged_loops: Vec<JudgedLoopGate>,
    #[serde(default)]
    pub bounds: Vec<CommandBound>,
    #[serde(default)]
    pub steps: Vec<StepGate>,
    #[serde(default)]
    pub argument_rules: Vec<ArgumentRule>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ContractStatement {
    pub version: u32,
    #[serde(default)]
    pub requires: Vec<Capability>,
    #[serde(default)]
    pub can_use: Vec<Capability>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ArgumentType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct OperationArgument {
    pub name: String,
    pub r#type: ArgumentType,
    #[serde(default = "yes")]
    pub required: bool,
    #[serde(default)]
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct DeclaredOperation {
    pub section: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<OperationArgument>,
    #[serde(default)]
    pub required: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Declaration {
    pub format_version: u32,
    #[serde(default)]
    pub gates: Gates,
    #[serde(default)]
    pub message_rules: Vec<MessageRule>,
    #[serde(default)]
    pub settings: Vec<SettingDeclaration>,
    #[serde(default)]
    pub chat_triggers: Vec<ChatTrigger>,
    #[serde(default)]
    pub contract: Option<ContractStatement>,
    #[serde(default)]
    pub operations: Vec<DeclaredOperation>,
}

// ---------- the manifest (written by the Release Builder; JSON, snake_case keys) ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32, // MANIFEST_SCHEMA_VERSION = 3 (Release Builder)
    pub ordin_version: String,
    pub compiler: String, // "ordin <version>"; replaces wrightrun's apm_version
    pub package: PackageFacts,
    pub authority: Authority,
    pub instruction_count: u32,
    pub skills: Vec<SkillFacts>,
    pub tools: Vec<ToolSection>,
    pub mcp_servers: Vec<McpEntry>,
    pub commands: Vec<CommandFacts>,
    pub reviewer: Option<ReviewerFacts>,
    pub recurring_duties: Vec<RecurringDuty>,
    pub memory_skill: bool,
    pub declaration: Option<Declaration>,
}
// Each of these is #[serde(rename_all = "snake_case", deny_unknown_fields)]:
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PackageFacts {
    pub name: String,
    pub version: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Authority {
    pub outcome: String,
    pub modes: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SkillFacts {
    pub slug: String,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ToolSection {
    pub name: String,
    pub use_for: String,
    pub never: String,
    pub connection: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct McpEntry {
    pub name: String,
    pub registry: bool,
    pub transport: Option<McpTransport>,
    pub command: Option<String>,
    pub url: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CommandFacts {
    pub slug: String,
    pub argument_hint: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReviewerFacts {
    pub name: String,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RecurringDuty {
    pub skill_slug: String,
    pub loop_file: String,
}

// ---------- the knowledge agent section (AgentConfiguration.md amendment January 2026, ADR-113) ----------
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct KnowledgeAgentConfig {
    pub name: String,                           // 1..=80; shown to askers
    pub repositories: Vec<KnowledgeRepository>, // 1..=200
    #[serde(default)]
    pub glossaries: Vec<KnowledgeRepository>, // 0..=20; listed order is precedence
    pub audiences: Vec<String>,                 // 1..=16; each 1..=256; unique
    pub group_claim: String,                    // 1..=128; a top-level claim of the token
    #[serde(default)]
    pub front_doors: Vec<String>, // 0..=32 service-identity subjects allowed to ask as front doors; each 1..=256
    #[serde(default)]
    pub proxies: Vec<KnowledgeProxy>, // 0..=32
    #[serde(default = "eight")]
    pub max_proxies: u32, // 1..=32
    #[serde(default)]
    pub service_identity: Option<ServiceIdentity>,
    #[serde(default)]
    pub reading: ReadingConfig,
    #[serde(default = "fifteen")]
    pub fetch_interval_minutes: u32, // 5..=1_440
    #[serde(default)]
    pub hot_copy: HotCopyConfig,
    #[serde(default)]
    pub mcp: McpEndpointConfig,
    #[serde(default)]
    pub sources_record: SourcesRecordConfig,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct KnowledgeRepository {
    pub url: String, // the https rule
    #[serde(default)]
    pub path: Option<String>, // 1..=512; no leading "/", no "..", no empty segment
    #[serde(default)]
    pub branch: Option<String>, // 1..=255; a valid Git branch name; None = the remote's default branch
    pub credential_env: String, // the variable-name rule; a read token
    #[serde(default)]
    pub audience_may_read: bool, // must be true (SA-ACFG-KA-01); absent reads false, refused in V-8a
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct KnowledgeProxy {
    pub name: String,     // SLUG; unique
    pub url: String,      // the https rule; the proxy's base address
    pub audience: String, // 1..=256; the token audience the proxy's `auth` expects
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ServiceIdentity {
    pub token_url: String, // the https rule; the identity provider's token endpoint
    pub client_id: String, // 1..=256
    pub client_secret_env: String, // the variable-name rule
    #[serde(default)]
    pub scope: Option<String>, // 1..=512
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ReadingConfig {
    #[serde(default = "thousand")]
    pub daily_documents: u32,
} // 0..=100_000
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HotCopyConfig {
    #[serde(default = "two_hundred")]
    pub asks_per_day: u32, // 1..=100_000
    #[serde(default)]
    pub credential_env: Option<String>, // None = hot copies off
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct McpEndpointConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
} // default true
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SourcesRecordConfig {
    #[serde(default)]
    pub enabled: bool, // default false
    #[serde(default = "three_hundred_sixty_five")]
    pub retention_days: u32, // 1..=3_650
}

// ---------- hand-written serde forms (AgentConfiguration.md *Types*) ----------

/// A YAML string is `Short`; a mapping is `Policy`, whose own key errors are reported as they are.
impl<'de> Deserialize<'de> for ModelSetting {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Form;
        impl<'de> serde::de::Visitor<'de> for Form {
            type Value = ModelSetting;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a model name `provider:model` or a mapping")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<ModelSetting, E> {
                Ok(ModelSetting::Short(v.to_owned()))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<ModelSetting, A::Error> {
                ModelPolicyConfig::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(ModelSetting::Policy)
            }
        }
        d.deserialize_any(Form)
    }
}

impl Serialize for ModelSetting {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ModelSetting::Short(model) => s.serialize_str(model),
            ModelSetting::Policy(policy) => policy.serialize(s),
        }
    }
}

/// A mapping is `Fixed` and a sequence is `Steps`, each reporting its own key errors.
impl<'de> Deserialize<'de> for ChatContext {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Form;
        impl<'de> serde::de::Visitor<'de> for Form {
            type Value = ChatContext;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a mapping of context keys or a sequence of context steps")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<ChatContext, A::Error> {
                ChatContextFixed::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(ChatContext::Fixed)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                seq: A,
            ) -> Result<ChatContext, A::Error> {
                Vec::<ContextStep>::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
                    .map(ChatContext::Steps)
            }
        }
        d.deserialize_any(Form)
    }
}

impl Serialize for ChatContext {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ChatContext::Fixed(fixed) => fixed.serialize(s),
            ChatContext::Steps(steps) => steps.serialize(s),
        }
    }
}

// ---------- defaults: the values the `serde(default = …)` functions give ----------

impl Default for ChatContextFixed {
    fn default() -> Self {
        ChatContextFixed {
            transcript_messages: two_hundred(),
            images: yes(),
            image_max_bytes: ten_mib(),
            channel_documents: false,
            channel_documents_max_bytes: None,
        }
    }
}

impl Default for StatusSignals {
    fn default() -> Self {
        StatusSignals {
            received: eyes(),
            working: hourglass_flowing_sand(),
            done: white_check_mark(),
            failed: warning(),
        }
    }
}

impl Default for ReadingConfig {
    fn default() -> Self {
        ReadingConfig {
            daily_documents: thousand(),
        }
    }
}

impl Default for HotCopyConfig {
    fn default() -> Self {
        HotCopyConfig {
            asks_per_day: two_hundred(),
            credential_env: None,
        }
    }
}

impl Default for McpEndpointConfig {
    fn default() -> Self {
        McpEndpointConfig { enabled: yes() }
    }
}

impl Default for SourcesRecordConfig {
    fn default() -> Self {
        SourcesRecordConfig {
            enabled: false,
            retention_days: three_hundred_sixty_five(),
        }
    }
}

// The default functions the `serde(default = …)` attributes above name; each value is the one the
// field's comment in AgentConfiguration.md states.
fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}
fn three() -> u32 {
    3
}
fn five() -> u32 {
    5
}
fn eight() -> u32 {
    8
}
fn ten() -> u32 {
    10
}
fn fifteen() -> u32 {
    15
}
fn thirty() -> u32 {
    30
}
fn one_hundred() -> i32 {
    100
}
fn two_hundred() -> u32 {
    200
}
fn three_hundred_sixty_five() -> u32 {
    365
}
fn thousand() -> u32 {
    1_000
}
fn twelve_thousand() -> u32 {
    12_000
}
fn body_64k() -> u32 {
    65_536
}
fn ten_mib() -> u32 {
    10_485_760
}
fn twenty_mib() -> u32 {
    IMAGES_TOTAL_BYTES
}
fn message_field() -> String {
    "message".to_owned()
}
fn signature_header() -> String {
    "X-Wrightrun-Signature".to_owned()
}
fn rs256() -> Vec<String> {
    vec!["RS256".to_owned()]
}
fn english() -> String {
    "english".to_owned()
}
fn groups_claim() -> String {
    "groups".to_owned()
}
fn eyes() -> String {
    "eyes".to_owned()
}
fn hourglass_flowing_sand() -> String {
    "hourglass_flowing_sand".to_owned()
}
fn white_check_mark() -> String {
    "white_check_mark".to_owned()
}
fn warning() -> String {
    "warning".to_owned()
}

/// The images step's total, the former fixed 20 MiB of Channel Gateway K.3.7.
// SPEC-GAP-4: AgentConfiguration.md keeps `IMAGES_TOTAL_BYTES` among its own crate's constants, yet
// `ChatContext::steps` and the default `twenty_mib`, which it places here, use it. The value is held
// here privately so the two places agree; the Agent Configuration crate keeps the public constant.
const IMAGES_TOTAL_BYTES: u32 = 20_971_520;

// ---------- the reserved environment names (AgentConfiguration.md Shared Context, ADR-107) ----------

/// Names `model.environment` may never declare: every name the Claude Local Adapter copies into, sets in
/// or keeps out of the Claude Code child (ClaudeLocalAdapter.md step 8), compared ignoring case.
/// Every name beginning `ORDIN_` is reserved as well (ordin's own variables, the hook pairs among them).
/// Placed here with the types, so that the Claude Local Adapter, which has no edge to the Agent
/// Configuration crate, checks the same list (ClaudeLocalAdapter.md `new`).
pub const RESERVED_ENVIRONMENT_NAMES: [&str; 20] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SYSTEMROOT",
    "TEMP",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CONFIG_DIR",
];
pub const RESERVED_ENVIRONMENT_PREFIX: &str = "ORDIN_";

// ---------- types added by the Model Policy spec (ModelPolicy.md *Types added to 2C*) ----------

/// Amended January 2026 (ADR-107): the provider is the text before ":" of the configured model, as written
/// (MODEL_ID's `^[a-z0-9_-]+`), not a closed enum; ordin holds no provider-specific code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ModelProvider(pub String);

impl ModelProvider {
    /// The provider whose key is the organisation's model key (`ANTHROPIC_API_KEY`, MP-3).
    pub const DEFAULT: &'static str = "anthropic";
    pub fn is_default(&self) -> bool {
        self.0 == Self::DEFAULT
    }
}

/// The organisation's model key. Never serialised, never displayed; `Debug` prints `OrganisationKey(<redacted>)`.
#[derive(Clone)]
pub struct OrganisationKey(String);

impl OrganisationKey {
    /// The only constructor; Model Policy calls it with the key text it read (amended January 2026).
    pub fn new(key: String) -> OrganisationKey {
        OrganisationKey(key)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for OrganisationKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OrganisationKey(<redacted>)")
    }
}

/// One resolved `model.environment` entry (ADR-107). The value is never serialised, displayed or logged;
/// `Debug` prints `HarnessEnvVar { name: "<NAME>", value: <redacted> }`, for a literal and a secret alike.
#[derive(Clone)]
pub struct HarnessEnvVar {
    pub name: String,
    value: String,
}

impl HarnessEnvVar {
    /// The only constructor; Model Policy calls it for each `model.environment` entry (amended January 2026).
    pub fn new(name: String, value: String) -> HarnessEnvVar {
        HarnessEnvVar { name, value }
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Debug for HarnessEnvVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "HarnessEnvVar {{ name: {:?}, value: <redacted> }}",
            self.name
        )
    }
}

/// What the harness session is given about models. Built by Model Policy; applied by the Claude Local Adapter.
#[derive(Debug, Clone)]
pub struct HarnessModelSettings {
    pub provider: ModelProvider,
    pub primary_model: String, // the text after "provider:", e.g. "claude-sonnet-4-6"
    pub fallback_model: Option<String>, // tried once, by the harness's own fallback option (C-77)
    pub gateway_url: Option<String>, // the gateway the harness sends model calls to (C-77)
    /// Amended January 2026 (ADR-107): None only for a provider other than the default with no key set (MP-3.3).
    pub key: Option<OrganisationKey>,
    /// Amended January 2026 (ADR-107): in the configuration's order (name order of the YAML mapping).
    pub environment: Vec<HarnessEnvVar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModelFailureCause {
    /// Every model the policy allows failed (the harness reports an API error after its fallback).
    ModelFailed,
    /// A content check blocked the question or the answer. Amended January 2026 (ADR-107): raised by
    /// ordin's own `content-check` control (Rules Engine `check_content`), never read from harness output.
    ContentFilter,
    /// The harness could not shorten a conversation that grew too long.
    SummaryFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPlace {
    Agent,
    Subagent,
}

// ---------- types added by the Agent Loader spec (AgentLoader.md *The load*) ----------
// Read by the server program, which maps `LoadedAgent.session` into the `SessionSetup` its
// `SessionSetupSource` gives the Claude Local Adapter.

#[derive(Debug, Clone)]
pub struct HostedSessionSetup {
    pub release_id: String,
    pub digest: Option<String>,
    pub working_dir: std::path::PathBuf, // the bundle folder; read-only
    pub instructions_file: std::path::PathBuf, // <bundle>/.ordin/claude/instructions.md (C-61: --append-system-prompt-file)
    pub plugin_dir: std::path::PathBuf,        // <bundle>/.ordin/claude/plugin (C-61: --plugin-dir)
    pub model: HarnessModelSettings,
    pub mcp_servers: Vec<String>, // MCP Front servers the session is given: "agent", "files", and "knowledge" when any knowledge tool is exposed
    pub tool_policy: HarnessToolPolicy,
    pub subagents: Vec<SubagentSetup>,
    pub single_call: SingleCallForm,
    pub run_limit_secs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessToolPolicy {
    pub builtin_file_writing: bool, // false (ADR-098 item 3; C-79)
    pub builtin_file_reading_bundle_only: bool, // true (C-79)
    pub shell: bool,                // false (ADR-098 item 3: off)
    pub web: bool,                  // false (assumed here: wrightrun offered no web tool)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentSetup {
    pub name: String,
    pub tools: SubagentTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentTools {
    SameAsAgent,
    ReadAndSearchOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleCallForm {
    OneTurnNoTool,
    NoBoundTool,
}
