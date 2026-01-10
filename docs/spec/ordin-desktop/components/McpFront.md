# SPEC: MCP Front

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-mcp-front` · **Build phase:** 4 · **Programs:** D P A
> **Dependencies:** Rules Engine, Journal, Service API

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment January 2026 — Reading other teams' knowledge (ADR-110)](#amendment-january-2026--reading-other-teams-knowledge-adr-110)
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)

---

## Purpose

The MCP Front is the hub through which every ordin service reaches a harness session as an MCP server (02:514, 09 ADR-037). It owns the `ToolProvider` trait (2B-2 D4, 09 ADR-100): the Sandbox, Memory Service, Craft Toolkit, Ordin Host, Knowledge Service, Mail Service, Telegram Service, Authoring Service, Project Service, Connection Binder and Agent Loader implement it and register with the MCP Front at start-up. For each session it answers the tool list with only the tools that session is offered (09 ADR-062), checks every call's arguments in fixed code, has the Rules Engine decide every call before a provider acts (02:515), relays progress counts to the Journal (09 ADR-068), and serves its own `work` tools — `report`, `plan` and `read_rest` — itself. It owns the per-session list of offered servers and tools (02:517) and no durable data.

## Approach

The MCP Front is a registry plus a per-session dispatcher. Providers register before start; the registry is then frozen. Each harness session reaches the core through stdio servers whose command is the ordin hook in its MCP-bridge mode (SA-MCP-01): the bridge connects to a Unix domain socket of the MCP Front (a named pipe on Windows), authenticates with a per-session token, and from then on relays MCP JSON-RPC lines unchanged. The MCP server side is built on `rmcp` 3.5.1 (SA-MCP-01), one server instance per bridge connection, serving `initialize`, `ping`, `tools/list` and `tools/call`. Every `tools/call` follows one fixed pipeline: offered check → argument check → Rules Engine decision → provider call under a deadline → output limit → journal. Rejected: serving MCP over streamable HTTP on `127.0.0.1` (SA-MCP-01 keeps it only as the C-22 fallback, because it opens a TCP port); one shared socket with the Hook Relay (the Hook Relay does not depend on the MCP Front, and neither may own the other's listener).

The component's error prefix is `MCP_` *(chosen here)*. Block-check codes keep their 2C prefix `BLOCK_`.

Reconciliation assumptions (Phase 3 reconciliation, rulings R2, R8, R9, R15):

- **SA-RECON-01** `ToolProvider` as defined here is the one provider trait (ruling R2). `SessionGrant` and `ToolCallContext` carry `caller`, `scope_id` and `coding_task_id`, because the Sandbox (refuses hosted agents), the Memory Service (crew members and hosted agents), the Knowledge Service (agent-profile scopes), the Mail and Telegram Services (grant checks), the Craft Toolkit and the Project Service (coding-task sessions) decide their offer from them (reconciliation rows 283, 284). Providers reach the hub through the `ToolRegistry` trait (row 297).
- **SA-RECON-02** `tools_changed(server)` re-runs the serving provider's `offer` for every open session and sends MCP `notifications/tools/list_changed` (row 298, ruling R2). It changes the tool list of a server already launched for a session; a server the session was not launched with cannot be added to a running harness, so it reaches that session at its next `open_session`.
- **SA-RECON-03** The MCP bridge uses this component's own socket with the per-session token (ruling R8, conflict X2). The Hook Relay carries no bridge traffic. The launch line is `ordin-hook mcp-bridge <server> --agent-instance <agent_instance_id>`: the `mcp-bridge` sub-command and its flags belong to the hook program (OrdinHook.md owns the command line, X2, X3); the four `ORDIN_MCP_*`/`ORDIN_SESSION_ID` variables carry the endpoint, session, server and token.
- **SA-RECON-04** Tool names follow wrightrun's pattern `^[A-Za-z][A-Za-z0-9_-]{0,63}$` (ruling R9, ADR-098 keeps wrightrun's format); ordin's own snake_case names fit it. Descriptions are 1–120 characters in the desktop and personal-server profiles and 1–1,024 characters in the agent profile, where wrightrun's texts are carried unchanged (rows 288, 289). The MCP Front keeps sending `mcp__<server>__<tool>` to the Rules Engine; the Rules Engine strips the prefix before matching approval lists, argument rules, counts and Telemetry labels (ruling R9, row 308).
- **SA-RECON-05** The Sandbox owns the `ordin_tools` module generator (ruling R15). The MCP Front only routes: it lists a session's sandbox-callable tools and runs a call from agent-written code through the same pipeline as a harness call, through the `ToolDispatch` trait it defines and implements (rows 292, 293). The note tools `note_write`, `note_replace`, `note_retire` are served on server `knowledge` (ruling R15, row 318).
- **SA-RECON-06** A provider whose servers come and go while the program runs (the Craft Toolkit, one `craft_<app>` server per installed app) claims a server-name prefix at registration; servers under that prefix may appear and disappear after `start` and are picked up at `open_session` and `tools_changed`. There is no `unregister` (row 299).
- **SA-CLOSE-01** (*chosen here*; closing decision K4) The program's `RefResolver` is bound after construction with `bind_ref_resolver`, exactly once; the binding is held in a `OnceLock`. The `work` tools that check blocks (`report`, `plan`) fail with `MCP_REF_RESOLVER_NOT_BOUND` until it is bound; every other tool is unaffected.
- **SA-RECON-08** The MCP Front calls the Rules Engine by its own names (ruling R5): `decide_tool_call` (never fails), `wait_for_approval` and `record_outcome`; the Rules Engine owns the `ToolRequest` record. It writes to the Journal with `append_event`, `store_report`, `state_plan`, `mark_step` and `current_plan` (ruling R3), and checks blocks with the Service API's `BlockCatalogueCheck` over a `RefResolver` the program implements (reconciliation rows 312, 313). The Service API edge is in the 2B-3 row (Phase 2 correction); each of these calls is defined by its owner.
- **SA-RECON-09** Metrics go to Telemetry's families 48 and 49 through `McpMetrics`, which the server program implements; the span is Telemetry's `ordin.tool_call` (ruling R4, rows 310, 311).
- **SA-RECON-07** A `block_shown` event whose serialised data would exceed 256 KiB is emitted as `block_available { workspace_id, block_id }` instead (row 315; ServiceApi.md event table). The MCP Front is an emitter of `run_progress` (tool progress counts) and of `block_shown` / `block_available` (tool `report`); `show` is the Ordin Host's tool on server `ordin` (row 314).

## Shared Context

### Shared types from Phase 2C (copied; defined in `ordin-types`, `ordin-service-api` and `ordin-runtime-adapter`, never redefined here)

```rust
// ordin-types — identifiers (UUIDv7 newtypes, 36-character lowercase form)
pub struct RunId(pub uuid::Uuid);
pub struct WorkspaceId(pub uuid::Uuid);
pub struct AgentInstanceId(pub uuid::Uuid);
pub struct ApprovalId(pub uuid::Uuid);
pub struct ToolRequestId(pub uuid::Uuid);
pub struct ReportId(pub uuid::Uuid);
pub struct CrewRunId(pub uuid::Uuid);
pub struct ScopeId(pub uuid::Uuid);
pub struct CodingTaskId(pub uuid::Uuid);
pub struct BlockId(uuid::Uuid);            // wire part §0
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }

