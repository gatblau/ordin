# SPEC: Copilot Local Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-copilot-adapter` · **Build phase:** 5 · **Programs:** D P
> **Dependencies:** Runtime Adapter interface, MCP Front, Hook Relay, Rules Engine, Identity

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

## Purpose

The Copilot Local Adapter is the second local adapter (02:543; 09 ADR-038). It runs one local agent session with the GitHub Copilot SDK for Rust (`github-copilot-sdk`) against a bundled, pinned Copilot CLI server started for that session, with the agent folder as its working folder (05:15; A-11). It checks the Copilot CLI's own sign-in through the SDK's status call (09 ADR-044), registers the MCP Front's servers for the session (09 ADR-037), sends every tool request to ordin's decision path before it executes — through the SDK's permission handler and pre-tool-use hook, both routed to the Hook Relay in process (09 ADR-045) — and turns the session's streaming events into the `RunEvent` stream the Session Supervisor consumes. It owns the SDK session identifiers and the mapping of SDK events to run events (02:547). It runs on the desktop and the personal server; the agent profile does not offer Copilot (09 ADR-097 item 2; 10 C-75).

## Approach

**Reconciliation assumptions** (decision 80): **SA-RECON-02** — the run-start decision is the Rules Engine's `begin_run`; the run-end decision is the Hook Relay's on `HookMessage::Stop` (R5). **SA-RECON-03** — `StartRun.caller`, `scope_id` and `coding_task_id` are copied into `SessionGrant` (R2). **SA-RECON-01** — the Setup Assistant's Copilot `HarnessProbe` is implemented by the program over this adapter's `probe_*` calls, since this crate does not depend on the Setup Assistant (D3, row 252).

The adapter keeps its logic behind an internal port trait, `CopilotPort` *(chosen here)*, with one production implementation binding `github-copilot-sdk` and one scripted test double, so every rule below is tested without the CLI. One CLI server is started per session (A-11), the safe reading of C-03, and the per-run state machine mirrors the Claude Local Adapter's (start, pause on approval or sign-in, resume, end). Tool requests use the in-process SDK route of 09 ADR-045 rather than a hook file in the agent folder, so the decision, the approval hold and the fail-closed rule are the Hook Relay's single implementation. Error prefix: `CPA_` (internal codes, logged and journalled); at the trait boundary each failure is a 2C `AdapterError` with its `RA_` code. Rejected: one shared CLI server for every session — unconfirmed (10 C-03) and it would mix agent folders; a `preToolUse` command hook written into the agent folder — the Installers own that folder and the in-process route avoids a second channel.

## Shared Context

### Types from `ordin-runtime-adapter` (2C wire §2), used unchanged

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind { ClaudeLocal, CopilotLocal, HostedAgent, SlackThread }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterVerb { Start, Progress, Pause, Resume, Read, Cancel }

