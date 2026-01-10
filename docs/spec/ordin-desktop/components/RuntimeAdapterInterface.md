# SPEC: Runtime Adapter interface

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-runtime-adapter` · **Build phase:** 1 · **Programs:** D P A
> **Dependencies:** Shared types

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

## Purpose

`ordin-runtime-adapter` is the contract crate between the modules that run agents — the Session Supervisor, and through it the Crew Orchestrator and the Ordin Host — and the four adapters that reach a runtime: Claude Local, Copilot Local, Hosted Agent and Slack Thread (`02-architecture.md:494-502`, ADR-003). It holds the `RuntimeAdapter` trait in ordin's caller vocabulary (start, progress, pause, resume, read, cancel), the run event types, the adapter error type, the level-to-model contract the Usage Planner reaches the harness adapters through (2B-2 D9), the registry type the programs pass to the Session Supervisor, and the checks that every adapter's output must pass before the Supervisor acts on it: start-request limits, the order and content of each run's event stream, resume decisions and read snapshots. No module crate depends on a concrete adapter (ADR-100 item 2); each depends on this crate.

## Approach

The crate is a trait plus pure checking code, with one asynchronous helper: a stream wrapper (`guard_events`) that enforces the event order 2C fixes, so every adapter is held to one rule set in one place rather than each adapter checking itself. Adapters are constructed by the programs and registered in an `AdapterRegistry` keyed by `AdapterKind`. The error prefix is `RA_` (2C wire §0). Rejected: *adding a model parameter to every adapter call* — the model is fixed per run, so it belongs in `StartRun`; *letting the Session Supervisor validate events itself* — the Crew Orchestrator and the Ordin Host also consume runs, and three copies of the order rule would drift.

Reconciliation assumptions (decision 80, rulings R6 and R7):

- **SA-RECON-01** — This crate is the provider of the adapter contract (R6); the four adapters, the Session Supervisor, the Usage Planner and the two programs use the names and signatures listed under *Who calls what*, and the 2C wire §2 definitions (decision 80: C-11 to C-14) are the ones copied here (R7).
- **SA-RECON-02** — An adapter reports a failure of its own input check as `AdapterError::InvalidRequest { code }` through `From<CheckError>`, so the check code reaches the Session Supervisor unchanged (2C wire §2, C-13).
- **SA-RECON-04** — Ruling R2: `StartRun` carries the MCP grant context (`caller: ToolCaller`, `scope_id`, `coding_task_id`) that the local adapters copy into the MCP Front's `SessionGrant` (2C wire §2, decision 80). `ToolCaller` lives in `ordin-types` (`ordin_types::session`), because the MCP Front has no build edge to this crate; this crate re-exports it. The Session Supervisor sets `caller` (`Agent` for a task, `CrewMember` for a crew step, `Ordin` for Ordin's own run, `HostedAgent` in the agent profile).
- **SA-RECON-03** — The trait has no session export or import call (decision 80). The harness's session record of a paused agent-profile run travels through the Run Queue: the Claude Local Adapter pushes it to the Session Supervisor's `save_session_record` at a pause and reads it back with `load_session_record` before a resume (C-80). The remote adapters keep the session on the remote side, so there is nothing to carry.

## Shared Context

### Types placed in `ordin-types` and re-exported here (SharedTypes spec, Internal Logic step 5)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    ClaudeLocal,
    CopilotLocal,
    /// The wrightrun HTTP Adapter; the Hosted Agent Adapter from revision 0.43.0.
    HostedAgent,
    SlackThread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterVerb { Start, Progress, Pause, Resume, Read, Cancel }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunTarget {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunLimits {
    pub wall_clock_secs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
    pub idle_timeout_secs: u32,
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
    /// Present only when the source carried a real total ("12 of 40"); absent → "12 so far".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// 2C wire §2 (`StartRun`), placed in `ordin-types` (`wire::run`) by 2C wire §1 *Placement*
/// (decision 80: C-11, C-17); this crate re-exports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLevel { Automatic, Smaller, Standard, MostCapable }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel { Low, Automatic, High }

/// The model a run starts on. `model` is the harness's own model name (`"sonnet"`, a Copilot
/// model name); `None` means the harness's own automatic selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ModelChoice {
    pub level: ModelLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
```

From `ordin-types` (domain part): `RunId`, `WorkspaceId`, `ArtefactId`, `ApprovalId`, `AgentInstanceId`, `RemoteAgentId`, `CrewRunId`, `ScopeId`, `CodingTaskId` (UUIDv7 newtypes, 36-character lowercase hyphenated), `Timestamp` (RFC 3339, UTC, milliseconds, `Z`), `CodedError`, `RepairAction`, and:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued, Running, Paused,
    Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }
