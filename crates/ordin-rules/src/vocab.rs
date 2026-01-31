//! The control vocabulary carried into Rust (RulesEngine.md *Public Interface*, "The control
//! vocabulary"), the agent's own tool policy and what one agent instance brings.

use ordin_types::domain::agent_profile::ContentCheck;
use ordin_types::domain::agents::Authority;
use ordin_types::domain::places::{FolderAccess, Grantee};
use ordin_types::{AgentInstanceId, PlaceId};
use serde::{Deserialize, Serialize};

fn one() -> u32 {
    1
}

fn three() -> u8 {
    3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Point {
    Start,
    BeforeTool,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
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
    },
    RunInProgress,
    StartCount {
        of: CountOf,
        more_than: u32,
        period_seconds: u32,
    },
    RepeatedMessage {
        period_seconds: u32,
    },
    Keywords {
        words: Vec<String>,
        #[serde(default)]
        r#match: MatchMode,
    },
    KeywordShare {
        words: Vec<String>,
        share: DecimalShare,
    },
    Pattern {
        patterns: Vec<String>,
        #[serde(default)]
        r#match: MatchMode,
    },
    Setting {
        name: String,
        equals: serde_json::Value,
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
    },
    ChangedWorkFolder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CountOf {
    Command,
    Scope,
    Asker,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchMode {
    #[default]
    Any,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallTarget {
    Tool,
    Subagent,
}

/// A share written as a decimal string in 0.50–1.00 (no float enters the model, 2C Conventions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecimalShare(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DecidingOutcome {
    End { ending: String },
    Reply { text: String },
    Command { command: String },
    Tools { tools: Vec<String> },
    SingleCall { command: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AdditiveOutcome {
    AttachAction { action: String },
    Delivery { to: DeliveryTo },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryTo {
    Asker,
    Thread,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GateOutcome {
    Refuse,
    RefuseEnding {
        #[serde(default = "three")]
        pause_after: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MessageRule {
    pub name: String,
    #[serde(default)]
    pub priority: i16,
    #[serde(default)]
    pub command: Option<String>,
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub decides: Option<DecidingOutcome>,
    #[serde(default)]
    pub adds: Vec<AdditiveOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct StepGate {
    pub name: String,
    pub point: Point,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    pub conditions: Vec<Condition>,
    pub outcome: GateOutcome,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ArgumentRule {
    pub name: String,
    pub tool: String,
    pub argument: String,
    pub form: ArgumentForm,
    #[serde(default)]
    pub outcome: ArgumentOutcome,
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
#[serde(rename_all = "kebab-case")]
pub enum ArgumentForm {
    RefuseWhen,
    RefuseUnless,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArgumentOutcome {
    #[default]
    Refuse,
    Pause,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Check {
    pub kind: CheckKind,
    #[serde(default)]
    pub condition: Option<Condition>,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckKind {
    File,
    Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HeartbeatForm {
    pub entry: String,
    pub endings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub max_failed_cycles: Option<u32>,
    #[serde(default)]
    pub heartbeat: Option<HeartbeatForm>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct JudgedLoopGate {
    pub commands: Vec<String>,
    pub reviewer: String,
    #[serde(default = "three")]
    pub pause_after: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

/// The parts of `wrightrun-declaration.yaml` this engine reads; parsed by the Package Inspector.
// spec: ordin-desktop/components/RulesEngine#SA-CLOSE-01
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Declaration {
    pub format_version: u32,
    #[serde(default)]
    pub gates: Gates,
    #[serde(default)]
    pub message_rules: Vec<MessageRule>,
    /// YAML `approval-required`: tool names (or `task`, `task:<sub-agent>`) that always ask a person.
    #[serde(default)]
    pub approval_required: Vec<String>,
}

/// Agent profile: the agent configuration's `controls` section, and its schedule entries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredControls {
    pub message_rules: Vec<MessageRule>,
    pub steps: Vec<StepGate>,
    pub argument_rules: Vec<ArgumentRule>,
    /// (duty, command) of each schedule entry naming a duty.
    pub schedule_entries: Vec<(String, String)>,
    /// ADR-107; empty everywhere but the agent profile.
    #[serde(default)]
    pub content_checks: Vec<ContentCheck>,
}

/// The agent's own tool policy (ADR-090 item 1): it can only narrow.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentToolPolicy {
    /// Verbs the agent may never use, e.g. "git.push".
    pub prohibited_verbs: Vec<String>,
    /// Flags the agent's tool bindings forbid, e.g. "--no-verify".
    pub forbidden_flags: Vec<String>,
    /// Verbs or tool names sent for approval.
    pub requires_approval: Vec<String>,
    /// MCP server names the agent declares; started only through the trust policy (Checker).
    pub mcp_servers: Vec<String>,
}

/// What one agent instance brings (handed in by the Package Inspector and the Resolver).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstanceRuleSet {
    pub agent_instance_id: AgentInstanceId,
    pub package_name: String,
    pub authority: Authority,
    /// Absolute path of the agent folder (desktop, personal server) or of the bundle (agent profile).
    pub agent_folder: std::path::PathBuf,
    /// Agent profile: built-in writes are refused everywhere.
    pub agent_folder_read_only: bool,
    pub declaration: Option<Declaration>,
    pub configured: Option<ConfiguredControls>,
    pub tool_policy: Option<AgentToolPolicy>,
    /// The package's approval list: tools that always ask.
    pub approval_list: Vec<String>,
    /// Tools of the package's own MCP servers that the package declares as reading only.
    pub package_read_only_tools: Vec<String>,
    /// Bound tool descriptions: name and argument places.
    pub tools: Vec<ToolDescription>,
    /// Operations the package names that no connection maps (agent profile).
    pub unmapped_operations: Vec<String>,
    /// Agent profile: every offered tool name of a connection marked `external: true`.
    pub external: Vec<String>,
    pub commands: Vec<String>,
    pub reviewer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolDescription {
    pub name: String,
    /// Places in this engine's form: `.`-separated names, `[]` for list items, a final `.*`.
    // spec: ordin-desktop/components/RulesEngine#SA-CLOSE-02
    pub arguments: Vec<String>,
    /// Agent profile: `<connection>/<exposed operation>`; shown in a control line's `detail` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub served_by: Option<String>,
}

/// A place granted in a workspace, resolved to a local root by the Workspace Service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GrantedPlace {
    pub grantee: Grantee,
    pub place_id: PlaceId,
    pub root: std::path::PathBuf,
    pub access: FolderAccess,
}
