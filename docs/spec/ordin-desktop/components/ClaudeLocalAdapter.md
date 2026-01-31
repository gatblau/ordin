# SPEC: Claude Local Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended 2026-01-10 (SA-RECON-06 the person's own input, SA-RECON-08 the adopted run's limit; approved by the user on 2026-01-10) · amended January 2026 (`--model` from `StartRun.model` when `SessionSetup.model` is `None`, CLA-REC-1; Approved by the user 2026-01-10) · amended January 2026 (the start request's attachments passed to `session_setup`, CLA-REC-2; Approved by the user 2026-01-10) · amended January 2026 (folder paths in their plain form on Windows; Approved by the user 2026-01-29) · amended January 2026 (Ordin's conversation stays open between turns, CLA-CONV-1..6; Approved by the user 2026-01-29) · amended January 2026 (an answer of up to 16,384 characters, CLA-ANS-1; Approved by the user 2026-01-29) · amended January 2026 (the folder of `claude_path` on the child's `PATH`, CLA-PATH-1; user decision)
> **Mode:** greenfield
> **derivedFromHld:** 0.66.0
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
- [Amendment January 2026 — The encoded agent folder on Windows](#amendment-november-2026--the-encoded-agent-folder-on-windows)
- [Amendment January 2026 — Ordin's conversation stays open between turns](#amendment-november-2026--ordins-conversation-stays-open-between-turns)
- [Amendment January 2026 — An answer carries a person's whole message](#amendment-november-2026--an-answer-carries-a-persons-whole-message)

## Purpose

The Claude Local Adapter runs one local agent session on the Claude Code CLI and presents it to the core as a `RuntimeAdapter` (02:507–516; 09 ADR-038, ADR-044, ADR-045). On the desktop and the personal server it drives the person's installed, unmodified Claude Code with the person's own sign-in; in the agent profile it drives the organisation-installed Claude Code with the organisation's model key (09 ADR-097 item 2). For each run it checks the harness's version and sign-in, starts one CLI process in headless streamed-JSON mode with the agent folder as its working folder, registers the MCP Front's servers and the ordin hook for that session only, passes the agent's instructions and plugin produced by the Claude Code Installer (10 C-61), and turns the CLI's output and the Hook Relay's session events into the ordered `RunEvent` stream the Session Supervisor consumes. It owns the session processes and the mapping of CLI events to run events (02:515).

## Approach

**Closing assumption** (*chosen here*): **SA-CLOSE-01** (closes G-CLA-3; *amended January 2026, ADR-107*) — in the agent profile this adapter holds no provider-specific code. What the harness's own provider mode needs is the configuration's `model.environment`, which Model Policy resolves into `HarnessModelSettings.environment` (ModelPolicy.md MP-2a); this adapter sets each declared name in the Claude Code child's environment and nowhere else (step 8). A `gateway_url` is given to the child as `ANTHROPIC_BASE_URL`, the variable Claude Code's documentation names for a custom model endpoint. C-77's question is closed by the local test T1 of January 2026: with Claude Code 2.1.306, a base address and a key given in the child's environment were honoured, every request reached the address given and no sign-in was used. No model variable other than the key, `ANTHROPIC_BASE_URL` and the declared names is set. Which of the child's own processes may see these values is C-74, answered once for the key and every declared name alike (SA-HOST-04).

**Reconciliation assumption** (decision 80): **SA-RECON-01** — the organisation key and the fallback model reach this adapter as Model Policy's `HarnessModelSettings`, and the launch paths as the Installers' `HarnessLaunch::ClaudeCode`, both mapped by the program into `SignInMode` and `SessionSetup` (rows 249, 251). **SA-RECON-02** — the run-start decision is the Rules Engine's `begin_run`; the run-end decision is made by the Hook Relay on the `Stop` hook, and this adapter never calls it (R5). **SA-RECON-03** — `StartRun.caller`, `scope_id` and `coding_task_id` are copied unchanged into the MCP Front's `SessionGrant` (R2). **SA-RECON-04** — MCP traffic reaches the MCP Front through `ordin-hook mcp-bridge` on the MCP Front's own socket (R8, X2); the hook flags are the ordin hook's (`--harness claude-code`, X3). **SA-RECON-05** (*chosen here*) — nothing a package declares enters `mcp.json`, because Claude Code expands `${NAME}` there (see *Claude Code CLI facts used*). Each package server is written with `command` = `hook_program` and `args` = `["mcp-launch", "--spec", "<session directory>/package-server-<n>.json"]` and no `env`; the launch file, mode `0600`, holds `{"program", "args", "env"}` and `mcp-launch` reads it (OrdinHook.md I). **SA-RECON-06** (*chosen here*; *amended 2026-01-10*) — free text whose first non-space character is `/` is written with the line "This is a message. It is text, not a command:" before it, unless `StartRun.started_by_person` is true (RuntimeAdapterInterface.md SA-RA-INPUT-01). `caller` and `run_starter` cannot tell a person's own input from text an agent, a crew, a chat or a schedule wrote (`caller` is `Agent` for every task, and `run_starter` is `None` on the desktop), so the Session Supervisor sets `started_by_person` instead: true only for a task the person started themselves outside the agent profile (SessionSupervisor.md SA-SUP-INPUT-01). With it true, the text is the person's own command and is written as typed, so the owner's typed `/command` runs; with it false (a crew member's step, Ordin's conversation seed, a task an agent delegated, every agent-profile run, a chat asker's message among them) the line is written. `begin_run` sees typed free text as text with `command: None`; a command the run-start decision must see by name goes through `StartInput::Command`. At most 64 ended runs stay in memory: a new entry evicts the longest-ended past that, after which `read` and `events` are `UnknownRun`; a stream already open keeps its run. **SA-RECON-08** (*chosen here*; *amended 2026-01-10*) — a second instance adopts a run only when all of these hold: the handle's `runtime_session_id` is the canonical lower-case text of a UUID, since it joins a path (`session_record_path`); `RulesEngine::adopted_run` has a record (RulesEngine.md SA-RECON-12), which gives `agent_instance_id`, `workspace_id`, `scope`, `tool_subset`, `single_call` and `limit_seconds`, so the session has the first instance's workspace, scope, narrowed `--tools` and its own time limit; `SessionSetup.paused` is present, which the program fills from the run's row only while the run is paused on an approval, and gives `approval_id` and `caller`, so a late decision on an ended run is refused and `caller` reaches the `SessionGrant` as SA-RECON-03 says; the decision names that approval; and `SessionRecordStore::load` has a record, which the Run Queue hands over only with a run taken after its approval was decided. No record, no `paused`, a bad identifier or a scope that does not parse → `CLA_UNKNOWN_RUN`; another approval → `CLA_RESUME_MISMATCH`; the engine unreachable → `CLA_RULES_UNAVAILABLE`, retryable; the record store failing → `CLA_SESSION_RECORD_MISSING`, as it reports. The record's `limit_seconds` lowers the adopted session's wall-clock limit as `start` step 6 does, counted in full from the adoption (*chosen here*: the instance that paused the run records no time used against it); `None` leaves the Session Supervisor's limits alone, as at `start`. The adopted run's `coding_task_id` is `None`, which is what the first instance granted: adoption happens only in the agent profile, where no run has a coding task (the Project Service runs on the desktop and the personal server only, and the Session Supervisor builds every agent-profile `StartRun` with none). An adopted run writes no first turn, so its `started_by_person` is false.

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
    /// The person's own input (SA-RECON-06); set by the Session Supervisor, false when absent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub started_by_person: bool,
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

2C validation rows this adapter enforces at `start`/`resume`: `target` must be `agent_instance` (`RA_TARGET_MISMATCH`); `input.name` 1–64 characters (`RA_COMMAND_UNKNOWN`); `arguments` 0–16,384 and `text` 1–16,384 characters (`RA_INPUT_TOO_LARGE`); `ResumeDecision` `approval_id` equal to the one the run paused on (`RA_RESUME_MISMATCH`); `reason` ≤ 500 characters, `Answer.text` 1–16,384 characters (amended January 2026, CLA-ANS-1; `RA_INPUT_TOO_LARGE`). Each is returned as `AdapterError::InvalidRequest { code }` (2C C-13).

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
    /// {"mcpServers": {"<name>": {"type":"stdio","command":"…","args":["mcp-bridge","<server>","--agent-instance","<uuid>"],"env":{…}}}}
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

- Flags, from `claude --help` of Claude Code 2.1.316 (re-read January 2026; first read on 2.1.304, January 2026; every flag below keeps the meaning given here): `-p`, `--input-format stream-json`, `--output-format stream-json`, `--verbose`, `--restricted` (removes Bash, PowerShell, the other code-running tools and WebFetch unless `--tools` names them; ignores user, project and local settings files; `--settings` still applies; confines file tools to the working directories; refuses `bypassPermissions`), `--strict-mcp-config`, `--mcp-config`, `--settings`, `--permission-prompts none` (anything that would prompt is denied), `--session-id <uuid>`, `--resume <session id>`, `--tools`, `--disallowedTools`, `--model`, `--effort` (`low`, `medium`, `high`, `xhigh`, `max`), `--fallback-model`, `--plugin-dir`, `--append-system-prompt[-file]` (the `-file` form is named only in the help text of `--bare`; it was run on 2.1.316 and honoured), `--bare` (never used). Built-in tool names visible there: `Bash`, `Edit`, `Read`, `WebFetch`, `PowerShell`; `Write` from 10 C-26. Run on 2.1.316: `--tools TodoWrite` gives no tool; the init event and `--tools` name `Task`, and the hook payload and the model request name the same tool `Agent`.
- `claude auth status --json` fields `loggedIn` (bool), `authMethod`, `apiProvider`, `projectsDirectory` (observed January 2026 on 2.1.304 and January 2026 on 2.1.316, which exits 1 with the same JSON when signed out; 09 ADR-044); `claude auth login` completes with no terminal (10 C-25).
- `claude --version` prints `2.1.316 (Claude Code)`.
- Streamed-JSON stdin: one line per person's turn, `{"type":"user","message":{"role":"user","content":"<text>"}}`; stdout events `system`/`init` per turn, `assistant`, `user` (tool result), `rate_limit_event`, one `result` per turn; `--resume <session id>` continues in a new process (08:81; 10 C-19).
- Under `--restricted` nothing is read from the agent folder; the Claude Code Installer writes `instructions.md` (passed with `--append-system-prompt-file`) and `plugin/` (passed with `--plugin-dir`); commands are then named `<agent>:<command>` (02:513, 02:457; 10 C-61; R-94).
- `--tools` with an empty value, run on 2.1.316 under `--restricted --strict-mcp-config`: the init event's `tools` is `[]` and the model request carries no tool; a call of `Read` is answered `Error: No such tool available: Read. Read is disabled for this session, in subagents as well as here.` Without the flag the session has the default set, so the empty value is kept on the command line and the flag is never dropped (`crates/ordin-claude-adapter/tests/harness.rs`). The empty value removes built-in tools only; MCP tools stay and are held by the hook.
- `"onFailure": "block"` on a `PreToolUse` command hook, run on 2.1.316: the tool does not run when the program is missing, exits with a status other than 0 or 2, dies on a signal, outlasts `timeout`, or prints JSON that fails validation; the model is told `... failed; blocking because onFailure is "block"` (`timed out; blocking ...` for the timeout). Without the setting the same failures run the tool, and an unknown value of the key is ignored without a message and behaves the same. A hook that exits 0 with no output, or with output that is not JSON, passes the call: the setting does not cover it, so `ordin-hook` must end every decision in the deny or allow form (OrdinHook.md). Exit 2 blocks with or without the setting. A `--settings` path that does not exist ends the CLI at start with exit 1 (`crates/ordin-claude-adapter/tests/harness.rs`).
- A `--plugin-dir` folder under `--restricted --strict-mcp-config`, run on 2.1.316: `hooks/hooks.json` is loaded, and its `SessionStart`, `PreToolUse` and `PostToolUse` command hooks run; `.mcp.json` is not loaded, because of `--strict-mcp-config` (without that flag its server starts and registers as `plugin:<plugin>:<server>`, with or without `--restricted`). Commands register as `<plugin>:<command>`. A plugin hook's `allow` does not lift a `--settings` hook's `deny` or a failure under `"onFailure": "block"`.
- `--mcp-config` values, run on 2.1.316: `${NAME}` in a stdio server's `args` and `env` is replaced with the CLI's own variable before the server starts, so `${ORDIN_HOOK_SECRET}` and `${ANTHROPIC_API_KEY}` arrived as their values; a bare `$NAME` is not replaced.
- Hooks in front matter, run on 2.1.316 under `--restricted --strict-mcp-config` (`crates/ordin-claude-adapter/tests/harness.rs`): a plugin skill's `PreToolUse`, `PostToolUse` and `Stop` hooks run as shell commands when the skill is invoked; `SessionStart`, `UserPromptSubmit` and `SubagentStop` declared the same way did not run in that session. A plugin command's front-matter hooks did not run for any of those events.
- Stdio MCP servers, run on 2.1.316 under `--restricted --strict-mcp-config`: a server inherits the whole environment of Claude Code whether `mcp.json` gives `env`, an empty `env` or none; `env` only adds names. Observed names include `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, every variable of the CLI's own environment, and the CLI's additions `CLAUDECODE`, `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_SESSION_ID` and `CLAUDE_PROJECT_DIR`. So a package server also receives what `start` step 8 gives the child, the hook variables included.
- A hook `allow` and `--restricted`, run on 2.1.316: `allow` does not lift the confinement of the file tools. `Read`, `Write`, `Glob` and `Grep` outside the working folder and `--add-dir` folders are refused with `<path> is outside <folders>; --restricted confines the file tools ...`, with the hook answering `allow` or absent; a file in an `--add-dir` folder is read. `Bash` is not confined: with `--tools` naming it and a hook `allow`, a shell command read a file outside the working folder (with no hook it is denied, since nothing can answer the prompt).

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
    /// Agent profile: the run as the Session Supervisor holds it, while it is paused on an approval;
    /// `None` for any other run, an ended one included. A second instance adopts only such a run
    /// (SA-RECON-08).
    pub paused: Option<PausedRun>,
}

/// What the program reads from the run's row for a run another instance paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PausedRun {
    pub approval_id: ApprovalId,
    pub caller: ToolCaller,
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
  "disableSkillShellExecution": true,
  "hooks": {
    "PreToolUse":  [{"matcher": "*", "hooks": [{"type": "command", "command": "/Users/asha/Library/Application Support/ordin/bin/ordin-hook --harness claude-code --event pre-tool-use --agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff", "timeout": 60, "onFailure": "block"}]}],
    "PostToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": "/Users/asha/Library/Application Support/ordin/bin/ordin-hook --harness claude-code --event post-tool-use --agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff", "timeout": 60}]}],
    "Stop":        [{"hooks": [{"type": "command", "command": "/Users/asha/Library/Application Support/ordin/bin/ordin-hook --harness claude-code --event stop --agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff", "timeout": 60}]}]
  }
}
```

A hook program path is written as it stands on macOS and Linux when it holds only ASCII letters, digits and the characters `/ . _ - :`; any other path, such as one with the space of "Application Support", is quoted in single quotes (a single quote inside the path written `'\''`), because Claude Code gives `command` to a shell and an unquoted space splits the path, which with `onFailure: block` blocks every tool call. On Windows Claude Code runs the command through Git Bash, which reads `\` as an escape, so every path is written with `/` in place of `\`, inside double quotes, with `"`, `$` and `` ` `` escaped by a backslash: `C:\o\ordin-hook.exe` is written `"C:/o/ordin-hook.exe"` *(amended January 2026: written unquoted with `\`, the path reached Git Bash as `C:oordin-hook.exe` and every tool call was refused)*. The same rule applies to the paths of the `apiKeyHelper` line. The example above shows the unquoted form for readability. The `hooks`/`matcher`/`type`/`command`/`timeout` keys follow Claude Code's documented settings form *(assumed here: the HLD names `--settings`, `PreToolUse` and `onFailure` but not the surrounding keys; the C-26 spike used this registration)*. With `KeyRoute::SettingsApiKeyHelper` the file also holds `"apiKeyHelper": "<hook_program> api-key --run <run_id> --data-root <app data root>"` and the adapter writes the key to `key` in the session directory, mode `0600` (OrdinHook.md `api-key` mode). `--harness claude-code`, `--event` and `--agent-instance` are the ordin hook's own flags (OrdinHook.md; X3).

`mcp.json` (passed with `--mcp-config`): `SessionMcpConfig::to_mcp_servers_json()` with each `PackageMcpServer` added under its own name as a stdio server, as SA-RECON-05 states; a package server whose name equals an MCP Front server name is refused (`CLA_SERVER_NAME_CLASH`).

### Command line (desktop and personal server)

```
claude -p --input-format stream-json --output-format stream-json --verbose
       --session-id <runtime_session_id>            (new session)   | --resume <runtime_session_id> (continued)
       --restricted --strict-mcp-config
       --mcp-config <dir>/mcp.json --settings <dir>/settings.json
       --permission-prompts none
       --append-system-prompt-file <SessionSetup.append_system_prompt_file>   (when the file exists)
       --plugin-dir <SessionSetup.plugin_dir>                                 (when the folder exists)
       --tools Read,Edit,Write,Glob,Grep,Task[,Bash]                  (Bash only when shell_allowed)
       [--add-dir <dir>]...                                           (one per extra_dirs entry)
       [--model <model>] [--effort <level>]
```

Working folder: the agent folder. `--bare` is never passed (09 ADR-044). The built-in tool list `Read,Edit,Write,Glob,Grep,Task` is the constant `DESKTOP_BUILT_IN_TOOLS`, checked by the init-event conformance test below at the pinned version. `TodoWrite` left the list in January 2026: Claude Code 2.1.316 no longer has it, and `--tools` ignores an unknown name with no error. Shell, code-running and web tools stay off because `--restricted` removes them and `--tools` does not name them (02:510).

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
6. **Run-start decision.** `rules.begin_run(RunStartFacts { run_id, agent_instance_id, workspace_id, crew_charter_id: `SessionSetup.crew_charter_id`, command and text from `StartRun.input`, scope: `StartRun.scope_id` as text, asker and started_by: in the agent profile from `SessionSetup.run_starter` — `Caller` → "caller", `ScheduleEntry` → "schedule", `Intake` → "intake", `ChatConnection { asker, .. }` → "chat" with `asker: Some(asker)`, otherwise `asker: None`; elsewhere `asker: None` and started_by "person" | "crew" | "trigger" by `StartRun.caller`, started_by_id })`. Without a `run_starter`, `ToolCaller::Ordin` and `ToolCaller::Agent` give "person" and the agent instance's id as text, `ToolCaller::CrewMember { crew_run_id }` gives "crew" and the crew run's id as text, and `ToolCaller::HostedAgent` gives "trigger" and the agent instance's id as text *(chosen here: `ToolCaller` has no variant that says a trigger started a session, and these are the identifiers that stay the same across that caller's runs)*. The `RunStarter` variants give "caller", "schedule", "intake" and "chat", with `caller_id`, `entry`, `intake` and `asker` as the identifier. `Proceed` → continue, with `command` replacing the first message's command when present, `tool_subset` narrowing `--tools`, `single_call` ending the run after the first `result`, and `limit_seconds` lowering the wall-clock limit. `Ended { ending }` → emit `Ended { Completed, final_message: ending }` (no process started). `Replied { text }` → emit `Ended { Completed, final_message: text }`. `RulesError` → `CLA_RULES_UNAVAILABLE`, emit `Ended { Failed }` (fail closed).
7. **Register the session.** `mcp.open_session(SessionGrant { run_id, workspace_id, agent_instance_id, harness: ClaudeCode, profile, caller: StartRun.caller, scope_id: StartRun.scope_id, coding_task_id: StartRun.coding_task_id })` → `SessionMcpConfig`; `MCP_SESSION_EXISTS` is mapped to `CLA_MCP_OPEN_FAILED` permanent, other errors retryable. `hooks.open_session(HookSessionSpec { run_id, agent_instance_id, workspace_id, harness: ClaudeCode })` → `HookSessionEnv`; error → `CLA_HOOK_OPEN_FAILED` (retryable), after calling `mcp.close_session`. Subscribe `hooks.session_events(run_id)`.
8. **Write session files and build the environment.** Create the session directory and write `settings.json` and `mcp.json` (mode `0600`); a write failure → `CLA_CONFIG_WRITE_FAILED`. Build the child environment from an empty map *(chosen here, 2D-1c "never inherits wholesale")*: copy only `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_ALL`, `TMPDIR`, `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` (either case), and on Windows `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `SystemRoot`, `TEMP` (`PATH` with the folder of `claude_path` added at the end when missing, CLA-PATH-1, *amended January 2026 (user decision; Approved by the user January 2026)*); add the four pairs of `HookSessionEnv::env_vars()`; in the agent profile with `KeyRoute::Environment` and `models.key` present add `ANTHROPIC_API_KEY`; in the agent profile with `models.gateway_url` present add `ANTHROPIC_BASE_URL` = that URL (SA-CLOSE-01); in the agent profile add each `models.environment` entry, in order (*amended January 2026, ADR-107*; none can replace a name above, `new` having refused reserved names). `ANTHROPIC_API_KEY` (other profiles), `CLAUDE_CODE_OAUTH_TOKEN` and `CLAUDE_CONFIG_DIR` are never present (09 ADR-044; 2D-1c). Declared names are set in this child's environment only, never in ordin's own process environment; no value and no copy of the environment map is logged.
9. **Agent profile resume.** When `resume_session_id` is present and the profile is the agent profile, call `records.load(run_id, runtime_session_id)`; `None`, or a `RecordError` (its code logged), → emit `Ended { Interrupted }`, code `CLA_SESSION_RECORD_MISSING`. Otherwise write the record to `<projectsDirectory>/<encoded agent folder>/<runtime_session_id>.jsonl` *(assumed here: Claude Code's session-record location; `projectsDirectory` is a field of `claude auth status --json` on 2.1.304 and 2.1.316, conditional on 10 C-80, whose fallback is that the run is recorded as interrupted and not started again, 09 ADR-098 item 3)*. The agent profile never runs `claude auth status`, so the adapter does not read that field: with `CLAUDE_CONFIG_DIR` absent from the child, as step 8 requires, 2.1.316 reports `<HOME>/.claude/projects`, and the adapter uses that path, taking `HOME` (otherwise `USERPROFILE`) from ordin's own environment. A host-supplied `projects_root` replaces it. `<encoded agent folder>` is the canonical path of the agent folder with every character that is not an ASCII letter or digit replaced by `-` (observed on 2.1.316).
10. **Spawn.** `tokio::process::Command` with the command line of Public Interface, working folder = agent folder, stdin/stdout piped, stderr piped to a 64 KiB ring buffer (never logged; after a failure its last 2 KiB stay with the run entry, because this crate has no edge to a Diagnostics sink *(chosen here)*), `kill_on_drop(true)`, new process group on Unix. Spawn error → `CLA_SPAWN_FAILED` (`RA_UNREACHABLE`, retryable). Log `claude.session.spawned` with `run_id`, `runtime_session_id`, `pid`, `harness_version`.
11. **First turn.** Build the text: `Command` → `/<plugin_name>:<name> <arguments>` (R-94; trailing space trimmed when arguments are empty); `FreeText` → the text, with SA-RECON-06 (written as typed when `StartRun.started_by_person` is true). When `attachment_paths` is not empty, append one line per path: `Attached: <path>` *(chosen here)*. Write one stream-json `user` line and flush. Write failure → see "CLI exits".
12. Return `RunHandle { run_id, adapter: ClaudeLocal, runtime_session_id: Some(..), remote_thread_id: None }`. Steps 1–8 and 10–12 complete within `ctx.deadline`; past it → `AdapterError::Deadline` after removing everything registered so far (`mcp.close_session`, `hooks.close_session`, kill child, remove the session directory).

### Sign-in flow (desktop, personal server; 09 ADR-044; 10 C-25)

1. Emit `Paused { SignIn }`. Spawn `claude auth login` with stdin null, no terminal, same environment as step 8 without hook variables. Log `claude.sign_in.started`.
2. Every 5 s *(chosen here)* run `claude auth status --json`; on `loggedIn: true`, kill the login process if still alive, emit `Resumed`, continue at `start` step 6. After a sign-in no caller is waiting for the `Err` results of steps 6 to 10, so such a failure is logged as `claude.run.ended` with its code at warn level, the run emits `Ended { Failed }` with no final message, and the session closes. 
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

`HarnessPlan` (conditional on 10 C-41): where the `assistant` stream carries the harness's to-do list *(the `TodoWrite` tool input, assumed here)*, the adapter emits `HarnessPlan { steps }` with each step's text; when C-41 shows no such content, no `HarnessPlan` is emitted and the run shows events only (09 ADR-068 item 4). Claude Code 2.1.316 shows no such content, so no `HarnessPlan` arrives from it; the path stays for other tested versions, and an agent states its plan through `work.plan`.

### `resume(ctx, run, decision)`

1. Unknown `run_id` → `UnknownRun`, except that in the agent profile a run another instance paused is adopted first (SA-RECON-08). Run not paused → `CLA_NOT_PAUSED` (`RA_RESUME_MISMATCH`).
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
| Rules Engine unavailable while adopting a run (SA-RECON-08) | `Err(transient)` | `CLA_RULES_UNAVAILABLE` | `AdapterError::Unreachable` |
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

  Scenario: Agent profile — an adopted run keeps its workspace, scope and tool subset
    Given the Rules Engine recorded workspace "W", scope "S" and tool subset "Read" at the run's start
    When a second adapter instance receives resume approve "A1" for the run
    Then the MCP Front session opens for "W" and "S"
    And the arguments contain "--tools Read"

  Scenario: Agent profile — a run with no record cannot be adopted
    Given the Rules Engine has no record of the run
    When a second adapter instance receives resume approve "A1" for the run
    Then the call fails with CLA_UNKNOWN_RUN

  Scenario: Agent profile — an ended run cannot be adopted
    Given the program reports no paused run for the run in SessionSetup.paused
    When a second adapter instance receives resume approve "A1" for the run
    Then the call fails with CLA_UNKNOWN_RUN

  Scenario: Agent profile — a decision for another approval does not adopt the run
    Given SessionSetup.paused names approval "A2"
    When a second adapter instance receives resume approve "A1" for the run
    Then the call fails with CLA_RESUME_MISMATCH and no run entry is kept

  Scenario: Free text beginning with a slash is plain text
    Given the free text is "/analyst:deploy"
    And StartRun.started_by_person is false
    When start is called
    Then the first turn does not begin with "/"
    And the command runs only when the input is StartInput::Command

  Scenario: The owner's typed command runs (SA-RECON-06, amended 2026-01-10)
    Given the free text is "/analyst:deploy now"
    And StartRun.started_by_person is true
    When start is called
    Then the first turn is "/analyst:deploy now"

  Scenario: Agent profile — an adopted run keeps its own time limit (SA-RECON-08, amended 2026-01-10)
    Given the Rules Engine's record of a paused run has limit_seconds 0
    When a second adapter instance adopts the run and resumes it with approve "A1"
    Then the run ends stopped_at_limit
    And the session's SessionGrant has no coding_task_id

  Scenario: Ended runs are evicted past the limit
    Given 64 runs have ended
    When another run starts
    Then the longest-ended run is UnknownRun to read

  Scenario: A package server starts through the launcher
    Given a package server whose argument is "${ORDIN_HOOK_SECRET}"
    When start writes mcp.json
    Then its command is the hook program and its args are "mcp-launch --spec <launch file>"
    And mcp.json holds no "$"

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

## Amendment January 2026 — The Usage Planner's model from the start request

> **Status:** Approved (user, 2026-01-10). Counterpart: `components/OrdinDesktop.md`, amendment January 2026 — Phase 11 reconciliation (DSK-REC-7; SPEC-GAP-10 (a)).

- **CLA-REC-1** The Session Supervisor puts the Usage Planner's choice in `StartRun.model` (`ModelChoice { level, model }`), but this adapter builds `--model` only from `SessionSetup.model`, and a program's `SessionSetupSource` receives no `WorkKind` to ask the Usage Planner itself. In `start`, when `SessionSetup.model` is `None` and `StartRun.model` is `Some(ModelChoice { model: Some(name), .. })`, the adapter passes `--model <name>`; otherwise the command is built as before. `SessionSetup.model`, when set, still wins. `StartRun.thinking` is not mapped to `--effort` by this amendment.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the start request's model reaches the command
    Given SessionSetup.model is None and StartRun.model names "sonnet"
    When the adapter starts the session
    Then the command carries --model sonnet

  Scenario: Edge case — the session setup's model wins
    Given SessionSetup.model is "opus" and StartRun.model names "sonnet"
    When the adapter starts the session
    Then the command carries --model opus
```

### Gaps

None.

## Amendment January 2026 — The start request's attachments passed to the session setup

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations"). Counterparts: `components/CopilotLocalAdapter.md` CPA-REC-1 (the same change to its own trait), `components/OrdinDesktop.md`, amendment January 2026 — Follow-ups of 2026-01-10 (DSK-REC-29, closing OQ-REC-7 as the user chose it on 2026-01-10: crew members receive the documents they are given as attachments, not only tasks).

- **CLA-REC-2** `SessionSetupSource::session_setup` gains a third argument, the start request's attachments:

  ```rust
  #[async_trait::async_trait]
  pub trait SessionSetupSource: Send + Sync {
      /// `attachments`: `StartRun.attachments` of the run being started, in the order given
      /// (at most 50 after `check_start`, 2C). The program turns them into
      /// `SessionSetup.attachment_paths`; the adapter does not read them itself.
      async fn session_setup(&self, run_id: RunId, agent_instance_id: AgentInstanceId,
                             attachments: &[ArtefactId])
          -> Result<SessionSetup, SessionSetupError>;
  }
  ```

  In `start`, step 2 reads: "Call `setup.session_setup(run_id, agent_instance_id, &req.attachments)` within `ctx.deadline`", with every error mapping of step 2 unchanged. The adapter passes the slice as it received it; it does not remove duplicates, check the count or read any artefact (the Runtime Adapter Interface's `check_start` has already refused more than 50, `RA_INPUT_TOO_LARGE`). `SessionSetup.attachment_paths` keeps its meaning and its comment ("already checked against the run's folder grants"), and step 11 still names each path in the first turn. Each program's implementation takes the new argument: the desktop program reads it as DSK-REC-29 states; the server program (OrdinServer.md, the `SessionSetupSource` row) takes the argument too; how it fills `attachment_paths` is not changed by this amendment.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the start request's attachments reach the session setup (CLA-REC-2)
    Given a StartRun whose attachments are artefacts A and B
    When the adapter starts the session
    Then session_setup is called with attachments [A, B] in that order

  Scenario: Edge case — a run with no attachments (CLA-REC-2)
    Given a StartRun whose attachments are empty
    When the adapter starts the session
    Then session_setup is called with an empty attachments slice
```

### Gaps

None.

## Amendment January 2026 — The encoded agent folder on Windows

*Amended January 2026 (Approved by the user 2026-01-29). On Windows the operating system resolves a folder to the verbatim form `\\?\C:\...`, which the command interpreter refuses (it starts in `C:\Windows` instead) and which no `file:///C:/...` address lies under. The user's decision of 29 January 2026: a resolved folder path keeps its resolved meaning but is kept and handed on in the plain form `C:\...` whenever that form names the same folder.*

- **CLA-PATH-1** The canonical path of the agent folder from which `<encoded agent folder>` is made (Internal Logic 9) is passed through `ordin_types::paths::plain_form` (SharedTypes.md ST-PATH-1) before each character that is not an ASCII letter or digit becomes `-`. On Windows Claude Code names the folder from its own working folder, which is in the plain form `C:\...`; the verbatim form would add leading `-` characters and name a folder Claude Code never reads. On macOS and Linux nothing changes.

```gherkin
  Scenario: Happy path — the session record lands where Claude Code reads it on Windows (CLA-PATH-1)
    Given the agent folder C:\Users\me\ordin\agents\a1
    When the record path of a resumed session is made
    Then the encoded folder is C--Users-me-ordin-agents-a1
```

## Amendment January 2026 — Ordin's conversation stays open between turns

*Amended January 2026 (Approved by the user 2026-01-29). The Session Supervisor (`start_conversation`, J.1) and the Ordin Host (steps 3–5) carry Ordin's conversation as one run whose every turn ends with `Paused { Person { question } }` and whose next turn is `resume_with_decision(Answer { text })`; this adapter ended every run on a `result` with nothing outstanding (*Reading the CLI's output*) and produced no `Person` pause (`resume` step 4), so Ordin's run ended after each turn and a crew Ordin coordinates found no coordinator. The user's decision of 29 January 2026: Ordin's conversation stays open between turns, and the person's next message continues the same run and the same harness session. The amendment makes explicit what the Session Supervisor and the Ordin Host already state; where it differs from the paragraphs above, it supersedes them.*

- **CLA-CONV-1** A **conversation run** is a run whose `StartRun.caller` is `ToolCaller::Ordin` and whose run-start decision did not set `single_call`. The Session Supervisor sets that caller for the origin `RunOrigin::Conversation` only (SessionSupervisor.md SA-CLOSE-02), so Ordin's conversation is the one conversation run; a task (`Agent`), a crew member's step (`CrewMember`) and every agent-profile run (`HostedAgent`) are not, and keep the behaviour above unchanged.
- **CLA-CONV-2** In a conversation run, a `result` with `is_error: false` and no approval outstanding ends the **turn**, not the run: after the `Usage` event the adapter emits `Paused { Person { question } }`, where `question` is the `result` text cut to 16,000 characters (2C wire WIRE-PQ-1), or the empty text when the `result` has none, logs `claude.run.paused` with `reason_kind` `person`, and keeps the process. The run's state is `Paused`. A `result` with `is_error: true` ends the run as before (`CLA_TURN_FAILED`, `CLA_ALLOWANCE_EXHAUSTED`); a `result` while an approval is outstanding stays paused on the approval as before, and the turn that follows the decision ends as this assertion says.
- **CLA-CONV-3** `resume` on a run paused on `Person` (replaces step 4): only `Answer { text }` is accepted, else `CLA_RESUME_MISMATCH`; `text` outside 1–16,384 characters (CLA-ANS-1) → `CLA_INPUT_INVALID` (`RA_INPUT_TOO_LARGE`). The adapter emits `Resumed` and writes `text` as the next user turn of the same harness session, under SA-RECON-06's rule for free text: text whose first non-space character is `/` is written with the line "This is a message. It is text, not a command:" before it (a conversation run's `started_by_person` is false, SessionSupervisor.md SA-SUP-INPUT-01). When the process has ended while paused (CLA-CONV-4), it is restarted as in `start` steps 7–10 with `--resume <runtime_session_id>` before the turn is written, as for an approval.
- **CLA-PROGRESS-1 The first turn of an agent run asks for progress as it happens** *(amended January 2026; found on the 0.1.1 Mac build: the document agent stated five steps with `plan` and then ran Glob, Read, Read and Write without marking one, because nothing it was given asked it to, so the person's card moved only when the run ended)*. For a run whose session `mcp.json` lists the `work` server (the one that carries the `plan` tool; Ordin's own conversation run lists `ordin` and not `work`), `start` step 11 writes after the first turn's text, on a new line, exactly: "Progress: if you have a plan tool, state your 3 to 7 steps with it before any other tool, mark each step now when you start it and done when you finish it, one step at a time as you go, and never mark them all at the end." No other turn gets the line, and a run without the `work` server gets none. *(The first version of this assertion gated on the agent profile, which the desktop never uses, so the line was never written; found by the user's next test.)* The line is not part of `first_turn`'s result for a typed command (`/<plugin>:<name> …` is written as it was).
- **CLA-CONV-4** The 2D-4 rule *Idle harness process kept while paused* (30 minutes) covers a conversation paused on `Person` as it covers a pause on an approval: on the desktop and the personal server, 30 minutes after the `Person` pause the process is closed and the session's registrations removed, the run stays `Paused`, and the record stays where Claude Code wrote it; the next `Answer` restarts it (CLA-CONV-3). A conversation run never reaches *Pausing in the agent profile*, since no agent-profile run has the caller `Ordin`.
- **CLA-CONV-5** A conversation run ends only by the paths that end any run, none of them a turn's end: `cancel` (the Ordin Host's summary, *Start fresh*, put away and workspace removal, the person's **Stop**, and the Session Supervisor's `stop_idle_sessions` on the personal server), `shutdown` at quit, the Rules Engine's `limit_seconds` and the Session Supervisor's wall-clock limit (8 hours of running time for Ordin, OrdinHost.md step 3.3; SessionSupervisor.md D.1), a failed turn, 20 malformed lines, and the CLI exiting unexpectedly. The Session Supervisor's idle timer runs only while a run is `running` (SessionSupervisor.md D.2), so a conversation waiting for the person is not stopped by it.
- **CLA-CONV-6** The run-end decision on the `Stop` hook (SA-RECON-02) is still made at the end of each conversation turn: the Hook Relay asks the Rules Engine at every `Stop`, and an allowed ending lets the turn end; the run continues as CLA-CONV-2 says.

