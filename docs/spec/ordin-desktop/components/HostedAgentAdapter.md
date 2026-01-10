# SPEC: Hosted Agent Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-hosted-agent-adapter` · **Build phase:** 2 · **Programs:** D P
> **Dependencies:** Identity, Runtime Adapter interface

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

The Hosted Agent Adapter is the Runtime Adapter for agents that run elsewhere as **hosted ordin agents** (an ordin server in the agent profile). It was the wrightrun HTTP Adapter until revision 0.43.0 (02:549–558, 09 ADR-098 item 8). It starts a run on a new or an existing thread of the hosted agent's caller interface with the person's company sign-in (an identity-provider token from Identity and Credentials), follows the run's event stream instead of polling, resumes a paused run with approve or reject, reads state and result, cancels a run, and keeps a second request for a busy thread waiting until the first run on that thread has ended (02:557, wrightrun ADR-064). It lets the Session Supervisor, the Crew Orchestrator and, through them, the Ordin Host use a hosted agent exactly as they use a local one (09 ADR-003).

## Approach

**Closing assumption** (*chosen here*): **SA-CLOSE-01** (closes G-HAA-2; closing decision K15) — wrightrun's spec defines no approver route, so the Caller API serves `GET /v1/approvals?state=pending` and `POST /v1/approvals/{approval_id}/decision` with `{ "decision": "approve" | "reject", "note"?: string }`, the approver's identity taken from the caller token. This adapter is their client: it lists the person's approver cards for *Needs you* and sends the decision; the Caller API passes it to the Approval Service, which applies the asker rule and resumes the run.

**Reconciliation assumptions** (decision 80): **SA-RECON-02** — refusals return 2C `AdapterError::Refused { code }` or `InvalidRequest { code }`, and a pause for the hosted agent's approvers is `PauseReason::RemoteApprovers` (2C C-13, C-14). **SA-RECON-03** — `CompanyTokenSource` is implemented by the program over Identity's audience-token calls (row 425). **SA-RECON-01** — the run's event stream is the Caller API's (`run_state_changed`, `run_progress`, `: keep-alive`, no reset frame, cancel answered *Cancelled*); this adapter reads the run once where the stream carries less (X1, rows 418–423).

The adapter is a stateless-on-disk HTTP client: every fact needed to recover a run after a restart is in the `RunHandle` the Session Supervisor journals (remote run identifier in `runtime_session_id`, thread identifier in `remote_thread_id`), and the adapter keeps only in-memory state (live stream followers and the per-thread waiting queue). Each run is followed by one task that reads the run's Server-Sent Events stream (`reqwest` 0.13.5 with `rustls-platform-verifier` 0.7.1 and `sse-stream` 0.3.0, SA-HTTP-02) and maps each event to a `RunEvent`; when the stream cannot be opened it falls back to reading the run every 5 seconds (SA-RATE-01). Error prefix *chosen here*: `HAA_`; at the `RuntimeAdapter` boundary every failure is an `AdapterError` (`RA_…` codes). Rejected: polling only (the wrightrun behaviour, 02:552) — ADR-098 gives the caller interface an event stream so progress is live; a table of the adapter's own for handles and queues — the crate does not depend on the Journal (2B-3), and the Supervisor's journalled `RunHandle` already carries the identifiers recovery needs.

## Shared Context

### Shared types used (copied from Phase 2C wire part §2 and domain part; crate `ordin-runtime-adapter` and `ordin-types`)

