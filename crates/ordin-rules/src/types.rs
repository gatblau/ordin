//! The engine's own public types other than the control vocabulary (RulesEngine.md *Public
//! Interface*: how a caller describes a call, outcomes, the host trait, calls outside the three
//! decision points, and the ADR-107 content check).

use std::sync::Arc;

use ordin_types::domain::agent_profile::{CheckPoint, OnBlock};
use ordin_types::domain::email::EmailAttachment;
use ordin_types::domain::places::FolderAccess;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::{ActionProposal, ToolDecision};
use ordin_types::{
    AgentInstanceId, ApprovalId, CrewCharterId, PlaceId, RunId, ToolRequestId, WorkspaceId,
};
use serde::{Deserialize, Serialize};

use crate::vocab::{DeliveryTo, MessageRule, Point};

/// The caller's knowledge of a call beyond the 2C `ToolCall`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CallContext {
    /// The harness's sub-agent that made the call, when the hook input names one (C-78).
    pub subagent: Option<String>,
    /// What the call does, when the caller knows it; `None` lets the engine classify it.
    pub class: Option<ToolClass>,
}

// The serde tag is `class`, because the `External` variant has a field named `kind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum ToolClass {
    ReadOnly,
    /// Writes inside the agent folder, a granted place or the work folder.
    LocalWrite,
    /// Has an effect outside ordin. `kind` is one of the kinds `server.external_actions_off[]` may name.
    External {
        kind: String,
        approval: ApprovalNeed,
        destination: String,
        leaves_company: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalNeed {
    /// Ask a person (the default for externally visible actions, ADR-005).
    Always,
    /// The provider already holds the person's own approval for this action.
    NotNeeded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RulesOutcome {
    pub decision: ToolDecision,
    pub tool_request_id: ToolRequestId,
    /// Present when the verdict is `ask`: the approval the Approval Service holds open.
    pub approval_id: Option<ApprovalId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunStartFacts {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub crew_charter_id: Option<CrewCharterId>,
    pub command: Option<String>,
    /// The message, or the command's arguments; cut to `MAX_MATCH_CHARS` for matching.
    pub text: String,
    /// Agent profile: `<connection>:<channel>`; None is the agent-wide scope.
    pub scope: Option<String>,
    /// Agent profile chat runs: the asker's chat user identifier.
    pub asker: Option<String>,
    pub started_by: String,
    pub started_by_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunStartDecision {
    pub outcome: StartOutcome,
    /// A rule started another command: the caller expands it and replaces the first message.
    pub command: Option<String>,
    pub tool_subset: Option<Vec<String>>,
    pub single_call: bool,
    /// Lowered to a command's declared wall-clock bound when a rule started that command.
    pub limit_seconds: Option<u32>,
    pub actions_attached: Vec<String>,
    pub delivery: Option<DeliveryTo>,
    /// (control name, what it did), in order; copied into the run summary.
    pub controls_decided: Vec<(String, String)>,
}

/// What the engine recorded at a run's start that a second instance needs to take the run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptedRun {
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub scope: Option<String>,
    pub tool_subset: Option<Vec<String>>,
    pub single_call: bool,
    /// The run's own wall-clock limit, as the start decision lowered it (SA-RECON-12).
    pub limit_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartOutcome {
    Proceed,
    /// A gate or rule ended the run before the model: state `completed` with this named ending.
    Ended {
        control: String,
        ending: String,
    },
    /// A rule's stated reply ended the run.
    Replied {
        control: String,
        text: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndDecision {
    End,
    /// Send the run back to the model with this fixed text.
    Continue {
        control: String,
        message: String,
    },
    /// The ending was refused `pause_after` times: a person decides whether to set the gates aside.
    Ask {
        gates: Vec<String>,
        approval_id: ApprovalId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlLine {
    pub name: String,
    pub source: ControlSource,
    pub point: Point,
    pub command: Option<String>,
    pub enforced: bool,
    pub reason: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    Package,
    Configuration,
    Application,
    AgentPolicy,
}

/// What `parse_address` returns (wrightrun section E).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
    pub host_is_ip: bool,
    pub host_is_single_label: bool,
}

#[derive(Debug, Clone)]
pub struct RunKey {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone)]
pub enum Proposal {
    ShellCommand {
        command: String,
        runs_in: String,
    },
    ExternalTool {
        tool: String,
        kind: String,
        destination: String,
    },
    PackageTool {
        server: String,
        tool: String,
    },
    ApprovalListTool {
        tool: String,
    },
    ArgumentRulePause {
        rule: String,
        tool: String,
    },
    AgentPolicyApproval {
        verb: String,
        tool: String,
    },
    EndGatesSetAside {
        gates: Vec<String>,
    },
}

/// The words of one card, in the person's locale, from the message catalogue (never from a model).
#[derive(Debug, Clone)]
pub struct CardWording {
    pub question: String,
    pub verb: String,
    pub where_line: String,
    pub affects: Vec<AffectedItem>,
    pub changes: String,
    /// "See the command" for a shell command; None otherwise.
    pub content_label: Option<String>,
    pub proposal: ActionProposal,
}

#[derive(Debug, thiserror::Error)]
#[error("fact unavailable: {0}")]
pub struct FactError(pub String);

/// A counter sink for the agent profile's metric families 11–15 and 50 (Metrics.md *Public
/// Interface*: a component without a Telemetry edge declares its own sink).
pub trait MetricsSink: Send + Sync {
    fn add(&self, name: &str, labels: &[(&str, &str)], by: u64);
}

/// Facts the engine cannot see itself and the person-facing wording, implemented by the program.
#[async_trait::async_trait]
pub trait RulesHost: Send + Sync + 'static {
    /// Work folder (agent profile: File Service namespace of the agent; elsewhere the agent folder).
    async fn path_exists(&self, run: &RunKey, path: &str) -> Result<bool, FactError>;
    async fn path_contains(&self, run: &RunKey, path: &str, text: &str) -> Result<bool, FactError>;
    async fn read_text(&self, run: &RunKey, path: &str) -> Result<Option<String>, FactError>;
    async fn append_line(&self, run: &RunKey, path: &str, line: &str) -> Result<(), FactError>;
    async fn remove(&self, run: &RunKey, path: &str) -> Result<(), FactError>;
    /// Runs queued, executing or paused with this command. Includes the calling run.
    async fn runs_in_progress(&self, command: &str) -> Result<u32, FactError>;
    /// Whether a paused run other than `except` has one of `commands`.
    async fn paused_run_with_command(
        &self,
        commands: &[String],
        except: RunId,
    ) -> Result<bool, FactError>;
    /// Whether a run with this identifier exists and has ended; None when it does not exist.
    async fn run_ended(&self, run_id: &str) -> Result<Option<bool>, FactError>;
    /// Settings Store (agent profile): the value of a declared setting in a scope.
    async fn setting_value(
        &self,
        scope: Option<&str>,
        name: &str,
    ) -> Result<serde_json::Value, FactError>;
    /// Settings Store: the message rules kept as settings in a scope, with the setting's name.
    async fn setting_rules(&self, scope: &str) -> Result<Vec<(String, MessageRule)>, FactError>;
    /// Localisation: card words for a proposal.
    fn word(&self, run: &RunKey, proposal: &Proposal) -> CardWording;
    /// Localisation: the start-up report sentence for one control line (≤ 200 characters).
    fn plain_line(&self, line: &ControlLine) -> String;
    /// The checker behind the instance's content-check controls; None outside the agent profile.
    // spec: ordin-desktop/components/RulesEngine#ADR-107
    fn content_checker(&self) -> Option<Arc<dyn ContentChecker>> {
        None
    }
    /// The program hands its sink here, in the same form as the ADR-107 `content_checker`; None
    /// (the default) records nothing, as on the desktop (Metrics N-7).
    fn metrics_sink(&self) -> Option<Arc<dyn MetricsSink>> {
        None
    }
}

/// Which program the engine runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramKind {
    Desktop,
    PersonalServer,
    AgentSingle,
    AgentScaled,
}

impl ProgramKind {
    pub(crate) fn is_agent(self) -> bool {
        matches!(self, ProgramKind::AgentSingle | ProgramKind::AgentScaled)
    }
}

#[derive(Debug, Clone)]
pub struct RulesConfig {
    /// The MCP server names the MCP Front serves at start: "ordin", "work", "sandbox", "memory",
    /// "code", "mail", "telegram", "slack", "knowledge", and each "craft_<app>" served then.
    pub ordin_servers: Vec<String>,
    /// The server-name prefixes the MCP Front claims (for example "craft_").
    pub ordin_server_prefixes: Vec<String>,
    /// Absolute paths never granted, plus the application data root.
    pub protected_roots: Vec<std::path::PathBuf>,
    pub app_data_root: std::path::PathBuf,
    pub program: ProgramKind,
    /// This instance's holder name for locks (agent profile: the pod name; elsewhere "local").
    pub instance_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RunRestriction {
    pub external_actions_allowed: bool,
}

/// What `wait_for_approval` returns. `Pending` = `until` passed with no decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalWait {
    Approved,
    Rejected,
    Expired,
    Pending,
}

/// A path checked outside a tool call (K.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PathAccessRequest {
    /// The workspace whose grants apply; None = the agent folder only.
    pub workspace_id: Option<WorkspaceId>,
    /// None = ordin's own action (no agent).
    pub agent: Option<AgentInstanceId>,
    /// The crew whose grants also apply, when the agent acts as a crew member.
    pub crew_charter_id: Option<CrewCharterId>,
    pub path: std::path::PathBuf,
    pub access: FolderAccess,
}

/// Who reads, for `may_read` (K.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaceReader {
    Agent {
        agent_instance_id: AgentInstanceId,
    },
    CrewMember {
        crew_charter_id: CrewCharterId,
        agent_instance_id: AgentInstanceId,
    },
    /// Ordin itself (ADR-027): it has no file tool, so this governs only what it is shown.
    Ordin,
    /// The hosted agent (agent profile): it has no granted places.
    HostedAgent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlaceRead {
    pub workspace_id: WorkspaceId,
    pub reader: PlaceReader,
    pub place_id: PlaceId,
    /// Relative to the place's root, `/`-separated, 1–4,096 characters.
    pub path: String,
}

/// One email for the profile's rules (K.7). Addresses are plain `local@domain` text.
#[derive(Debug, Clone)]
pub struct EmailCheck<'a> {
    pub to: &'a [String],
    pub cc: &'a [String],
    pub bcc: &'a [String],
    pub attachments: &'a [EmailAttachment],
    pub drafted_by_agent: bool,
    pub body: &'a str,
    /// The localised foot line the Mail Service adds; empty skips the foot-line test.
    pub foot_line: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum EmailVerdict {
    Allowed,
    Refused { reasons: Vec<EmailViolation> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EmailViolation {
    EmailOff,
    RecipientOutsideCompany { address: String },
    TooManyRecipients { count: u32, max: u32 },
    AttachmentFromClosedPlace { place_id: PlaceId, path: String },
    FootLineMissing,
}

/// A command ordin runs itself (K.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommandRequest {
    pub workspace_id: WorkspaceId,
    pub origin: CommandOrigin,
    /// 1–8,000 characters.
    pub command: String,
    /// Absolute.
    pub working_dir: std::path::PathBuf,
    /// The command is one the project declares as a check.
    pub declared_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOrigin {
    Person,
    Run {
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
    },
}

/// One GitHub or GitLab operation kind's approval setting for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitHostSetting {
    pub kind: String,
    pub approval_required: bool,
}

/// The checker contract (ADR-107). Request body sent to the checker operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ContentCheckRequest {
    pub point: CheckPoint,
    pub run_id: String,
    pub text: String,
}

/// The checker's reply. Any other shape counts as unreachable (L.4).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ContentCheckReply {
    pub verdict: CheckerVerdict,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckerVerdict {
    Allow,
    Block,
}

/// Implemented by the server program over the Connection Binder's bound checker operation.
#[async_trait::async_trait]
pub trait ContentChecker: Send + Sync {
    async fn check(
        &self,
        connection: &str,
        operation: &str,
        request: &ContentCheckRequest,
        timeout: std::time::Duration,
    ) -> Result<ContentCheckReply, CheckerUnreachable>;
}

/// A timeout, a refused connection, a status outside 200–299 or a reply that is not `ContentCheckReply`.
#[derive(Debug)]
pub struct CheckerUnreachable {
    /// "timeout" | "refused" | "status" | "reply"
    pub reason: &'static str,
}

/// The engine's answer for one point of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentCheckOutcome {
    /// No content check covers this point, or every one allowed (or was unreachable with `allow`).
    Passed,
    /// The first control in configuration order that blocked.
    Blocked {
        control: String,
        category: Option<String>,
        on_block: OnBlock,
        unreachable: bool,
    },
}