```gherkin
  Scenario: Happy path — Ordin's conversation survives a turn and takes the next message (CLA-CONV-2, CLA-CONV-3)
    Given a run started with caller Ordin and the seed "Hello"
    When Claude Code's turn ends with result "Hi, how can I help?"
    Then the adapter emits Usage then Paused with reason person and question "Hi, how can I help?"
    And the process is still running
    When resume is called with Answer "Write a note"
    Then the adapter emits Resumed and writes "Write a note" to the same process
    When that turn ends with result "Done."
    Then the adapter emits Paused with reason person and question "Done."

  Scenario: Edge — a task run still ends on its turn's result (CLA-CONV-1)
    Given a run started with caller Agent
    When Claude Code's turn ends with result "Done."
    Then the adapter emits Ended with reason completed and final message "Done."

  Scenario: Edge — a conversation turn with no text (CLA-CONV-2)
    Given a conversation run
    When Claude Code's turn ends with a result that has no text
    Then the adapter emits Paused with reason person and an empty question

  Scenario: Edge — the idle process of a waiting conversation is restarted (CLA-CONV-4)
    Given a conversation run paused on person for 30 minutes on the desktop
    Then its process has been closed and the run is still paused
    When resume is called with Answer "Next"
    Then the process is restarted with --resume and the turn "Next" is written

  Scenario: Error — an approval decision against a conversation's turn end (CLA-CONV-3)
    Given a conversation run paused on person
    When resume is called with Approve
    Then resume fails with CLA_RESUME_MISMATCH
```