```

### Types defined in this crate, copied from 2C wire §2

```rust
#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> AdapterKind;

    /// ADR-003: each adapter states the verbs it cannot offer; the start-up report shows them.
    fn unsupported_verbs(&self) -> &'static [AdapterVerb];

    /// start: a command with arguments, or free text.
    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError>;

    /// progress events and pauses for a decision arrive on one ordered stream per run.
    /// The stream ends after exactly one `RunEvent::Ended`.
    fn events(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError>;

    /// resume with approve or reject.
    async fn resume(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), AdapterError>;

    /// read state and result.
    async fn read(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<RunSnapshot, AdapterError>;

    /// cancel, where the route offers it; otherwise `AdapterError::VerbNotOffered`.
    async fn cancel(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError>;

}

/// Per-call context: correlation identifier and a deadline for this call (not for the run).
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
    /// Harness session identifier (local) or remote run identifier (hosted, Slack).
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
    /// Artefacts attached to the task (04 — Task, *attached artefacts*).
    pub attachments: Vec<ArtefactId>,
    pub limits: RunLimits,
    /// An existing remote thread to continue (hosted agent); absent starts a new thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The model level and model the Usage Planner chose (ADR-062, C-37); absent for remote
    /// adapters, whose model is the remote side's. 2C wire §2, decision 80: C-11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelChoice>,
    /// The thinking setting the Usage Planner chose; absent uses the harness's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
    /// Who the session's ordin tools act for; the local adapters copy it, with `scope_id` and
    /// `coding_task_id`, into the MCP Front's `SessionGrant` (2C wire §2; SA-RECON-04).
    /// Remote adapters ignore these three fields.
    pub caller: ToolCaller,
    /// The hosted agent's scope (agent profile); absent elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    /// The coding task whose worktree the session works in (Project Service); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
}

// `ToolCaller` is defined in `ordin-types` (`ordin_types::session`, 2C wire §2) and re-exported
// below; the MCP Front imports it from `ordin-types` too.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartInput {
    Command { name: String, arguments: String },
    FreeText { text: String },
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
    /// The harness's own to-do list, where the adapter can read it (ADR-068 item 4, C-41).
    HarnessPlan { steps: Vec<String> },
    Paused { reason: PauseReason },
    Resumed,
    Usage { usage: UsageDelta },
    Ended { end: RunEnd },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PauseReason {
    /// The Rules Engine decided *ask*; the Approval Service holds the card.
    Approval { approval_id: ApprovalId },
    /// The harness is signed out (ADR-044).
    SignIn,
    /// A question for the person (a member's question, a person-decided condition).
    Person { question: String },
    /// Slack route: the decision is made in Slack (ADR-009); the link opens the thread.
    DecisionElsewhere { link: url::Url },
    /// Hosted agent: waiting for the hosted agent's approvers, who decide on their own cards;
    /// no `Answer` is accepted. 2C wire §2, decision 80: C-14.
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
    /// The final message, plain text, at most 32,768 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,
    pub artefacts: Vec<ArtefactId>,
}

/// The ended states of `Run`; the subset of `RunState` an adapter may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEndReason {
    Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResumeDecision {
    Approve { approval_id: ApprovalId },
    Reject {
        approval_id: ApprovalId,
        /// Told to the model so it can continue.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Answer to `PauseReason::Person`.
    Answer { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub state: RunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<RunEnd>,
    /// When the route was last read; drives "last checked 20 seconds ago" (11 §5.4).
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
    /// The request failed the adapter's own validation; `code` is one of `RA_TARGET_MISMATCH`,
    /// `RA_COMMAND_UNKNOWN`, `RA_INPUT_TOO_LARGE`, `RA_HANDLE_INVALID`, `RA_RESUME_MISMATCH`.
    /// 2C wire §2, decision 80: C-13.
    #[error("invalid request")] InvalidRequest { code: String },
    /// The remote side refused the request (no access, unknown agent, refused resume); `code` is
    /// the adapter's journalled code. 2C wire §2, decision 80: C-13.
    #[error("refused")] Refused { code: String },
}
```

### Validation rules copied from 2C wire §2 (LW-9 to LW-16)

| Field | Rule | Error code |
|---|---|---|
| call of an unsupported verb | refused without contacting the route | `RA_VERB_NOT_OFFERED` |
| `runtime_session_id` | 1–256 characters | `RA_HANDLE_INVALID` |
| `remote_thread_id` | 1–256 characters; required for `hosted_agent` and `slack_thread` once started | `RA_HANDLE_INVALID` |
| `input.name` | 1–64 characters, the name of a task the instance declares | `RA_COMMAND_UNKNOWN` |
| `input.arguments` | 0–16,384 characters | `RA_INPUT_TOO_LARGE` |
| `input.text` | 1–16,384 characters | `RA_INPUT_TOO_LARGE` |
| `attachments` | at most 50; each readable under the run's folder grants | `RA_ATTACHMENT_NOT_GRANTED` |
| `limits.wall_clock_secs` | 1–28,800 | `RA_LIMIT_OUT_OF_RANGE` |
| `limits.idle_timeout_secs` | 30–3,600 | `RA_LIMIT_OUT_OF_RANGE` |
| `target` | `remote_agent` only for `hosted_agent` and `slack_thread`; `agent_instance` only for local adapters | `RA_TARGET_MISMATCH` |
| `limits.request_budget` | absent, or ≥ 1 | `RA_LIMIT_OUT_OF_RANGE` |
| `model` | absent for `hosted_agent` and `slack_thread`; `model.model`, when present, accepted by the harness's `HarnessModels::accepts_model` | `RA_MODEL_UNKNOWN` |
| stream order | exactly one `started` first and one `ended` last | `RA_EVENT_ORDER` |
| `HarnessPlan.steps` | 3–7 items, each 1–80 characters; otherwise dropped, run continues with events only | `RA_PLAN_INVALID` (journal only) |
| `Person.question` | 1–500 characters | `RA_EVENT_INVALID` |
| `NoReply` | only from `slack_thread`; `Abandoned` only from `hosted_agent` and `slack_thread` | `RA_EVENT_INVALID` |
| `ResumeDecision` reason | 0–500 characters | `RA_INPUT_TOO_LARGE` |
| `ResumeDecision` approval | must be the approval the run paused on | `RA_RESUME_MISMATCH` |
| `Answer.text` | 1–4,000 characters | `RA_INPUT_TOO_LARGE` |
| `RunSnapshot.end` | present if and only if `state` is an ended state | `RA_SNAPSHOT_INCONSISTENT` |
| unknown `AdapterKind` on read | closed set | `RA_ADAPTER_UNKNOWN` |

Verbs not offered per adapter (2C wire §2, from `02-architecture.md:504-568`): `claude_local` none; `copilot_local` none; `hosted_agent` none from revision 0.43.0; `slack_thread` `pause`, `resume`, `cancel`, `progress`.

### Level-to-model inputs

- Levels shown to the person (ADR-062, `09-decisions.md:1290-1291`): *Automatic*, *Smaller and quicker*, *Standard*, *Most capable*. Wire spellings `automatic`, `smaller`, `standard`, `most_capable` (2D-2 `usage.levels_allowed.<kind>`).
- Organisation profile key `usage.level_models.<harness>.<level>` (2D-2), read by the Usage Planner: default for Claude Code `smaller` → Haiku, `standard` → Sonnet, `most_capable` → Opus; for Copilot, `automatic` is Copilot's own automatic selection.
- C-37 (bound assertion): "Model and thinking level per session; usage reported" — carried by the Usage Planner, Claude Local Adapter and Copilot Local Adapter.
- C-31 (bound assertion): ACP permission requests and IDE support — no first-release use in this interface.

## Public Interface

### Re-exports

```rust
pub use ordin_types::wire::run::{
    AdapterKind, AdapterVerb, RunTarget, RunLimits, RunProgress, ProgressCount,
    ModelLevel, ThinkingLevel, ModelChoice,
};
pub use ordin_types::session::ToolCaller;
pub use ordin_types::domain::runs::RunState;
pub use ordin_types::domain::identity::HarnessKind;
```

### Who calls what

Every public item below, with the components that implement or call it (each consumer's spec uses these exact names; SA-RECON-01).

| Item | Implemented by | Called or used by |
|---|---|---|
| `RuntimeAdapter` (all seven methods) | Claude Local Adapter, Copilot Local Adapter, Hosted Agent Adapter, Slack Thread Adapter | Session Supervisor (and, through it, Crew Orchestrator and Ordin Host) |
| `StartRun` (with `model`, `thinking`, `caller`, `scope_id`, `coding_task_id`) | built by the Session Supervisor from the Usage Planner's choice and the run's origin | the four adapters; the local adapters copy `caller`, `scope_id`, `coding_task_id` into the MCP Front's `SessionGrant` |
| `PauseReason::RemoteApprovers` | emitted by the Hosted Agent Adapter | Session Supervisor |
| `AdapterError::InvalidRequest`, `AdapterError::Refused` | raised by the four adapters | Session Supervisor |
| `ModelLevel`, `ThinkingLevel`, `ModelChoice`, `ToolCaller` (re-exported from `ordin-types`) | — | Usage Planner (returns the choice), Session Supervisor, Crew Orchestrator, local adapters |
| `HarnessModels` | Claude Local Adapter, Copilot Local Adapter | Usage Planner (given `Vec<Arc<dyn HarnessModels>>` by the program); Ordin Desktop and Ordin Server programs construct and pass them |
| `AdapterRegistry` (`new`, `register`, `get`) | held inside the Session Supervisor; the Ordin Desktop and Ordin Server programs hand it each adapter through `SessionSupervisor::register_adapter` | Session Supervisor (`register` from `register_adapter`; `get` before every adapter call) |
| `AdapterRegistry::kinds`, `AdapterRegistry::verbs_not_offered`, `check_handle` | — | no caller in this spec set (the Session Supervisor builds start-up report lines from `RuntimeAdapter::unsupported_verbs`) |
| `check_start`, `check_verb`, `check_resume`, `check_snapshot`, `guard_events`, `end_state` | — | Session Supervisor (Internal Logic step 12) |
| `CheckError` → `AdapterError::InvalidRequest` (`From`) | — | the four adapters, for their own input checks |
| `ScriptedAdapter` (feature `testkit`) | — | no component spec in this set uses it; offered to the tests of any crate that depends on this one |

### Types defined here (2C wire §2, decision 80: C-11) and *chosen here*

```rust
/// A harness's map from level to model name. A level absent from the map uses the harness's
/// automatic selection.
pub type LevelModels = std::collections::BTreeMap<ModelLevel, String>;

/// Implemented by each local harness adapter (Claude Local, Copilot Local) and given to the
/// Usage Planner by the program (2B-2 D9). Remote adapters do not implement it: a remote
/// agent's model is the remote side's.
pub trait HarnessModels: Send + Sync {
    fn harness(&self) -> HarnessKind;
    /// The map used when the organisation profile has no `usage.level_models.<harness>` entry.
    fn default_level_models(&self) -> LevelModels;
    /// True when the harness accepts a model per session (C-37). When false, `StartRun.model`
    /// is ignored by the adapter and the run records `ModelChoice { level, model: None }`.
    fn per_session_model(&self) -> bool;
    /// True when `name` is a model this harness version accepts; false makes the Usage Planner
    /// fall back to the harness's automatic selection and journal `RA_MODEL_UNKNOWN`.
    fn accepts_model(&self, name: &str) -> bool;
}

/// The adapters a program constructed, kept by the Session Supervisor (filled through
/// `SessionSupervisor::register_adapter`).
#[derive(Default, Clone)]
pub struct AdapterRegistry { /* private: HashMap<AdapterKind, Arc<dyn RuntimeAdapter>> */ }

impl AdapterRegistry {
    pub fn new() -> Self;
    /// Refuses a second adapter of the same kind (`RA_ADAPTER_DUPLICATE`).
    pub fn register(&mut self, adapter: std::sync::Arc<dyn RuntimeAdapter>) -> Result<(), RegistryError>;
    /// `RA_ADAPTER_UNKNOWN` when no adapter of `kind` was registered in this program.
    pub fn get(&self, kind: AdapterKind) -> Result<std::sync::Arc<dyn RuntimeAdapter>, RegistryError>;
    pub fn kinds(&self) -> Vec<AdapterKind>;
    /// Every (kind, verb) pair not offered, for the start-up report's `verb_not_offered` items.
    pub fn verbs_not_offered(&self) -> Vec<(AdapterKind, AdapterVerb)>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("adapter kind already registered")] Duplicate { kind: AdapterKind },
    #[error("adapter kind not registered")] Unknown { kind: AdapterKind },
}

/// A failed check of adapter input or output.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct CheckError {
    pub code: &'static str,
    /// The field that failed, as a dotted path (`input.arguments`), for Diagnostics only.
    pub field: &'static str,
}

/// One item of a guarded event stream.
#[derive(Debug, Clone)]
pub enum Guarded {
    /// An event that passed every check, in stream order.
    Event(RunEvent),
    /// An event dropped, or a synthetic end added, with the reason; the caller journals it.
    Violation(CheckError),
}
```

### Functions

```rust
/// Checks a start request against the limits above and the target rule for `kind`.
/// Does not check that a command name exists or that attachments are granted: the Session
/// Supervisor checks those against the agent instance and the folder grants before calling.
pub fn check_start(kind: AdapterKind, req: &StartRun) -> Result<(), CheckError>;

/// Refuses a verb `adapter` declares unsupported, without contacting the route.
pub fn check_verb(adapter: &dyn RuntimeAdapter, verb: AdapterVerb) -> Result<(), AdapterError>;

/// Checks a run handle's identifiers once `started` has been seen.
pub fn check_handle(handle: &RunHandle) -> Result<(), CheckError>;

/// Checks a resume decision against the reason the run paused on.
pub fn check_resume(decision: &ResumeDecision, paused_on: &PauseReason) -> Result<(), CheckError>;

/// Checks that a snapshot's `end` matches its `state`.
pub fn check_snapshot(snapshot: &RunSnapshot) -> Result<(), CheckError>;

/// Wraps an adapter's event stream so it yields only events in the order 2C fixes.
pub fn guard_events(
    kind: AdapterKind,
    run_id: RunId,
    inner: futures::stream::BoxStream<'static, RunEvent>,
) -> futures::stream::BoxStream<'static, Guarded>;

/// Maps an ended reason to the `RunState` the Session Supervisor stores.
pub fn end_state(reason: RunEndReason) -> RunState;

impl CodedError for AdapterError { /* see Error Table */ }
/// An adapter's own input check becomes `AdapterError::InvalidRequest { code }` (SA-RECON-02).
impl From<CheckError> for AdapterError { /* InvalidRequest { code: e.code.to_string() } */ }
impl CodedError for CheckError { /* code = self.code; retryable = false */ }
impl CodedError for RegistryError { /* RA_ADAPTER_DUPLICATE / RA_ADAPTER_UNKNOWN; not retryable */ }
```

### Feature `testkit` *(chosen here)*

```rust
/// A scripted adapter for other crates' tests: `start` returns a handle, `events` replays
/// `script` in order, `read` returns `snapshot`, unsupported verbs return VerbNotOffered.
pub struct ScriptedAdapter {
    pub kind: AdapterKind,
    pub unsupported: &'static [AdapterVerb],
    pub script: Vec<RunEvent>,
    pub snapshot: Option<RunSnapshot>,
    pub start_error: Option<AdapterError>,
}
impl RuntimeAdapter for ScriptedAdapter { /* as above */ }
```

Enabled only through a crate's `[dev-dependencies]`; never in a program's normal build (the arch test reads `cargo metadata` without dev edges).

##### Example

```rust
use ordin_runtime_adapter::*;

let req = StartRun {
    run_id: RunId::parse("0192a1c2-3344-7556-8677-8899aabbccdd")?,
    workspace_id: WorkspaceId::parse("0192a0f1-1111-7222-8333-444455556666")?,
    target: RunTarget::AgentInstance {
        agent_instance_id: AgentInstanceId::parse("0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff")?,
    },
    input: StartInput::Command { name: "compare-quotes".into(), arguments: "Kestrel, Acme and Northwind Q3 quotes".into() },
    attachments: vec![],
    limits: RunLimits { wall_clock_secs: 3600, request_budget: None, idle_timeout_secs: 600 },
    thread: None,
    model: Some(ModelChoice { level: ModelLevel::Standard, model: Some("sonnet".into()) }),
    thinking: None,
    caller: ToolCaller::Agent,
    scope_id: None,
    coding_task_id: None,
};
check_start(AdapterKind::ClaudeLocal, &req)?;                       // Ok
assert_eq!(check_start(AdapterKind::HostedAgent, &req).unwrap_err().code, "RA_TARGET_MISMATCH");

let adapter = registry.get(AdapterKind::SlackThread)?;
assert!(matches!(check_verb(adapter.as_ref(), AdapterVerb::Cancel),
                 Err(AdapterError::VerbNotOffered { verb: AdapterVerb::Cancel })));
```

JSON of `StartRun.model`: `{"level":"standard","model":"sonnet"}`; JSON of a guarded violation as the Session Supervisor journals it: `{"code":"RA_EVENT_ORDER","field":"events"}`.

## Internal Logic

1. **`check_start(kind, req)`.**
   1. Target rule: `RunTarget::RemoteAgent` with `ClaudeLocal`/`CopilotLocal`, or `RunTarget::AgentInstance` with `HostedAgent`/`SlackThread` → `RA_TARGET_MISMATCH` (field `target`).
   2. Input: `Command.name` 1–64 scalar values and matching `^[a-z0-9][a-z0-9_-]{0,63}$` *(assumed here: task names in agent folders are kebab-case file stems)* → `RA_COMMAND_UNKNOWN` (field `input.name`); `Command.arguments` 0–16,384 → `RA_INPUT_TOO_LARGE`; `FreeText.text` 1–16,384 → `RA_INPUT_TOO_LARGE`. Lengths count Unicode scalar values after `ordin_types::text::normalise_text`; a control character → `TEXT_CONTROL_CHAR`.
   3. `attachments.len() > 50` → `RA_INPUT_TOO_LARGE` (field `attachments`) *(assumed here: 2C names no code for the count; the grant check keeps `RA_ATTACHMENT_NOT_GRANTED`)*.
   4. Limits: `wall_clock_secs` outside 1–28,800, `idle_timeout_secs` outside 30–3,600, or `request_budget == Some(0)` → `RA_LIMIT_OUT_OF_RANGE`.
   5. `thread`: present only for `HostedAgent` and 1–256 characters → otherwise `RA_HANDLE_INVALID` (field `thread`).
   6. `model`: when present with `model: Some(name)`, `name` 1–128 characters of `[A-Za-z0-9._:-]` → `RA_MODEL_INVALID` *(assumed here: covers Claude aliases and Copilot model identifiers; the length matches LW-17's tool-name bound)*. A `model` given for `HostedAgent` or `SlackThread` → `RA_MODEL_UNKNOWN` (2C wire §2 validation table). `thinking` is not checked: a remote adapter ignores it.
   7. Grant context: `caller` `HostedAgent` or a present `scope_id` with any adapter other than a local one in the agent profile is not checked here (the program knows the profile); `CrewMember` requires nothing more. `coding_task_id` is passed through unchecked; the MCP Front refuses an unknown one when it builds the grant.
   8. Return `Ok(())`. No logging: the caller journals the refusal with the run.
2. **`StartRun.model` and `StartRun.thinking` (2C wire §2, decision 80: C-11).** The Usage Planner chooses a level and a thinking setting per step and each harness adapter maps the level to a model (ADR-062, C-37). Both fields are optional and skipped when absent, so a JSON body without them still parses. A local adapter that receives `model: Some(..)` and whose `per_session_model()` is true passes the model to the harness at session start; when false, it ignores the field and emits `RunEvent::Usage` without a level. The Session Supervisor records `UsageCounters.model_level` from `ModelChoice.level`.
3. **`check_verb(adapter, verb)`.** If `adapter.unsupported_verbs()` contains `verb`, return `AdapterError::VerbNotOffered { verb }`; else `Ok`. The Session Supervisor calls it before `cancel` and before `resume`; it never calls `cancel` on an adapter that lists it.
4. **`check_handle(handle)`.** `runtime_session_id` when present 1–256 characters; `remote_thread_id` when present 1–256 characters; for `HostedAgent` and `SlackThread`, `remote_thread_id` absent → `RA_HANDLE_INVALID`.
5. **`check_resume(decision, paused_on)`.**
   1. `Approve { approval_id }` or `Reject { approval_id, .. }` against `PauseReason::Approval { approval_id: p }`: `approval_id != p` → `RA_RESUME_MISMATCH`.
   2. `Approve`/`Reject` against any other pause reason → `RA_RESUME_MISMATCH`.
   3. `Answer { text }` against anything but `PauseReason::Person` → `RA_RESUME_MISMATCH`; `text` 1–4,000 → `RA_INPUT_TOO_LARGE`.
   4. `Reject.reason` over 500 characters → `RA_INPUT_TOO_LARGE`.
   5. Against `PauseReason::SignIn`, `DecisionElsewhere` or `RemoteApprovers`, every decision → `RA_RESUME_MISMATCH` (a sign-in resumes by the adapter itself once the harness reports signed in; a Slack decision is made in Slack; the hosted agent's approvers decide on their own cards and the hosted agent resumes the run itself).
6. **`check_snapshot(snapshot)`.** Ended states are `Completed`, `Failed`, `StoppedAtLimit`, `Cancelled`, `Interrupted`, `Abandoned`, `NoReply`. `end.is_some()` must equal "state is ended" → otherwise `RA_SNAPSHOT_INCONSISTENT`. When both present, `end_state(end.reason)` must equal `state` → otherwise `RA_SNAPSHOT_INCONSISTENT`.
7. **`guard_events(kind, run_id, inner)`** — a state machine over `{BeforeStart, Running, Ended}`:
   1. `BeforeStart` + `Started` → yield it, go to `Running`. `BeforeStart` + any other event → drop it, yield `Violation(RA_EVENT_ORDER)`.
   2. `Running` + `Started` → drop, `Violation(RA_EVENT_ORDER)`.
   3. `Running` + `HarnessPlan { steps }`: steps outside 3–7, or any step outside 1–80 characters → drop, `Violation(RA_PLAN_INVALID)`; the run continues.
   4. `Running` + `Progress { progress }`: `line` outside 1–160 characters, or `count.total < count.done` → drop, `Violation(RA_EVENT_INVALID)`.
   5. `Running` + `Paused { Person { question } }` with `question` outside 1–500 → drop, `Violation(RA_EVENT_INVALID)`. `Running` + `Paused { RemoteApprovers }` from any adapter but `HostedAgent`, or `Paused { DecisionElsewhere }` from any adapter but `SlackThread` → drop, `Violation(RA_EVENT_INVALID)` *(chosen here: each reason belongs to one route, 2C wire §2 comments)*.
   6. `Running` + `Ended { end }`: `NoReply` from any adapter but `SlackThread`, or `Abandoned` from a local adapter → replace the reason with `Failed`, yield `Violation(RA_EVENT_INVALID)` then the corrected `Ended`; `final_message` over 32,768 characters → truncated to 32,768 and `Violation(RA_EVENT_INVALID)`. Go to `Ended`.
   7. `Ended` + any event → drop, `Violation(RA_EVENT_ORDER)`.
   8. Inner stream finishes in `BeforeStart` or `Running` → yield `Violation(RA_EVENT_ORDER)` then a synthetic `Ended { end: RunEnd { reason: Interrupted, final_message: None, artefacts: vec![] } }`.
   9. Every violation is logged once at `warn` with `run_id`, `adapter` and `code` (never the event's text).
   The guard never reorders events and never buffers more than one event.
8. **`end_state(reason)`** maps each `RunEndReason` variant to the `RunState` variant of the same name.
9. **`AdapterRegistry`.** `register` reads `adapter.kind()`; a kind already present → `RegistryError::Duplicate`. `get` clones the `Arc`. `verbs_not_offered` iterates kinds in the order `ClaudeLocal`, `CopilotLocal`, `HostedAgent`, `SlackThread` and each adapter's `unsupported_verbs()` in declaration order, so the start-up report is stable.
10. **`AdapterError` as `CodedError`.** See the Error Table for each variant's code, retryability and repair action. `InvalidRequest { code }` and `Refused { code }` return their own `code`; neither is retryable; no repair action.
11. **Session records (C-80).** The trait carries no session record. In the agent profile the Claude Local Adapter saves the harness's session record of a paused run through the Session Supervisor's `save_session_record` and reads it back through `load_session_record` before a resume; the Run Queue carries it between instances. The size limit of a record is the Run Queue's (`RUN_SESSION_RECORD_TOO_LARGE`); this crate does not check it.
12. **What the Session Supervisor calls, in order.** Before `start`: `check_start(kind, &req)`. Around `events`: `guard_events(kind, run_id, stream)`, journalling each `Guarded::Violation`. Before `cancel` and `resume`: `check_verb(adapter, verb)`; before `resume`, also `check_resume(&decision, &paused_on)`. After `read`: `check_snapshot(&snapshot)`. On `Ended`: `end_state(end.reason)` gives the stored `RunState`. Adapters are reached only through `AdapterRegistry::get(kind)`.

## Data Model

N/A — the crate owns no table. `RunHandle` is journalled by the Session Supervisor in the `runs` table's identifier columns (`runtime_session_id`, `remote_thread_id`); `ModelChoice.level` is stored by the Session Supervisor as `UsageCounters.model_level` (snake_case level name).

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Verb in the adapter's unsupported list | `Err(permanent)` | `RA_VERB_NOT_OFFERED` | `AdapterError::VerbNotOffered { verb }` |
| Harness signed out | `Err(permanent)`, repair `SignInAgain` | `RA_SIGNED_OUT` | `AdapterError::SignedOut` |
| Harness version not in the tested list | `Err(permanent)`, repair `SendToIt` | `RA_HARNESS_UNTESTED` | `AdapterError::HarnessUntested { found }` |
| Route unreachable (network, process failed to start) | `Err(retryable)`, repair `TryAgain` | `RA_UNREACHABLE` | `AdapterError::Unreachable` |
| Plan allowance exhausted (harness limit message) | `Err(permanent)` | `RA_ALLOWANCE_EXHAUSTED` | `AdapterError::AllowanceExhausted { service_message }` |
| Remote thread busy | `Err(retryable)` | `RA_THREAD_BUSY` | `AdapterError::ThreadBusy` |
| Adapter does not know the run | `Err(permanent)` | `RA_UNKNOWN_RUN` | `AdapterError::UnknownRun` |
| An adapter's own input check fails | `Err(permanent)` | the check's code (`RA_TARGET_MISMATCH`, `RA_COMMAND_UNKNOWN`, `RA_INPUT_TOO_LARGE`, `RA_HANDLE_INVALID`, `RA_RESUME_MISMATCH`) | `AdapterError::InvalidRequest { code }` |
| The remote side refused the request | `Err(permanent)` | the adapter's journalled code | `AdapterError::Refused { code }` |
| A pause reason from the wrong route (`remote_approvers` not from `hosted_agent`, `decision_elsewhere` not from `slack_thread`) | `Guarded::Violation` | `RA_EVENT_INVALID` | `CheckError { code, field: "reason" }` |
| Call deadline passed | `Err(retryable)`, repair `TryAgain` | `RA_DEADLINE` | `AdapterError::Deadline` |
| Target kind does not fit the adapter | `Err(permanent)` | `RA_TARGET_MISMATCH` | `CheckError { code, field: "target" }` |
| Command name malformed | `Err(permanent)` | `RA_COMMAND_UNKNOWN` | `CheckError { code, field: "input.name" }` |
| Arguments, text, attachment count, reason or answer too long | `Err(permanent)` | `RA_INPUT_TOO_LARGE` | `CheckError { code, field }` |
| Wall-clock, idle timeout or request budget out of range | `Err(permanent)` | `RA_LIMIT_OUT_OF_RANGE` | `CheckError { code, field: "limits" }` |
| Thread, session or remote identifier malformed or missing | `Err(permanent)` | `RA_HANDLE_INVALID` | `CheckError { code, field }` |
| Model name malformed | `Err(permanent)` | `RA_MODEL_INVALID` | `CheckError { code, field: "model.model" }` |
| Model given to a remote adapter | `Err(permanent)` | `RA_MODEL_UNKNOWN` | `CheckError { code, field: "model" }` |
| Model name not accepted by the harness (Usage Planner falls back) | journal only | `RA_MODEL_UNKNOWN` | — |
| Resume decision does not answer the pause | `Err(permanent)` | `RA_RESUME_MISMATCH` | `CheckError { code, field: "decision" }` |
| Snapshot end and state disagree | `Err(permanent)` | `RA_SNAPSHOT_INCONSISTENT` | `CheckError { code, field: "end" }` |
| Event before `started`, after `ended`, a second `started`, or a stream with no `ended` | `Guarded::Violation` | `RA_EVENT_ORDER` | `CheckError { code, field: "events" }` |
| Harness plan outside 3–7 steps or 1–80 characters | `Guarded::Violation` | `RA_PLAN_INVALID` | `CheckError { code, field: "steps" }` |
| Progress line, question, final message or end reason invalid | `Guarded::Violation` | `RA_EVENT_INVALID` | `CheckError { code, field }` |
| Second adapter of one kind registered | `Err(permanent)` | `RA_ADAPTER_DUPLICATE` | `RegistryError::Duplicate { kind }` |
| No adapter of the kind in this program; unknown kind on read | `Err(permanent)` | `RA_ADAPTER_UNKNOWN` | `RegistryError::Unknown { kind }` |
| An `AttachmentNotGranted` found by the Session Supervisor | `Err(permanent)` | `RA_ATTACHMENT_NOT_GRANTED` | raised by the Session Supervisor with this code |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Runtime Adapter interface

  Scenario: Happy path — a guarded stream passes through unchanged
    Given a ScriptedAdapter of kind claude_local scripted with started, progress "Read supplier quotes" 12 of 40, ended completed
    When the Session Supervisor reads guard_events over its event stream
    Then it receives exactly those three events in that order and no violation

  Scenario: Error path — an event before started
    Given a stream that yields progress before started
    When it is guarded
    Then the progress event is dropped
    And a violation with code RA_EVENT_ORDER is yielded first

  Scenario: Error path — a stream that ends without ended
    Given a stream that yields started and then finishes
    When it is guarded
    Then a violation with code RA_EVENT_ORDER is yielded
    And a synthetic ended event with reason interrupted follows

  Scenario: Edge case — a harness plan of two steps is dropped, the run continues
    Given a running stream that yields harness_plan with steps ["Read", "Write"]
    When it is guarded
    Then a violation with code RA_PLAN_INVALID is yielded
    And the next progress event is still delivered

  Scenario: Error path — no_reply from a local adapter
    Given a copilot_local stream that ends with reason no_reply
    When it is guarded
    Then a violation with code RA_EVENT_INVALID is yielded
    And the ended event carries reason failed

  Scenario: Error path — a remote agent given to a local adapter
    Given a StartRun whose target is remote_agent
    When check_start is called for claude_local
    Then it fails with code RA_TARGET_MISMATCH

  Scenario: Edge case — the longest allowed arguments
    Given a StartRun whose command arguments are 16,384 characters
    When check_start is called for claude_local
    Then it succeeds
    And with 16,385 characters it fails with code RA_INPUT_TOO_LARGE

  Scenario: Error path — cancel on the Slack route
    Given the slack_thread adapter, which lists cancel as unsupported
    When check_verb is called with cancel
    Then it returns VerbNotOffered with code RA_VERB_NOT_OFFERED
    And the adapter's cancel method is not called

  Scenario: Error path — approving a different card
    Given a run paused on approval 0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f
    When check_resume is called with approve for approval 0192a1c3-0000-7000-8000-000000000001
    Then it fails with code RA_RESUME_MISMATCH

  Scenario: Error path — an inconsistent snapshot
    Given a snapshot with state running and an end with reason completed
    When check_snapshot is called
    Then it fails with code RA_SNAPSHOT_INCONSISTENT

  Scenario: Error path — two adapters of one kind
    Given a registry holding a claude_local adapter
    When a second claude_local adapter is registered
    Then registration fails with code RA_ADAPTER_DUPLICATE

  Scenario: Error path — answering a run that waits for the hosted agent's approvers
    Given a hosted_agent run paused with reason remote_approvers
    When check_resume is called with answer "Go ahead"
    Then it fails with code RA_RESUME_MISMATCH

  Scenario: Error path — remote_approvers from a local adapter
    Given a running claude_local stream that yields paused with reason remote_approvers
    When it is guarded
    Then the paused event is dropped
    And a violation with code RA_EVENT_INVALID is yielded

  Scenario: Error path — a model given to a remote adapter
    Given a StartRun with target remote_agent and model {"level":"standard","model":"sonnet"}
    When check_start is called for hosted_agent
    Then it fails with code RA_MODEL_UNKNOWN

  Scenario: Happy path — an adapter's own check becomes InvalidRequest
    Given a CheckError with code RA_HANDLE_INVALID
    When it is converted into an AdapterError
    Then the result is InvalidRequest with code RA_HANDLE_INVALID and CodedError::code returns RA_HANDLE_INVALID

  Scenario: Happy path — a crew step's grant context reaches the adapter
    Given a StartRun with caller {"kind":"crew_member","crew_run_id":"0192a1c5-0000-7000-8000-000000000001"} and no scope_id or coding_task_id
    When it is serialised
    Then the JSON carries "caller":{"kind":"crew_member","crew_run_id":"0192a1c5-0000-7000-8000-000000000001"}
    And it carries no "scope_id" or "coding_task_id" key
    And check_start for claude_local succeeds

  Scenario: Happy path — verbs not offered for the start-up report
    Given a registry with claude_local and slack_thread adapters
    When verbs_not_offered is called
    Then it returns slack_thread pause, resume, cancel and progress, in the order the adapter declares them
```

## Performance, Security, Observability

- **Performance targets:** `guard_events` adds at most 10 µs p95 per event and holds at most one event in memory; `check_start` at most 50 µs p95 on 16,384-character arguments (dominated by NFC normalisation). Measured with `criterion` (`cargo bench -p ordin-runtime-adapter`).
- **Security:** the crate never logs the text of an input, an event, a question, an answer or a final message; violation logs carry only `run_id`, `adapter` and `code`. `AdapterError::AllowanceExhausted.service_message` is the harness's own sentence and is shown only through Localisation's wrapper (R-29), never logged. `#![forbid(unsafe_code)]`.
- **Observability:** one log event, `ra.event_violation` at `warn`, fields `run_id`, `adapter`, `code`. No metric or span on the desktop and the personal server (SA-XC-04, SA-XC-05); in the agent profile the Session Supervisor owns the run span, and this crate adds none.

## Gaps

None.
