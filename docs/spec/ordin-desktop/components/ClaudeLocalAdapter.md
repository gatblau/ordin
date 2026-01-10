# SPEC: Claude Local Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-claude-adapter` · **Build phase:** 5 · **Programs:** D P A
> **Dependencies:** Runtime Adapter interface, MCP Front, Hook Relay, Rules Engine

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

The Claude Local Adapter runs one local agent session on the Claude Code CLI and presents it to the core as a `RuntimeAdapter` (02:507–516; 09 ADR-038, ADR-044, ADR-045). On the desktop and the personal server it drives the person's installed, unmodified Claude Code with the person's own sign-in; in the agent profile it drives the organisation-installed Claude Code with the organisation's model key (09 ADR-097 item 2). For each run it checks the harness's version and sign-in, starts one CLI process in headless streamed-JSON mode with the agent folder as its working folder, registers the MCP Front's servers and the ordin hook for that session only, passes the agent's instructions and plugin produced by the Claude Code Installer (10 C-61), and turns the CLI's output and the Hook Relay's session events into the ordered `RunEvent` stream the Session Supervisor consumes. It owns the session processes and the mapping of CLI events to run events (02:515).

## Approach

**Closing assumption** (*chosen here*): **SA-CLOSE-01** (closes G-CLA-3; *amended January 2026, ADR-107*) — in the agent profile this adapter holds no provider-specific code. What the harness's own provider mode needs is the configuration's `model.environment`, which Model Policy resolves into `HarnessModelSettings.environment` (ModelPolicy.md MP-2a); this adapter sets each declared name in the Claude Code child's environment and nowhere else (step 8). A `gateway_url` is given to the child as `ANTHROPIC_BASE_URL`, the variable Claude Code's documentation names for a custom model endpoint. C-77's question is closed by the local test T1 of January 2026: with Claude Code 2.1.306, a base address and a key given in the child's environment were honoured, every request reached the address given and no sign-in was used. No model variable other than the key, `ANTHROPIC_BASE_URL` and the declared names is set. Which of the child's own processes may see these values is C-74, answered once for the key and every declared name alike (SA-HOST-04).

**Reconciliation assumption** (decision 80): **SA-RECON-01** — the organisation key and the fallback model reach this adapter as Model Policy's `HarnessModelSettings`, and the launch paths as the Installers' `HarnessLaunch::ClaudeCode`, both mapped by the program into `SignInMode` and `SessionSetup` (rows 249, 251). **SA-RECON-02** — the run-start decision is the Rules Engine's `begin_run`; the run-end decision is made by the Hook Relay on the `Stop` hook, and this adapter never calls it (R5). **SA-RECON-03** — `StartRun.caller`, `scope_id` and `coding_task_id` are copied unchanged into the MCP Front's `SessionGrant` (R2). **SA-RECON-04** — MCP traffic reaches the MCP Front through `ordin-hook mcp-bridge` on the MCP Front's own socket (R8, X2); the hook flags are the ordin hook's (`--harness claude-code`, X3).

The adapter is a per-run state machine driven by three inputs — the CLI's stdout lines, the Hook Relay's `HookSessionEvent` stream for the run, and the Session Supervisor's verb calls — with one tokio task per run owning the child process and emitting `RunEvent`s into a replayable per-run buffer. Everything the CLI needs is passed on the command line or in per-session files the adapter writes (`--settings`, `--mcp-config`), because `--restricted` reads nothing from the agent folder or the person's own settings (10 C-26, C-61). Error prefix: `CLA_` (internal codes, journalled and logged); at the trait boundary every failure is a 2C `AdapterError` variant with its `RA_` code. Rejected: driving Claude Code through a TypeScript Agent SDK sidecar — a second language runtime, and settled against by 09 ADR-045 (OQ-29); reading tool calls from the stream's `assistant` content for enforcement — enforcement is the hook's job (09 ADR-045), the stream is used for progress and results only.

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
    /// The model level and model the Usage Planner chose (ADR-062, C-37).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
    /// Who the session's ordin tools act for; copied into the MCP Front's `SessionGrant`.
    pub caller: ToolCaller,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
}
// `ToolCaller` (2C wire, `ordin-types`): Ordin | Agent | CrewMember { crew_run_id } | HostedAgent.

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
    pub state: RunState,                 // domain part of 2C: queued, running, paused, completed, failed,
                                         // stopped_at_limit, cancelled, interrupted, abandoned, no_reply
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

`RunProgress` (2C wire §1): `{ line: String /* 1–160 chars */, count: Option<ProgressCount> }`; `ProgressCount { done: u64, total: Option<u64> }`.

2C validation rows this adapter enforces at `start`/`resume`: `target` must be `agent_instance` (`RA_TARGET_MISMATCH`); `input.name` 1–64 characters (`RA_COMMAND_UNKNOWN`); `arguments` 0–16,384 and `text` 1–16,384 characters (`RA_INPUT_TOO_LARGE`); `ResumeDecision` `approval_id` equal to the one the run paused on (`RA_RESUME_MISMATCH`); `reason` ≤ 500 characters, `Answer.text` 1–4,000 characters (`RA_INPUT_TOO_LARGE`). Each is returned as `AdapterError::InvalidRequest { code }` (2C C-13).