## Amendment January 2026 — An answer carries a person's whole message

*Amended January 2026 (Approved by the user 2026-01-29). The answer's bound is 1–16,384 characters for every adapter (2C wire WIRE-ANS-1, RuntimeAdapterInterface.md RA-ANS-1).*

- **CLA-ANS-1** `resume` checks an `Answer`'s text against the Runtime Adapter interface's `MAX_ANSWER_CHARS` (16,384), both on a run paused on `Person` (CLA-CONV-3) and on a run paused on `SignIn`; empty or longer → `CLA_INPUT_INVALID` (`RA_INPUT_TOO_LARGE`). The text is written to the session whole.

```gherkin
  Scenario: Edge — a person's longest message reaches Claude Code whole (CLA-ANS-1)
    Given a conversation run paused on person
    When resume is called with an Answer of 16,384 characters
    Then the next user turn written to the process carries all 16,384 characters
    When resume is called with an Answer of 16,385 characters
    Then resume fails with CLA_INPUT_INVALID
```

## Amendment January 2026 — A session's whole process tree ends with it on Windows

> **Status:** amended January 2026 (user decision 2026-01-30, "proceed with recommendations"). Source: the journeys on Windows — after ordin was stopped while an agent worked, the agent's Claude Code went on running, changing the person's folders, through every later start, and kept the data folder open so that **Start with empty data** could not set it aside (`DSK_DATA_ROOT_UNWRITABLE`). On Windows `cancel` step 2 terminated only the process the adapter started; Claude Code installed with npm is started through `claude.cmd`, a command shell whose child does the work, and `kill_on_drop` (Internal Logic 10) acts only when the adapter lets the process go, never when ordin itself ends.