```rust
// ordin-types (domain part of 2C) — identifiers and timestamp
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`
pub struct RunId(pub uuid::Uuid);            // UUIDv7
pub struct WorkspaceId(pub uuid::Uuid);
pub struct AgentInstanceId(pub uuid::Uuid);
pub struct RemoteAgentId(pub uuid::Uuid);
pub struct ApprovalId(pub uuid::Uuid);
pub struct ArtefactId(pub uuid::Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued, Running, Paused,
    Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteAgent {
    pub id: RemoteAgentId,
    pub display_name: String,
    pub route: RemoteRoute,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_channel: Option<String>,
    /// Name of the identity-provider settings entry in the active organisation profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_provider_ref: Option<String>,
    pub status: RemoteAgentStatus,
    pub listed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRoute { Http, Slack }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentStatus { Listed, Enabled, Verified, Withdrawn }
```

`RemoteAgent` rule (2C domain): `route = http` requires `deployment_address` (`https`, ≤ 2048 bytes) and `identity_provider_ref`; `RAG_HTTP_FIELDS_INVALID` otherwise.

```rust
// ordin-runtime-adapter (wire part of 2C §2)
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
    /// The stream ends after exactly one `RunEvent::Ended`.
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

// All 12 fields copied from RuntimeAdapterInterface.md. The Hosted Agent Adapter ignores the last five
// (`model`, `thinking`, `caller`, `scope_id`, `coding_task_id`): the remote side picks its own model and
// thinking, and no ordin tool session is opened for a remote run.
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
    /// Hosted agent: waiting for the hosted agent's approvers; no `Answer` is accepted (2C C-14).
    RemoteApprovers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunProgress {
    /// One plain sentence, 1–160 characters.
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<ProgressCount>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProgressCount {
    pub done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
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
    /// Plain text, at most 32,768 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,
    pub artefacts: Vec<ArtefactId>,
}

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
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Answer { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub state: RunState,
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
//        RA_ALLOWANCE_EXHAUSTED, RA_THREAD_BUSY, RA_UNKNOWN_RUN, RA_DEADLINE.
```

Validation rules from 2C that this adapter applies: `remote_thread_id` and `runtime_session_id` 1–256 characters (`RA_HANDLE_INVALID`); `target` must be `remote_agent` (`RA_TARGET_MISMATCH`); `input.arguments` 0–16,384 and `input.text` 1–16,384 characters (`RA_INPUT_TOO_LARGE`); `Reject.reason` 0–500 characters; the event stream has exactly one `started` first and one `ended` last (`RA_EVENT_ORDER`); `Abandoned` may come from `hosted_agent`; `NoReply` never does (`RA_EVENT_INVALID`). Declared verbs not offered for `hosted_agent`: none from revision 0.43.0 (2C §2 table; 02:557–558).

### Caller interface of a hosted agent (from `agent-wright-runtime/docs/spec/components/RuntimeServer.md`, kept by ADR-098)

| Method and path | Name | Success | Body |
|---|---|---|---|
| `POST /v1/runs` | `runs.start` | 202 *Accepted* | `{"command"?, "arguments"?}` or `{"message"}` (*StartRun*, ≤ 1,048,576 bytes) |
| `POST /v1/threads/{thread_id}/runs` | `threads.runs.start` | 202 *Accepted* | same |
| `GET /v1/runs/{run_id}` | `runs.read` | 200 *Run* | — |
| `POST /v1/runs/{run_id}/resume` | `runs.resume` | 202 *Accepted* | `{"decisions":[{"type":"approve"\|"reject","message"?}], "set_aside"?}` (≤ 65,536 bytes; `message` ≤ 2,000 characters, reject only) |
| `GET /v1/runs/{run_id}/events` | `runs.events` — *chosen here* (ADR-098 addition; 08:43 "paths fixed in `/spec`") | 200, `text/event-stream` | — |
| `POST /v1/runs/{run_id}/cancel` | `runs.cancel` (CallerApi.md route 6) | 202 *Cancelled* | `{}` |
| `GET /v1/approvals?state=pending` | approver cards — *chosen here* (SA-CLOSE-01; wrightrun's spec defines no approver route) | 200 *ApproverCards* | — |
| `POST /v1/approvals/{approval_id}/decision` | approver decision — *chosen here* (SA-CLOSE-01) | 200 `{"approval_id", "state": "approved" \| "rejected"}` | `{"decision": "approve" \| "reject", "note"?: string}` (`note` ≤ 2,000 characters) |

*ApproverCards* (200): `{"approvals": [{"approval_id": "<uuid>", "run_id": "<uuid>", "asked_by": "<caller's display name>", "question": "<one sentence>", "scope": "<what the action reaches>", "asked_at": "<RFC 3339>"}]}`, at most 100 entries, only cards the token's subject may decide (an approver, never the asker; the Caller API applies the Approval Service's rules and takes the approver's identity from the token).

*StartRun* schema: `command` matches `^[a-z0-9]+(-[a-z0-9]+)*$`, ≤ 64 characters; `arguments` ≤ 262,144 characters; `message` 1–262,144 characters; exactly one of `command` and `message`; `arguments` only with `command`.

*Accepted* (202): `{"thread_id": "<uuid>", "run_id": "<uuid>", "state": "queued"}`.

*Cancelled* (202): `{"thread_id", "run_id", "state": "cancelled" | "executing"}`; `executing` means cancel was asked and the stream ends later with `cancelled` (row 422).

*Run* (200): `run_id`, `thread_id`, `state` ∈ {`queued`, `executing`, `paused`, `finished`, `ended-by-control`, `ended-at-limit`, `failed`, `interrupted`}, `started_by` {`kind`, `id`, `asker`}, `command`, `created_at`, `started_at`, `ended_at`, `result` (null while queued or executing) {`reply`, `ending`, `pending`: [{`kind`: `tool-call`\|`refused-ending`, `tool`, `gate`, `reason`}], `error_code`, `safe_reply`}. A later minor release may add properties; the adapter ignores unknown properties on read (2C §0: types the core reads from outside its own emission accept unknown fields only where the producer is another program — here the hosted agent). This interface adds one state, `cancelled`, for a run ended by `runs.cancel` (*chosen here*, with the cancel route).

Error form (SA-ENV-02): `{"error": "<sentence>", "code": "<CODE>"}`. Codes the adapter reacts to: `AUTH_MISSING` 401, `AUTH_INVALID` 401, `AUTH_NO_ROLE` 403, `AUTH_FORBIDDEN` 403, `AUTH_KEYS_UNAVAILABLE` 503, `SRV_BODY_TOO_LARGE` 413, `SRV_BAD_REQUEST` 400, `SRV_UNKNOWN_COMMAND` 400, `SRV_THREAD_NOT_FOUND` 404, `SRV_RUN_NOT_FOUND` 404, `RUN_NOT_PAUSED` 409, `RUN_NOT_CANCELLABLE` 409, `RUN_THREAD_BUSY` 409, `SRV_DECISION_MISMATCH` 400, `DB_UNAVAILABLE` 503, `SRV_INTERNAL` 500, `SRV_NO_ROUTE` 404, `SRV_METHOD_NOT_ALLOWED` 405. Every request carries `Authorization: Bearer <token>`.

### Event stream of one run (the Caller API's emitter, CallerApi.md route 5; X1, SA-RECON-01)

SSE frames with `id:` the run's event sequence number and `event:` either `run_state_changed` or `run_progress`; the adapter dispatches on the `event:` line, not on a field of `data`. Any SSE comment line counts as a heartbeat (the Caller API sends `: keep-alive` every 20 seconds). Reconnection sends `Last-Event-ID`. The stream closes after a final state (`finished`, `failed`, `ended-at-limit`, `ended-by-control`, `interrupted`, `cancelled`) or after 3,600 s. The stream carries no pending list, no end body and no reset frame: the adapter reads `GET /v1/runs/{id}` for those (step 9).

```rust
/// `data:` of an `event: run_state_changed` frame. Read with unknown fields ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct RunStateChangedData { pub run_id: String, pub state: String, pub at: String }
/// `data:` of an `event: run_progress` frame; `line` ≤ 160 characters.
#[derive(Debug, Clone, Deserialize)]
pub struct RunProgressData { pub run_id: String, pub line: String, pub at: String }
```

### Configuration

| Name | Source | Value |
|---|---|---|
| `remote_agents[]` {`name`, `route` = `http`, `address`, `identity` {`issuer`, `client_id`, `audience`}} | Organisation profile (2D-2) | resolved by the caller into a `HostedAgentTarget` |
| Poll interval when the stream is unavailable | SA-RATE-01 | 5 seconds minimum; `Retry-After` honoured on 429 and 503 |
| Stream silence before reconnect | *chosen here*, matches 2D-4 Server Connector | 45 seconds |
| Reconnect backoff | *chosen here*, matches 2D-4 Server Connector | 1 s doubling to 60 s, with ±20 % jitter |
| Unreachable time before the run is abandoned | *chosen here* | 15 minutes of no successful answer |
| Waiting runs per thread | *chosen here* | 10 |
| Request time limit (connect + answer, non-stream calls) | *chosen here* | 30 seconds |

No environment variable is read (2D, desktop reads none of its own).

## Public Interface

```rust
// crate ordin-hosted-agent-adapter

/// A hosted agent resolved from the organisation profile's remote agent catalogue.
#[derive(Debug, Clone)]
pub struct HostedAgentTarget {
    pub remote_agent: RemoteAgent,          // route = Http
    pub base_url: url::Url,                 // from deployment_address, https only
    pub identity: OidcSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcSettings {
    pub issuer: url::Url,                   // https
    pub client_id: String,                  // 1–255 characters
    pub audience: String,                   // 1–255 characters
}

/// Resolves the `RemoteAgentId` of a `StartRun` to its target (chosen here).
/// Implemented by the program from the Organisation Profile's `remote_agents[]` and the remote agent record.
pub trait RemoteAgentDirectory: Send + Sync + 'static {
    fn resolve_hosted(&self, id: RemoteAgentId) -> Option<HostedAgentTarget>;
}

/// The person's company access token (owned here). Implemented by the program over
/// `Identity::access_token_for_audience(issuer, client_id, audience)` and
/// `Identity::invalidate_audience_token(issuer, client_id, audience)` (IdentityAndCredentials.md);
/// a `CredError` of a missing or refused sign-in maps to `TokenError::SignedOut`, any other to `Unreachable`.
#[async_trait::async_trait]
pub trait CompanyTokenSource: Send + Sync + 'static {
    /// A valid access token for `settings`, refreshed when it expires within 5 minutes (Identity's margin).
    async fn company_access_token(&self, settings: &OidcSettings) -> Result<AccessToken, TokenError>;
    /// Drops a cached token the hosted agent refused, so the next call refreshes it.
    async fn invalidate(&self, settings: &OidcSettings);
}

/// Bearer token. `Debug` and `Display` print `AccessToken(***)`; never logged.
#[derive(Clone)]
pub struct AccessToken(String);
impl AccessToken { pub fn expose(&self) -> &str { &self.0 } }

#[derive(Debug, Clone, thiserror::Error)]
pub enum TokenError {
    /// No sign-in, or refresh refused: the person must sign in again.
    #[error("signed out")] SignedOut,
    /// Identity provider unreachable; retry later.
    #[error("identity provider unreachable")] Unreachable,
}

#[derive(Debug, Clone)]
pub struct HostedAgentConfig {
    pub poll_interval: std::time::Duration,          // 5 s; values below 5 s are raised to 5 s
    pub stream_silence: std::time::Duration,         // 45 s
    pub backoff_min: std::time::Duration,            // 1 s
    pub backoff_max: std::time::Duration,            // 60 s
    pub abandon_after_unreachable: std::time::Duration, // 15 min
    pub max_waiting_per_thread: usize,               // 10
    pub request_timeout: std::time::Duration,        // 30 s
}
impl Default for HostedAgentConfig { /* the values in the comments above */ }

pub struct HostedAgentAdapter { /* private */ }

impl HostedAgentAdapter {
    pub fn new(
        config: HostedAgentConfig,
        directory: std::sync::Arc<dyn RemoteAgentDirectory>,
        tokens: std::sync::Arc<dyn CompanyTokenSource>,
        http: reqwest::Client,   // built by the program with rustls-platform-verifier and the system proxy
    ) -> Self;
}

// Called by:
// - `new`: Ordin Desktop and Ordin Server (personal profile) programs, with their `RemoteAgentDirectory`
//   (over the Organisation Profile's `remote_agents[]`) and `CompanyTokenSource` (over Identity and Credentials).
// - `RuntimeAdapter` methods: Session Supervisor, through the program's `AdapterRegistry`; `cancel` also by the kill switch path.
// - `approver_cards`: Ordin Desktop program, mapped into Home and Search's Needs you with `workspace_id: None`
//   (HomeAndSearch.md SA-CLOSE-05).
// - `decide_approver_card`: Ordin Desktop program, for the Service API command `hosted.decide_card` (ServiceApi.md SA-CLOSE-05).

#[async_trait::async_trait]
impl RuntimeAdapter for HostedAgentAdapter {
    fn kind(&self) -> AdapterKind { AdapterKind::HostedAgent }
    fn unsupported_verbs(&self) -> &'static [AdapterVerb] { &[] }
    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError>;
    fn events(&self, ctx: &AdapterContext, run: &RunHandle)
        -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError>;
    async fn resume(&self, ctx: &AdapterContext, run: &RunHandle, decision: ResumeDecision)
        -> Result<(), AdapterError>;
    async fn read(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<RunSnapshot, AdapterError>;
    async fn cancel(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError>;
}

/// One approval card a hosted agent shows this person as an approver (SA-CLOSE-01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApproverCard {
    pub remote_agent_id: RemoteAgentId,
    pub approval_id: ApprovalId,
    pub remote_run_id: String,
    pub asked_by: String,       // 1–200 characters
    pub question: String,       // 1–500 characters
    pub scope: String,          // 1–500 characters
    pub asked_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApproverDecision { Approve, Reject }

impl HostedAgentAdapter {
    /// The cards pending for this person at one hosted agent (03:1662). Called by: the Ordin Desktop
    /// program, for *Needs you*, every 60 s and when Needs you opens.
    pub async fn approver_cards(&self, agent: RemoteAgentId) -> Result<Vec<ApproverCard>, AdapterError>;
    /// The person's decision on one card (03:1664–1667). Called by: the Ordin Desktop program, on the card's press,
    /// which reaches it as the Service API command `hosted.decide_card` (ServiceApi.md SA-CLOSE-05).
    pub async fn decide_approver_card(&self, agent: RemoteAgentId, approval_id: ApprovalId,
                                      decision: ApproverDecision, note: Option<String>) -> Result<(), AdapterError>;
}
```

### State mapping (caller-interface state → `RunEvent` / `RunState`)

| Caller state | Event emitted | `RunState` in `read` | `RunEndReason` |
|---|---|---|---|
| `queued` (after Accepted) | `Started { runtime_session_id: run_id, remote_thread_id: thread_id }` once | `Running` | — |
| `executing` | `Progress { line: "Working on it" }` on entry (no line repeats until another state) | `Running` | — |
| `paused` | `Progress { line }`, then `Paused { reason: RemoteApprovers }` (step 7) | `Paused` | — |
| `paused` → `executing` | `Resumed` | `Running` | — |
| `finished` | `Ended` | `Completed` | `Completed`, `final_message` = `result.reply` |
| `ended-by-control` | `Ended` | `Failed` | `Failed`, `final_message` = `result.ending` |
| `ended-at-limit` | `Ended` | `StoppedAtLimit` | `StoppedAtLimit`, `final_message` = `result.ending` |
| `failed` | `Ended` | `Failed` | `Failed`, `final_message` = none |
| `interrupted` | `Ended` | `Interrupted` | `Interrupted` |
| `cancelled` | `Ended` | `Cancelled` | `Cancelled` |
| (unreachable 15 min, or kill switch with cancel failing) | `Ended` | `Abandoned` | `Abandoned` |

A request still waiting in the adapter's per-thread queue reads as `Queued` and emits nothing until it is sent. `artefacts` is always empty: the hosted agent's results stay in its own work folder (ADR-097 item 9: the hosted agent never reaches the desktop's files).

### Example

```text
start(StartRun{ run_id: 0192a1c2-3344-7556-8677-8899aabbccdd,
  target: RemoteAgent{ remote_agent_id: 0192a0f3-…-0001 },
  input: Command{ name: "run-go-no-go", arguments: "RC-12" }, thread: None, … })

→ POST https://support-agent.acme.internal/v1/runs
  Authorization: Bearer eyJhbGciOiJSUzI1NiIs…
  {"command": "run-go-no-go", "arguments": "RC-12"}
← 202 {"thread_id": "7c2e1d7a-3f0b-4c55-9a40-0d2f6b1c8e11", "run_id": "1f0a9b3c-6d2e-4f7a-8b19-5c3d2e1f0a9b", "state": "queued"}

returns RunHandle{"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","adapter":"hosted_agent",
                  "runtime_session_id":"1f0a9b3c-6d2e-4f7a-8b19-5c3d2e1f0a9b",
                  "remote_thread_id":"7c2e1d7a-3f0b-4c55-9a40-0d2f6b1c8e11"}

events() yields:
  {"kind":"started","runtime_session_id":"1f0a9b3c-…","remote_thread_id":"7c2e1d7a-…"}
  {"kind":"progress","progress":{"line":"Working on it"}}
  {"kind":"progress","progress":{"line":"Read the release checklist","count":{"done":3,"total":7}}}
  {"kind":"ended","end":{"reason":"completed","final_message":"RC-12 is a go: all 7 checks pass.","artefacts":[]}}
```

## Internal Logic

**State held in memory** (no disk state):
- `followers: Mutex<HashMap<RunId, Follower>>` — per run: target, remote run and thread identifiers, last SSE id, last known caller state, `broadcast`-style sender of `RunEvent`s with a replay buffer of the last 256 events (assumed here: enough for one run's state changes between `start` and the Supervisor's `events` call), last successful contact time.
- `threads: Mutex<HashMap<(RemoteAgentId, String /*thread_id*/), ThreadSlot>>` — per thread: the run in progress (if any) and a FIFO of waiting `StartRun`s (at most `max_waiting_per_thread`).

**Approver cards (SA-CLOSE-01; 03:1640–1667)**
1. `approver_cards(agent)`: resolve as `start` step 2 (`HAA_AGENT_UNKNOWN`); token as step 6; `GET {base}/v1/approvals?state=pending`. 200 → parse *ApproverCards*; an entry failing the field limits is skipped and logged `HAA_EVENT_MALFORMED`. 403 `AUTH_NO_ROLE`/`AUTH_FORBIDDEN` (the person is no approver there) → `Ok(vec![])`, logged at `debug`. 404/405 (a hosted agent without the routes) → `Ok(vec![])`, logged `HAA_APPROVALS_UNSUPPORTED` once per agent per process. 401 → as step 6. Network → `Unreachable`.
2. `decide_approver_card(agent, id, decision, note)`: `note` over 2,000 characters → `InvalidRequest { code: "RA_INPUT_TOO_LARGE" }`; `POST {base}/v1/approvals/{id}/decision` with `{"decision", "note"?}`. 200 → `Ok`. 409 (the card was already decided, possibly by another approver) → `Err(Refused { code: "HAA_CARD_DECIDED" })`. 403 (the asker, or no approver role) → `Err(Refused { code: "HAA_NO_ACCESS" })`. 404 → `Err(Refused { code: "HAA_CARD_UNKNOWN" })`. Others as `start` step 6. Each decision is logged `info` `hosted_card_decided {remote_agent_id, approval_id, decision}`; the note is never logged.

**`start(ctx, req)`**
1. Validate: `req.target` must be `RemoteAgent`; otherwise log `HAA_TARGET_MISMATCH` and return `AdapterError::InvalidRequest { code: "RA_TARGET_MISMATCH" }`. Text limits per Shared Context; a breach logs `HAA_INPUT_INVALID` and returns `AdapterError::InvalidRequest { code: "RA_INPUT_TOO_LARGE" }` (the Error Table row).
2. Resolve the target with `directory.resolve_hosted(id)`. `None` → log `HAA_AGENT_UNKNOWN`, return `AdapterError::Refused { code: "HAA_AGENT_UNKNOWN" }`. `base_url` scheme not `https` → `HAA_ADDRESS_INVALID`, same return.
3. Build the body: `Command{name, arguments}` → `{"command": name, "arguments": arguments}` (omit `arguments` when empty); `FreeText{text}` → `{"message": text}`. `name` must match `^[a-z0-9]+(-[a-z0-9]+)*$` (the caller interface's slug form) else `HAA_COMMAND_FORM` → `InvalidRequest { code: "RA_COMMAND_UNKNOWN" }`. `attachments` non-empty → logged `HAA_ATTACHMENTS_DROPPED` (hosted agents cannot read the desktop's files, ADR-097 item 9) and the run proceeds without them.
4. If `req.thread` is `Some(t)` and `threads[(agent, t)]` has a run in progress: append `req` to its FIFO (full → `Err(AdapterError::ThreadBusy)`, log `HAA_QUEUE_FULL`); create the follower in state `Queued`; return `RunHandle{runtime_session_id: None, remote_thread_id: Some(t)}`. Nothing is sent.
5. Otherwise send (step 6) and return the handle with both identifiers.
6. **Send:** get a token (`tokens.company_access_token`): `SignedOut` → `Err(AdapterError::SignedOut)` (`HAA_SIGNED_OUT`); `Unreachable` → `Err(AdapterError::Unreachable)`. `POST {base}/v1/runs` or `POST {base}/v1/threads/{t}/runs` with the token, time limit `min(request_timeout, ctx.deadline)`. Outcomes:
   - 202 → parse *Accepted*; record identifiers in the follower; mark the thread busy with this run; spawn the follow task (step 9); emit `Started` into the follower.
   - 401 `AUTH_INVALID`/`AUTH_MISSING` → `tokens.invalidate`, retry once with a fresh token; second 401 → `AdapterError::SignedOut`.
   - 403 `AUTH_NO_ROLE`/`AUTH_FORBIDDEN` → `Err(AdapterError::Refused { code: "HAA_NO_ACCESS" })`; the Session Supervisor fails the run and the screens show "You don't have access to [agent]. Ask IT if you need it." (03:1660; the words are the screens' (Localisation), not this adapter's or Caller Authentication's; row 424).
   - 409 `RUN_THREAD_BUSY` (another device's run on the thread) → queue as step 4 and retry the send every `poll_interval × 6` (30 s) until accepted, or until the queue entry is cancelled.
   - 404 `SRV_THREAD_NOT_FOUND` → `Err(AdapterError::Refused { code: "HAA_THREAD_UNKNOWN" })`.
   - 400 `SRV_UNKNOWN_COMMAND`/`SRV_BAD_REQUEST`, 413 → `Err(AdapterError::Refused { code: "HAA_REQUEST_REFUSED" })`; the `error` sentence is logged at `debug` only.
   - 429/503/500, connection refused, TLS failure, timeout → `Err(AdapterError::Unreachable)` (`HAA_UNREACHABLE`). `ctx.deadline` passed → `Err(AdapterError::Deadline)`.
7. **Pause.** On `paused`, the adapter emits `Progress { line }` and then `Paused { reason: RemoteApprovers }` (2C C-14; no `Answer` is accepted). The line is built from `result.pending`: "[agent display name] is waiting for approval of N action(s)." with N the count of `tool-call` entries, at most 500 characters; tool names are not included (11 §3: tool names are never shown). Refused-ending entries only → "[agent display name] stopped and is waiting for a decision.".
8. **Thread release.** When a followed run ends, its thread slot pops the next waiting `StartRun` and sends it (step 6). A send failure there ends that queued run with `Ended{Failed}` (logged with its `HAA_` code).
9. **Follow task (per sent run).**
   1. Open `GET {base}/v1/runs/{id}/events` with `Accept: text/event-stream`, the token and `Last-Event-ID` when one is known. No overall time limit; read silence over `stream_silence` closes and reopens.
   2. For each frame: by its `event:` line parse `data` as `RunStateChangedData` or `RunProgressData`; another event name or malformed data → log `HAA_EVENT_MALFORMED`, skip the frame. Map by the state table; never emit a second `Started` or anything after `Ended`. Progress lines are trimmed to 160 characters; a line containing a control character other than `\n`/`\t` is dropped (2C §0 text rule). On a state of `paused` or a final state, read `GET /v1/runs/{id}` once for `result.pending` (step 7), `result.reply` and `result.ending` (state table) before emitting (row 419).
   3. Stream ended or failed → reconnect with backoff (1 s doubling to 60 s, ±20 % jitter). 401 → invalidate and refresh the token once per reconnect; `TokenError::SignedOut` → emit `Paused{SignIn}` once, then try the token every 30 s; when it succeeds emit `Resumed` and reconnect.
   4. Fallback: 404 `SRV_NO_ROUTE` or 405 on the events route (a hosted agent without the ADR-098 addition), or three reconnects in a row that fail before the first frame → switch to polling `GET /v1/runs/{id}` every `poll_interval`, honouring `Retry-After`; emit events from each state change seen.
   5. After every reconnect, before reading frames: one `GET /v1/runs/{id}`, emit what changed since the last event seen; this replaces a reset frame (row 420), because the Caller API keeps at most 10,000 events per run and sends none.
   6. No successful answer (frame, comment line or 2xx) for `abandon_after_unreachable` → emit `Ended{Abandoned}` and stop following (log `HAA_ABANDONED`).
   7. `ctx.cancel` of the `events` caller does not stop the follow task; only `Ended` or `cancel` does.
10. **`events(ctx, run)`.** Look up the follower by `run.run_id`; unknown and `run.runtime_session_id` is `Some` (after a restart) → create a follower from the handle, read the run once (`read`), emit `Started` with the handle's identifiers, then follow (step 9). Unknown and no remote identifier → `Err(AdapterError::UnknownRun)`. Returns a stream that first replays buffered events, then live ones, and ends after `Ended`.
11. **`resume(ctx, run, decision)`.** The follower must be in `Paused`, else `Err(AdapterError::InvalidRequest { code: "RA_RESUME_MISMATCH" })`. Read the run once to learn the number of `tool-call` entries in `pending` (n ≥ 0). Body: `Approve` → n × `{"type":"approve"}`; `Reject{reason}` → n × `{"type":"reject","message": reason}` (`message` omitted when absent, cut to 2,000 characters); `Answer` → `Err(AdapterError::VerbNotOffered{verb: Resume})` (the caller interface has no answer form). The `approval_id` in the decision is not sent (the caller interface has none); it is logged. `POST /v1/runs/{id}/resume`: 202 → ok (the stream will show `executing`); 409 `RUN_NOT_PAUSED` → ok if the next read shows the run no longer paused (decided elsewhere, by an approver), else `Refused { code: "HAA_RESUME_REFUSED" }`; 400 `SRV_DECISION_MISMATCH` → re-read once and retry with the new n; 403/404 → `Err(AdapterError::Refused { code: "HAA_RESUME_REFUSED" })` (the configuration lets only its approvers decide, ADR-097 item 4); network → `Unreachable`.
12. **`read(ctx, run)`.** Queued in the adapter → `RunSnapshot{state: Queued}`. Else `GET /v1/runs/{id}`: 200 → map by the state table, `checked_at` = now; 404 → `Err(AdapterError::UnknownRun)`; 401 twice → `SignedOut`; network → `Unreachable`.
13. **`cancel(ctx, run)`.** Queued in the adapter → remove it from the FIFO and emit `Ended{Cancelled}`. Else `POST /v1/runs/{id}/cancel`: 202 *Cancelled* with `state` `cancelled` → ok; with `executing` → ok, the stream ends later with `cancelled`; 409 `RUN_NOT_CANCELLABLE` (the run already ended) → ok; 404/405 on the route (no ADR-098 addition) → `Err(AdapterError::VerbNotOffered{verb: Cancel})`; network → `Unreachable`. The kill switch path: the Session Supervisor calls `cancel`; on `Unreachable` it marks the run abandoned (03:376).

## Data Model

N/A — the adapter keeps no table. Its crate depends only on Identity and the Runtime Adapter interface (2B-3), so it has no `Datastore` handle; the remote run and thread identifiers it owns (02:557 "remote run handles") are carried in `RunHandle`, which the Session Supervisor journals (`runs.runtime_session_id`, `runs.remote_thread_id`), and the per-thread waiting queue is in memory. On restart, a run that was waiting in the queue has no remote identifier; `events`/`read` return `UnknownRun` and the Session Supervisor's recovery sequence (03:409) decides to start it again or mark it interrupted.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `StartRun.target` is an agent instance | `Err(permanent)` `InvalidRequest { code: "RA_TARGET_MISMATCH" }` | `HAA_TARGET_MISMATCH` (2C `RA_TARGET_MISMATCH`) | — (in-process; journal records the code) |
| `StartRun.attachments` non-empty | none: `Ok`, the run proceeds without them; logged `warn` | `HAA_ATTACHMENTS_DROPPED` | — (hosted agents cannot read the desktop's files, ADR-097 item 9) |
| Text over 2C limits or command not a slug | `Err(permanent)` `InvalidRequest { code: "RA_INPUT_TOO_LARGE" \| "RA_COMMAND_UNKNOWN" }` | `HAA_INPUT_INVALID` / `HAA_COMMAND_FORM` | — |
| Remote agent not in the directory, or not `https` | `Err(permanent)` `Refused { code }` | `HAA_AGENT_UNKNOWN` / `HAA_ADDRESS_INVALID` | — |
| No company sign-in, refresh refused, or two 401s | `Err(permanent)` `SignedOut` | `HAA_SIGNED_OUT` | remote 401 `{"error":"the request's token was not accepted","code":"AUTH_INVALID"}` |
| Token lacks caller role | `Err(permanent)` `Refused { code }` | `HAA_NO_ACCESS` | remote 403 `{"error":"the token carries no role of this agent","code":"AUTH_NO_ROLE"}` |
| Thread busy, own FIFO full | `Err(retryable)` `ThreadBusy` | `HAA_QUEUE_FULL` | — |
| Remote 409 `RUN_THREAD_BUSY` | queued, no error | `HAA_THREAD_BUSY_REMOTE` | remote 409 `{"error":"The thread has a run that has not ended.","code":"RUN_THREAD_BUSY"}` |
| Thread unknown at the hosted agent | `Err(permanent)` `Refused { code }` | `HAA_THREAD_UNKNOWN` | remote 404 `SRV_THREAD_NOT_FOUND` |
| Request refused (400/413) | `Err(permanent)` `Refused { code }` | `HAA_REQUEST_REFUSED` | remote 400 `SRV_UNKNOWN_COMMAND` / `SRV_BAD_REQUEST`, 413 `SRV_BODY_TOO_LARGE` |
| Network, TLS, 429, 5xx, request time limit | `Err(retryable)` `Unreachable` | `HAA_UNREACHABLE` | remote 503 `DB_UNAVAILABLE` / `AUTH_KEYS_UNAVAILABLE`, 500 `SRV_INTERNAL`, or none |
| `ctx.deadline` passed | `Err(retryable)` `Deadline` | `HAA_DEADLINE` | — |
| SSE frame not parseable | skipped, run continues | `HAA_EVENT_MALFORMED` | — |
| Events route absent (404/405) | fallback to polling | `HAA_STREAM_UNSUPPORTED` | remote 404 `SRV_NO_ROUTE` / 405 `SRV_METHOD_NOT_ALLOWED` |
| No answer for 15 minutes while following | `Ended{Abandoned}` | `HAA_ABANDONED` | — |
| `resume` with `Answer` | `Err(permanent)` `VerbNotOffered{Resume}` | `HAA_ANSWER_NOT_OFFERED` | — |
| `resume` when not paused, or refused by the hosted agent | `Err(permanent)` `Refused { code }` | `HAA_RESUME_REFUSED` (2C `RA_RESUME_MISMATCH`) | remote 409 `RUN_NOT_PAUSED`, 403 `AUTH_FORBIDDEN`, 404 `SRV_RUN_NOT_FOUND` |
| `cancel` on a hosted agent without the route | `Err(permanent)` `VerbNotOffered{Cancel}` | `HAA_CANCEL_UNSUPPORTED` | remote 404 `SRV_NO_ROUTE` |
| Approver card already decided | `Err(permanent)` `Refused { code }` | `HAA_CARD_DECIDED` | remote 409 |
| Approver card unknown | `Err(permanent)` `Refused { code }` | `HAA_CARD_UNKNOWN` | remote 404 |
| Hosted agent without the approver routes | `Ok(vec![])`, logged | `HAA_APPROVALS_UNSUPPORTED` | remote 404 `SRV_NO_ROUTE` / 405 |
| `read`/`events` of a run the adapter cannot find | `Err(permanent)` `UnknownRun` | `HAA_RUN_UNKNOWN` | remote 404 `SRV_RUN_NOT_FOUND` |

The person never sees a code (07:21). The adapter owns no sentence shown to the person; the Session Supervisor and screens map `AdapterError` and `RunEndReason` to 11 §8 labels.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Hosted Agent Adapter

  Background:
    Given a remote agent "support-agent" with route http at "https://support-agent.acme.internal"
    And the person holds a valid company token with the caller role

  Scenario: Attachments are dropped for a hosted agent
    Given a start request for a hosted_agent run with one attachment
    When the adapter starts the run
    Then the run starts without the attachment
    And HAA_ATTACHMENTS_DROPPED is logged

  Scenario: Happy path — start on a new thread and follow the stream to the result
    When start is called with command "run-go-no-go" and arguments "RC-12" and no thread
    Then the adapter sends POST /v1/runs with {"command":"run-go-no-go","arguments":"RC-12"} and a bearer token
    And it returns a RunHandle with runtime_session_id "1f0a9b3c-6d2e-4f7a-8b19-5c3d2e1f0a9b" and remote_thread_id "7c2e1d7a-3f0b-4c55-9a40-0d2f6b1c8e11"
    And the events stream yields started, then progress "Working on it", then ended with reason completed and final message "RC-12 is a go."

  Scenario: Edge — a second request on a busy thread waits until the first ends
    Given run A is executing on thread "T"
    When start is called for run B with thread "T"
    Then no request is sent for run B
    And read of run B returns state queued
    When run A's stream sends an ended frame
    Then the adapter sends POST /v1/threads/T/runs for run B
    And run B's events stream yields started

  Scenario: Edge — the hosted agent answers 409 RUN_THREAD_BUSY
    When start is called with thread "T" and the hosted agent answers 409 with code RUN_THREAD_BUSY
    Then start returns a RunHandle with no runtime_session_id
    And the adapter retries the send every 30 seconds until it is accepted

  Scenario: Edge — the events route is absent, the adapter polls
    Given the hosted agent answers 404 SRV_NO_ROUTE on GET /v1/runs/R/events
    When the run is followed
    Then the adapter reads GET /v1/runs/R no more often than every 5 seconds
    And a change from executing to finished yields an ended event with reason completed

  Scenario: Pause and resume with reject
    Given run R is paused with two pending tool-call entries
    Then the events stream yields paused with a person question "Support agent is waiting for approval of 2 action(s)."
    When resume is called with Reject and reason "RC-12 is frozen until Thursday"
    Then the adapter sends {"decisions":[{"type":"reject","message":"RC-12 is frozen until Thursday"},{"type":"reject","message":"RC-12 is frozen until Thursday"}]}
    And a later executing frame yields resumed

  Scenario: Cancel a running run
    Given run R is executing
    When cancel is called
    Then the adapter sends POST /v1/runs/R/cancel
    And the events stream ends with reason cancelled

  Scenario: Error — the token is refused twice
    When start is called and the hosted agent answers 401 AUTH_INVALID to both the first and the refreshed token
    Then start returns AdapterError::SignedOut
    And no token value appears in any log line

  Scenario: Error — the token carries no caller role
    When start is called and the hosted agent answers 403 AUTH_NO_ROLE
    Then start returns AdapterError::Refused with code HAA_NO_ACCESS and the code is journalled

  Scenario: Error — unreachable for 15 minutes
    Given run R is executing and the hosted agent stops answering
    When 15 minutes pass with no frame, heartbeat or successful read
    Then the events stream ends with reason abandoned

  Scenario: Error — sign-in expires during the run
    Given run R is executing and the stream returns 401
    And the token source reports SignedOut
    Then the events stream yields paused with reason sign_in
    When the person signs in again and a token is obtained
    Then the events stream yields resumed and the stream reconnects with Last-Event-ID

  Scenario: Recovery after restart
    Given the application restarted and the journalled RunHandle carries runtime_session_id "R" and remote_thread_id "T"
    When events is called with that handle
    Then the adapter reads GET /v1/runs/R once, yields started, and follows the stream

  Scenario: Answer is not offered
    When resume is called with Answer "yes"
    Then it returns AdapterError::VerbNotOffered with verb resume

  Scenario: A pause read from the Caller API's state event
    Given the run is followed on the event stream
    When a frame "event: run_state_changed" arrives with state "paused"
    Then the adapter reads GET /v1/runs/{id} once
    And it emits Paused with a question counting the tool-call entries of result.pending

  Scenario: Cancel of a run that is still finishing
    When cancel is called and the hosted agent answers 202 with state "executing"
    Then cancel returns ok
    And the stream later ends with state "cancelled" and Ended is emitted with reason Cancelled

  Scenario: An approver sees and decides a hosted agent's card
    Given the person is an approver at "support-agent" and one card is pending there
    When approver_cards is called for "support-agent"
    Then one ApproverCard is returned with its question and scope
    When decide_approver_card is called with approve
    Then POST /v1/approvals/{approval_id}/decision is sent with decision "approve"

  Scenario: A card another approver already decided
    Given the card was decided by another approver
    When decide_approver_card is called
    Then it fails with Refused HAA_CARD_DECIDED
```

## Performance / Security / Observability

- **Performance targets:** adapter overhead (excluding network and the hosted agent) p95 ≤ 20 ms for `start`, `resume`, `read`, `cancel`; an SSE frame becomes a `RunEvent` within 50 ms p95 of its arrival (within 07's 1 s first-progress target, 07 NFR); at most 50 runs followed at once per installation (*assumed here*: crews of 8 members plus Ordin's single runs), each follower ≤ 64 KiB of memory including its 256-event buffer; polling never more often than every 5 seconds per run (SA-RATE-01).
- **Security:** every request uses `https` and the platform trust store (SA-HTTP-02); the bearer token is fetched per request from `CompanyTokenSource`, held only in an `AccessToken` whose `Debug` is redacted, never logged, journalled, put in an error or written to disk; the `error` sentence of a remote refusal is logged at `debug` only and never shown to the person; SSE `data` and `Run` bodies are input from another program: parsed with typed deserialisation, text checked for control characters and length before use (SA-XC-12); response bodies over 1 MiB (non-stream) or a single SSE frame over 256 KiB are refused (`HAA_EVENT_MALFORMED`, *assumed here*, matching LW-6); the adapter sends no attachment, file content or path to a hosted agent; redirects are not followed (*assumed here*: a redirect could move the bearer token to another host).
- **Observability:** no metrics or spans (desktop and personal server, SA-XC-04/05). `tracing` events, target `ordin_hosted_agent_adapter`, with fields `run_id`, `remote_agent_id`, `remote_run_id`, `remote_thread_id`, `correlation_id`, `code`, `http_status`, `attempt`: `info` `hosted_run_sent`, `hosted_run_queued`, `hosted_run_ended` (+ `reason`), `hosted_stream_fallback_polling`; `warn` `hosted_unreachable` (+ `since_secs`), `hosted_abandoned`, `hosted_token_refused`; `debug` `hosted_frame` (+ `kind`, `sse_id`; never `line` text). Never logged: token, `arguments`, `message`, `reply`, `ending`, progress line text.

## Gaps

None.