### Hook protocol types (2C wire §3, §4)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOrigin { BuiltIn, OrdinMcpFront { server: String }, PackageMcpServer { server: String } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCall {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub point: DecisionPoint,
    pub tool: String,
    pub origin: ToolOrigin,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionPoint { RunStart, BeforeToolCall, RunEnd }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolDecision { #[serde(flatten)] pub verdict: Verdict, pub rule: RuleRef, pub decided_at: Timestamp }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Verdict { Allow, Deny { reason: String }, Ask { proposal: ActionProposal } }
```

Hook environment (2C wire §4, names fixed):

| Variable | Value |
|---|---|
| `ORDIN_HOOK_ENDPOINT` | `<app data root>/run/hook.sock` (mode `0600`) on macOS and Linux; `\\.\pipe\ordin-hook-<installation id>` on Windows |
| `ORDIN_HOOK_SECRET` | 32 random bytes, base64url without padding (43 characters), per session; never journalled or logged |
| `ORDIN_SESSION_ID` | the `RunId` of the session |
| `ORDIN_HOOK_PROTOCOL` | `1` |

The program is `ordin-hook` (`ordin-hook.exe` on Windows), registered for Claude Code through `--settings` as a `PreToolUse` command hook with `"onFailure": "block"`; the harness hook timeout ordin writes is 60 s (LW-23, 2D-4), the hook's own deadline is 50 s (LW-22).

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

impl SessionMcpConfig {
    /// {"mcpServers": {"<name>": {"type":"stdio","command":"…","args":["mcp-bridge"],"env":{…}}}}
    pub fn to_mcp_servers_json(&self) -> serde_json::Value;
}

impl McpFront {
    pub async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError>;
    pub async fn close_session(&self, run_id: RunId) -> Result<(), McpError>;
}
```

`McpError` codes this adapter reacts to: `MCP_SESSION_EXISTS`, `MCP_SESSION_UNKNOWN`, `MCP_NOT_STARTED`, `MCP_PROVIDER_FAILED`. `ORDIN_MCP_TOKEN` inside `env` is a secret.

### Hook Relay boundary (crate `ordin-hook-relay`, HookRelay.md)

```rust
pub struct HookSessionSpec { pub run_id: RunId, pub agent_instance_id: AgentInstanceId, pub workspace_id: WorkspaceId, pub harness: HarnessKind }
pub struct HookSessionEnv { pub endpoint: String, pub secret: String, pub session_id: RunId, pub protocol: u16 }
impl HookSessionEnv {
    /// [("ORDIN_HOOK_ENDPOINT", …), ("ORDIN_HOOK_SECRET", …), ("ORDIN_SESSION_ID", …), ("ORDIN_HOOK_PROTOCOL", "1")]
    pub fn env_vars(&self) -> [(&'static str, String); 4];
}

impl HookRelay {
    pub fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError>;
    pub fn close_session(&self, run_id: RunId);
    pub fn session_events(&self, run_id: RunId) -> futures::stream::BoxStream<'static, HookSessionEvent>;
}

pub enum HookSessionEvent {
    Holding { approval_id: ApprovalId, tool: String },
    DeniedAtDeadline { approval_id: ApprovalId, tool: String },
    Decided { tool: String, allowed: bool },
    ToolUsed { tool: String, origin: ToolOrigin, succeeded: bool },
    TurnStopping,
}
```

### Rules Engine boundary (crate `ordin-rules`, RulesEngine.md)

```rust
impl RulesEngine {
    /// DecisionPoint::RunStart, before the harness starts (R5).
    pub async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError>;
}
pub struct RunStartFacts {
    pub run_id: RunId, pub agent_instance_id: AgentInstanceId, pub workspace_id: WorkspaceId,
    pub crew_charter_id: Option<CrewCharterId>, pub command: Option<String>, pub text: String,
    pub scope: Option<String>, pub asker: Option<String>,
    pub started_by: String, pub started_by_id: String,
}
pub struct RunStartDecision {
    pub outcome: StartOutcome, pub command: Option<String>, pub tool_subset: Option<Vec<String>>,
    pub single_call: bool, pub limit_seconds: Option<u32>,
    /// Not read by this adapter. The run summary's `actions_attached`, `delivery` and `controls_decided`
    /// are filled by the Session Supervisor at run end from the Journal's external actions for the run
    /// (`external_actions_for_run`, behind `journal.run_external_actions`; SessionSupervisor.md H.2).
    pub actions_attached: Vec<String>, pub delivery: Option<DeliveryTo>, pub controls_decided: Vec<(String, String)>,
}
pub enum StartOutcome { Proceed, Ended { control: String, ending: String }, Replied { control: String, text: String } }
pub enum DeliveryTo { Asker, Thread }

// 2C domain (phase-2c-shared-types-domain.md), copied verbatim: who started an agent-profile run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunStarter {
    Caller { caller_id: String },
    ScheduleEntry { entry: String },
    Intake { intake: String },
    ChatConnection { connection: String, asker: String },
}
```

The adapter calls only `begin_run`. `BeforeToolCall` decisions arrive through the ordin hook and the Hook Relay; the run-end decision (`decide_run_end`) is made by the Hook Relay on the harness's `Stop` hook, so a refused ending makes Claude Code continue the turn itself and an asked ending arrives as `HookSessionEvent::Holding` (RulesEngine.md: no adapter calls it).

### Configuration consumed

| Item | Source | Value |
|---|---|---|
| Tested Claude Code versions | organisation profile `harnesses[]` (`name: claude_code`, `tested_versions[]`) via the program | list of strings, e.g. `2.1.304` |
| Stop deadline | SA-STOP-01 | 10 s (agent profile: the deployment's stop period, given in config) |
| Idle harness process kept while paused | 2D-4 "Idle harness session stopped" | 30 minutes |
| Hook timeout written in settings | 2D-4, LW-23 | 60 s |
| Concurrent local sessions | 2D-4 (Session Supervisor enforces) | 8 |
| `ANTHROPIC_API_KEY` | 2D-1c | removed (desktop, personal server); set from `ORDIN_MODEL_KEY_FILE` or the secret store in the agent profile when `HarnessModelSettings.key` is present (ADR-107: absent for a provider other than the default with no key) |
| `CLAUDE_CODE_OAUTH_TOKEN` | 2D-1c | always removed |
| `ANTHROPIC_BASE_URL` | SA-CLOSE-01 | set only in the agent profile, from `HarnessModelSettings.gateway_url`; never inherited |
| Each name of `model.environment` | SA-CLOSE-01 (ADR-107) | set only in the agent profile, from `HarnessModelSettings.environment`, in the child alone; never inherited, never logged. A name in Agent Configuration's `RESERVED_ENVIRONMENT_NAMES` or beginning `ORDIN_` is refused by `new` (`CLA_ENV_RESERVED`) |

### Claude Code CLI facts used (verified)

- Flags, from `claude --help` of Claude Code 2.1.304 (read January 2026): `-p`, `--input-format stream-json`, `--output-format stream-json`, `--verbose`, `--restricted` (removes Bash, PowerShell, the other code-running tools and WebFetch unless `--tools` names them; ignores user, project and local settings files; `--settings` still applies; confines file tools to the working directories; refuses `bypassPermissions`), `--strict-mcp-config`, `--mcp-config`, `--settings`, `--permission-prompts none` (anything that would prompt is denied), `--session-id <uuid>`, `--resume <session id>`, `--tools`, `--disallowedTools`, `--model`, `--effort` (`low`, `medium`, `high`, `xhigh`, `max`), `--fallback-model`, `--plugin-dir`, `--append-system-prompt[-file]`, `--bare` (never used). Built-in tool names visible there: `Bash`, `Edit`, `Read`, `WebFetch`, `PowerShell`; `Write` from 10 C-26.
- `claude auth status --json` fields `loggedIn` (bool), `authMethod`, `apiProvider` (observed January 2026 on 2.1.304; 09 ADR-044); `claude auth login` completes with no terminal (10 C-25).
- `claude --version` prints `2.1.304 (Claude Code)`.
- Streamed-JSON stdin: one line per person's turn, `{"type":"user","message":{"role":"user","content":"<text>"}}`; stdout events `system`/`init` per turn, `assistant`, `user` (tool result), `rate_limit_event`, one `result` per turn; `--resume <session id>` continues in a new process (08:81; 10 C-19).
- Under `--restricted` nothing is read from the agent folder; the Claude Code Installer writes `instructions.md` (passed with `--append-system-prompt-file`) and `plugin/` (passed with `--plugin-dir`); commands are then named `<agent>:<command>` (02:513, 02:457; 10 C-61; R-94).

## Public Interface

```rust
// crate ordin-claude-adapter

pub struct ClaudeLocalAdapter { /* private */ }

#[derive(Debug, Clone)]
pub struct ClaudeAdapterConfig {
    /// Absolute path of the `claude` executable found by the Setup Assistant.
    pub claude_path: std::path::PathBuf,
    /// Absolute path of `ordin-hook` (`ordin-hook.exe`).
    pub hook_program: std::path::PathBuf,
    /// Application data root; per-session files go under `<root>/run/sessions/<run_id>/`.
    pub app_data_root: std::path::PathBuf,
    pub profile: ProgramProfile,
    pub tested_versions: Vec<String>,
    pub sign_in: SignInMode,
    /// 10 s on the desktop and personal server (SA-STOP-01); the deployment's stop period in the agent profile.
    pub stop_deadline: std::time::Duration,
}

/// Chosen here.
#[derive(Clone)]
pub enum SignInMode {
    /// Desktop and personal server: the person's own sign-in (09 ADR-044).
    PersonSignIn,
    /// Agent profile: the organisation's key (09 ADR-097 item 2).
    /// `models` is Model Policy's `ordin_types::agent_profile::HarnessModelSettings` (row 249,
    /// SA-RECON-01): `models.key` (`Option<OrganisationKey>`) is the key; `models.fallback_model` gives
    /// `--fallback-model`; `models.provider` is not read here (ADR-107: no provider-specific code);
    /// `models.gateway_url` becomes `ANTHROPIC_BASE_URL` and each `models.environment` entry is set in the
    /// child's environment (SA-CLOSE-01).
    OrganisationKey { models: HarnessModelSettings, route: KeyRoute },
}

/// How the key reaches Claude Code (10 C-74). Chosen here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRoute {
    /// `ANTHROPIC_API_KEY` in the Claude Code child's environment only (default while C-74 is unconfirmed or passes).
    Environment,
    /// C-74 failed: the key is given through Claude Code's own `apiKeyHelper` setting in `--settings`.
    SettingsApiKeyHelper,
}

/// Everything about one session that the adapter cannot derive from StartRun.
/// Implemented by the program from the Session Supervisor, the Agent Folder Manager and the
/// Usage Planner, Installers and Model Policy, plus the Crew Orchestrator (`crew_charter_id`) and,
/// in the agent profile, the Run Queue (`run_starter`) (owned here; D3 pattern).
#[async_trait::async_trait]
pub trait SessionSetupSource: Send + Sync {
    async fn session_setup(&self, run_id: RunId, agent_instance_id: AgentInstanceId)
        -> Result<SessionSetup, SessionSetupError>;
}

/// The program could not assemble a session's setup. Named so that it does not clash with
/// `ordin_setup::SetupError` (SetupAssistant.md SA-RECON-07). `code` is the error code of the
/// owner whose call failed (for example a Session Supervisor, Installers or Usage Planner code).
#[derive(Debug, thiserror::Error)]
#[error("session setup failed: {code}")]
pub struct SessionSetupError { pub code: String }

#[derive(Debug, Clone)]
pub struct SessionSetup {
    /// Absolute path of the agent folder (the CLI's working folder, A-11). The program fills it from
    /// the Installers' `HarnessLaunch::ClaudeCode.working_dir` (row 251).
    pub agent_folder: std::path::PathBuf,
    /// `HarnessLaunch::ClaudeCode.append_system_prompt_file` (`--append-system-prompt-file`).
    pub append_system_prompt_file: std::path::PathBuf,
    /// `HarnessLaunch::ClaudeCode.plugin_dir` (`--plugin-dir`).
    pub plugin_dir: std::path::PathBuf,
    /// `HarnessLaunch::ClaudeCode.plugin_name`; commands are `<plugin_name>:<command>` (R-94).
    pub plugin_name: String,
    /// Model chosen by the Usage Planner for this step (09 ADR-062; 10 C-37).
    pub model: Option<String>,
    pub effort: Option<ClaudeEffort>,
    /// Agent profile: the configuration's fallback model (09 ADR-098 item 3), copied by the
    /// program from `HarnessModelSettings.fallback_model`; `None` elsewhere.
    pub fallback_model: Option<String>,
    /// Absolute paths of the attachments, already checked against the run's folder grants.
    pub attachment_paths: Vec<std::path::PathBuf>,
    /// Directories outside the agent folder the run may read and write (folder grants, 09 ADR-025).
    pub extra_dirs: Vec<std::path::PathBuf>,
    /// True only for a system-changing package on a project with approved shell commands (A-06; 09 ADR-037).
    pub shell_allowed: bool,
    /// Package-declared MCP servers with their environment (SA-CRED-01), merged into --mcp-config.
    pub package_servers: Vec<PackageMcpServer>,
    /// Present when the run continues an earlier harness session (Recovery after restart; agent-profile resume).
    pub resume_session_id: Option<String>,
    /// Crew member runs (`StartRun.caller` is `ToolCaller::CrewMember { crew_run_id }`): the crew run's
    /// charter, which the program reads from `CrewOrchestrator::get(crew_run_id)` (`CrewRun.crew_charter_id`);
    /// `None` for every other caller.
    pub crew_charter_id: Option<CrewCharterId>,
    /// Agent profile: the run record's `started_by` (2C `RunStarter`), which the program reads from the
    /// Run Queue's `TakenRun.view.record.started_by`; `None` on the desktop and the personal server.
    pub run_starter: Option<RunStarter>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeEffort { Low, Medium, High, Xhigh, Max }

#[derive(Clone)]
pub struct PackageMcpServer {
    pub name: String,                    // 1–64 chars, ^[a-zA-Z0-9_.-]+$ (LW-19)
    pub command: std::path::PathBuf,
    pub args: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,   // may hold secrets; never logged
}

/// Agent profile: where the harness's session record is kept between a pause and a resume
/// (09 ADR-098 item 3; 10 C-80). Implemented by the program over the Session Supervisor's
/// `save_session_record` / `load_session_record`.
#[async_trait::async_trait]
pub trait SessionRecordStore: Send + Sync {
    async fn save(&self, run_id: RunId, runtime_session_id: &str, record: Vec<u8>) -> Result<(), RecordError>;
    async fn load(&self, run_id: RunId, runtime_session_id: &str) -> Result<Option<Vec<u8>>, RecordError>;
}

/// A failure of the program's `SessionRecordStore`. The program builds it from the
/// `SupervisorError` that `save_session_record` / `load_session_record` returned:
/// `code` = `SupervisorError::code()` (for example `SUP_RUN_NOT_FOUND`), `retryable` =
/// `SupervisorError::retryable()`.
#[derive(Debug, thiserror::Error)]
#[error("session record store failed: {code}")]
pub struct RecordError { pub code: &'static str, pub retryable: bool }

impl ClaudeLocalAdapter {
    /// Fails with CLA_CONFIG_INVALID when a path is not absolute, the profile/sign-in pair is
    /// inconsistent (OrganisationKey outside the agent profile, PersonSignIn inside it), or the
    /// key is present and empty. Fails with CLA_ENV_RESERVED (ADR-107) when a `models.environment`
    /// name is in `ordin_types::agent_profile::RESERVED_ENVIRONMENT_NAMES` (ignoring case) or begins
    /// `RESERVED_ENVIRONMENT_PREFIX` — a second check of the rule the configuration check already applies.
    pub fn new(
        config: ClaudeAdapterConfig,
        mcp: std::sync::Arc<ordin_mcp_front::McpFront>,
        hooks: std::sync::Arc<ordin_hook_relay::HookRelay>,
        rules: std::sync::Arc<ordin_rules::RulesEngine>,
        setup: std::sync::Arc<dyn SessionSetupSource>,
        records: Option<std::sync::Arc<dyn SessionRecordStore>>,   // required in the agent profile
    ) -> Result<Self, ClaudeAdapterError>;

    /// Stops every session within the stop deadline; runs still going end `Interrupted` (SA-XC-13).
    pub async fn shutdown(&self);

    /// Whether Claude Code is signed in on this machine: `claude auth status --json`, exit 0 and
    /// `loggedIn: true` → `SignedIn`; non-zero exit or `loggedIn: false` → `SignedOut` (row 247).
    /// Timeout 10 s → `CLA_SIGN_IN_STATUS_FAILED` (retryable). Personal server and desktop only;
    /// with `SignInMode::OrganisationKey` → `CLA_CONFIG_INVALID`.
    pub async fn sign_in_status(&self) -> Result<SignInStatus, ClaudeAdapterError>;

    /// The `HarnessModels` the program gives the Usage Planner (RuntimeAdapterInterface.md, O63).
    pub fn harness_models(&self) -> std::sync::Arc<dyn HarnessModels>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInStatus { SignedIn, SignedOut }

impl RuntimeAdapter for ClaudeLocalAdapter {
    // kind() == AdapterKind::ClaudeLocal
    // unsupported_verbs() == &[]   (2C AdapterVerb table: claude_local offers every verb)
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeAdapterError {
    #[error("adapter configuration invalid: {0}")] ConfigInvalid(&'static str),
    #[error("sign-in status could not be read")] SignInStatusFailed,
    /// ADR-107. Names the variable, never its value.
    #[error("the variable {0} is set by ordin itself and cannot be declared")] EnvReserved(String),
}
```

`HarnessModels` for Claude Code: `harness()` = `ClaudeCode`; `per_session_model()` = true (`--model`); `accepts_model(name)` = `name` is 1–100 characters of `[A-Za-z0-9._-]`; `default_level_models()` = the map of 2D's default `usage.level_models.claude_code`.

**Called by.**

| Item | Caller |
|---|---|
| `new`, `shutdown`, `harness_models` | Ordin Desktop and Ordin Server programs (all three profiles); `harness_models` is passed to the Usage Planner through `UsagePlanner::set_harnesses`, which each program calls once its local adapters are built |
| `sign_in_status` | Ordin Server program, personal profile (Always on: at start and every 6 hours) |
| `RuntimeAdapter` methods | Session Supervisor, through the program's `AdapterRegistry` |
| `SessionSetupSource` (implemented, not called) | implemented by the programs over the Session Supervisor, the Installers' `record(id).launch`, Model Policy, the Usage Planner, the Crew Orchestrator's `get` (`crew_charter_id`) and, agent profile, the Run Queue's run record (`run_starter`) |
| `SessionRecordStore` (implemented, not called) | implemented by the programs over `SessionSupervisor::save_session_record` and `load_session_record` (row 250; the adapter pushes the record on pause, so `RuntimeAdapter` gains no export call) |

The adapter contributes no start-up report items of its own: it offers every verb, so `unsupported_verbs()` is empty, and the tool and rule items come from the MCP Front (`offered_tools`) and the Rules Engine, assembled by the Session Supervisor.

### Files written per session (chosen here)

Directory `<app data root>/run/sessions/<run_id>/`, mode `0700`; each file mode `0600`; the directory is removed when the session ends for good.

`settings.json` (passed with `--settings`):

```json
{
  "hooks": {
    "PreToolUse":  [{"matcher": "*", "hooks": [{"type": "command", "command": "/Users/asha/Library/Application Support/ordin/bin/ordin-hook --harness claude-code --event pre-tool-use --agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff", "timeout": 60, "onFailure": "block"}]}],
    "PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/Users/asha/Library/Application Support/ordin/bin/ordin-hook --harness claude-code --event post-tool-use --agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff", "timeout": 60}]}],
    "Stop":        [{"hooks": [{"type": "command", "command": "/Users/asha/Library/Application Support/ordin/bin/ordin-hook --harness claude-code --event stop --agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff", "timeout": 60}]}]
  }
}
```

The `hooks`/`matcher`/`type`/`command`/`timeout` keys follow Claude Code's documented settings form *(assumed here: the HLD names `--settings`, `PreToolUse` and `onFailure` but not the surrounding keys; the C-26 spike used this registration)*. With `KeyRoute::SettingsApiKeyHelper` the file also holds `"apiKeyHelper": "<hook_program> api-key --run <run_id> --data-root <app data root>"` and the adapter writes the key to `key` in the session directory, mode `0600` (OrdinHook.md `api-key` mode). `--harness claude-code`, `--event` and `--agent-instance` are the ordin hook's own flags (OrdinHook.md; X3).

`mcp.json` (passed with `--mcp-config`): `SessionMcpConfig::to_mcp_servers_json()` with each `PackageMcpServer` added under its own name as a stdio server; a package server whose name equals an MCP Front server name is refused (`CLA_SERVER_NAME_CLASH`).

### Command line (desktop and personal server)

```
claude -p --input-format stream-json --output-format stream-json --verbose
       --session-id <runtime_session_id>            (new session)   | --resume <runtime_session_id> (continued)
       --restricted --strict-mcp-config
       --mcp-config <dir>/mcp.json --settings <dir>/settings.json
       --permission-prompts none
       --append-system-prompt-file <SessionSetup.append_system_prompt_file>   (when the file exists)
       --plugin-dir <SessionSetup.plugin_dir>                                 (when the folder exists)
       --tools Read,Edit,Write,Glob,Grep,TodoWrite,Task[,Bash]        (Bash only when shell_allowed)
       [--add-dir <dir>]...                                           (one per extra_dirs entry)
       [--model <model>] [--effort <level>]
```

Working folder: the agent folder. `--bare` is never passed (09 ADR-044). The built-in tool list `Read,Edit,Write,Glob,Grep,TodoWrite,Task` is the constant `DESKTOP_BUILT_IN_TOOLS` *(assumed here: `Read`, `Edit`, `Write` are verified; `Glob`, `Grep`, `TodoWrite`, `Task` are taken from Claude Code's tool reference at the pinned version and checked by the init-event conformance test below)*. Shell, code-running and web tools stay off because `--restricted` removes them and `--tools` does not name them (02:510).

### Command line differences in the agent profile

- No `claude auth status` sign-in check and never `claude auth login` (02:508).
- `--tools Read` and `--disallowedTools Write,Edit` — file-writing tools off, reading limited to the bundle, which is the working folder (02:508; 10 C-79, conditional: if C-79 shows that instructions and skills stop loading or that reading needs more tools, the list changes at the pinned version; the HLD names no other fallback). `Bash` is never added: shell runs only in the sandbox (07 security posture, hosted agent).
- `--fallback-model <model>` when `SessionSetup.fallback_model` is present (09 ADR-098 item 3; C-77).
- `ANTHROPIC_API_KEY` in the child environment, or `apiKeyHelper` in settings (KeyRoute), when `models.key` is present.
- `ANTHROPIC_BASE_URL` in the child environment when `models.gateway_url` is present (SA-CLOSE-01).
- Each `models.environment` name and value in the child environment (SA-CLOSE-01; ADR-107).

### Example

Input:

```json
{"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","workspace_id":"0192a0f1-1111-7222-8333-444455556666",
 "target":{"kind":"agent_instance","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"},
 "input":{"kind":"command","name":"compare-quotes","arguments":"Kestrel, Acme and Northwind Q3 quotes"},
 "attachments":[],"limits":{"wall_clock_secs":3600,"idle_timeout_secs":600}}
```

`SessionSetup.plugin_name = "supplier-analyst"`. First stdin line:

```json
{"type":"user","message":{"role":"user","content":"/supplier-analyst:compare-quotes Kestrel, Acme and Northwind Q3 quotes"}}
```

Returned `RunHandle`:

```json
{"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","adapter":"claude_local","runtime_session_id":"0192a1c2-4410-7aa1-9b22-c3d4e5f60718"}
```

Event stream:

```json
{"kind":"started","runtime_session_id":"0192a1c2-4410-7aa1-9b22-c3d4e5f60718"}
{"kind":"progress","progress":{"line":"Read a file","count":{"done":1}}}
{"kind":"progress","progress":{"line":"Ran code in the sandbox","count":{"done":2}}}
{"kind":"paused","reason":{"kind":"approval","approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f"}}
{"kind":"resumed"}
{"kind":"usage","usage":{"model_turns":2,"tool_calls":5,"input_tokens":18234,"output_tokens":1210,"cached_input_tokens":15002}}
{"kind":"ended","end":{"reason":"completed","final_message":"Kestrel is cheapest on 7 of 9 lines; the summary is in Q3 rates.md.","artefacts":[]}}
```

## Internal Logic

### `start(ctx, req)`

1. Check `req.target` is `AgentInstance`; otherwise `CLA_TARGET_MISMATCH` (`RA_TARGET_MISMATCH`). Check input lengths (Shared Context rows); otherwise `CLA_INPUT_INVALID`. Refuse a `run_id` already held in the run table with `CLA_SESSION_EXISTS`. Log `claude.start.received` with `run_id`, `agent_instance_id`, `workspace_id`.
2. Call `setup.session_setup(run_id, agent_instance_id)` within `ctx.deadline`. Errors → `CLA_SETUP_FAILED` (`RA_UNREACHABLE`, retryable; the `SessionSetupError` code is logged with `run_id`). Check `agent_folder` exists and is a directory → else `CLA_AGENT_FOLDER_MISSING` (permanent).
3. **Version check.** Run `claude --version` (stdin closed, 10 s timeout, environment built as in step 8 without hook variables). Parse the first token before ` (Claude Code)`. Missing executable → `CLA_HARNESS_NOT_FOUND` (`RA_UNREACHABLE`, permanent). Timeout or unparsable output → `CLA_VERSION_CHECK_FAILED` (`RA_UNREACHABLE`, retryable). A version not in `tested_versions` → `CLA_HARNESS_UNTESTED` (`AdapterError::HarnessUntested { found }`, permanent). An empty `tested_versions` list accepts any version *(assumed here: with no profile the organisation states no tested list)*.
4. **Sign-in check (desktop, personal server only).** Run `claude auth status --json` (10 s timeout). Parse `loggedIn`. Unreadable output → `CLA_AUTH_STATUS_UNREADABLE` (`RA_UNREACHABLE`, retryable). `loggedIn: false` → the run starts in the *signing in* state: go to step 5, then run "Sign-in flow" below instead of step 9. `authMethod` and `apiProvider` are logged (never the `email` field).
5. Mint `runtime_session_id`: `SessionSetup.resume_session_id` when present, otherwise a new UUIDv7 (SA-ID-01) passed with `--session-id`. Create the run entry (state `Running`, event buffer) and emit `RunEvent::Started { runtime_session_id }` as the first buffered event.
6. **Run-start decision.** `rules.begin_run(RunStartFacts { run_id, agent_instance_id, workspace_id, crew_charter_id: `SessionSetup.crew_charter_id`, command and text from `StartRun.input`, scope: `StartRun.scope_id` as text, asker and started_by: in the agent profile from `SessionSetup.run_starter` — `Caller` → "caller", `ScheduleEntry` → "schedule", `Intake` → "intake", `ChatConnection { asker, .. }` → "chat" with `asker: Some(asker)`, otherwise `asker: None`; elsewhere `asker: None` and started_by "person" | "crew" | "trigger" by `StartRun.caller`, started_by_id })`. `Proceed` → continue, with `command` replacing the first message's command when present, `tool_subset` narrowing `--tools`, `single_call` ending the run after the first `result`, and `limit_seconds` lowering the wall-clock limit. `Ended { ending }` → emit `Ended { Completed, final_message: ending }` (no process started). `Replied { text }` → emit `Ended { Completed, final_message: text }`. `RulesError` → `CLA_RULES_UNAVAILABLE`, emit `Ended { Failed }` (fail closed).
7. **Register the session.** `mcp.open_session(SessionGrant { run_id, workspace_id, agent_instance_id, harness: ClaudeCode, profile, caller: StartRun.caller, scope_id: StartRun.scope_id, coding_task_id: StartRun.coding_task_id })` → `SessionMcpConfig`; `MCP_SESSION_EXISTS` is mapped to `CLA_MCP_OPEN_FAILED` permanent, other errors retryable. `hooks.open_session(HookSessionSpec { run_id, agent_instance_id, workspace_id, harness: ClaudeCode })` → `HookSessionEnv`; error → `CLA_HOOK_OPEN_FAILED` (retryable), after calling `mcp.close_session`. Subscribe `hooks.session_events(run_id)`.
8. **Write session files and build the environment.** Create the session directory and write `settings.json` and `mcp.json` (mode `0600`); a write failure → `CLA_CONFIG_WRITE_FAILED`. Build the child environment from an empty map *(chosen here, 2D-1c "never inherits wholesale")*: copy only `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_ALL`, `TMPDIR`, `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` (either case), and on Windows `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `SystemRoot`, `TEMP`; add the four pairs of `HookSessionEnv::env_vars()`; in the agent profile with `KeyRoute::Environment` and `models.key` present add `ANTHROPIC_API_KEY`; in the agent profile with `models.gateway_url` present add `ANTHROPIC_BASE_URL` = that URL (SA-CLOSE-01); in the agent profile add each `models.environment` entry, in order (*amended January 2026, ADR-107*; none can replace a name above, `new` having refused reserved names). `ANTHROPIC_API_KEY` (other profiles), `CLAUDE_CODE_OAUTH_TOKEN` and `CLAUDE_CONFIG_DIR` are never present (09 ADR-044; 2D-1c). Declared names are set in this child's environment only, never in ordin's own process environment; no value and no copy of the environment map is logged.
9. **Agent profile resume.** When `resume_session_id` is present and the profile is the agent profile, call `records.load(run_id, runtime_session_id)`; `None`, or a `RecordError` (its code logged), → emit `Ended { Interrupted }`, code `CLA_SESSION_RECORD_MISSING`. Otherwise write the record to `<projectsDirectory>/<encoded agent folder>/<runtime_session_id>.jsonl` *(assumed here: Claude Code's session-record location; `projectsDirectory` is a field of `claude auth status --json` on 2.1.304, conditional on 10 C-80, whose fallback is that the run is recorded as interrupted and not started again, 09 ADR-098 item 3)*.
10. **Spawn.** `tokio::process::Command` with the command line of Public Interface, working folder = agent folder, stdin/stdout piped, stderr piped to a 64 KiB ring buffer (never logged; its last 2 KiB go to Diagnostics only on failure), `kill_on_drop(true)`, new process group on Unix. Spawn error → `CLA_SPAWN_FAILED` (`RA_UNREACHABLE`, retryable). Log `claude.session.spawned` with `run_id`, `runtime_session_id`, `pid`, `harness_version`.
11. **First turn.** Build the text: `Command` → `/<plugin_name>:<name> <arguments>` (R-94; trailing space trimmed when arguments are empty); `FreeText` → the text. When `attachment_paths` is not empty, append one line per path: `Attached: <path>` *(chosen here)*. Write one stream-json `user` line and flush. Write failure → see "CLI exits".
12. Return `RunHandle { run_id, adapter: ClaudeLocal, runtime_session_id: Some(..), remote_thread_id: None }`. Steps 1–8 and 10–12 complete within `ctx.deadline`; past it → `AdapterError::Deadline` after removing everything registered so far (`mcp.close_session`, `hooks.close_session`, kill child, remove the session directory).

### Sign-in flow (desktop, personal server; 09 ADR-044; 10 C-25)

1. Emit `Paused { SignIn }`. Spawn `claude auth login` with stdin null, no terminal, same environment as step 8 without hook variables. Log `claude.sign_in.started`.
2. Every 5 s *(chosen here)* run `claude auth status --json`; on `loggedIn: true`, kill the login process if still alive, emit `Resumed`, continue at `start` step 6. 
3. After 15 minutes *(chosen here)* without success, kill the login process; the run stays paused on `SignIn`. A `resume(Answer { .. })` while paused on `SignIn` (the person's **Sign in again**, 11 §8) starts the flow again from step 1 without a second `Paused` event. `cancel` ends the run `Cancelled`.
4. The login process exiting non-zero → log `CLA_SIGN_IN_FAILED`, the run stays paused on `SignIn`.

### Reading the CLI's output (one task per run)

For each stdout line (at most 4 MiB per line *(assumed here)*; a longer line is discarded with `CLA_STREAM_LINE_TOO_LONG`):

1. Parse JSON; failure → `CLA_STREAM_MALFORMED` logged with `run_id` and line length only; 20 consecutive malformed lines *(chosen here)* end the run `Failed`.
2. Dispatch on `type`:
   - `system` with `subtype` `init` *(field name assumed here)*: first one per process → log `claude.session.init`; the conformance test (Acceptance Criteria) compares its tool list (`tools`, *assumed here*) with `DESKTOP_BUILT_IN_TOOLS`.
   - `assistant`, `user`: not used for enforcement or progress; counted only for the idle signal the Session Supervisor reads through `read().checked_at`.
   - `rate_limit_event`: kept as the last rate-limit notice of the turn (raw text ≤ 500 characters, never shown verbatim).
   - `result`: end of a turn. Read `is_error` (bool), `result` (final text), `usage.input_tokens`, `usage.output_tokens`, `usage.cache_read_input_tokens` *(field names assumed here; conditional on 10 C-37: where usage is absent the token fields stay `None`)*. Emit `Usage { model_turns: 1, tool_calls: <ToolUsed events since the last Usage>, … }`. Then:
     - `is_error: false` and no approval outstanding (below) → the run-end decision was already made on the `Stop` hook by the Hook Relay (a refused ending makes Claude Code continue, so no `result` arrives until the ending is allowed): emit `Ended { Completed, final_message: result truncated to 32,768 characters, artefacts: [] }` and stop the process (below). With `single_call` from step 6, the first `result` ends the run the same way.
     - `is_error: false` and an approval outstanding → stay paused (below); the process is kept.
     - `is_error: true` and the turn's `rate_limit_event` reported the allowance used up *(the event's status field assumed here)* → emit `Ended { Failed, final_message: <the service's message, ≤ 500 chars> }`, code `CLA_ALLOWANCE_EXHAUSTED`; `read` returns this run with `end` set (03 golden path: "the run fails with the service's message shown").
     - `is_error: true` otherwise → `Ended { Failed }`, code `CLA_TURN_FAILED`.
   - any other `type` → ignored and counted in the debug log.
3. Artefacts are not reported by the adapter: `RunEnd.artefacts` is empty and the Session Supervisor records artefacts from the File Service and the Journal *(assumed here: the CLI stream names no ordin artefact)*.

### Hook Relay session events → run events

| `HookSessionEvent` | Effect |
|---|---|
| `Holding { approval_id }` | Remember `approval_id` as outstanding; emit `Progress { line: "Waiting for your approval" }` (11:246). |
| `DeniedAtDeadline { approval_id }` | Emit `Paused { Approval { approval_id } }` once per `approval_id`. The model has been told "waiting for the person's approval" by the hook (09 ADR-045). |
| `Decided { allowed }` | Clears an outstanding `Holding` decided within the deadline; no event. |
| `ToolUsed { tool, origin, succeeded }` | Increment the tool-call counter; emit `Progress` with the plain line from the table below and `count.done` = tool calls so far in the run. |
| `TurnStopping` | Logged only (`claude.turn.stopping`); the Hook Relay and the Ordin Host act on it (09 ADR-036). |

Plain progress lines *(chosen here; never a tool identifier, 11:75)*: built-in `Read`, `Glob`, `Grep` → "Read files"; `Edit`, `Write` → "Changed a file"; `Bash` → "Ran an approved command"; `TodoWrite` → no line; `Task` → "Asked a helper"; server `sandbox` → "Ran code in the sandbox"; `memory` → "Checked what it remembers"; `mail` → "Worked with email"; `work` → no line (plan and report make their own events); `ordin` → no line; `code` → "Looked at the code"; `telegram` → "Worked with Telegram"; a `craft_<app>` server → "Worked on a file in an ArtCraft app"; a package server → "Used one of the agent's own tools"; anything else → "Used a tool". A failed call adds " (it didn't work)".

`HarnessPlan` (conditional on 10 C-41): where the `assistant` stream carries the harness's to-do list *(the `TodoWrite` tool input, assumed here)*, the adapter emits `HarnessPlan { steps }` with each step's text; when C-41 shows no such content, no `HarnessPlan` is emitted and the run shows events only (09 ADR-068 item 4).

### `resume(ctx, run, decision)`

1. Unknown `run_id` → `UnknownRun`. Run not paused → `CLA_NOT_PAUSED` (`RA_RESUME_MISMATCH`).
2. Paused on `Approval`: the decision's `approval_id` must equal the one paused on → else `CLA_RESUME_MISMATCH`. `Answer` → `CLA_RESUME_MISMATCH`.
   - Paused at run start or run end (Rules Engine `Ask`): continue as in the steps above.
   - Paused on a tool approval with the process alive: emit `Resumed`; write a user turn *(wording chosen here)*: Approve → "The person approved the action you asked for. Make the same call again now."; Reject → "The person did not approve the action. Reason: <reason, or 'none given'>. Continue without it." The Rules Engine allows the approved call once when it is repeated (02:652).
   - Process ended while paused (agent profile, or desktop after 30 idle minutes): restart it as in `start` steps 7–10 with `--resume <runtime_session_id>` (agent profile: `records.load` first), then write the same user turn.
3. Paused on `SignIn`: only `Answer` (restart sign-in flow) is accepted.
4. Paused on `Person`: not produced by this adapter; `Answer` there → `CLA_RESUME_MISMATCH`.

### Pausing in the agent profile (09 ADR-097 item 4, ADR-098 item 3; 10 C-80)

On `Paused { Approval }` the adapter waits for the current turn's `result`, closes stdin, waits up to the stop deadline for exit, reads the session record from the location of `start` step 9 and calls `records.save(run_id, runtime_session_id, bytes)`; save failure → `CLA_SESSION_RECORD_SAVE_FAILED`, the run ends `Interrupted`. Then it calls `hooks.close_session` and `mcp.close_session` and removes the session directory. Whichever instance takes the run calls `resume`, which follows step 2's "process ended" branch. Desktop and personal server keep the process for up to 30 minutes of pause *(2D-4 idle harness session)*, then end it the same way without `records.save` (the record stays where Claude Code wrote it).

### `cancel(ctx, run)` and stopping the process

1. Unknown run → `UnknownRun`; an ended run → `Ok(())` (idempotent).
2. Close stdin; on Unix send `SIGTERM` to the process group, on Windows terminate the process; wait up to `stop_deadline`; then kill (`CLA_STOP_DEADLINE` logged).
3. Emit `Ended { Cancelled }` (cancel), or the reason that triggered the stop.
4. Clean up: `hooks.close_session(run_id)`, `mcp.close_session(run_id)` (errors logged, not returned), remove the session directory, drop the run's secrets from memory.

### CLI exits unexpectedly

Child exit before the run's `Ended` (non-zero or zero) → emit `Ended { Failed }`, code `CLA_HARNESS_EXITED`, with the exit status in the log (03 golden path: "the run is marked failed"; the person may resume it from the persisted session through the Session Supervisor, which calls `start` with `resume_session_id`).

### `events(ctx, run)`

Returns a stream that replays the run's buffered events from `Started` and then follows live events, ending after `Ended`. Several subscribers are allowed; each receives the full ordered sequence. Unknown run → `UnknownRun`. The buffer holds at most 10,000 events *(assumed here)*; beyond that consecutive `Progress` events are merged (count kept, line of the latest).

### `read(ctx, run)`

Returns `RunSnapshot { run_id, state, end, checked_at: <time of the last stdout line or hook event> }` from the in-memory run table. `state` maps: running → `Running`, any pause → `Paused`, ended → the `RunEndReason`'s state. A run not in memory (after a restart) → `UnknownRun`; the Session Supervisor then calls `start` with `resume_session_id` or marks it interrupted (03:409–440).

### `shutdown()`

For every live run: stop as in `cancel` step 2 and emit `Ended { Interrupted }`; the whole shutdown completes within `stop_deadline` + 1 s.

## Data Model

N/A — the adapter owns no table. Run state is the Session Supervisor's (`runs`), tool requests and decisions are the Journal's and the Rules Engine's, and the agent-profile session record is kept by the Session Supervisor through `SessionRecordStore`. The adapter's state is in memory (run table, event buffers) plus the per-session files of Public Interface, which are removed at session end; on start-up the adapter removes any `<app data root>/run/sessions/*` directory left by a crash *(chosen here)*.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Configuration path not absolute, profile and sign-in mode inconsistent, empty key | `Err(permanent)` from `new` | `CLA_CONFIG_INVALID` | `ClaudeAdapterError::ConfigInvalid("<field>")` |
| A `models.environment` name is reserved (ADR-107) | `Err(permanent)` from `new` | `CLA_ENV_RESERVED` | `ClaudeAdapterError::EnvReserved("CLAUDE_CONFIG_DIR")` |
| `start` target is a remote agent | `Err(permanent)` | `CLA_TARGET_MISMATCH` | `AdapterError::InvalidRequest { code: \"RA_TARGET_MISMATCH\" }` |
| Command name, arguments or text outside 2C limits | `Err(permanent)` | `CLA_INPUT_INVALID` | `AdapterError::InvalidRequest { code: "RA_COMMAND_UNKNOWN" | "RA_INPUT_TOO_LARGE" }` |
| `start` for a run already held | `Err(permanent)` | `CLA_SESSION_EXISTS` | `AdapterError::InvalidRequest { code: \"RA_HANDLE_INVALID\" }` |
| Session setup source fails | `Err(retryable)` | `CLA_SETUP_FAILED` | `AdapterError::Unreachable` |
| `sign_in_status`: `claude auth status --json` times out after 10 s or prints no JSON | `Err(retryable)` | `CLA_SIGN_IN_STATUS_FAILED` | `ClaudeAdapterError::SignInStatusFailed` |
| `sign_in_status` called with `SignInMode::OrganisationKey` | `Err(permanent)` | `CLA_CONFIG_INVALID` | `ClaudeAdapterError::ConfigInvalid` |
| Agent folder missing | `Err(permanent)` | `CLA_AGENT_FOLDER_MISSING` | `AdapterError::Unreachable` |
| `claude` executable not found | `Err(permanent)` | `CLA_HARNESS_NOT_FOUND` | `AdapterError::Unreachable` |
| `claude --version` times out or is unparsable | `Err(retryable)` | `CLA_VERSION_CHECK_FAILED` | `AdapterError::Unreachable` |
| Version not in the tested list | `Err(permanent)` | `CLA_HARNESS_UNTESTED` | `AdapterError::HarnessUntested { found: "2.1.299" }` |
| `claude auth status --json` unreadable | `Err(retryable)` | `CLA_AUTH_STATUS_UNREADABLE` | `AdapterError::Unreachable` |
| Harness signed out at start | run event | `CLA_SIGNED_OUT` | `Paused { SignIn }`; person sees "Sign in again" (11:1050) |
| `claude auth login` exits non-zero | run stays paused | `CLA_SIGN_IN_FAILED` | no new event |
| Rules Engine ends the run at start (`StartOutcome::Ended` or `Replied`) | run event | `CLA_RUN_START_ENDED` | `Ended { Completed, final_message: <ending or reply> }` |
| Rules Engine unavailable at run start or end | run event (fail closed) | `CLA_RULES_UNAVAILABLE` | `Ended { Failed }` |
| MCP Front `open_session` fails | `Err(retryable)` (`MCP_SESSION_EXISTS`: permanent) | `CLA_MCP_OPEN_FAILED` | `AdapterError::Unreachable` |
| Hook Relay `open_session` fails | `Err(retryable)` | `CLA_HOOK_OPEN_FAILED` | `AdapterError::Unreachable` |
| Package server name equals an MCP Front server name | `Err(permanent)` | `CLA_SERVER_NAME_CLASH` | `AdapterError::Unreachable` |
| Session file cannot be written | `Err(retryable)` | `CLA_CONFIG_WRITE_FAILED` | `AdapterError::Unreachable` |
| Child cannot be spawned | `Err(retryable)` | `CLA_SPAWN_FAILED` | `AdapterError::Unreachable` |
| `start` exceeds `ctx.deadline` | `Err(retryable)` | `CLA_DEADLINE` | `AdapterError::Deadline` |
| Stdout line not JSON (20 in a row ends the run) | logged; then run event | `CLA_STREAM_MALFORMED` | `Ended { Failed }` after the 20th |
| Stdout line above 4 MiB | logged, line dropped | `CLA_STREAM_LINE_TOO_LONG` | — |
| `result` with `is_error` after an allowance notice | run event | `CLA_ALLOWANCE_EXHAUSTED` | `Ended { Failed, final_message: "<service message>" }` |
| `result` with `is_error` otherwise | run event | `CLA_TURN_FAILED` | `Ended { Failed }` |
| Child exits before the run ended | run event | `CLA_HARNESS_EXITED` | `Ended { Failed }` |
| `resume`/`read`/`cancel`/`events` for an unknown run | `Err(permanent)` | `CLA_UNKNOWN_RUN` | `AdapterError::UnknownRun` |
| `resume` of a run that is not paused | `Err(permanent)` | `CLA_NOT_PAUSED` | `AdapterError::InvalidRequest { code: \"RA_RESUME_MISMATCH\" }` |
| `resume` with another `approval_id` or a wrong decision kind | `Err(permanent)` | `CLA_RESUME_MISMATCH` | `AdapterError::InvalidRequest { code: \"RA_RESUME_MISMATCH\" }` |
| Agent profile: no session record for a resume, or `SessionRecordStore::load` returns a `RecordError` | run event | `CLA_SESSION_RECORD_MISSING` | `Ended { Interrupted }` |
| Agent profile: session record cannot be saved at a pause | run event | `CLA_SESSION_RECORD_SAVE_FAILED` | `Ended { Interrupted }` |
| Child did not exit within the stop deadline | logged; child killed | `CLA_STOP_DEADLINE` | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Claude Local Adapter

  Background:
    Given a fake "claude" executable that records its arguments and environment and replays scripted stream-json output
    And an MCP Front, a Hook Relay and a Rules Engine test double
    And the profile lists tested version "2.1.304"

  Scenario: Happy path — a command runs to completion
    Given "claude --version" prints "2.1.304 (Claude Code)" and "claude auth status --json" prints {"loggedIn":true,"authMethod":"claude.ai"}
    And the Rules Engine's begin_run returns Proceed
    When start is called with the command "compare-quotes" and plugin name "supplier-analyst"
    Then the child is started in the agent folder with "--restricted", "--strict-mcp-config", "--permission-prompts none" and "--session-id <runtime_session_id>"
    And the arguments do not contain "--bare"
    And the first stdin line is {"type":"user","message":{"role":"user","content":"/supplier-analyst:compare-quotes Kestrel, Acme and Northwind Q3 quotes"}}
    And when the fake prints a result line with is_error false and result "Kestrel is cheapest"
    Then the events are started, usage, ended with reason "completed" and final_message "Kestrel is cheapest", in that order

  Scenario: Instructions and plugin from the Claude Code Installer are passed by flag (C-61)
    Given the agent folder holds "instructions.md" and a "plugin" folder
    When start is called
    Then the arguments contain "--append-system-prompt-file <agent folder>/instructions.md" and "--plugin-dir <agent folder>/plugin"

  Scenario: Credential variables never reach the child on the desktop
    Given ordin's own environment holds ANTHROPIC_API_KEY, CLAUDE_CODE_OAUTH_TOKEN and CLAUDE_CONFIG_DIR
    When start is called in the desktop profile
    Then the child's environment holds none of the three
    And it holds ORDIN_HOOK_ENDPOINT, ORDIN_HOOK_SECRET, ORDIN_SESSION_ID and ORDIN_HOOK_PROTOCOL "1"

  Scenario: Edge — signed out, the harness's own sign-in runs and the run continues
    Given "claude auth status --json" prints {"loggedIn":false} once and then {"loggedIn":true}
    When start is called
    Then the events begin with started and paused with reason "sign_in"
    And "claude auth login" was started with no terminal
    And after the status reports signed in the next event is resumed and the session child is spawned

  Scenario: Edge — an approval outlives the hook deadline and is decided later
    Given the Hook Relay emits DeniedAtDeadline for approval "A1"
    Then the adapter emits paused with reason approval "A1"
    When resume is called with approve for approval "A1"
    Then the adapter emits resumed
    And writes a user turn telling the model the person approved and to make the same call again

  Scenario: Error — version not tested
    Given "claude --version" prints "2.1.299 (Claude Code)"
    When start is called
    Then start returns AdapterError::HarnessUntested with found "2.1.299"
    And no session child is spawned and the MCP Front has no session for the run

  Scenario: Error — resume with the wrong approval
    Given the run is paused on approval "A1"
    When resume is called with approve for approval "A2"
    Then resume fails with code CLA_RESUME_MISMATCH and the run stays paused

  Scenario: Error — the CLI exits during a turn
    Given the fake exits with status 1 before printing a result
    Then the last event is ended with reason "failed"
    And the MCP Front and the Hook Relay sessions for the run are closed
    And the session directory is removed

  Scenario: Error — the plan allowance is used up
    Given the fake prints a rate_limit_event reporting the allowance used up and then a result with is_error true
    Then the last event is ended with reason "failed" and the service's message as final_message

  Scenario: Cancel stops the child within the deadline
    Given the fake ignores SIGTERM
    When cancel is called
    Then the child is killed no later than 10 seconds after SIGTERM
    And the last event is ended with reason "cancelled"

  Scenario: Agent profile — organisation key, no sign-in, reading only
    Given the profile is the agent profile with KeyRoute Environment
    When start is called
    Then "claude auth status" and "claude auth login" are never run
    And the arguments contain "--tools Read" and "--disallowedTools Write,Edit"
    And ANTHROPIC_API_KEY is present only in the Claude Code child's environment

  Scenario: Agent profile — a pause saves the session record and another instance resumes
    Given the run pauses on approval "A1" in the agent profile
    Then the child is ended and SessionRecordStore.save is called with the runtime session id
    When a second adapter instance receives resume approve "A1" for the run
    Then it loads the record, starts the child with "--resume <runtime_session_id>" and emits resumed

  Scenario: Conformance — init event tool list at the pinned version (run against the real CLI in the harness test job)
    When a session starts at the pinned Claude Code version with DESKTOP_BUILT_IN_TOOLS
    Then the system init event's tool list contains exactly those built-in names plus the MCP tools registered
    And the result event carries the fields is_error, result and usage

  Scenario: Personal server — sign-in status read from the CLI
    Given "claude auth status --json" exits 0 with loggedIn false
    When the program calls sign_in_status
    Then the reply is SignedOut
    And no file under the person's Claude configuration folder is read

  Scenario: A gateway reaches Claude Code through its environment
    Given the agent profile's model settings name the gateway "https://models.gateway.example.com"
    When a hosted run starts
    Then the Claude Code child's environment holds ANTHROPIC_BASE_URL with that address
    And no model variable other than the key and the declared settings is set

  Scenario: A declared setting reaches the harness child only (ADR-107)
    Given the configuration declares model.environment SERVICE_REGION as the literal "eu-1"
    When a run starts in the agent profile
    Then the Claude Code child environment holds SERVICE_REGION=eu-1
    And the server's own environment is unchanged and no log line holds the value

  Scenario: A secret setting reaches the child and its name alone is reported (ADR-107)
    Given model.environment names TOKEN as {secret: MODEL_TOKEN} and the secret exists
    When a run starts in the agent profile
    Then the child receives TOKEN with the secret's value and the start-up report shows TOKEN without it

  Scenario: A provider other than the default with no key (ADR-107)
    Given the model settings carry no key and model.environment declares SERVICE_REGION
    When a run starts in the agent profile
    Then the child's environment holds SERVICE_REGION and no ANTHROPIC_API_KEY

  Scenario: A reserved name is refused at construction (ADR-107)
    Given the model settings' environment names CLAUDE_CONFIG_DIR
    When new is called
    Then it fails with CLA_ENV_RESERVED naming CLAUDE_CONFIG_DIR
```

## Performance / Security / Observability

- **Performance targets:** `start` returns within 3 s p95 when signed in (two CLI checks of ≤ 10 s timeout each dominate; typical 600 ms each *(assumed here)*). A stdout line or hook event becomes a `RunEvent` within 50 ms p95, so the first progress reaches the interface within 1 s (07 NFR). Adapter memory per run ≤ 8 MiB excluding the child (event buffer ≤ 10,000 events, stderr ring 64 KiB). Up to 8 concurrent sessions on the desktop (A-04) and `ORDIN_RUN_CONCURRENCY` (default 10) per run instance in the agent profile.
- **Security:** No inbound entry point; callers are in-process. Every `StartRun` and `ResumeDecision` field is checked against the 2C limits before use. The child's environment is built from an allow-list; credential variables are removed (09 ADR-044). `ORDIN_HOOK_SECRET`, `ORDIN_MCP_TOKEN`, the organisation key, the `models.environment` values (ADR-107), `PackageMcpServer.env` values, the session record bytes, stdin text, stdout content, stderr content and the `email` field of `claude auth status` are never logged or journalled; session files are mode `0600` in a `0700` directory and removed at session end. The adapter never reads Claude Code's credentials file (2D-1b). `--restricted` keeps the person's own CLAUDE.md, hooks, MCP servers, plugins, skills and memory out (10 C-26).
- **Observability:** `tracing` events with fields `run_id`, `runtime_session_id`, `agent_instance_id`, `workspace_id`, `correlation_id`, `code`: `claude.start.received`, `claude.version.checked` (`harness_version`), `claude.sign_in.status` (`logged_in`, `auth_method`, `api_provider`), `claude.sign_in.started`, `claude.session.spawned` (`pid`), `claude.session.init`, `claude.turn.result` (`is_error`, `model_turns`, `tool_calls`), `claude.run.paused` (`reason_kind`, `approval_id`), `claude.run.resumed`, `claude.run.ended` (`reason`, `exit_status`), `claude.stream.malformed` (`line_len`), `claude.stop.deadline`. Agent profile only (SA-XC-05): `claude.session` per stretch of a run with fields `run_id` and `reason` only is a local `tracing` span only, never exported (Tracing.md T-9: the exported spans are the five of the Telemetry allow list); the exported stretch span `ordin.run_stretch` is the Session Supervisor's. This adapter records no metric (Metrics.md N-1: only the 50 families of the closed list exist (49 before ADR-107)). No metrics or spans on the desktop or the personal server.

## Gaps

None.