- **CLA-JOB-1 One job object per session on Windows** *(amended January 2026)*. At `start` step 10, right after the spawn, the adapter creates a job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and assigns the child to it; the processes the child starts belong to the job. `cancel` step 2 (SA-STOP-01) and the kill after `stop_deadline` end the job (`TerminateJobObject`), which ends every process of the session; the job's handle is closed when the process is let go, and the operating system closes it when ordin's process ends in any way, ending what is left of the session. Where the operating system refuses the job, the session runs without one and step 2 terminates the started process as before. On macOS and Linux nothing changes (the session has its own process group). The job's calls are the crate's only unsafe code, in one Windows-only module.

```gherkin
  Scenario: Happy path — stopping a session started through a command shell ends Claude Code too (CLA-JOB-1)
    Given a session whose process is a command shell that started Claude Code
    When the session is stopped
    Then the command shell and Claude Code have both ended

  Scenario: Edge case — ordin ends without stopping the session (CLA-JOB-1)
    Given a session is running
    When ordin's process ends without stopping it
    Then the session's processes end with it
```

### Gaps

None.

## Amendment January 2026 — Reading a run this process does not hold is not a warning

> **Status:** amended January 2026 (user decision 2026-01-30, "proceed with recommendations"). Source: the journeys on Windows — every start after a stop wrote one `claude.call.failed` warning (`CLA_UNKNOWN_RUN`) for each conversation or task left open, because the Session Supervisor's *Recovery after restart* reads each run the journal shows in progress and this adapter holds runs in memory only. Logging.md V-1 keeps WARNING for a refused request, a dropped item or a retry; a read is a question, and `UnknownRun` is the answer recovery expects.