#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> AdapterKind;
    fn unsupported_verbs(&self) -> &'static [AdapterVerb];
    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError>;
    fn events(&self, ctx: &AdapterContext, run: &RunHandle)
        -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError>;
    async fn resume(&self, ctx: &AdapterContext, run: &RunHandle, decision: ResumeDecision)
        -> Result<(), AdapterError>;
    async fn read(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<RunSnapshot, AdapterError>;
    async fn cancel(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError>;
}

#[derive(Debug, Clone)]
pub struct AdapterContext {
    pub correlation_id: String,
    pub deadline: std::time::Instant,
    pub cancel: tokio_util::sync::CancellationToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunHandle {
    pub run_id: RunId,
    pub adapter: AdapterKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct StartRun {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,
    pub target: RunTarget,
    pub input: StartInput,
    pub attachments: Vec<ArtefactId>,
    pub limits: RunLimits,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
    /// Copied into the MCP Front's `SessionGrant` (2C wire).
    pub caller: ToolCaller,            // Ordin | Agent | CrewMember { crew_run_id } | HostedAgent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunTarget {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartInput {
    Command { name: String, arguments: String },
    FreeText { text: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunLimits {
    pub wall_clock_secs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
    pub idle_timeout_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        #[serde(skip_serializing_if = "Option::is_none")]
        runtime_session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_thread_id: Option<String>,
    },
    Progress { progress: RunProgress },
    HarnessPlan { steps: Vec<String> },
    Paused { reason: PauseReason },
    Resumed,
    Usage { usage: UsageDelta },
    Ended { end: RunEnd },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PauseReason {
    Approval { approval_id: ApprovalId },
    SignIn,
    Person { question: String },
    DecisionElsewhere { link: url::Url },
    /// Hosted agent: waiting for the hosted agent's approvers (2C C-14). This adapter never emits it.
    RemoteApprovers,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsageDelta {
    pub model_turns: u32,
    pub tool_calls: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunEnd {
    pub reason: RunEndReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,   // at most 32,768 characters (LW-15)
    pub artefacts: Vec<ArtefactId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEndReason { Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResumeDecision {
    Approve { approval_id: ApprovalId },
    Reject {
        approval_id: ApprovalId,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Answer { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub state: RunState,       // domain part of 2C
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<RunEnd>,
    pub checked_at: Timestamp,
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdapterError {
    #[error("verb not offered")] VerbNotOffered { verb: AdapterVerb },
    #[error("harness signed out")] SignedOut,
    #[error("harness version not tested")] HarnessUntested { found: String },
    #[error("route unreachable")] Unreachable,
    #[error("plan allowance exhausted")] AllowanceExhausted { service_message: String },
    #[error("remote thread busy")] ThreadBusy,
    #[error("unknown run")] UnknownRun,
    #[error("deadline passed")] Deadline,
    /// The adapter's own validation failed: `RA_TARGET_MISMATCH`, `RA_COMMAND_UNKNOWN`,
    /// `RA_INPUT_TOO_LARGE`, `RA_HANDLE_INVALID`, `RA_RESUME_MISMATCH` (2C C-13).
    #[error("invalid request")] InvalidRequest { code: String },
    /// The remote side refused the request; `code` is the adapter's journalled code (2C C-13).
    #[error("refused")] Refused { code: String },
}
// Codes: RA_VERB_NOT_OFFERED, RA_SIGNED_OUT, RA_HARNESS_UNTESTED, RA_UNREACHABLE,
// RA_ALLOWANCE_EXHAUSTED, RA_THREAD_BUSY, RA_UNKNOWN_RUN, RA_DEADLINE.
```

`RunProgress` (2C wire §1): `{ line: String /* 1–160 chars */, count: Option<ProgressCount> }`, `ProgressCount { done: u64, total: Option<u64> }`. 2C validation rows enforced: `target` = `agent_instance` (`RA_TARGET_MISMATCH`); `input.name` 1–64 characters (`RA_COMMAND_UNKNOWN`); `arguments` 0–16,384, `text` 1–16,384 characters (`RA_INPUT_TOO_LARGE`); resume `approval_id` matches the pause (`RA_RESUME_MISMATCH`); `reason` ≤ 500, `Answer.text` 1–4,000 characters.

### Hook protocol types (2C wire §3, §4)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOrigin { BuiltIn, OrdinMcpFront { server: String }, PackageMcpServer { server: String } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookMessage {
    PreToolUse {
        harness: HarnessKind,
        tool: String,
        origin: ToolOrigin,
        arguments: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        harness_tool_use_id: Option<String>,
        /// The sub-agent that asked, when the payload names it (C-78; 2C C-20).
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent: Option<String>,
    },
    PostToolUse {
        harness: HarnessKind,
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        harness_tool_use_id: Option<String>,
        succeeded: bool,
    },
    Stop { harness: HarnessKind },
}

/// The relay writes zero or one `Holding`, then exactly one final reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookReply {
    Holding { call_id: HookCallId, approval_id: ApprovalId, deadline: Timestamp },
    Allow { call_id: HookCallId },
    /// To a `stop`, `Deny` means "do not end yet": `reason` is given to the agent (2C C-20).
    Deny { call_id: HookCallId, reason: String },
    /// Reply to `post_tool_use` and `stop`.
    Ack { call_id: HookCallId },
}
```

Timing (LW-21, LW-22): allow or deny without *ask* within 2 s; the decision path's own deadline is 50 s; past it the answer is deny with reason "waiting for the person's approval" (09 ADR-045). The Copilot CLI documents a hook timeout as allow (08:94), which is why the deadline the adapter gives the SDK is never reached: the in-process call returns at 50 s.

### MCP Front boundary (crate `ordin-mcp-front`, B6 contract)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerName(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramProfile { Desktop, PersonalServer, AgentProfile }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionGrant {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    pub profile: ProgramProfile,
    pub caller: ToolCaller,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionMcpConfig { pub servers: Vec<McpServerLaunch> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct McpServerLaunch {
    pub name: ServerName,
    pub command: std::path::PathBuf,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
}

impl McpFront {
    pub async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError>;
    pub async fn close_session(&self, run_id: RunId) -> Result<(), McpError>;
}
```

Which tools a session is offered is decided by each tool provider from the `SessionGrant`, not by this adapter: the crew tools (`delegate`, `finish`, `escalate`, server `ordin`) reach a session only when the Ordin Host's provider recognises the run as a coordinator session (02:545 "Crew members receive the crew tools only when the session is a coordinator"; SA-NAME-01). `ORDIN_MCP_TOKEN` inside `env` is a secret.

### Hook Relay boundary (crate `ordin-hook-relay`, HookRelay.md)

```rust
pub struct HookSessionSpec { pub run_id: RunId, pub agent_instance_id: AgentInstanceId, pub workspace_id: WorkspaceId, pub harness: HarnessKind }
impl HookRelay {
    pub fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError>;
    pub fn close_session(&self, run_id: RunId);
    pub fn session_events(&self, run_id: RunId) -> futures::stream::BoxStream<'static, HookSessionEvent>;
    /// Copilot SDK route: the same decision path as a hook call, in process (no socket, no MAC).
    pub async fn decide_in_process(&self, run_id: RunId, message: HookMessage) -> HookReply;
}

pub enum HookSessionEvent {
    Holding { approval_id: ApprovalId, tool: String },
    DeniedAtDeadline { approval_id: ApprovalId, tool: String },
    Decided { tool: String, allowed: bool },
    ToolUsed { tool: String, origin: ToolOrigin, succeeded: bool },
    TurnStopping,
}
```

`open_session` is still called, so the relay keeps the per-session state its decision path needs; the returned `HookSessionEnv` is not passed to the CLI (no hook process runs, X3b).

### Identity boundary (crate `ordin-identity`, IdentityAndCredentials.md)

```rust
impl Identity {
    /// Refresh with the stored refresh token only, never a prompt. `Ok(true)` when a new access
    /// token was obtained; `Ok(false)` when there is no refresh token or the provider refused it.
    pub async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError>;
}
```

Used with `CredentialKind::Github` (03 golden path: "With Copilot, the application first refreshes the GitHub sign-in silently").

The Copilot CLI's own sign-in stays separate from ordin's GitHub sign-in and ordin never reads the CLI's credentials (09 ADR-044).

### Rules Engine boundary (crate `ordin-rules`, RulesEngine.md)

```rust
impl RulesEngine {
    pub async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError>;
}
// RunStartFacts { run_id, agent_instance_id, workspace_id, crew_charter_id, command, text, scope, asker, started_by, started_by_id }
// RunStartDecision { outcome: StartOutcome, command, tool_subset, single_call, limit_seconds, … }
// StartOutcome { Proceed, Ended { control, ending }, Replied { control, text } }
```

The adapter calls only `begin_run` (R5). The run-end decision is the Hook Relay's, reached through `decide_in_process(run_id, HookMessage::Stop { harness: CopilotCli })`.

### Configuration consumed

| Item | Source | Value |
|---|---|---|
| Copilot CLI path and pinned version | bundled sidecar (05:15, 05 "Copilot CLI (server mode)") | absolute path; version compiled in |
| Tested versions | organisation profile `harnesses[]` with `name: copilot` | list |
| Model per level | organisation profile `usage.level_models.copilot.<level>`; `automatic` is Copilot's own selection (2D-2) | string |
| Stop deadline | SA-STOP-01 | 10 s |
| Removed from the CLI's environment | 2D-1c | `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, `GITHUB_TOKEN` |

## Public Interface

```rust
// crate ordin-copilot-adapter

pub struct CopilotLocalAdapter { /* private */ }

#[derive(Debug, Clone)]
pub struct CopilotAdapterConfig {
    pub cli_path: std::path::PathBuf,
    pub cli_version: String,
    pub tested_versions: Vec<String>,
    pub app_data_root: std::path::PathBuf,
    pub profile: ProgramProfile,
    pub stop_deadline: std::time::Duration,
}

/// The Copilot CLI's device sign-in on a Linux server is unconfirmed (10 C-48); until it is,
/// the personal server offers Claude only. Compiled constant, chosen here.
pub const C48_SERVER_SIGN_IN_CONFIRMED: bool = false;

/// Same role as in the Claude Local Adapter: what StartRun does not carry. Implemented by the
/// program over the Session Supervisor, the Installers, the Usage Planner and the Crew Orchestrator
/// (`crew_charter_id`) (owned here; D3 pattern).
#[async_trait::async_trait]
pub trait SessionSetupSource: Send + Sync {
    async fn session_setup(&self, run_id: RunId, agent_instance_id: AgentInstanceId)
        -> Result<CopilotSessionSetup, SessionSetupError>;
}

/// The program could not assemble a session's setup. Named so that it does not clash with
/// `ordin_setup::SetupError` (SetupAssistant.md SA-RECON-07). `code` is the error code of the
/// owner whose call failed (for example a Session Supervisor, Installers or Usage Planner code).
#[derive(Debug, thiserror::Error)]
#[error("session setup failed: {code}")]
pub struct SessionSetupError { pub code: String }

/// Carries no folder grants: a Copilot session works in its agent folder only, and this adapter
/// assumes no granted directory outside it (the Agent Folder Manager makes no grant for Copilot).
#[derive(Debug, Clone)]
pub struct CopilotSessionSetup {
    pub agent_folder: std::path::PathBuf,
    /// `None` = Copilot's automatic selection (2D-2 `automatic`; 10 C-37).
    pub model: Option<String>,
    pub attachment_paths: Vec<std::path::PathBuf>,
    pub package_servers: Vec<PackageMcpServer>,
    /// Present when a persisted SDK session continues (Recovery after restart; 10 C-05).
    pub resume_session_id: Option<String>,
    /// Crew member runs (`StartRun.caller` is `ToolCaller::CrewMember { crew_run_id }`): the crew run's
    /// charter, which the program reads from `CrewOrchestrator::get(crew_run_id)` (`CrewRun.crew_charter_id`);
    /// `None` for every other caller.
    pub crew_charter_id: Option<CrewCharterId>,
}

#[derive(Clone)]
pub struct PackageMcpServer {
    pub name: String,                                    // 1–64 chars, ^[a-zA-Z0-9_.-]+$
    pub command: std::path::PathBuf,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>, // may hold secrets; never logged
}

impl CopilotLocalAdapter {
    /// Refuses `ProgramProfile::AgentProfile` with CPA_PROFILE_NOT_OFFERED (09 ADR-097 item 2; 10 C-75),
    /// and `PersonalServer` with CPA_SERVER_SIGN_IN_UNCONFIRMED while C48_SERVER_SIGN_IN_CONFIRMED is false.
    pub fn new(
        config: CopilotAdapterConfig,
        port: std::sync::Arc<dyn CopilotPort>,
        mcp: std::sync::Arc<ordin_mcp_front::McpFront>,
        hooks: std::sync::Arc<ordin_hook_relay::HookRelay>,
        rules: std::sync::Arc<ordin_rules::RulesEngine>,
        identity: std::sync::Arc<ordin_identity::Identity>,
        setup: std::sync::Arc<dyn SessionSetupSource>,
    ) -> Result<Self, CopilotAdapterError>;

    pub async fn shutdown(&self);

    /// Facts for the Setup Assistant's Copilot `HarnessProbe`, which the program implements over
    /// these three calls (this crate has no edge to the Setup Assistant; ADR-100 item 1 as amended,
    /// row 252, SA-RECON-01). `probe_sign_in` = `CopilotServer::auth_status` with the CLI server
    /// started if it is not running; `probe_start_sign_in` = `start_login`; `probe_repair` stops
    /// and restarts the CLI server.
    pub async fn probe_sign_in(&self) -> Result<CopilotAuthStatus, CopilotAdapterError>;
    pub async fn probe_start_sign_in(&self) -> Result<(), CopilotAdapterError>;
    pub async fn probe_repair(&self) -> Result<(), CopilotAdapterError>;

    /// The `HarnessModels` the program gives the Usage Planner (RuntimeAdapterInterface.md, O63).
    pub fn harness_models(&self) -> std::sync::Arc<dyn HarnessModels>;
}

impl RuntimeAdapter for CopilotLocalAdapter {
    // kind() == AdapterKind::CopilotLocal
    // unsupported_verbs() == &[]    (2C: copilot_local offers every verb)
}

#[derive(Debug, thiserror::Error)]
pub enum CopilotAdapterError {
    #[error("Copilot is not offered in this profile")] ProfileNotOffered,
    #[error("Copilot sign-in on a server is not confirmed")] ServerSignInUnconfirmed,
    #[error("adapter configuration invalid: {0}")] ConfigInvalid(&'static str),
    #[error("Copilot CLI server unreachable: {code}")] PortFailed { code: &'static str, retryable: bool },
}
```

`HarnessModels` for the Copilot CLI: `harness()` = `CopilotCli`; `per_session_model()` = true (`SessionOptions.model`; `None` is Copilot's automatic selection); `accepts_model(name)` = `name` is `automatic` or 1–100 characters of `[A-Za-z0-9._-]`; `default_level_models()` = 2D's default `usage.level_models.copilot` (`automatic` for every level).

No hook file is written for Copilot and no `ordin-hook` process runs: tool requests are decided in process through the Hook Relay (X3b; O57).

**Called by.**

| Item | Caller |
|---|---|
| `new`, `shutdown`, `harness_models` | Ordin Desktop program (`SdkCopilotPort`, Identity, the program's `SessionSetupSource`); Ordin Server program, personal profile, which skips construction on `ServerSignInUnconfirmed` (row 253); `harness_models` is passed to the Usage Planner through `UsagePlanner::set_harnesses`, which each program calls once its local adapters are built |
| `probe_sign_in`, `probe_start_sign_in`, `probe_repair` | the desktop program's Copilot `HarnessProbe`, registered with the Setup Assistant (`register_probe`); the personal server registers none, because the Setup Assistant skips `copilot` on the server (C-48) |
| `RuntimeAdapter` methods | Session Supervisor, through the program's `AdapterRegistry` |
| `SessionSetupSource` (implemented, not called) | implemented by the programs over the Session Supervisor, the Installers' `record(id).launch` (`HarnessLaunch::Copilot.working_dir` → `agent_folder`), the Usage Planner and the Crew Orchestrator's `get` (`crew_charter_id`) |

### `CopilotPort` — internal port over `github-copilot-sdk` *(chosen here)*

Every name below is this spec's; the production implementation `SdkCopilotPort` binds each to the SDK's client, session, permission handler, hook, MCP server, status, persistence and event features that 08:95 lists.

**SA-CLOSE-01** (*chosen here*; closes G-CPA-1). The port above is the contract this spec tests; binding it to the SDK is a codegen-time step against the pinned `github-copilot-sdk` version, not an open question. The mapping read on January 2026 from the crate's published README (docs.rs, version 1.0.23; the last version with generated API documentation is 1.0.15) is:

| Port item | `github-copilot-sdk` item |
|---|---|
| `CopilotPort::start_server` | `github_copilot_sdk::Client::start(ClientOptions)` (async), one client per session (A-11) |
| session start / resume | `Client::create_session(SessionConfig)` / `Client::resume_session(ResumeSessionConfig)` → `github_copilot_sdk::session::Session` |
| permission decisions | trait `github_copilot_sdk::handler::PermissionHandler` (`async fn handle(&self, SessionId, RequestId, PermissionRequestData) -> PermissionResult`), installed with `SessionConfig::with_permission_handler` |
| pre- and post-tool-use hooks | trait `github_copilot_sdk::hooks::SessionHooks`, installed with `SessionConfig::with_hooks`; events `HookEvent::PreToolUse`, `HookEvent::PostToolUse` (success only) and `on_post_tool_use_failure` for failures |
| MCP server option | the session configuration key `mcp_servers` |
| streamed events | `Session::subscribe()` → `EventSubscription` of `SessionEvent` (`event_type` text plus JSON `data`: `assistant.message_delta`, `assistant.message`, `assistant.reasoning_delta`, `assistant.reasoning`); `RecvErrorKind::Lagged` and `RecvErrorKind::Closed` |
| `auth_status`, `start_login` | not named in the README: `auth_status` is the CLI server's JSON-RPC `auth.getStatus` (08:93), bound at codegen to the SDK's RPC accessor (the README shows `session.rpc()`); when the pinned version offers no accessor for it, `auth_status` runs the bundled CLI's own status command as a child process and reads its output, and `start_login` runs the bundled CLI's `copilot login` as a child process, both as this spec already states |

Codegen confirms each row against the generated documentation of the pinned version and records the version in the crate's `Cargo.toml`; a row whose item is renamed at that version is bound to the renamed item without changing the port. The scripted test double implements the port, so every rule of this spec is tested without the SDK.

```rust
#[async_trait::async_trait]
pub trait CopilotPort: Send + Sync {
    /// Starts one Copilot CLI server for one session (A-11) with the given environment and working folder.
    async fn start_server(&self, spec: CliServerSpec) -> Result<Box<dyn CopilotServer>, PortError>;
}

pub struct CliServerSpec {
    pub cli_path: std::path::PathBuf,
    pub working_folder: std::path::PathBuf,
    pub env: std::collections::BTreeMap<String, String>,
}

#[async_trait::async_trait]
pub trait CopilotServer: Send + Sync {
    /// The SDK's auth.getStatus (09 ADR-044; 08:93).
    async fn auth_status(&self) -> Result<CopilotAuthStatus, PortError>;
    /// Starts the CLI's own `copilot login` flow (C-25, Copilot half open).
    async fn start_login(&self) -> Result<(), PortError>;
    async fn create_session(&self, opts: SessionOptions) -> Result<String /* session id */, PortError>;
    async fn resume_session(&self, session_id: &str, opts: SessionOptions) -> Result<(), PortError>;
    async fn send(&self, session_id: &str, text: &str) -> Result<(), PortError>;
    fn events(&self, session_id: &str) -> futures::stream::BoxStream<'static, CopilotEvent>;
    async fn abort(&self, session_id: &str) -> Result<(), PortError>;
    /// Stops the CLI server; kills it after `deadline`.
    async fn stop(&self, deadline: std::time::Duration);
}

pub struct CopilotAuthStatus { pub signed_in: bool, pub method: Option<String> }

pub struct SessionOptions {
    pub model: Option<String>,
    /// Same JSON as SessionMcpConfig::to_mcp_servers_json() plus package servers.
    pub mcp_servers: serde_json::Value,
    /// Called for every tool request before it executes; must return within 55 s.
    pub pre_tool_use: std::sync::Arc<dyn ToolGate>,
    /// Called for every permission request the CLI raises.
    pub permission: std::sync::Arc<dyn ToolGate>,
    /// Called after every tool call.
    pub post_tool_use: std::sync::Arc<dyn ToolObserver>,
}

#[async_trait::async_trait]
pub trait ToolGate: Send + Sync {
    async fn decide(&self, tool: &str, arguments: serde_json::Value, tool_use_id: Option<String>) -> GateAnswer;
}
pub enum GateAnswer { Allow, Deny { reason: String } }

#[async_trait::async_trait]
pub trait ToolObserver: Send + Sync {
    async fn tool_used(&self, tool: &str, tool_use_id: Option<String>, succeeded: bool);
}

pub enum CopilotEvent {
    SessionStarted,
    AssistantText { text: String },
    ToolStarted { tool: String },
    ToolFinished { tool: String, succeeded: bool },
    /// The CLI's to-do list, where its events carry it (10 C-41).
    TodoList { items: Vec<String> },
    Usage { requests: Option<u32>, input_tokens: Option<u64>, output_tokens: Option<u64>, cached_input_tokens: Option<u64> },
    /// The turn ended; `final_text` is the assistant's last message.
    TurnEnded { final_text: Option<String> },
    SignedOut,
    AllowanceExhausted { message: String },
    Error { message: String },
    ServerExited { status: Option<i32> },
}
```

### Example

`start` for `{"kind":"free_text","text":"Summarise the three quotes in one table"}` with `CopilotSessionSetup { model: None, .. }`:

```json
{"run_id":"0192a1d0-1111-7aaa-8bbb-ccccddddeeee","adapter":"copilot_local","runtime_session_id":"sess_8f3a2c"}
```

Events:

```json
{"kind":"started","runtime_session_id":"sess_8f3a2c"}
{"kind":"progress","progress":{"line":"Read files","count":{"done":1}}}
{"kind":"harness_plan","steps":["Read the three quotes","Compare the rates","Write the table"]}
{"kind":"usage","usage":{"model_turns":1,"tool_calls":3}}
{"kind":"ended","end":{"reason":"completed","final_message":"Here is the table of the three quotes.","artefacts":[]}}
```

A denied tool request: `ToolGate::decide("shell", {"command":"rm -rf build"}, Some("call_7"))` → `decide_in_process(run_id, PreToolUse { harness: CopilotCli, tool: "shell", origin: BuiltIn, … })` → `HookReply::Deny { reason: "Shell commands are not allowed for this agent." }` → `GateAnswer::Deny { reason }`.

## Internal Logic

### `start(ctx, req)`

1. Validate `req` against the 2C rows (`CPA_TARGET_MISMATCH`, `CPA_INPUT_INVALID`); refuse a run already held (`CPA_SESSION_EXISTS`). Log `copilot.start.received`.
2. `setup.session_setup(run_id, agent_instance_id)` → `CPA_SETUP_FAILED` on error (retryable; the `SessionSetupError` code is logged with `run_id`); agent folder missing → `CPA_AGENT_FOLDER_MISSING`.
3. Version: `cli_version` not in `tested_versions` (when the list is non-empty) → `CPA_HARNESS_UNTESTED` (`AdapterError::HarnessUntested { found }`).
4. **Environment** built from an empty map *(chosen here, 2D-1c)*: `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_ALL`, `TMPDIR`, `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY`, and on Windows `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `SystemRoot`, `TEMP`. `COPILOT_GITHUB_TOKEN`, `GH_TOKEN` and `GITHUB_TOKEN` are never present, so the CLI uses its own sign-in (2D-1c; 09 ADR-073 *Status*).
5. `port.start_server(CliServerSpec { cli_path, working_folder: agent_folder, env })` → `CPA_SERVER_START_FAILED` (`RA_UNREACHABLE`, retryable). One server per session (A-11). *Conditional on 10 C-03:* if C-03 confirms one SDK client can run sessions with different folders, a shared server is permitted by a later revision; this spec keeps one per session either way.
6. **Sign-in.** `auth_status()` → unreadable: `CPA_AUTH_STATUS_UNREADABLE` (retryable). `signed_in: false` →
   1. `identity.refresh_silently(CredentialKind::Github)` (03 golden path); then `auth_status()` again.
   2. Still signed out → emit `Started`, `Paused { SignIn }`, call `start_login()` (09 ADR-044). *Conditional on 10 C-25 (Copilot half open):* if `copilot login` cannot complete when started with no terminal, the start-up path still pauses on `SignIn` and the person signs in from the Copilot CLI itself (ADR-044 fallback); the adapter polls `auth_status()` every 5 s for 15 minutes *(chosen here)*, as below.
   3. On signed in: emit `Resumed`, continue at step 7. After 15 minutes the run stays paused on `SignIn`; `resume(Answer)` starts `start_login()` again.
7. Emit `Started` (when not already emitted). **Run-start decision:** `rules.begin_run(RunStartFacts { … })` filled as in the Claude Local Adapter outside the agent profile (`crew_charter_id` from `CopilotSessionSetup.crew_charter_id`, `asker: None`); `Proceed` → continue with `command`, `tool_subset`, `single_call` and `limit_seconds` applied; `Ended { ending }` / `Replied { text }` → `Ended { Completed, final_message }` (`CPA_RUN_START_ENDED`, no session created); `RulesError` → `Ended { Failed }` (`CPA_RULES_UNAVAILABLE`, fail closed).
8. **Register.** `mcp.open_session(SessionGrant { run_id, workspace_id, agent_instance_id, harness: CopilotCli, profile, caller: StartRun.caller, scope_id: StartRun.scope_id, coding_task_id: StartRun.coding_task_id })` → `CPA_MCP_OPEN_FAILED`; `hooks.open_session(HookSessionSpec { run_id, agent_instance_id, workspace_id, harness: CopilotCli })` → `CPA_HOOK_OPEN_FAILED` (closing the MCP session first). Merge package servers into the MCP servers JSON; a clash with an MCP Front server name → `CPA_SERVER_NAME_CLASH`.
9. **Session.** Build `SessionOptions { model, mcp_servers, pre_tool_use, permission, post_tool_use }` where both gates are a `RelayGate` *(chosen here)* bound to the run, and the observer forwards `PostToolUse` to `hooks.decide_in_process` (the relay journals it). `resume_session_id` present → `resume_session(..)`; error → *conditional on 10 C-05:* if a persisted session cannot be resumed, emit `Ended { Interrupted }` with `CPA_SESSION_NOT_RESUMABLE` (03 Recovery after restart). Otherwise `create_session(..)` → session id (`CPA_SESSION_CREATE_FAILED`, retryable). The session id is `runtime_session_id`.
10. **First turn.** `Command` → `/<name> <arguments>` *(assumed here: the form the Copilot CLI accepts for a prompt file installed by the vscode or agents-md Installer; conditional on 10 C-01 and C-60 — if Copilot does not load what the Installer writes, the command text is sent as is and the Installers, not this adapter, carry the change; the HLD names no other fallback)*; `FreeText` → the text; attachments as `Attached: <path>` lines. `send(session_id, text)` → `CPA_SEND_FAILED`.
11. Return `RunHandle { run_id, adapter: CopilotLocal, runtime_session_id: Some(session_id) }`. Everything up to here within `ctx.deadline`; past it → `AdapterError::Deadline` after stopping the server and closing the MCP and relay sessions.

### Tool requests — `RelayGate::decide(tool, arguments, tool_use_id)`

1. Classify origin: a tool name the MCP Front registered for this session (`mcp__<server>__<tool>` or the SDK's server-qualified form) → `OrdinMcpFront { server }`; a package server → `PackageMcpServer { server }`; otherwise `BuiltIn` (2C: the MCP Front classification is the core's, not the harness's).
2. `hooks.decide_in_process(run_id, PreToolUse { harness: CopilotCli, tool, origin, arguments, harness_tool_use_id })`, bounded by 55 s *(chosen here: above the relay's 50 s deadline)*.
3. `Allow` → `GateAnswer::Allow`. `Deny { reason }` → `GateAnswer::Deny { reason }`. A `Holding` reply is not returned by the in-process call; the relay emits `HookSessionEvent::Holding` instead. Timeout, error or panic in the call → `GateAnswer::Deny { reason: "ordin could not check this action." }` and `CPA_GATE_FAILED` logged (fail closed, 09 ADR-045).
4. *Conditional on 10 C-02 and C-27:* every tool request, including MCP and first-party tools, reaches the permission handler or the pre-tool-use hook and a deny blocks it. If either spike shows a tool kind that bypasses both, the HLD's fallback applies: the session runs with no built-in tools, so that only ordin's MCP Front can act (09 ADR-045); the adapter then passes an empty built-in tool list through `SessionOptions` *(field added to the port when the spike names the SDK option)*.

### Session events → run events

| `CopilotEvent` | Effect |
|---|---|
| `SessionStarted` | logged |
| `AssistantText` | not shown; updates `checked_at` |
| `ToolStarted` | none (the gate already decided) |
| `ToolFinished { tool, succeeded }` | increment tool calls; `Progress` with the plain line of the Claude Local Adapter's table (built-in file reads → "Read files", writes → "Changed a file", MCP servers by server name, otherwise "Used a tool"; never a tool identifier, 11:75) |
| `TodoList { items }` | *Conditional on 10 C-41:* 3–7 items of ≤ 80 characters → `HarnessPlan { steps }`; otherwise dropped and journalled `RA_PLAN_INVALID`; without C-41 the event never arrives and the run shows events only (09 ADR-068 item 4) |
| `Usage` | `Usage { model_turns: 1 per TurnEnded, tool_calls, tokens where present }` — *conditional on 10 C-37*: token fields `None` where Copilot does not report them; budgets for Copilot are counted in requests (09 ADR-038) |
| `TurnEnded { final_text }` | `hooks.decide_in_process(run_id, Stop { harness: CopilotCli })`: `Ack` or `Allow` → `Ended { Completed, final_message }`; `Deny { reason }` → `reason` sent as the next turn (the Rules Engine counts refusals and asks a person after its own limit, which arrives as `HookSessionEvent::Holding` → `Paused { Approval }`); skipped while an approval is outstanding (the run stays paused) |
| `SignedOut` | `Paused { SignIn }` and the sign-in flow of `start` step 6 |
| `AllowanceExhausted { message }` | `Ended { Failed, final_message: message (≤ 500 chars) }`, `CPA_ALLOWANCE_EXHAUSTED` |
| `Error { message }` | `Ended { Failed }`, `CPA_SESSION_ERROR`; message to Diagnostics only |
| `ServerExited` before `Ended` | `Ended { Failed }`, `CPA_SERVER_EXITED` |

Hook Relay session events: `Holding` → `Progress { "Waiting for your approval" }`; `DeniedAtDeadline { approval_id }` → `Paused { Approval { approval_id } }` once; `Decided` clears an outstanding hold; `ToolUsed` is ignored here (counted from `ToolFinished`).

### `resume(ctx, run, decision)`

Same rules as the Claude Local Adapter: unknown → `UnknownRun`; not paused → `CPA_NOT_PAUSED`; mismatching `approval_id` or decision kind → `CPA_RESUME_MISMATCH`. Approval at run start or end continues those steps. A tool approval: emit `Resumed`, `send` the turn "The person approved the action you asked for. Make the same call again now." or "The person did not approve the action. Reason: <reason>. Continue without it." The Rules Engine allows the approved call once when repeated. `SignIn` accepts only `Answer` (restarts `start_login`).

### `cancel(ctx, run)`, stop and shutdown

`abort(session_id)`, then `stop(stop_deadline)` (kill after 10 s, `CPA_STOP_DEADLINE` logged), emit `Ended { Cancelled }`; idempotent on an ended run. Clean-up: `hooks.close_session`, `mcp.close_session` (errors logged), drop the server. `shutdown()` stops every live run within `stop_deadline` + 1 s and emits `Ended { Interrupted }`.

### `events` and `read`

As the Claude Local Adapter: a replayable ordered buffer (≤ 10,000 events, consecutive `Progress` merged beyond), several subscribers, ending after `Ended`; `read` from memory, `UnknownRun` after a restart — the Session Supervisor then calls `start` with `resume_session_id` (C-05) or marks the run interrupted.

## Data Model

N/A — the adapter owns no table. Run state belongs to the Session Supervisor, tool requests and decisions to the Journal and the Rules Engine; SDK session persistence (C-05) is the Copilot CLI's own store under the person's account, which the adapter does not read.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Constructed for the agent profile | `Err(permanent)` from `new` | `CPA_PROFILE_NOT_OFFERED` | `CopilotAdapterError::ProfileNotOffered` |
| Constructed for the personal server while C-48 is unconfirmed | `Err(permanent)` from `new` | `CPA_SERVER_SIGN_IN_UNCONFIRMED` | `CopilotAdapterError::ServerSignInUnconfirmed` |
| Configuration path not absolute | `Err(permanent)` from `new` | `CPA_CONFIG_INVALID` | `CopilotAdapterError::ConfigInvalid("<field>")` |
| A probe call cannot reach or start the CLI server | `Err(retryable)` | `CPA_PROBE_FAILED` | `CopilotAdapterError::PortFailed { code: "CPA_PROBE_FAILED", retryable: true }` |
| Target is a remote agent | `Err(permanent)` | `CPA_TARGET_MISMATCH` | `AdapterError::InvalidRequest { code: \"RA_TARGET_MISMATCH\" }` |
| Input outside 2C limits | `Err(permanent)` | `CPA_INPUT_INVALID` | `AdapterError::InvalidRequest { code: "RA_COMMAND_UNKNOWN" | "RA_INPUT_TOO_LARGE" }` |
| Run already held | `Err(permanent)` | `CPA_SESSION_EXISTS` | `AdapterError::InvalidRequest { code: \"RA_HANDLE_INVALID\" }` |
| Session setup source fails | `Err(retryable)` | `CPA_SETUP_FAILED` | `AdapterError::Unreachable` |
| Agent folder missing | `Err(permanent)` | `CPA_AGENT_FOLDER_MISSING` | `AdapterError::Unreachable` |
| CLI version not tested | `Err(permanent)` | `CPA_HARNESS_UNTESTED` | `AdapterError::HarnessUntested { found: "1.0.97" }` |
| CLI server does not start | `Err(retryable)` | `CPA_SERVER_START_FAILED` | `AdapterError::Unreachable` |
| Sign-in status unreadable | `Err(retryable)` | `CPA_AUTH_STATUS_UNREADABLE` | `AdapterError::Unreachable` |
| Signed out after the silent GitHub refresh | run event | `CPA_SIGNED_OUT` | `Paused { SignIn }`; person sees "Sign in again" (11:1050) |
| Rules Engine ends the run at start (`StartOutcome::Ended` or `Replied`) | run event | `CPA_RUN_START_ENDED` | `Ended { Completed, final_message: <ending or reply> }` |
| Rules Engine unavailable at run start or end | run event | `CPA_RULES_UNAVAILABLE` | `Ended { Failed }` |
| MCP Front `open_session` fails | `Err(retryable)` | `CPA_MCP_OPEN_FAILED` | `AdapterError::Unreachable` |
| Hook Relay `open_session` fails | `Err(retryable)` | `CPA_HOOK_OPEN_FAILED` | `AdapterError::Unreachable` |
| Package server name clashes with an MCP Front server | `Err(permanent)` | `CPA_SERVER_NAME_CLASH` | `AdapterError::Unreachable` |
| SDK session cannot be created | `Err(retryable)` | `CPA_SESSION_CREATE_FAILED` | `AdapterError::Unreachable` |
| Persisted session cannot be resumed (C-05) | run event | `CPA_SESSION_NOT_RESUMABLE` | `Ended { Interrupted }` |
| First turn cannot be sent | `Err(retryable)` | `CPA_SEND_FAILED` | `AdapterError::Unreachable` |
| `start` past `ctx.deadline` | `Err(retryable)` | `CPA_DEADLINE` | `AdapterError::Deadline` |
| Decision call errors or exceeds 55 s | tool request denied | `CPA_GATE_FAILED` | `GateAnswer::Deny { reason: "ordin could not check this action." }` |
| Allowance used up | run event | `CPA_ALLOWANCE_EXHAUSTED` | `Ended { Failed, final_message: "<service message>" }` |
| SDK session error | run event | `CPA_SESSION_ERROR` | `Ended { Failed }` |
| CLI server exits before the run ended | run event | `CPA_SERVER_EXITED` | `Ended { Failed }` |
| Verb on an unknown run | `Err(permanent)` | `CPA_UNKNOWN_RUN` | `AdapterError::UnknownRun` |
| Resume of a run not paused | `Err(permanent)` | `CPA_NOT_PAUSED` | `AdapterError::InvalidRequest { code: \"RA_RESUME_MISMATCH\" }` |
| Resume with another approval or a wrong decision kind | `Err(permanent)` | `CPA_RESUME_MISMATCH` | `AdapterError::InvalidRequest { code: \"RA_RESUME_MISMATCH\" }` |
| CLI server not stopped within the deadline | logged; killed | `CPA_STOP_DEADLINE` | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Copilot Local Adapter

  Background:
    Given a scripted CopilotPort double and test doubles of the MCP Front, the Hook Relay, the Rules Engine and Identity
    And the profile lists tested Copilot CLI version "1.0.98"

  Scenario: Happy path — free text runs to completion
    Given auth_status reports signed in and the Rules Engine's begin_run returns Proceed
    When start is called with free text "Summarise the three quotes in one table"
    Then a CLI server is started with the agent folder as its working folder
    And create_session receives the MCP servers JSON returned by open_session
    And the events are started, progress, usage and ended with reason "completed"

  Scenario: Credential variables never reach the CLI server
    Given ordin's environment holds COPILOT_GITHUB_TOKEN, GH_TOKEN and GITHUB_TOKEN
    When start is called
    Then the CliServerSpec environment holds none of the three

  Scenario: Every tool request goes through the Hook Relay and a deny blocks it
    Given the Hook Relay answers Deny with reason "Shell commands are not allowed for this agent."
    When the session raises a pre-tool-use request for tool "shell"
    Then decide_in_process is called with harness "copilot_cli" and origin "built_in"
    And the gate answers Deny with the same reason

  Scenario: Fail closed — the decision call times out
    Given decide_in_process does not answer within 55 seconds
    When the session raises a permission request
    Then the gate answers Deny with reason "ordin could not check this action."

  Scenario: Edge — signed out, the GitHub sign-in is refreshed silently first
    Given auth_status reports signed out, refresh_silently(Github) returns true, and auth_status then reports signed in
    When start is called
    Then no Paused event is emitted and the session is created

  Scenario: Edge — still signed out, the run pauses for the person's sign-in
    Given auth_status reports signed out until start_login has run and 10 seconds have passed
    When start is called
    Then the events begin with started and paused with reason "sign_in"
    And after auth_status reports signed in the next event is resumed

  Scenario: Edge — an approval outlives the deadline and is approved later
    Given the Hook Relay emits DeniedAtDeadline for approval "A7"
    Then the adapter emits paused with reason approval "A7"
    When resume is called with approve "A7"
    Then the adapter emits resumed and sends a turn telling the model to make the same call again

  Scenario: Error — refused in the agent profile
    When the adapter is constructed with profile "agent_profile"
    Then construction fails with code CPA_PROFILE_NOT_OFFERED

  Scenario: Error — the CLI server exits during a turn
    Given the port emits ServerExited before TurnEnded
    Then the last event is ended with reason "failed"
    And the MCP Front and Hook Relay sessions for the run are closed

  Scenario: Edge — a harness to-do list becomes a plan (C-41)
    Given the port emits TodoList with items "Read the three quotes", "Compare the rates", "Write the table"
    Then the adapter emits harness_plan with those three steps

  Scenario: The program's Copilot probe reads the sign-in through the adapter
    Given the CLI server is not running and auth_status reports signed in with method "device"
    When the program's probe calls probe_sign_in
    Then the CLI server is started once
    And the reply has signed_in true and method "device"
```

## Performance / Security / Observability

- **Performance targets:** `start` returns within 5 s p95 when signed in (CLI server start dominates *(assumed here)*). An SDK event becomes a `RunEvent` within 50 ms p95 (07: first progress within 1 s). A gate decision without *ask* adds ≤ 2 s (LW-21). Adapter memory per run ≤ 8 MiB excluding the CLI server; up to 8 sessions at once (A-04), each with its own server (R-05: server memory unmeasured).
- **Security:** In-process callers only. Inputs checked against 2C limits. The CLI server's environment is built from an allow-list and never carries GitHub tokens (2D-1c). Every tool request is decided before it executes and any failure denies (09 ADR-045). Never logged or journalled: tool arguments (the relay journals a digest), assistant text, prompts, final messages, `PackageMcpServer.env` values, `ORDIN_MCP_TOKEN`, Identity tokens and the CLI's sign-in method beyond its kind. The adapter never reads the CLI's credentials (09 ADR-044).
- **Observability:** `tracing` events with fields `run_id`, `runtime_session_id`, `agent_instance_id`, `workspace_id`, `correlation_id`, `code`: `copilot.start.received`, `copilot.server.started` (`cli_version`), `copilot.sign_in.status` (`signed_in`), `copilot.sign_in.started`, `copilot.session.created`, `copilot.gate.decided` (`tool_kind` = built_in/ordin/package, `allowed`), `copilot.run.paused` (`reason_kind`), `copilot.run.resumed`, `copilot.run.ended` (`reason`), `copilot.stop.deadline`. No metrics or spans (D and P only; SA-XC-04/05).

## Gaps

None.