// ordin-types — Rules Engine tool decision (wire part §3)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOrigin {
    BuiltIn,
    OrdinMcpFront { server: String },
    PackageMcpServer { server: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCall {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub point: DecisionPoint,
    pub tool: String,
    pub origin: ToolOrigin,
    /// The arguments as the harness sent them; never journalled, only their digest.
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionPoint { RunStart, BeforeToolCall, RunEnd }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolDecision {
    #[serde(flatten)]
    pub verdict: Verdict,
    pub rule: RuleRef,
    pub decided_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Verdict {
    Allow,
    Deny { reason: String },
    Ask { proposal: ActionProposal },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RuleRef {
    pub source: RuleSource,
    pub rule_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<AgentInstanceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    DeclarationGate, DeclarationToolRule, DeclarationArgumentRule, AgentToolPolicy,
    AppFolderPolicy, AppShellPolicy, AppApprovalPolicy, FailClosed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActionProposal {
    pub action: String,
    pub destination: String,
    pub affects: Vec<AffectedItem>,   // AffectedItem: wire part §5
    pub consequence: String,
}

// ordin-service-api — events (wire part §1)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceEvent {
    RunStateChanged { run_id: RunId, state: RunState },
    RunProgress { run_id: RunId, progress: RunProgress },
    PlanChanged { run_id: RunId, plan_version: u32, steps: Vec<RunStep> },
    StepMarked { run_id: RunId, plan_version: u32, position: u8, mark: StepMark },
    StartUpReportReady { subject: StartUpSubject, items: Vec<StartUpReportItem> },
    ApprovalRequested { card: ApprovalCard },
    ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String },
    CrewRoundChanged { crew_run_id: CrewRunId, round: u32, state: CrewRunState },
    BlockShown { workspace_id: WorkspaceId, block_id: BlockId, block: Block },
    /// A shown block whose event would exceed 256 KiB; fetch it with `ordin.block` (ServiceApi.md; SA-RECON-07).
    BlockAvailable { workspace_id: WorkspaceId, block_id: BlockId },
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunProgress {
    /// One plain sentence, at most 160 characters.
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<ProgressCount>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProgressCount {
    pub done: u64,
    /// Present only when the source carried a real total.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMark { Next, Now, Done, Skipped }

// ordin-service-api — the `plan` tool's arguments (wire part §7)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanToolArgs {
    /// State (or restate) the plan; restating makes a new plan version.
    State { steps: Vec<String> },
    Mark { position: u8, mark: AgentStepMark },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStepMark { Now, Done, Skipped }

// ordin-service-api — the `report` tool's arguments (wire part §7)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportToolArgs {
    Report(ReportBlock),
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportBlock {
    pub title: String,
    pub as_of: Timestamp,
    pub parts: Vec<ReportPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportPart {
    pub heading: String,
    pub content: ReportPartContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportPartContent {
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
}
// TableBlock, ChartBlock, KeyFiguresBlock: wire part §7. The MCP Front deserialises them as
// typed values and passes them to the block check; it reads no field of them itself.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BlockRejection {
    pub code: String,
    /// JSON Pointer to the failing field, e.g. `/parts/2/content/source`.
    pub at: String,
    /// One sentence naming the correct form, for the model.
    pub reason: String,
}

// ordin-service-api — block check (ServiceApi.md, SA-RECON-08)
pub struct BlockCheckContext { pub workspace_id: WorkspaceId, pub run_id: Option<RunId>, pub now: Timestamp }
pub struct CheckedReport { pub block: ReportToolArgs, pub dropped_parts: Vec<BlockRejection> }
// RefResolver (content, email_message, entity, run_agent_name, plan_state, approval_verbs): implemented by the program.
// BlockCatalogueCheck::{check_block, check_report_tool, check_plan_tool}; report_to_domain(&ReportBlock, ReportId, WorkspaceId, RunId) -> Report
// ordin-types — EventKind (2C wire): EventKind::{RunProgress, BlockShown, BlockAvailable, ..}.as_str()

// ordin-journal — plan input (JournalAndArtefactStore.md)
pub struct PlannedStep { pub line: String, pub came_from: StepSource }

// ordin-rules (RulesEngine.md)
pub struct CallContext { pub subagent: Option<String>, pub class: Option<ToolClass> }
pub struct RulesOutcome { pub decision: ToolDecision, pub tool_request_id: ToolRequestId, pub approval_id: Option<ApprovalId> }
pub enum ApprovalWait { Approved, Rejected, Expired, Pending }

// ordin-types — persisted plan step (domain part, owned by the Journal)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunStep {
    pub id: RunStepId,
    pub run_id: RunId,
    pub plan_version: u32,
    pub position: u32,
    pub line: String,
    pub came_from: StepSource,
    pub mark: StepMark,
    pub marks_set: Vec<MarkSet>,
    pub events_seen: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepSource { Workflow, CrewPlan, AgentsPlan, HarnessTodoList, AddedByOrdin }

// ordin-types — start-up report item the MCP Front supplies (wire part §6)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartUpReportItem {
    EnforcedRule { rule: RuleRef, plain: String },
    RuleNotEnforced { rule: RuleRef, plain: String, reason: NotEnforcedReason },
    UnavailablePart { part: String, plain: String, reason: UnavailableReason },
    AvailableTool { tool: String, origin: ToolOrigin, plain: String },
    VerbNotOffered { adapter: AdapterKind, verb: AdapterVerb, plain: String },
    OwnScreensNotShown { server: String, plain: String },
}
```

Limits carried from 2C and used here: tool name 1–128 characters (LW-17); tool arguments at most 1 MiB serialised (LW-18); MCP server name 1–64 characters `[a-zA-Z0-9_.-]` (LW-19), narrowed here to `^[a-z][a-z0-9_]{0,31}$`; plan 3–7 steps of 1–80 characters, at most 20 plan versions per run (LW-35); block at most 2 MiB (LW-29); report 1–12 parts (A-34); block rejection reason ≤ 300 characters and at most 20 rejections per call (LW-42); start-up `plain` 1–200 characters (LW-28).

### Names fixed in Phase 1 and the HLD

| Name | Value | Source |
|---|---|---|
| MCP server names | `ordin` (Ordin and crew tools), `work` (`report`, `plan`, `read_rest`), `sandbox` (tools `python_sandbox`, and `shell_sandbox`, `javascript_sandbox` under the Sandbox's feature `extra-guests`), `memory`, `knowledge` (knowledge and note tools), `code`, `github`, `gitlab`, `checks` (Project Service), `mail`, `telegram`, `slack`, `craft_<app>` (prefix `craft_`); `crucible` (authoring tools, agent maker only); agent profile: `agent` (Connection Binder), `files` (Agent Loader over the File Service), `knowledge` | SA-NAME-01, SA-NAME-04, SA-NAME-08, 08 MCP Front row, 08 Authoring tools row; reconciliation rows 309, 324 |
| Tool names `show`, `report`, `plan` | `report` and `plan` on server `work` | 08 MCP Front row; ADR-067, ADR-068 |
| Sandbox tool module | `ordin_tools`, Python and JavaScript (an ES module); each function's name and arguments mirror the MCP tool; generated by the Sandbox from `ToolDispatch::session_tools` (SA-RECON-05) | SA-NAME-06; 08 Sandbox tool module row |
| MCP library | `rmcp` 3.5.1, Apache-2.0 | SA-MCP-01 |
| Local transport | stdio servers whose command is `ordin-hook` in its MCP-bridge mode, relaying over an authenticated local channel in the application data root, mode `0600`; named pipe restricted to the current user on Windows; no TCP port | SA-MCP-01; C-22 |
| Hashing, random and encoding crates | `sha2` 0.10, `rand` 0.8, `base64` 0.22.1, as already used in the repository (`memory/Cargo.toml:44`, `:46`, `:48`) | repository evidence |
| Concurrency and errors | `tokio`, `tokio-util` (`CancellationToken`), `futures`, `async-trait`, `thiserror`, `serde`, `serde_json`, `schemars` 1.2.3 | 2C wire part §2; SA-API-02 |
| Logging | `tracing` 0.1.44 | SA-OBS-01 |
| Metrics, agent profile only | `prometheus-client` 0.25.1; added counters take the prefix `wrightrun_` | SA-OBS-02 |
| Tracing, agent profile only | OpenTelemetry through `tracing-opentelemetry` | SA-OBS-03 |

### Configuration consumed

No environment variable and no organisation-profile key is read by this crate. The program passes `McpFrontConfig` (below). The setting **Give agents only the parts they need** is owned by the Usage Planner (2C domain `UsageSettings.give_agents_only_needed_parts`); the program passes its current value through `McpFront::set_only_needed_parts` whenever it changes.

### Names chosen here

| Name | Kind | Value |
|---|---|---|
| `ORDIN_MCP_ENDPOINT` | environment variable set in each bridge server's `env` | the MCP Front's socket path, `<app data root>/run/mcp.sock` (mode `0600`); on Windows `\\.\pipe\ordin-mcp-<installation id>` |
| `ORDIN_MCP_SERVER` | environment variable | the MCP server name this bridge instance serves |
| `ORDIN_MCP_TOKEN` | environment variable (secret) | 32 random bytes, base64url without padding (43 characters), one per session |
| `ORDIN_SESSION_ID` | environment variable (name fixed in 2C §4) | the session's `RunId` |
| `read_rest` | tool on server `work` | reads the rest of a trimmed output |
| bridge protocol version | integer | `1` |

## Public Interface

### Crate root (`ordin_mcp_front`)

```rust
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::{Duration, Instant}};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// MCP server name: ^[a-z][a-z0-9_]{0,31}$.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerName(String);

impl ServerName {
    /// Err(McpError::InvalidName) when the value breaks the pattern.
    pub fn new(value: impl Into<String>) -> Result<Self, McpError>;
    pub fn as_str(&self) -> &str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramProfile { Desktop, PersonalServer, AgentProfile }

/// Who the session is. Built by the adapter that opens the session (Claude Local Adapter,
/// Copilot Local Adapter) from `StartRun` — `caller`, `scope_id` and `coding_task_id` are
/// copied from `StartRun`'s fields of the same names (2C wire); providers decide what to offer
/// from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionGrant {
    pub run_id: RunId,
    /// In the agent profile: the single workspace identifier the Agent Loader assigns.
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    pub profile: ProgramProfile,
    /// Whose session it is (SA-RECON-01).
    pub caller: ToolCaller,
    /// Agent profile only: the scope of the conversation the run serves (a channel, a direct
    /// chat, or the agent-wide scope, ADR-097 item 5); `None` on the desktop and personal server,
    /// and `None` for the agent-wide scope where the provider's own API takes `Option<ScopeId>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    /// Set for a session that works on a coding task (servers `code`, `checks`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
}

/// Who a session belongs to. Defined in `ordin-types` (2C wire, beside `StartRun`) and
/// re-exported here; the agent instance is `SessionGrant::agent_instance_id`.
pub use ordin_types::session::ToolCaller;
// pub enum ToolCaller { Ordin, Agent, CrewMember { crew_run_id: CrewRunId }, HostedAgent }
//   #[serde(tag = "kind", rename_all = "snake_case")]

/// What the adapter registers with the harness for this session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionMcpConfig {
    pub servers: Vec<McpServerLaunch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct McpServerLaunch {
    pub name: ServerName,
    /// Absolute path of the ordin hook program (`ordin-hook`, `ordin-hook.exe`).
    pub command: PathBuf,
    /// Always `["mcp-bridge", "<server>", "--agent-instance", "<agent_instance_id>"]`
    /// (the hook program's command line, SA-RECON-03).
    pub args: Vec<String>,
    /// ORDIN_MCP_ENDPOINT, ORDIN_SESSION_ID, ORDIN_MCP_SERVER, ORDIN_MCP_TOKEN.
    pub env: BTreeMap<String, String>,
}

impl SessionMcpConfig {
    /// `{"mcpServers": {"<name>": {"type": "stdio", "command": "…", "args": ["mcp-bridge", "<name>", "--agent-instance", "<uuid>"], "env": {…}}}}`.
    /// The Claude Local Adapter writes it to its `--mcp-config` file; the Copilot Local Adapter maps it
    /// to the SDK's MCP server option.
    pub fn to_mcp_servers_json(&self) -> serde_json::Value;
}

/// One offered tool, for the start-up report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OfferedTool {
    pub server: ServerName,
    pub tool: String,
    /// The provider's `ToolSpec::plain`.
    pub plain: String,
}

impl OfferedTool {
    /// `StartUpReportItem::AvailableTool { tool: "mcp__<server>__<tool>", origin: OrdinMcpFront { server }, plain }`.
    pub fn to_start_up_item(&self) -> StartUpReportItem;
}

#[derive(Debug, Clone)]
pub struct McpFrontConfig {
    /// The application data root; the socket is `<root>/run/mcp.sock`.
    pub app_data_root: PathBuf,
    /// Windows only: used in `\\.\pipe\ordin-mcp-<installation id>`.
    pub installation_id: uuid::Uuid,
    /// Absolute path of `ordin-hook` / `ordin-hook.exe`, shipped beside the program.
    pub hook_program: PathBuf,
    pub profile: ProgramProfile,
    /// Default 300 s (assumed here: long enough for a sandbox run with brokered calls and an
    /// ArtCraft conversion of 120 s, 2D-4, with margin).
    pub call_timeout: Duration,
    /// Default 50 s: how long a call waits for a person's approval before answering
    /// "waiting for the person's approval" (assumed here: equal to the hook deadline, LW-22).
    pub approval_wait: Duration,
    /// Default 16,000 characters: above it, and with only-needed-parts on, output is trimmed.
    pub trim_above_chars: usize,
    /// Default 12,000 characters kept in the answer when trimming.
    pub trim_keep_chars: usize,
    /// Default 4 MiB: the most text one call returns, trimming or not.
    pub max_output_bytes: usize,
    /// Default 16 MiB: trimmed remainders kept per session, oldest dropped first.
    pub max_rest_bytes_per_session: usize,
    /// Default 64: concurrent `tools/call` per session.
    pub max_concurrent_calls_per_session: usize,
}

impl Default for McpFrontConfig { /* the defaults above; app_data_root, installation_id and hook_program have no default and are set by the program */ }

/// The hub. One per program.
pub struct McpFront { /* private */ }

impl McpFront {
    pub fn new(
        config: McpFrontConfig,
        rules: Arc<ordin_rules::RulesEngine>,
        journal: ordin_journal::Journal,
        blocks: Arc<dyn ordin_service_api::BlockCatalogueCheck>,
        metrics: Option<Arc<dyn McpMetrics>>,
    ) -> Self;

    /// Binds the program's `RefResolver` once, after the program has built the Journal and the
    /// File Service (SA-CLOSE-01). A second call → `MCP_REF_RESOLVER_ALREADY_BOUND` (permanent).
    pub fn bind_ref_resolver(&self, resolver: Arc<dyn ordin_service_api::RefResolver>) -> Result<(), McpError>;

    /// Registers a provider. Only before `start`.
    /// Err: MCP_REGISTRY_FROZEN, MCP_PROVIDER_DUPLICATE, MCP_SERVER_CLAIMED, MCP_TOOL_SPEC_INVALID.
    pub fn register(&self, provider: Arc<dyn ToolProvider>) -> Result<(), McpError>;

    /// Freezes the registry, binds the socket (or pipe) and starts accepting bridge connections.
    /// Err: MCP_ALREADY_STARTED, MCP_SOCKET_UNAVAILABLE.
    pub async fn start(&self, shutdown: CancellationToken) -> Result<(), McpError>;

    /// Opens a session: mints its token, asks every provider what to offer, returns the launch list.
    /// Err: MCP_NOT_STARTED, MCP_SESSION_EXISTS, MCP_PROVIDER_FAILED.
    pub async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError>;

    /// Idempotent. Closes the session's bridge connections, cancels its calls, drops its state,
    /// and calls `session_closed` on every provider.
    pub async fn close_session(&self, run_id: RunId) -> Result<(), McpError>;

    /// The tools offered to an open session. Err: MCP_SESSION_UNKNOWN.
    pub fn offered_tools(&self, run_id: RunId) -> Result<Vec<OfferedTool>, McpError>;

    /// Every server name a registered provider serves now (claimed names, plus the servers
    /// currently listed under a claimed prefix), sorted, and the claimed prefixes. Infallible.
    pub fn server_names(&self) -> (Vec<ServerName>, Vec<&'static str>);

    /// Turns trimming of long output on or off for calls that start after this point.
    pub fn set_only_needed_parts(&self, on: bool);
}

/// How providers reach the hub (SA-RECON-01). Implemented by `McpFront`; a provider holds an
/// `Arc<dyn ToolRegistry>` rather than the concrete hub.
pub trait ToolRegistry: Send + Sync {
    /// Same as `McpFront::register`. Only before `start`.
    /// Err: MCP_REGISTRY_FROZEN, MCP_PROVIDER_DUPLICATE, MCP_SERVER_CLAIMED, MCP_TOOL_SPEC_INVALID.
    fn register(&self, provider: Arc<dyn ToolProvider>) -> Result<(), McpError>;

    /// The provider serving `server` changed what it offers (a grant, a profile change, an app
    /// installed). Re-runs that provider's `offer` for every open session in the background and
    /// returns at once (SA-RECON-02). Before `start`, or for a server no provider serves, it does
    /// nothing. Infallible; failures are logged.
    fn tools_changed(&self, server: &ServerName);

    /// The tool names a registered provider's current `servers()` lists for `server`, sorted;
    /// `None` when no provider serves it. Infallible.
    fn served_tools(&self, server: &ServerName) -> Option<Vec<String>>;
}

/// The route for calls made by agent-written code through the Sandbox's `ordin_tools` module
/// (SA-RECON-05). Defined and implemented here; the Sandbox consumes it.
#[async_trait]
pub trait ToolDispatch: Send + Sync {
    /// The session's offered tools whose spec is `sandbox_callable`, sorted by server then tool.
    /// Err: MCP_SESSION_UNKNOWN.
    fn session_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError>;

    /// Runs one call with the pipeline of a harness call (`via = Sandbox`). Err only for
    /// session-level failures (MCP_SESSION_UNKNOWN); tool-level failures are in `ToolCallResult`.
    async fn dispatch(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError>;
}

/// Agent profile only: where call counts go. Owned here and implemented by the server program
/// over `Telemetry::record(TelemetryEvent::McpFrontCall { server, outcome, seconds })` (families
/// 48 and 49, reconciliation R4); this crate has no Telemetry edge. `None` on the desktop and the
/// personal server.
pub trait McpMetrics: Send + Sync {
    fn call_finished(&self, server: &ServerName, outcome: CallOutcome, seconds: f64);
}

/// Telemetry's `McpCallOutcome` values: `answered`, `refused`, `failed`, `cancelled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome { Answered, Refused, Failed, Cancelled }

/// The outcome of one tool call, as returned to the harness or to the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCallResult {
    pub is_error: bool,
    /// Text for the model.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    /// Stable code when `is_error`; never shown to the person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum McpError {
    #[error("invalid name")] InvalidName { value: String },
    #[error("registry frozen")] RegistryFrozen,
    #[error("provider registered twice")] ProviderDuplicate { provider_id: &'static str },
    #[error("server already served by another provider")] ServerClaimed { server: ServerName, by: &'static str },
    #[error("tool spec invalid")] ToolSpecInvalid { provider_id: &'static str, tool: String, rule: &'static str },
    #[error("already started")] AlreadyStarted,
    #[error("not started")] NotStarted,
    #[error("socket unavailable")] SocketUnavailable { detail: String },
    #[error("ref resolver already bound")] RefResolverAlreadyBound,
    #[error("ref resolver not bound")] RefResolverNotBound,
    #[error("session exists")] SessionExists { run_id: RunId },
    #[error("session unknown")] SessionUnknown { run_id: RunId },
    #[error("provider failed")] ProviderFailed { provider_id: &'static str, code: String },
}

impl McpError {
    /// MCP_INVALID_NAME, MCP_REGISTRY_FROZEN, MCP_PROVIDER_DUPLICATE, MCP_SERVER_CLAIMED,
    /// MCP_TOOL_SPEC_INVALID, MCP_ALREADY_STARTED, MCP_NOT_STARTED, MCP_SOCKET_UNAVAILABLE,
    /// MCP_SESSION_EXISTS, MCP_SESSION_UNKNOWN, MCP_PROVIDER_FAILED,
    /// MCP_REF_RESOLVER_ALREADY_BOUND, MCP_REF_RESOLVER_NOT_BOUND (both permanent).
    pub fn code(&self) -> &'static str;
    /// true for MCP_SOCKET_UNAVAILABLE and MCP_PROVIDER_FAILED (when the provider said retryable).
    pub fn retryable(&self) -> bool;
}
```

### The `ToolProvider` trait (owned here, ADR-100)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolKey {
    pub server: ServerName,
    /// ^[A-Za-z][A-Za-z0-9_-]{0,63}$ (SA-RECON-04)
    pub tool: String,
}

#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub name: ServerName,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// ^[A-Za-z][A-Za-z0-9_-]{0,63}$, unique within its server (SA-RECON-04).
    pub name: String,
    /// One line, no line break (ADR-062): 1–120 characters in the desktop and personal-server
    /// profiles, 1–1,024 characters in the agent profile (SA-RECON-04).
    pub description: String,
    /// JSON Schema (draft 2020-12) generated with `schemars` from the tool's argument type
    /// (SA-API-02); `"type": "object"` at the root.
    pub input_schema: serde_json::Value,
    /// One line, 1–300 characters: the correct form, quoted to the model on a wrong call.
    pub argument_form: String,
    /// Offered in the `ordin_tools` sandbox module (SA-NAME-06).
    pub sandbox_callable: bool,
    /// Start-up report sentence, 1–200 characters, with no identifier; also the progress line
    /// for this tool's counts.
    pub plain: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallVia { Harness, Sandbox }

/// Hands progress counts to the MCP Front, which relays them to the harness (when the call
/// carried a progress token) and to the Journal as `RunProgress` counts.
#[derive(Clone)]
pub struct ProgressSink { /* private: run_id, tool plain line, channel sender */ }

impl ProgressSink {
    /// `total` only when the source carried a real total (ADR-068). Never blocks; a full channel
    /// drops the oldest unsent count (the latest count supersedes it).
    pub fn report(&self, done: u64, total: Option<u64>);
}

#[derive(Clone)]
pub struct ToolCallContext {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    /// Copied from the session's `SessionGrant` (SA-RECON-01).
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,
    pub coding_task_id: Option<CodingTaskId>,
    /// Minted by the MCP Front per call (UUIDv7).
    pub call_id: ToolRequestId,
    pub correlation_id: String,
    pub deadline: Instant,
    pub cancel: CancellationToken,
    pub progress: ProgressSink,
    /// **Give agents only the parts they need** (ADR-062): return the relevant parts of long
    /// documents, each with a reference to the rest.
    pub only_needed_parts: bool,
    pub via: CallVia,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolOutput {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolFailure {
    /// `<PREFIX>_<NAME>` of the provider's registry entry.
    pub code: String,
    /// One sentence for the model, 1–300 characters; no path outside the session's places,
    /// no token, no stack.
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArgumentProblem {
    /// JSON Pointer to the failing field; `""` for the whole object.
    pub at: String,
    /// 1–300 characters.
    pub reason: String,
}

#[async_trait]
pub trait ToolProvider: Send + Sync + 'static {
    /// Stable identifier, ^[a-z][a-z0-9-]{0,39}$, e.g. "ordin-host".
    fn provider_id(&self) -> &'static str;

    /// Servers and tools this provider serves. Read at `register`, at every `open_session` and
    /// at every `tools_changed` for one of its servers, so a provider whose tools change (the
    /// Craft Toolkit, as ArtCraft apps are approved) is seen without a restart.
    fn servers(&self) -> Vec<ServerSpec>;

    /// A server-name prefix (for example `"craft_"`) under which this provider may list servers
    /// that appear after `start` (SA-RECON-06). Pattern `^[a-z][a-z0-9]{0,15}_$`. Default: none.
    fn server_prefix(&self) -> Option<&'static str> { None }

    /// Which of its tools this session is offered. Role, package and workspace grants are the
    /// provider's own knowledge (ADR-062). Must answer within 2 s.
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>;

    /// Fixed-code argument check, no model call (ADR-062). Typically typed deserialisation with
    /// unknown fields refused. Must not block.
    fn check_arguments(&self, key: &ToolKey, arguments: &serde_json::Value) -> Result<(), ArgumentProblem>;

    /// Runs a call the Rules Engine allowed (or the person approved). Must honour `ctx.cancel`
    /// and `ctx.deadline`.
    async fn call(&self, ctx: ToolCallContext, key: &ToolKey, arguments: serde_json::Value)
        -> Result<ToolOutput, ToolFailure>;

    /// The session ended; drop per-session state. Infallible.
    async fn session_closed(&self, run_id: RunId);
}
```

**Registration at start-up.** The program (`apps/desktop`, `apps/server`) builds the `McpFront`, then each provider, then calls `register` once per provider, then `start`. There is no separate seal call: `start` freezes the registry; the error type is `McpError` (reconciliation row 302). Each provider exposes its `Arc<dyn ToolProvider>` (for example `tool_provider()`, as OrdinHost.md does) and the program registers it; a provider does not register itself (rows 303, 325). Order (*chosen here*, so a failed registration names the earliest cause): Sandbox, Memory Service, Knowledge Service, Craft Toolkit, Project Service, Mail Service, Telegram Service, Slack Service, Authoring Service, Connection Binder, Agent Loader (which serves the agent profile's file tools from the File Service, 2B-2 D4), Ordin Host — each only in the programs 2B-3 lists for it. The MCP Front registers its own `work` provider inside `new`. A registration error stops the program's start with that error's code in the log. After `start`, `register` returns `MCP_REGISTRY_FROZEN`.

**Called by** (every interaction is listed on the caller's side too):

| Public item | Called by |
|---|---|
| `McpFront::new`, `register` / `ToolRegistry::register`, `start` | the desktop and server programs (OrdinDesktop, OrdinServer), once per provider in the order above |
| `bind_ref_resolver` | the desktop and server programs (OrdinDesktop, OrdinServer), once, after building the Journal and the File Service and before `start` |
| `set_only_needed_parts` | the programs, with the Usage Planner's `UsageSettings.give_agents_only_needed_parts` |
| `open_session`, `close_session`, `offered_tools`, `SessionMcpConfig::to_mcp_servers_json` | Claude Local Adapter, Copilot Local Adapter (I-11) |
| `ToolRegistry::tools_changed` | Mail Service (grant and profile changes), Telegram Service (link, bot and profile changes), Slack Service (link, sign-in, grant and profile changes), Craft Toolkit (apps installed or removed), Project Service (a project opened or closed in a session's workspace) |
| `ToolRegistry::served_tools` | Agent Loader (L-17: the `knowledge` server's tool names) |
| `server_names` | the programs, which pass the names and prefixes to the Rules Engine as its list of ordin servers (reconciliation row 309) |
| `ToolDispatch::session_tools`, `ToolDispatch::dispatch` | Sandbox (the `ordin_tools` module and its calls, I-10) |
| `ToolProvider` (implemented, not called) | Sandbox, Memory Service, Knowledge Service, Craft Toolkit, Project Service, Mail Service, Telegram Service, Slack Service, Authoring Service, Connection Binder, Agent Loader, Ordin Host |
| bridge protocol (socket) | `ordin-hook mcp-bridge` (OrdinHook, I-12) |

The MCP Front does not implement the Service API's `RefResolver`; the program does, over the File, Mail, Telegram and Knowledge Services and the Journal (reconciliation row 313). Because the File Service and the MCP Front sit in the same build phase and the resolver needs modules built after the MCP Front, the resolver is bound late with `bind_ref_resolver` (SA-CLOSE-01) rather than passed to `new`.

### Bridge protocol (core end; the bridge is `ordin-hook mcp-bridge`)

| Step | Direction | Content |
|---|---|---|
| 1 | bridge → core | One JSON line, at most 1 KiB: `{"protocol":1,"session":"<run_id>","server":"<server name>","token":"<ORDIN_MCP_TOKEN>"}` |
| 2 | core → bridge | `{"kind":"ready"}`, or `{"kind":"refused","code":"<MCP_…>"}` followed by close |
| 3 | both | MCP JSON-RPC 2.0 messages, one per line, relayed byte for byte between the harness's stdio and the socket until either side closes |

MCP methods the core answers on a ready connection:

| Method | Answer |
|---|---|
| `initialize` | `{"protocolVersion": <the requested version>, "capabilities": {"tools": {"listChanged": true}}, "serverInfo": {"name": "<server name>", "version": "<application version>"}}` |
| `notifications/initialized` | none |
| `ping` | `{}` |
| `tools/list` | `{"tools": [{"name", "description", "inputSchema"}, …]}` — only this server's tools offered to this session, sorted by name |
| `tools/call` | `{"content": [{"type": "text", "text": "…"}], "structuredContent": {…} (only when present), "isError": <bool>}` |
| `notifications/cancelled` | cancels the named in-flight call; no answer |
| any other method | JSON-RPC error `-32601` "method not found" |
| a line that is not JSON | JSON-RPC error `-32700` "parse error", id `null` |

During a `tools/call` that carried `params._meta.progressToken`, the core sends `notifications/progress` with `{"progressToken", "progress": done, "total": total (only when known)}`. After a `tools_changed` that altered a session's tools on a server, the core sends `notifications/tools/list_changed` (no params) on that server's connections for the session.

### The MCP Front's own provider: server `work`

| Tool | Offered to | Arguments | Result |
|---|---|---|---|
| `report` | every session in the desktop and personal-server profiles (agent sessions in a workspace and Ordin, 02:514) | `ReportToolArgs` | `"Shown: <title>."` with structured `{"block_id", "report_id", "left_out": [BlockRejection…]}` |
| `plan` | as `report` | `PlanToolArgs` | `"Plan noted: <n> steps."` or `"Marked step <p> <mark>."` with structured `{"plan_version"}` |
| `read_rest` | every session while trimming is on | `{"reference": "rest:<uuid>", "from": <u64 character offset>}` | up to `trim_keep_chars` characters from `from`, plus the next offset or "end" |

`ToolSpec` values:

| Tool | `description` | `argument_form` | `sandbox_callable` | `plain` |
|---|---|---|---|---|
| `report` | Show a report, table, chart or key figures to the person. | `report(kind: "report"\|"table"\|"chart"\|"key_figures", …the block's fields)` | false | Can show you reports, tables and charts. |
| `plan` | State your 3–7 steps before other tools, then mark each now or done. | `plan(action: "state", steps: [3–7 lines of ≤80 characters]) or plan(action: "mark", position: 1–7, mark: "now"\|"done"\|"skipped")` | false | Tells you its steps as it works. |
| `read_rest` | Read more of an output that was cut short. | `read_rest(reference: "rest:<id>", from: <offset>)` | true | Reads long documents in parts. |

### Example

Session open by the Claude Local Adapter:

```rust
let config = mcp_front.open_session(SessionGrant {
    run_id: RunId(uuid!("0192a1c2-3344-7556-8677-8899aabbccdd")),
    workspace_id: WorkspaceId(uuid!("0192a0f1-1111-7222-8333-444455556666")),
    agent_instance_id: AgentInstanceId(uuid!("0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff")),
    harness: HarnessKind::ClaudeCode,
    profile: ProgramProfile::Desktop,
    caller: ToolCaller::Agent,
    scope_id: None,
    coding_task_id: None,
}).await?;
```

`config.to_mcp_servers_json()`:

```json
{"mcpServers": {
  "sandbox": {"type": "stdio", "command": "/Applications/ordin.app/Contents/MacOS/ordin-hook", "args": ["mcp-bridge", "sandbox", "--agent-instance", "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"],
              "env": {"ORDIN_MCP_ENDPOINT": "/Users/asha/Library/Application Support/ordin/run/mcp.sock",
                      "ORDIN_SESSION_ID": "0192a1c2-3344-7556-8677-8899aabbccdd",
                      "ORDIN_MCP_SERVER": "sandbox",
                      "ORDIN_MCP_TOKEN": "q3J0bVx9a1Lk2mRb7sYw1AZt8VxN0pLk2mRb7sYw1Aa"}},
  "work": {"type": "stdio", "command": "/Applications/ordin.app/Contents/MacOS/ordin-hook", "args": ["mcp-bridge", "work", "--agent-instance", "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"],
           "env": {"ORDIN_MCP_ENDPOINT": "/Users/asha/Library/Application Support/ordin/run/mcp.sock",
                   "ORDIN_SESSION_ID": "0192a1c2-3344-7556-8677-8899aabbccdd",
                   "ORDIN_MCP_SERVER": "work",
                   "ORDIN_MCP_TOKEN": "q3J0bVx9a1Lk2mRb7sYw1AZt8VxN0pLk2mRb7sYw1Aa"}}
}}
```

A `plan` call and its answer on the `work` connection:

```json
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"plan","arguments":{"action":"state","steps":["Read the supplier quotes","Compare the Q3 rates","Write the summary"]}}}
{"jsonrpc":"2.0","id":7,"result":{"content":[{"type":"text","text":"Plan noted: 3 steps."}],"structuredContent":{"plan_version":1},"isError":false}}
```

A wrong call:

```json
{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"plan","arguments":{"action":"mark","position":9,"mark":"now"}}}
{"jsonrpc":"2.0","id":8,"result":{"content":[{"type":"text","text":"Wrong arguments for plan at /position: the plan has 3 steps. Correct form: plan(action: \"state\", steps: [3–7 lines of ≤80 characters]) or plan(action: \"mark\", position: 1–7, mark: \"now\"|\"done\"|\"skipped\")"}],"isError":true}}
```

## Internal Logic

### A. Construction and registration

1. `new` stores the configuration and dependencies, creates an empty registry (`BTreeMap<ServerName, ProviderSlot>`) and registers the built-in `work` provider (section F). Nothing is logged.
2. `register(provider)`:
   1. If started → `MCP_REGISTRY_FROZEN`.
   2. If a provider with the same `provider_id` is present → `MCP_PROVIDER_DUPLICATE`.
   3. If `provider.server_prefix()` is present: it must match `^[a-z][a-z0-9]{0,15}_$` (else `MCP_TOOL_SPEC_INVALID` with rule `server_prefix`), and no other provider's claimed name or prefix may start with it or be a start of it (else `MCP_SERVER_CLAIMED`). For each `ServerSpec` of `provider.servers()`: the name must pass `ServerName::new`; if another provider already serves it, or it starts with another provider's prefix → `MCP_SERVER_CLAIMED`. Each `ToolSpec` is checked: name pattern `^[A-Za-z][A-Za-z0-9_-]{0,63}$`, unique in its server, `description` with no line break and 1–120 characters (1–1,024 when `config.profile` is `AgentProfile`, SA-RECON-04), `argument_form` 1–300 characters, `plain` 1–200 characters, `input_schema` an object whose `"type"` is `"object"`. The first broken rule → `MCP_TOOL_SPEC_INVALID` with the rule's name.
   4. The provider is stored with the servers it claimed. Log `info` `mcp_front.provider_registered` {`provider_id`, `servers`, `tool_count`}.

### B. Start

3. `start(shutdown)`: if started → `MCP_ALREADY_STARTED`. Freeze the registry. On macOS and Linux: create `<app data root>/run/` with mode `0700` if absent, remove a stale `mcp.sock` left by an earlier process (only when no process accepts on it: a connect attempt fails), bind a `tokio::net::UnixListener`, then `chmod 0600`. On Windows: create the named pipe `\\.\pipe\ordin-mcp-<installation id>` with a security descriptor that grants the current user only. A bind failure → `MCP_SOCKET_UNAVAILABLE` (retryable). Log `info` `mcp_front.started` {`endpoint`}.
4. An accept loop runs until `shutdown` is cancelled. Each connection is served in its own task (section D). On shutdown: stop accepting, cancel every session's token (in-flight calls end with `MCP_SHUTTING_DOWN`), remove the socket file. Log `info` `mcp_front.stopped`.

### C. Opening and closing a session

5. `open_session(grant)`:
   1. Not started → `MCP_NOT_STARTED`. A session for `grant.run_id` is open → `MCP_SESSION_EXISTS`.
   2. Mint the token: 32 bytes from `rand::rngs::OsRng`, base64url without padding. Keep only its SHA-256 digest in memory.
   3. For each registered provider, in registration order, read `servers()` and call `offer(&grant)` with a 2 s timeout. A server listed under the provider's claimed prefix that no other provider serves is accepted after its `ToolSpec`s pass the checks of step 2.3 (a failing spec is dropped and logged `warn` `mcp_front.offer_ignored`). Keep only keys whose server the provider claimed at registration or lists under its prefix, and whose tool exists in its current `servers()`; drop others and log `warn` `mcp_front.offer_ignored` {`provider_id`, `server`, `tool`}. A provider error or timeout aborts the open with `MCP_PROVIDER_FAILED` {`provider_id`, `code`}, retryable as the provider's `ToolFailure.retryable` says (a timeout is retryable), and no session is kept: a session never starts with a silently smaller tool set. Log `warn` `mcp_front.open_failed`.
   4. Build the session state: grant, token digest, offered map `ToolKey → (provider index, ToolSpec)`, an empty plan tracker, an empty trimmed-rest store, a `CancellationToken` child of the shutdown token, a semaphore of `max_concurrent_calls_per_session`.
   5. Build `SessionMcpConfig`: one `McpServerLaunch` per server with at least one offered tool, sorted by server name, with `command = hook_program`, `args = ["mcp-bridge", "<server>", "--agent-instance", "<agent_instance_id>"]`, and `env` holding `ORDIN_MCP_ENDPOINT`, `ORDIN_SESSION_ID`, `ORDIN_MCP_SERVER`, `ORDIN_MCP_TOKEN`.
   6. Log `info` `mcp_front.session_opened` {`run_id`, `workspace_id`, `agent_instance_id`, `harness`, `servers`, `tool_count`}. The token is never logged.
6. `close_session(run_id)`: if absent → `Ok(())`. Cancel the session token (its calls end with `MCP_SESSION_CLOSED`), close its bridge connections, drop the state, then call `session_closed(run_id)` on every provider concurrently, waiting at most 5 s in all. Log `info` `mcp_front.session_closed` {`run_id`, `calls`, `denied`, `failed`}.
7. `offered_tools(run_id)` returns the offered map as `OfferedTool`s, sorted by server then tool; unknown → `MCP_SESSION_UNKNOWN`.

### C2. Tool-set changes and registry reads

7a. `tools_changed(server)`: before `start`, or when no registered provider serves `server` (neither claimed nor listed under its prefix), return at once and log `debug` `mcp_front.tools_changed_ignored` {`server`}. Otherwise spawn one task and return. The task, for each open session in turn:
   1. Read the provider's `servers()` and call `offer(&session.grant)` with the 2 s timeout of step 5.3.
   2. Success: keep the returned keys for the servers launched for this session (filtered as in step 5.3); replace the provider's entries in the session's offered map with them. Keys on a server not launched for this session are not added (SA-RECON-02) and are logged `debug` `mcp_front.tools_changed_unlaunched` {`run_id`, `server`}.
   3. Error or timeout: remove the provider's entries from the session's offered map (fail closed: a tool whose grant cannot be confirmed is withdrawn) and log `warn` `mcp_front.tools_changed_failed` {`run_id`, `provider_id`, `code`}.
   4. When the session's tool set on a launched server differs from before, send `notifications/tools/list_changed` on each of that server's bridge connections for the session. A call already in flight for a withdrawn tool runs to its end; a later call for it gets `MCP_TOOL_NOT_OFFERED`.
   5. Log `info` `mcp_front.tools_changed` {`server`, `sessions`, `added`, `removed`}.
   Two `tools_changed` for the same server while a task runs are merged: the task runs once more after it ends.
7b. `served_tools(server)` reads the serving provider's current `servers()` and returns that server's tool names sorted; `None` when no provider serves it. `server_names()` returns the claimed names plus the servers currently listed under each claimed prefix, sorted, and the prefixes.

### D. A bridge connection

8. Peer check (macOS and Linux): the peer's user id (`SO_PEERCRED` on Linux, `getpeereid` on macOS) must equal the process's own; otherwise close without reading and log `warn` `mcp_front.bridge_refused` {`code`: `MCP_BRIDGE_PEER_REFUSED`}.
9. Read the handshake line within 5 s, at most 1 KiB. A timeout, an over-long line or malformed JSON → reply `refused` `MCP_BRIDGE_MALFORMED` and close. `protocol` other than `1` → `MCP_BRIDGE_PROTOCOL_UNSUPPORTED`. Unknown session → `MCP_SESSION_UNKNOWN`. SHA-256 of the presented token compared with the stored digest by a fixed-time XOR fold over the 32 bytes; mismatch → `MCP_BRIDGE_UNAUTHENTICATED`. `server` not among the session's launched servers → `MCP_SERVER_NOT_OFFERED`. Each refusal is logged `warn` `mcp_front.bridge_refused` {`run_id` when parsed, `server`, `code`}; the presented token is never logged.
10. Reply `ready`. Serve MCP JSON-RPC over the connection with an `rmcp` server instance bound to `(session, server)`. Lines longer than 4 MiB (the 1 MiB argument limit plus envelope) close the connection with `MCP_BRIDGE_MALFORMED` logged.
11. `tools/list` answers the server's offered tools (name, description, input schema) in name order.

### E. The call pipeline (`tools/call` and `ToolDispatch::dispatch`)

12. Acquire a permit from the session's semaphore; none within 30 s → result `MCP_TOO_MANY_CALLS` (is_error).
13. Mint a local call id (UUIDv7) for logs and `correlation_id` (`<run_id>:<local call id>`). Start timer. A call that reaches the decision (step 17) takes the Rules Engine's `tool_request_id` as its `call_id`; refusals in steps 14–16 have no tool request and are logged with the local id.
14. **Offered check.** The key must be in the session's offered map; for `dispatch` its spec must also be `sandbox_callable`. Otherwise result `MCP_TOOL_NOT_OFFERED`, text "This tool is not offered to this session." No Rules Engine call.
15. **Size check.** Serialised arguments above 1 MiB (LW-18) → `MCP_ARGUMENTS_TOO_LARGE`, text "The arguments are larger than 1 MiB; pass a reference to the document instead."
16. **Argument check.** `provider.check_arguments(key, args)`; on `ArgumentProblem { at, reason }` → `MCP_ARGUMENTS_INVALID`, text `Wrong arguments for <tool> at <at>: <reason>. Correct form: <argument_form>` — no model call, no Rules Engine call (ADR-062). Log `debug` `mcp_front.arguments_invalid` {`run_id`, `server`, `tool`, `at`}.
17. **Decision.** Build `ToolCall { run_id, agent_instance_id, workspace_id, point: BeforeToolCall, tool: "mcp__<server>__<tool>", origin: OrdinMcpFront { server }, arguments }` and call `rules.decide_tool_call(&call, &CallContext { subagent: None, class: None })` (interaction I-1), which returns `RulesOutcome { decision, tool_request_id, approval_id }` and never fails (an internal error of the engine is a deny, fail closed). The tool name uses the form the harness shows and the hook reports, so one rule matches both routes (C-76); the Rules Engine strips `mcp__<server>__` before matching its lists (SA-RECON-04). The Rules Engine records the `ToolRequest` and the decision itself.
    - `Deny { reason }` → `MCP_DENIED`, text = `reason`.
    - `Ask` with `approval_id` → `rules.wait_for_approval(approval_id, Some(now + approval_wait))` (interaction I-2): `Approved` → continue to step 18; `Rejected` → `MCP_REJECTED`, text "The person rejected this."; `Expired` → `MCP_REJECTED`, text "The approval expired without a decision."; an `Err(RulesError)` → fail closed, `MCP_DECISION_FAILED`, text "This call could not be checked, so it was not run.", log `error` `mcp_front.decision_failed` {`run_id`, `tool`, `code`}; `Pending` → `MCP_AWAITING_APPROVAL`, text "Waiting for the person's approval. Call again with the same arguments once they have decided." The call is not run. When the person approves later, the Rules Engine allows the identical call once (02:651 amendment, C-80); the Session Supervisor resumes the session with the decision (02:522). The session's cancel token cancels the wait.
    - `Allow` → step 18.
18. **Provider call.** Build `ToolCallContext` with `deadline = now + call_timeout` (never later than a harness-imposed deadline when the transport carries one), a child cancel token, a `ProgressSink` for this call, `only_needed_parts` as currently set, `via`, and `caller`, `scope_id`, `coding_task_id` copied from the session's grant. Run `provider.call` under `tokio::time::timeout_at(deadline)`:
    - timeout → cancel the child token, result `MCP_CALL_TIMEOUT`, text "The tool took too long and was stopped."
    - a `notifications/cancelled` for this call or session close → cancel, result `MCP_CALL_CANCELLED` (no answer is sent to a cancelled request, per MCP).
    - `Err(ToolFailure)` → result with the provider's `code` and `message`, `is_error = true`.
    - a provider panic is caught at the task boundary → `MCP_PROVIDER_PANICKED`, text "The tool failed." Log `error` with `provider_id`.
    - `Ok(ToolOutput)` → step 19.
19. **Output limit.** If `only_needed_parts` is on and the text exceeds `trim_above_chars`: keep the first `trim_keep_chars` characters (cut at the last line break before it when one exists within the final 1,000 characters), store the remainder in the session's rest store under `rest:<uuidv7>` (evicting oldest entries above `max_rest_bytes_per_session`), and append one line: `[Cut short. Call read_rest with reference "rest:<id>" and from <offset> to read on.]`. Independently, a text above `max_output_bytes` is cut to that size at a character boundary with the line `[Cut at 4 MiB.]`, logged `warn` `mcp_front.output_truncated`.
20. **Outcome.** For a call allowed in step 17 (directly or after approval): `rules.record_outcome(tool_request_id, executed)` (interaction I-3), `executed = true` when `provider.call` was invoked, `false` when the call was cancelled before it. `record_outcome` returns nothing; the Rules Engine logs its own failures. Refusals in steps 14–17 record nothing here (14–16 have no tool request; a denied or unapproved request is closed by the Rules Engine).
21. Log `info` `mcp_front.call` {`run_id`, `call_id`, `server`, `tool`, `via`, `outcome` (`ok`, `denied`, `rejected`, `awaiting_approval`, `invalid`, `not_offered`, `timeout`, `cancelled`, `failed`), `code`, `duration_ms`, `arguments_digest` (SHA-256 hex of the canonical JSON), `output_chars`}. Never the arguments or the output. Open the span `ordin.tool_call` (Telemetry's name and fields) with `tool` = the tool name, `origin = "ordin_mcp_front"`, `outcome` (`allowed` for `ok`; `refused` for `denied`, `rejected`, `invalid`, `not_offered`; `paused` for `awaiting_approval`; `failed` for `timeout`, `failed`, `cancelled`) and `control` = the deciding rule's id or `none`. In the agent profile call `metrics.call_finished(server, outcome, seconds)` with `Answered` for `ok`, `Refused` for `denied`, `rejected`, `awaiting_approval`, `invalid`, `not_offered`, `Failed` for `timeout`, `failed`, and `Cancelled` for `cancelled`.
22. **Progress.** Each `ProgressSink::report(done, total)`: if `total` is present and `done > total`, drop the report and log `debug`. Otherwise (a) when the call carried a progress token, send `notifications/progress`; (b) at most once per second per call, and always for the last report, emit `ServiceEvent::RunProgress { run_id, progress: RunProgress { line: <ToolSpec.plain cut to 160 characters>, count: Some(ProgressCount { done, total }) } }` with `journal.append_event(EventKind::RunProgress.as_str(), event.to_journal_data())` (interaction I-4). An append error is logged `error` `mcp_front.journal_failed` and the call goes on.

### F. The `work` provider

23. `offer(grant)`: `report` and `plan` when `grant.profile` is `Desktop` or `PersonalServer`; `read_rest` when trimming is on at open time. In the agent profile the `work` server offers `read_rest` only *(assumed here: ADR-067 and ADR-068 tools are not in the hosted agent's kept tool set, 08 "Tools a hosted agent sees")*.
24. `check_arguments`: typed deserialisation into `ReportToolArgs`, `PlanToolArgs` or `ReadRestArgs { reference: String, from: u64 }` with unknown fields refused; the serde error's path becomes `at`.
25. `report`:
    1. Read the bound resolver; none bound → the call fails, not retryable, `MCP_REF_RESOLVER_NOT_BOUND` with text "Reports can't be shown yet." and log `error` `mcp_front.ref_resolver_missing` (SA-CLOSE-01). `blocks.check_report_tool(&raw_arguments, &BlockCheckContext { workspace_id, run_id: Some(run_id), now }, refs.as_ref())` (interaction I-5) → `CheckedReport { block, dropped_parts }`: for a `Report`, failing parts are removed and listed in `dropped_parts`. `Err(rejections)` (no part left, a rejection outside the parts, or any rejection of a `Table`, `Chart`, `KeyFigures`) → the call fails `BLOCK_REPORT_REFUSED` with structured `{"rejections": [...]}` (at most 20) and text naming the first rejection's reason.
    2. For `ReportToolArgs::Report(r)`: mint `ReportId`; `journal.store_report(report_to_domain(&r, report_id, workspace_id, run_id))` (interaction I-6). A single table, chart or key-figures block is shown without a stored report (no `report_id`) *(chosen here: the Journal's `Report` is the multi-part form)*. A store error → `MCP_JOURNAL_FAILED` logged, the call fails retryable with text "The report could not be saved; try again." Mint `BlockId`. Emit `ServiceEvent::BlockShown { workspace_id, block_id, block }` with `journal.append_event(EventKind::BlockShown.as_str(), event.to_journal_data())`; when the event's serialised data would exceed 256 KiB, emit `BlockAvailable { workspace_id, block_id }` instead (event kind `block_available`, SA-RECON-07), and the screens fetch the block by its id.
    3. Answer `Shown: <title>.` (for a single table or chart: its title; for key figures: "Key figures"), structured `{"block_id", "report_id" (only for a report), "left_out": dropped_parts}`. When parts were left out the text adds "<n> part(s) left out; see left_out." so Ordin can say so in one line (11:727).
26. `plan`:
    0. No resolver bound → the call fails as step 25.1 (`MCP_REF_RESOLVER_NOT_BOUND`). `blocks.check_plan_tool(&raw_arguments, run_id, refs.as_ref())` (interaction I-5) applies the Service API's rules: 3–7 steps of 1–80 characters (`BLOCK_PLAN_INVALID`), at most 20 plan versions (`BLOCK_PLAN_TOO_MANY_VERSIONS`), a mark position within the current plan (`BLOCK_PLAN_POSITION_INVALID`). Any rejection fails the call with that code and the rejection's reason.
    1. `State { steps }`: `journal.state_plan(run_id, steps → PlannedStep { line, came_from: StepSource::AgentsPlan })` (interaction I-7) returns the new plan's `RunStep`s; the new `plan_version` is their `plan_version`; the Journal emits `plan_changed`. A plan stated after another tool call is accepted (ADR-068 asks the agent to state it first; the progress screen shows what was seen either way).
    2. `Mark { position, mark }`: no plan yet → `MCP_PLAN_NOT_STATED`; `mark = now` while another step is `now` → `MCP_PLAN_ONE_NOW`, text "Mark step <p> done or skipped first."; otherwise `journal.mark_step(run_id, position as u32, mark as StepMark)` (interaction I-8; it acts on the highest plan version and emits `step_marked`). A Journal refusal `RSTEP_IMMUTABLE` is answered as `MCP_PLAN_ONE_NOW`.
    3. The tracker (current version, step count, which step is `now`) is held in the session state and rebuilt from `journal.current_plan(run_id)` (interaction I-9; an empty list means no plan) when a session is opened for a run that already has a plan (a resumed run).
27. `read_rest`: reference absent from the session's store (never issued, evicted, or another session's) → `MCP_REST_UNKNOWN`, text "That reference has expired; ask for the document again."; `from` beyond the end → `MCP_REST_OFFSET_INVALID`. Otherwise return up to `trim_keep_chars` characters from `from`, plus a final line `[Next: from <n>]` or `[End.]`.

### G. The sandbox module

28. `session_tools(run_id)`: unknown run → `MCP_SESSION_UNKNOWN`. Otherwise the session's offered map filtered to specs with `sandbox_callable = true`, as `(ToolKey, ToolSpec)` pairs sorted by server then tool. The MCP Front generates no module text: the Sandbox builds the `ordin_tools` module from this list (SA-RECON-05, interaction I-10).
29. `dispatch(run_id, key, arguments, cancel)`: unknown run → `MCP_SESSION_UNKNOWN`. Otherwise steps 12–22 with `via = Sandbox` and `cancel` as the parent of the call's cancel token; the Rules Engine sees the same `ToolCall` as for a harness call (ADR-062: "under the same Rules Engine decision").

## Data Model

N/A — the MCP Front owns no table. Its only state is per session and in memory: the token digest, the offered tool map, the plan tracker (rebuilt from the Journal, step 26.3) and trimmed remainders, all dropped at `close_session` or process exit. Tool requests, decisions, plans, marks and reports are written through the Journal, which owns those tables (2C domain `ToolRequest`, `RunStep`, `Report`). No migration is registered.

## Error Table

In-process API errors (`McpError`) and tool-call results (`ToolCallResult.is_error = true`, sent to the harness as MCP `isError: true`). The person never sees a code (07); tool-call texts are for the model.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `ServerName::new` given a value breaking `^[a-z][a-z0-9_]{0,31}$` | `Err(permanent)` `McpError::InvalidName` | `MCP_INVALID_NAME` | `McpError::InvalidName { value }` |
| `register` after `start` | `Err(permanent)` `McpError::RegistryFrozen` | `MCP_REGISTRY_FROZEN` | `McpError::RegistryFrozen` |
| Same `provider_id` registered twice | `Err(permanent)` `McpError::ProviderDuplicate` | `MCP_PROVIDER_DUPLICATE` | `{ provider_id }` |
| A server name already claimed by another provider, or a name or prefix that overlaps another provider's prefix | `Err(permanent)` `McpError::ServerClaimed` | `MCP_SERVER_CLAIMED` | `{ server, by }` |
| A `ToolSpec` breaks a rule of step 2.3 (name pattern, description length for the profile, `argument_form`, `plain`, schema), or the server prefix breaks its pattern | `Err(permanent)` `McpError::ToolSpecInvalid` | `MCP_TOOL_SPEC_INVALID` | `{ provider_id, tool, rule }` |
| A server under a provider's prefix appears after `start` with a `ToolSpec` that breaks step 2.3 | spec dropped, not offered | `MCP_TOOL_SPEC_INVALID` | log only (`warn` `mcp_front.offer_ignored`) |
| `tools_changed`: the provider's `offer` fails or exceeds 2 s for an open session | that provider's tools withdrawn from the session (fail closed) | the provider's code, or `MCP_PROVIDER_FAILED` on timeout | log only (`warn` `mcp_front.tools_changed_failed`); `notifications/tools/list_changed` sent |
| `tools_changed` for a server no provider serves, or before `start` | no effect | — | log only (`debug` `mcp_front.tools_changed_ignored`) |
| `bind_ref_resolver` called a second time | `Err(permanent)` `McpError::RefResolverAlreadyBound` | `MCP_REF_RESOLVER_ALREADY_BOUND` | `McpError::RefResolverAlreadyBound` |
| A `report` or `plan` call before the program bound the resolver | tool call fails, not retryable | `MCP_REF_RESOLVER_NOT_BOUND` | tool text "Reports can't be shown yet." (`report`), "The plan can't be recorded yet." (`plan`) |
| `start` called twice | `Err(permanent)` `McpError::AlreadyStarted` | `MCP_ALREADY_STARTED` | `McpError::AlreadyStarted` |
| Socket or pipe cannot be bound | `Err(retryable)` `McpError::SocketUnavailable` | `MCP_SOCKET_UNAVAILABLE` | `{ detail }` (OS error kind, no path outside the data root) |
| `open_session` before `start` | `Err(retryable)` `McpError::NotStarted` | `MCP_NOT_STARTED` | `McpError::NotStarted` |
| `open_session` for a run with an open session | `Err(permanent)` `McpError::SessionExists` | `MCP_SESSION_EXISTS` | `{ run_id }` |
| A provider's `offer` fails or exceeds 2 s | `Err(retryable when the provider said so)` `McpError::ProviderFailed` | `MCP_PROVIDER_FAILED` | `{ provider_id, code }` |
| `offered_tools` / `session_tools` / `dispatch` for an unknown run | `Err(permanent)` `McpError::SessionUnknown` | `MCP_SESSION_UNKNOWN` | `{ run_id }` |
| Bridge peer is another OS user | connection closed | `MCP_BRIDGE_PEER_REFUSED` | none (closed before reading) |
| Handshake missing within 5 s, over 1 KiB, not JSON | `{"kind":"refused"}` | `MCP_BRIDGE_MALFORMED` | `{"kind":"refused","code":"MCP_BRIDGE_MALFORMED"}` |
| Handshake `protocol` ≠ 1 | `{"kind":"refused"}` | `MCP_BRIDGE_PROTOCOL_UNSUPPORTED` | `{"kind":"refused","code":"MCP_BRIDGE_PROTOCOL_UNSUPPORTED"}` |
| Handshake session not open | `{"kind":"refused"}` | `MCP_SESSION_UNKNOWN` | `{"kind":"refused","code":"MCP_SESSION_UNKNOWN"}` |
| Handshake token does not match | `{"kind":"refused"}` | `MCP_BRIDGE_UNAUTHENTICATED` | `{"kind":"refused","code":"MCP_BRIDGE_UNAUTHENTICATED"}` |
| Handshake server not launched for the session | `{"kind":"refused"}` | `MCP_SERVER_NOT_OFFERED` | `{"kind":"refused","code":"MCP_SERVER_NOT_OFFERED"}` |
| More than 64 calls in flight for 30 s | `isError` | `MCP_TOO_MANY_CALLS` | text "Too many calls at once; wait for one to finish." |
| Tool not offered to the session (or not sandbox-callable from the sandbox) | `isError` | `MCP_TOOL_NOT_OFFERED` | text "This tool is not offered to this session." |
| Arguments above 1 MiB | `isError` | `MCP_ARGUMENTS_TOO_LARGE` | text "The arguments are larger than 1 MiB; pass a reference to the document instead." |
| `check_arguments` refuses | `isError` | `MCP_ARGUMENTS_INVALID` | text `Wrong arguments for <tool> at <at>: <reason>. Correct form: <argument_form>` |
| Waiting for the person's approval fails (`wait_for_approval` returns `Err`) | `isError` (fail closed) | `MCP_DECISION_FAILED` | text "This call could not be checked, so it was not run." |
| The approval expired without a decision | `isError` | `MCP_REJECTED` | text "The approval expired without a decision." |
| `report`: `journal.store_report` fails | `isError` (retryable) | `MCP_JOURNAL_FAILED` | text "The report could not be saved; try again." |
| Rules Engine denies | `isError` | `MCP_DENIED` | text = the rule's reason |
| The person rejects | `isError` | `MCP_REJECTED` | text "The person rejected this." plus the reason |
| No decision within `approval_wait` | `isError` | `MCP_AWAITING_APPROVAL` | text "Waiting for the person's approval. Call again with the same arguments once they have decided." |
| Provider call exceeds `call_timeout` | `isError` | `MCP_CALL_TIMEOUT` | text "The tool took too long and was stopped." |
| Call cancelled by the harness or session close | no answer (MCP cancellation) | `MCP_CALL_CANCELLED` / `MCP_SESSION_CLOSED` | none; journalled and logged |
| Program shutting down | `isError` | `MCP_SHUTTING_DOWN` | text "ordin is stopping; try again after it restarts." |
| Provider returns `ToolFailure` | `isError` | the provider's code | text = `ToolFailure.message` |
| Provider panics | `isError` | `MCP_PROVIDER_PANICKED` | text "The tool failed." |
| `report` arguments fail the block check entirely | `isError` | `BLOCK_REPORT_REFUSED` | text = first rejection's reason; structured `{"rejections":[BlockRejection…]}` |
| `plan` state breaks 3–7 steps or 1–80 characters | `isError` | `BLOCK_PLAN_INVALID` | text naming the rule and the correct form |
| A 21st plan version | `isError` | `BLOCK_PLAN_TOO_MANY_VERSIONS` | text "This run already has 20 plans; mark the current one." |
| `plan` mark with no plan | `isError` | `MCP_PLAN_NOT_STATED` | text "State your steps first with plan(action: \"state\")." |
| `plan` mark position outside the plan | `isError` | `BLOCK_PLAN_POSITION_INVALID` | text "The plan has <n> steps." plus the correct form |
| `plan` marks a second step `now` | `isError` | `MCP_PLAN_ONE_NOW` | text "Mark step <p> done or skipped first." |
| `read_rest` reference unknown or evicted | `isError` | `MCP_REST_UNKNOWN` | text "That reference has expired; ask for the document again." |
| `read_rest` offset beyond the end | `isError` | `MCP_REST_OFFSET_INVALID` | text "That offset is past the end (<n> characters)." |
| Journal write fails after a call | answer unchanged | `MCP_JOURNAL_FAILED` | log only (`error`) |
| Output above 4 MiB | answer cut | `MCP_OUTPUT_TRUNCATED` | log only (`warn`); text ends "[Cut at 4 MiB.]" |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: MCP Front

  Background:
    Given an MCP Front started with the providers "sandbox" (server sandbox, tool python_sandbox, sandbox_callable false)
      and "memory-service" (server memory, tools recall and remember, sandbox_callable true)
      and the built-in work provider
    And a Rules Engine double

  Scenario: Happy path — a session is offered only its tools and a call runs
    Given the memory provider offers recall only to agent instance "A"
    When open_session is called for run R1 of agent instance "A" on Claude Code in the desktop profile
    Then the returned config lists the servers "memory", "sandbox" and "work", each with command ordin-hook and args ["mcp-bridge", "<server>", "--agent-instance", "A"]
    And a bridge for server "memory" that presents R1's token gets {"kind":"ready"}
    And tools/list on it returns exactly ["recall"]
    When the bridge sends tools/call recall with {"query":"Q3 rates"} and the Rules Engine allows "mcp__memory__recall"
    Then the memory provider's call runs once with a context whose run_id is R1
    And the answer has isError false
    And the log line mcp_front.call has outcome "ok" and an arguments_digest, and holds neither the query nor the answer text

  Scenario: Edge — registration after start is refused
    Given the MCP Front has started
    When the program registers a provider "late"
    Then register returns MCP_REGISTRY_FROZEN

  Scenario: Edge — two providers claim the same server
    When a provider "other" registers a server named "memory"
    Then register returns MCP_SERVER_CLAIMED naming "memory-service"

  Scenario: Error — a bridge with the wrong token is refused
    Given session R1 is open
    When a bridge connects with session R1, server "memory" and a token that is not R1's
    Then the core answers {"kind":"refused","code":"MCP_BRIDGE_UNAUTHENTICATED"} and closes
    And the presented token appears in no log line

  Scenario: Error — wrong arguments are answered without a decision
    Given session R1 is open
    When tools/call plan is sent with {"action":"mark","position":9,"mark":"now"} before any plan exists
    Then the answer has isError true and code MCP_ARGUMENTS_INVALID or MCP_PLAN_NOT_STATED
    And the Rules Engine double received no call

  Scenario: Error — a denied call is not run
    Given the Rules Engine denies "mcp__sandbox__python_sandbox" with reason "Code may not reach the network."
    When tools/call python_sandbox is sent
    Then the sandbox provider's call is not invoked
    And the answer has isError true with text "Code may not reach the network."

  Scenario: Edge — an ask not decided within 50 seconds
    Given the Rules Engine answers ask with approval A1 and wait_for_approval returns Pending after 50 s
    When tools/call remember is sent
    Then after 50 s the answer has isError true and code MCP_AWAITING_APPROVAL
    And the memory provider's call is not invoked

  Scenario: Happy path — an approved ask runs the call
    Given the Rules Engine answers ask with approval A2 and wait_for_approval returns Approved after 3 s
    When tools/call remember is sent
    Then the memory provider's call runs once and the answer has isError false

  Scenario: Error — waiting for an approval fails, the call is denied
    Given the Rules Engine answers ask with approval A3 and wait_for_approval returns an error
    When tools/call remember is sent
    Then the answer has code MCP_DECISION_FAILED and the provider is not called

  Scenario: Edge — a provider times out
    Given call_timeout is 2 s and the memory provider's call sleeps 5 s
    When tools/call recall is allowed
    Then after 2 s the answer has code MCP_CALL_TIMEOUT and the call's cancel token is cancelled

  Scenario: Happy path — a plan is stated and marked
    When tools/call plan is sent with {"action":"state","steps":["Read the supplier quotes","Compare the Q3 rates","Write the summary"]}
    Then journal.state_plan is called with 3 steps whose came_from is agents_plan
    And the answer text is "Plan noted: 3 steps."
    When tools/call plan is sent with {"action":"mark","position":1,"mark":"now"}
    And tools/call plan is sent with {"action":"mark","position":2,"mark":"now"}
    Then the second answer has code MCP_PLAN_ONE_NOW

  Scenario: Edge — a report with one failing part is shown without it
    Given the block check rejects /parts/1/content/source of a 3-part report
    When tools/call report is sent with that report
    Then journal.store_report receives a report with 2 parts
    And a BlockShown event is emitted
    And the answer's structured left_out holds one rejection

  Scenario: Edge — a report too large for one event is announced by id
    When tools/call report is sent with a report whose block serialises to 300 KiB
    Then a block_available event with the workspace_id and block_id is emitted
    And no block_shown event is emitted for it

  Scenario: Edge — long output is trimmed with a reference
    Given only-needed-parts is on and the recall provider returns 50,000 characters
    When tools/call recall is allowed
    Then the answer text has 12,000 characters or fewer before its final line
    And the final line names a reference "rest:<id>"
    When tools/call read_rest is sent with that reference and from 12000
    Then the next part of the text is returned

  Scenario: Happy path — progress counts are relayed
    Given the sandbox provider reports progress 12 of 40 during a call that carried a progress token
    Then the bridge receives notifications/progress with progress 12 and total 40
    And journal.append_event receives kind "run_progress" with count {"done":12,"total":40}

  Scenario: Happy path — the Sandbox is given only sandbox-callable tools
    When session_tools is called for R1
    Then it returns memory/recall and no entry for sandbox/python_sandbox
    And dispatch for memory/recall reaches the same Rules Engine decision as a harness call, with via Sandbox

  Scenario: Happy path — a grant made during a session reaches it
    Given session R1 is open with the mail server launched and offering email_search only
    When the mail provider starts offering email_read to R1 and calls tools_changed("mail")
    Then the mail bridge connection of R1 receives notifications/tools/list_changed
    And tools/list on it returns ["email_search", "email_read"]

  Scenario: Edge — a tools_changed whose offer fails withdraws the provider's tools
    Given session R1 is open with the mail server offering email_search
    When the mail provider's offer fails and tools_changed("mail") is called
    Then tools/list on R1's mail connection returns []
    And a later tools/call email_search answers MCP_TOOL_NOT_OFFERED

  Scenario: Edge — a server added under a claimed prefix after start
    Given a provider "craft-toolkit" registered with server_prefix "craft_" and no servers
    And the MCP Front has started
    When the provider lists server "craft_artcraft" with tool "convert" and session R2 is opened
    Then R2's config launches server "craft_artcraft"
    And served_tools("craft_artcraft") returns ["convert"]

  Scenario: Edge — a wrightrun tool name and long description register in the agent profile
    Given an MCP Front configured with profile AgentProfile
    When a provider registers server "agent" with tool "getTicket" and a description of 900 characters
    Then register returns Ok
    And the same registration in the desktop profile returns MCP_TOOL_SPEC_INVALID with rule "description"

  Scenario: Happy path — the grant's caller and scope reach the provider
    Given session R3 is opened with caller CrewMember of crew run C1 and scope_id None
    When tools/call recall is allowed for R3
    Then the memory provider's call context has caller CrewMember with crew_run_id C1 and scope_id None

  Scenario: Edge — closing a session cancels its calls and tells providers
    Given a recall call is in flight for R1
    When close_session R1 is called
    Then the call's cancel token is cancelled
    And every provider's session_closed is called with R1
    And a later bridge handshake for R1 is refused with MCP_SESSION_UNKNOWN

  Scenario: The ref resolver is bound once
    Given the program bound its RefResolver with bind_ref_resolver
    When bind_ref_resolver is called again
    Then it fails with MCP_REF_RESOLVER_ALREADY_BOUND

  Scenario: A report before the resolver is bound
    Given no RefResolver has been bound
    When a session calls work.report with a valid table block
    Then the call fails with MCP_REF_RESOLVER_NOT_BOUND and is not retryable
    And no block is shown
```

## Performance, Security, Observability

- **Performance targets.**
  - `open_session` p95 ≤ 50 ms excluding providers' `offer` time, with 13 providers (*amended January 2026, ADR-102*) and 200 tools; each `offer` must answer within 2 s.
  - Pipeline overhead per `tools/call` (steps 12–17 and 19–21, excluding the Rules Engine and the provider) p95 ≤ 5 ms for arguments ≤ 64 KiB.
  - Bridge handshake p95 ≤ 10 ms; relayed line latency p95 ≤ 2 ms.
  - Progress: at most one `RunProgress` journal event per second per call; first progress event reaches the Journal within 1 s of the provider's report (07 — NFR "first progress event within 1 s").
  - Memory: ≤ 64 KiB per open session excluding trimmed remainders; remainders ≤ 16 MiB per session; 8 concurrent sessions on the desktop (A-04) and up to 10 per run instance in the agent profile (A-49) → under 140 MiB worst case.
- **Security.**
  - Entry points: the bridge socket (peer user check, per-session token compared in fixed time, protocol check, server must be launched for the session) and the in-process API (callers are the program and crates that depend on this one).
  - Every call to an ordin-served tool is decided by the Rules Engine before a provider acts (09 ADR-037), including calls from agent-written code in the sandbox (09 ADR-062). A Rules Engine failure denies (fail closed).
  - Input validation: size ≤ 1 MiB, provider's typed check with unknown fields refused (SA-API-02, 07 — Input validation), offered-tool check.
  - The socket is mode `0600` in a `0700` folder under the application data root; on Windows the pipe admits the current user only (ADR-072: local channels in the application data root). No TCP port is opened.
  - Never logged or journalled: the session token, tool arguments (a SHA-256 digest only), tool output, report contents, plan step texts (07 — Logging, SA-XC-03).
  - Tool descriptions are one line and are the provider's own text; tool output is passed as data and never interpreted by the MCP Front.
  - No `resources/*` or `prompts/*` methods are served, so no MCP Apps interface (`ui://`) reaches a harness from the MCP Front (ADR-067).
- **Observability.**
  - Logs (`tracing`, JSON): `mcp_front.provider_registered` {provider_id, servers, tool_count}; `mcp_front.started` {endpoint}; `mcp_front.stopped`; `mcp_front.session_opened` {run_id, workspace_id, agent_instance_id, harness, servers, tool_count}; `mcp_front.session_closed` {run_id, calls, denied, failed}; `mcp_front.open_failed` {run_id, provider_id, code}; `mcp_front.offer_ignored` {provider_id, server, tool}; `mcp_front.bridge_refused` {run_id?, server?, code}; `mcp_front.arguments_invalid` {run_id, server, tool, at}; `mcp_front.decision_failed` {run_id, tool, code}; `mcp_front.call` {run_id, call_id, server, tool, via, outcome, code, duration_ms, arguments_digest, output_chars}; `mcp_front.output_truncated` {run_id, call_id, bytes}; `mcp_front.journal_failed` {run_id, call_id, code}; `mcp_front.tools_changed` {server, sessions, added, removed}; `mcp_front.tools_changed_failed` {run_id, provider_id, code}; `mcp_front.tools_changed_ignored` {server}; `mcp_front.tools_changed_unlaunched` {run_id, server}. Every line carries `component = "mcp_front"` and `correlation_id` where a call is in scope.
  - Metrics: none on the desktop and the personal server (SA-XC-04). Agent profile only (SA-OBS-02): Telemetry's families 48 `wrightrun_mcp_front_calls_total{server, outcome}` and 49 `wrightrun_mcp_front_call_seconds{server}`, fed through `McpMetrics::call_finished`, which the server program implements over `Telemetry::record(TelemetryEvent::McpFrontCall { .. })` (ruling R4). This crate registers no metric.
  - Tracing: none on the desktop and the personal server (SA-XC-05). Agent profile, when switched on (SA-OBS-03): span `ordin.tool_call` with Telemetry's fields `tool`, `origin` (`ordin_mcp_front`), `outcome`, `control` (step 21); no argument, output or person's identifier.

## Interactions with other components (boundary signatures this spec requires)

| # | Caller → callee | Signature required | Defined in |
|---|---|---|---|
| I-1 | MCP Front → Rules Engine | `RulesEngine::decide_tool_call(&self, call: &ToolCall, ctx: &CallContext) -> RulesOutcome`; `RulesOutcome { decision: ToolDecision, tool_request_id: ToolRequestId, approval_id: Option<ApprovalId> }` | RulesEngine.md |
| I-2 | MCP Front → Rules Engine | `RulesEngine::wait_for_approval(&self, approval_id: ApprovalId, until: Option<tokio::time::Instant>) -> Result<ApprovalWait, RulesError>`; `ApprovalWait { Approved, Rejected, Expired, Pending }` | RulesEngine.md |
| I-3 | MCP Front → Rules Engine | `RulesEngine::record_outcome(&self, tool_request_id: ToolRequestId, executed: bool)` | RulesEngine.md |
| I-4 | MCP Front → Journal | `Journal::append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>` with `EventKind::as_str()` and `ServiceEvent::to_journal_data()` | JournalAndArtefactStore.md; 2C wire; ServiceApi.md |
| I-5 | MCP Front → Service API | `BlockCatalogueCheck::check_report_tool(&raw, &BlockCheckContext, &dyn RefResolver) -> Result<CheckedReport, Vec<BlockRejection>>`; `check_plan_tool(&raw, run_id, &dyn RefResolver) -> Result<PlanToolArgs, Vec<BlockRejection>>`; `report_to_domain`; `RefResolver` implemented by the program | ServiceApi.md |
| I-6 | MCP Front → Journal | `Journal::store_report(&self, report: Report) -> Result<(), JournalError>` | JournalAndArtefactStore.md |
| I-7 | MCP Front → Journal | `Journal::state_plan(&self, run_id: RunId, steps: Vec<PlannedStep>) -> Result<Vec<RunStep>, JournalError>` (emits `plan_changed`) | JournalAndArtefactStore.md |
| I-8 | MCP Front → Journal | `Journal::mark_step(&self, run_id: RunId, position: u32, mark: StepMark) -> Result<RunStep, JournalError>` (emits `step_marked`) | JournalAndArtefactStore.md |
| I-9 | MCP Front → Journal | `Journal::current_plan(&self, run_id: RunId) -> Result<Vec<RunStep>, JournalError>` | JournalAndArtefactStore.md |
| I-10 | Sandbox → MCP Front | `ToolDispatch::session_tools` and `ToolDispatch::dispatch` (above); the Sandbox generates the `ordin_tools` module and routes its calls to `dispatch` | mine |
| I-11 | Claude / Copilot Local Adapters → MCP Front | `open_session`, `close_session`, `offered_tools`, `SessionMcpConfig::to_mcp_servers_json` | mine |
| I-12 | ordin hook (`apps/hook`) → MCP Front | `ordin-hook mcp-bridge <server> --agent-instance <uuid>` (command line owned by OrdinHook, SA-RECON-03) reads `ORDIN_MCP_ENDPOINT`, `ORDIN_SESSION_ID`, `ORDIN_MCP_SERVER`, `ORDIN_MCP_TOKEN`, sends the handshake line, then relays stdio | mine (protocol); needs the hook program to implement |
| I-13 | every provider → MCP Front | `ToolProvider` (above); registered by the program before `start` | mine |
| I-14 | Mail Service, Telegram Service, Slack Service, Craft Toolkit, Project Service → MCP Front | `ToolRegistry::tools_changed(&self, server: &ServerName)` | mine |
| I-15 | Agent Loader → MCP Front | `ToolRegistry::served_tools(&self, server: &ServerName) -> Option<Vec<String>>` | mine |
| I-16 | program → MCP Front → Rules Engine | `McpFront::server_names(&self) -> (Vec<ServerName>, Vec<&'static str>)`, passed by the program to the Rules Engine's list of ordin servers | mine; the Rules Engine's input is its own |

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

The Slack Service (`SlackService.md`, server `slack`, SA-NAME-08) is a provider of this hub. The sections above name it in place:

- **Names fixed in Phase 1 and the HLD:** the *MCP server names* row lists `slack` beside `mail` and `telegram`, sourced from SA-NAME-08.
- **Registration order:** the Slack Service registers after the Telegram Service and before the Authoring Service, in the desktop and the personal-server programs only (2B-3: D P).
- **Called-by table:** the Slack Service calls `ToolRegistry::tools_changed` on link, sign-in, grant and profile changes, and implements `ToolProvider`.
- **Interactions table:** I-14 includes the Slack Service.
- **`server_names()`:** unchanged in code; it lists the servers of the registered providers, so `slack` appears in the first list in the programs that register the Slack Service, and the Rules Engine receives it as an ordin server (RulesEngine.md `RulesConfig.ordin_servers`).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: the Slack Service's server is registered and named
    Given the desktop program registers the Slack Service's provider after the Telegram Service's
    When start is called
    Then server_names() lists "slack" among the ordin servers
    And a session whose workspace links a Slack channel is offered the tools the Slack Service's offer returns
```

### Gaps

None.

---

## Amendment January 2026 — Reading other teams' knowledge (ADR-110)

> Source: decision 90 (the user's decisions of January 2026); HLD revision 0.59.0, ADR-110 (amends ADR-103: one own folder, any number of read folders; adds Adopt and folders the company names). Owners of the tools: `components/MemoryService.md` (server `memory`, `recall` schema amended) and `components/OrdinHost.md` (server `ordin`, `recall` schema amended).
> Nothing above changes. This amendment records the one assertion ADR-110 places on the MCP Front.

### Purpose (addition)

**Adopt** — copying one item from another team's folder into the workspace's own folder — is a person's action in the interface only. The MCP Front serves no tool that adopts, so no harness session, crew member, hosted agent or Ordin can copy one team's knowledge into another team's folder, even when content it read asks for exactly that.

### Approach (addition)

The Front serves what registered providers list and adds no tool of its own. The `recall` tools of servers `memory` and `ordin` gain `searches` and `folders` in their input schemas; the Front carries each provider's schema unchanged in `tools/list` and dispatches as before. No provider registered by the desktop or the personal-server program lists a tool that reaches the Team Knowledge Service's `adopt`, `adopt_again` or `keep_as_is`; the Team Knowledge Service registers no provider.

### Shared Context, Public Interface, Internal Logic, Data Model, Error Table

N/A — no type, method, table or code changes.

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: No served tool adopts
    Given the desktop program started with every provider it registers
    When the registered tool names of every server are listed
    Then none of them is, or reaches, team_knowledge.adopt, team_knowledge.adopt_again or team_knowledge.keep_as_is
    And no provider_id is "team-knowledge"

  Scenario: The amended recall schema reaches the session unchanged
    Given a crew-member session offered memory/recall
    When the session sends tools/list on server memory
    Then the recall inputSchema has properties query, searches, folders and result_cap and additionalProperties false
```

### Performance, Security, Observability (additions)

N/A — no change.

### Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (amends ADR-030: every agent session remembers through the one memory, under its own memory rules). Owners of the tools: `components/MemoryService.md` (server `memory`: `remember` schema amended, offered to agent task sessions with memory rules) and `components/OrdinHost.md` (server `ordin`: Ordin's `remember` schema amended). Owner of the rules' registration: `components/SessionSupervisor.md` (amendment January 2026).
> Nothing above changes in the Front's own logic. This amendment records what ADR-111 places on the MCP Front.

### Purpose (addition)

Agent task sessions — a single agent Ordin delegates to, outside a crew — may now be offered the `memory` server's `recall` and `remember`, so every agent session remembers through the one memory (open question 1, settled "yes, every agent gets memory"). Whether a session gets them stays the Memory Service's decision in `offer`; the Front carries the decision and the amended schemas unchanged.

### Approach (addition)

The Front already asks every provider's `offer(&grant)` at session open (step 5.3) and serves exactly the keys returned. The Memory Service's `offer` now returns the two `memory` keys for `ToolCaller::Agent` when memory rules are registered for `grant.agent_instance_id`, and nothing otherwise; the Session Supervisor registers the rules before it starts the run, so they are in place when the adapter calls `open_session`. The Rules Engine still decides every call (`mcp__memory__remember`, `mcp__memory__recall`) as for a crew member. Rejected: *a Front-level switch that adds memory to every session* — it would bypass the agent's own memory rules (ADR-090 item 3), which decide whether an agent keeps facts at all.

### Shared Context, Public Interface, Internal Logic, Data Model, Error Table

N/A — no type, method, table or code changes in the Front. The `remember` input schema served on server `memory` gains `entities` (0–16 × `{name, other_names?}`) and `links` (0–16 × `{from, kind, to, ended?, confidence_permille?}`), with `additionalProperties: false` at every level; the `ordin` server's `remember` also gains `made_from`. The `recall` result gains `recall_id` and `links`. The Front's argument check (step 16) applies whatever schema the provider lists.

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: An agent task session with memory rules is offered memory
    Given memory rules registered for agent instance "A" by the Session Supervisor
    When the Claude Local Adapter opens a session with caller Agent and agent_instance_id "A"
    Then the returned config lists the server "memory" beside "sandbox" and "work"
    And tools/list on server memory returns recall and remember

  Scenario: An agent task session without memory rules is not offered memory
    Given no memory rules registered for agent instance "B"
    When a session opens with caller Agent and agent_instance_id "B"
    Then the returned config lists no server "memory"

  Scenario: The amended remember schema reaches the session unchanged
    Given a crew-member session offered memory/remember
    When the session sends tools/list on server memory
    Then the remember inputSchema has properties statement, about, fact_class, source, entities and links and additionalProperties false

  Scenario: Error — a link object with an unknown field is refused before the provider
    Given an agent session offered memory/remember
    When it calls remember with links [{"from":"Borealis","kind":"supplies","to":"packaging","why":"x"}]
    Then the answer is MCP_ARGUMENTS_INVALID naming "/links/0/why" and the provider's call is not invoked
```

### Performance, Security, Observability (additions)

- **Performance:** no change; `offer` keeps its 2 s timeout.
- **Security:** an agent session receives memory tools only when its package's memory rules are registered; every call still passes the Rules Engine and the provider's own class checks.
- **Observability:** no change; calls are counted by the existing families 48 and 49 with `server="memory"`.

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents (ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-113 (a desktop's recall may also ask the workspace's front doors; knowledge agents serve a read-only MCP endpoint of their own). Owners of the tools: `components/MemoryService.md` (server `memory`: `recall` schema gains `wider`) and `components/OrdinHost.md` (server `ordin`: `recall` schema gains `wider`). Owner of the knowledge MCP endpoint: `components/KnowledgeQueryServer.md`, mounted by `components/CallerApi.md` (route 39).
> Nothing above changes in the Front's own logic. This amendment records what ADR-113 places on the MCP Front.

### Purpose (addition)

A session that recalls may ask for the answer to include what the workspace's knowledge agents hold, by setting `wider: true` on `recall`; without it, the Memory Service decides from the local result (ADR-113). The MCP endpoint that knowledge agents serve to outside MCP clients is **not** part of the MCP Front: the Front serves ordin's own agent sessions through the local socket the MCP bridge connects to (SA-MCP-01), and it opens no TCP port, in any program or profile.

### Approach (addition)

The Front carries each provider's schema unchanged in `tools/list` and dispatches as before, so the added property needs no change here. The knowledge endpoint lives in the Knowledge Query Server and is served on the Caller API's port, under the Caller API's token check; keeping it out of the Front keeps SA-MCP-01 (no HTTP serving, no TCP port) and keeps the Front's grant model — sessions opened by ordin, tools offered per session — separate from a public, read-only endpoint whose callers ordin does not start. Rejected: *serving the knowledge tools through the Front over HTTP* — SA-MCP-01 rejected HTTP for the Front because it opens a TCP port, and the knowledge endpoint's callers are not ordin sessions with a grant.

Assumptions (*chosen here*):

- **SA-MCP-KA-01** No provider registered with the Front lists `knowledge_search` or `knowledge_contents`; an agent session on a desktop reaches knowledge agents only through `recall` (with `wider`), which sends the query and nothing else from the conversation.
- **SA-MCP-KA-02** The Rules Engine decides `mcp__memory__recall` and `mcp__ordin__recall` as before, whatever `wider` holds: asking a front door is part of recall, not a separate action needing approval, because only the query leaves the computer and the person added the front door.

### Shared Context, Public Interface, Internal Logic, Data Model, Error Table

N/A — no type, method, table or code changes in the Front. The `recall` input schema served on servers `memory` and `ordin` gains one optional property, `wider` (boolean; `true` asks the workspace's front doors whatever the local result; absent or `false` leaves the choice to the Memory Service), with `additionalProperties: false` kept. The `recall` result gains the knowledge agents' facts and which agents did not answer in time (MemoryService.md, amendment January 2026 — knowledge agents). The Front's argument check (step 16) applies whatever schema the provider lists.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: MCP Front — recall that may look wider

  Scenario: The amended recall schema reaches the session unchanged
    Given a crew-member session offered memory/recall
    When the session sends tools/list on server memory
    Then the recall inputSchema has properties query, searches, folders, result_cap and wider and additionalProperties false
    And wider has type boolean

  Scenario: Error — wider of the wrong type is refused before the provider
    Given an agent session offered memory/recall
    When it calls recall with {"query": "Borealis payment terms", "wider": "yes"}
    Then the answer is MCP_ARGUMENTS_INVALID naming "/wider" and the provider's call is not invoked

  Scenario: No served tool is a knowledge agent's endpoint tool
    Given the desktop program started with every provider it registers
    When the registered tool names of every server are listed
    Then none of them is knowledge_search or knowledge_contents

  Scenario: The Front opens no TCP port in the agent profile
    Given the agent-profile program started with a knowledge-agent section in its configuration
    When the listening sockets of the process are listed
    Then the only TCP listener is the Caller API's port, and the MCP Front listens on its local socket only
```

### Performance, Security, Observability (additions)

- **Performance:** no change in the Front; the time a wider recall takes is the Memory Service's (first deadline 1,000 ms; MemoryService.md).
- **Security:** SA-MCP-01 holds in every profile; a session sends only its query through `recall`, and every call still passes the Rules Engine.
- **Observability:** no change; calls are counted by the existing families 48 and 49.

### Gaps

None.