- **CLA-LOG-1 A read of an unknown run is logged at INFO** *(amended January 2026)*. `read` for a run this process does not hold still answers `Err(AdapterError::UnknownRun)` (Error table, `CLA_UNKNOWN_RUN`), and logs `claude.read.unknown_run` at INFO with `run_id` and `code`, not `claude.call.failed`. `resume`, `cancel` and `events` for an unknown run are refused requests and keep `claude.call.failed` at WARNING.

```gherkin
  Scenario: Edge case — reading a run this process does not hold is not a warning (CLA-LOG-1)
    Given no run with id R is held by the adapter
    When read is called for R
    Then the call fails with CLA_UNKNOWN_RUN
    And claude.read.unknown_run is logged at INFO
    And no claude.call.failed line is written for R
```

### Gaps

None.

## Amendment January 2026 — The folder of `claude` is on its own PATH

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: ordin opened from Finder or the Dock gets the system's short `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`). The desktop program finds `claude` in the person's own folders when `PATH` lacks it (OrdinDesktop.md DSK-CC-1; SetupAssistant.md SET-R1-7) and passes that path as `ClaudeAdapterConfig.claude_path` (OQ-REC-4 option (b)), but step 8 copied `PATH` unchanged, so a Claude Code installed in `~/.local/bin` or a Homebrew folder started without its own folder on `PATH`, and what it starts from beside itself was not found. Every section above stays in force except where this amendment changes it.

- **CLA-PATH-1 `claude`'s folder on the child's `PATH`** *(amended January 2026)*. Wherever the adapter builds a child environment — step 8's session, the CLI checks (`claude --version`, `claude auth status`) and `claude auth login` — the copied `PATH` gains the folder holding `claude_path` at its end when `claude_path` is absolute and no entry is already that folder; when ordin's own environment has no `PATH`, the child's `PATH` is that folder alone. A relative `claude_path` changes nothing. No other variable changes, and nothing about `PATH` is logged.

```gherkin
  Scenario: Happy path — a session of a Claude Code in the person's own folder (CLA-PATH-1)
    Given claude_path is "/Users/asha/.local/bin/claude" and ordin's PATH is "/usr/bin:/bin"
    When a session starts
    Then the child's PATH is "/usr/bin:/bin:/Users/asha/.local/bin"

  Scenario: Edge case — the folder is already on PATH (CLA-PATH-1)
    Given claude_path is "/bin/claude" and ordin's PATH is "/usr/bin:/bin"
    When the CLI checks run
    Then the child's PATH is "/usr/bin:/bin"
```

### Gaps

None.

## Amendment January 2026 — the person's effort reaches `--effort` (CLA-REC-3) (amended January 2026; user decision; Approved by the user January 2026)

- **CLA-REC-3 `--effort` from `StartRun.effort` (amended January 2026; user decision; Approved by the user January 2026).** `StartRun` gains `effort: Option<String>` (`components/RuntimeAdapterInterface.md` C-37 amendment), the person's effort for the step's kind of work (`components/UsagePlanner.md` SA-AGM-01). In `start`, when `SessionSetup.effort` is `None`, the adapter reads `StartRun.effort` with `ClaudeEffort::parse`, which accepts exactly `low`, `medium`, `high`, `xhigh` and `max` (`ClaudeEffort::ALL`), and passes `--effort <value>`; any other text is dropped and no `--effort` is passed. `SessionSetup.effort`, when set, still wins. `StartRun.thinking` is still not mapped.

```gherkin
Feature: The person's effort reaches the command (CLA-REC-3)

  Scenario: Happy path — a known effort is passed; an unknown one is dropped
    Given the session setup names no effort
    When a run starts with effort "high"
    Then the command carries "--effort high"
    And a run started with effort "extreme" carries no "--effort"
```

## Amendment 2026-11-21 — Named tool servers reach the session grant

> **Status:** Approved by the user, 2026-11-21 ("approve as written"). Every section above stays in force.

- In the step that registers the session with the MCP Front, `SessionGrant.only_servers` is `StartRun.only_servers`, copied unchanged like `caller`, `scope_id` and `coding_task_id` (McpFront.md SA-MCP-SRV-01). An adopted run has `None`.
