# SPEC: Slack Thread Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-slack-thread-adapter` · **Build phase:** 2 · **Programs:** D P
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
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)

## Purpose

The Slack Thread Adapter is the Runtime Adapter for remote agents reached through Slack, as the person (09 ADR-008). It posts a message that mentions the agent in a thread of the dedicated channel with the person's Slack user token, reads that thread's replies at a fixed interval, and treats the agent's reply as the run's result (02:564–572). From revision 0.43.0 the agent it reaches is a hosted ordin agent (ADR-098 item 8). When the agent asks for an approval in Slack, the adapter reports the run as paused with a link to the thread, and the decision is made in Slack (02:568, ADR-008). It requires the remote agent's trigger to deliver into the thread (`delivery: thread`), because a private reply cannot be read back (09 ADR-009, C-04).

## Approach

**Closing assumptions** (*chosen here*): **SA-CLOSE-01** (closes G-STA-1) — `slack.dedicated_channel` (2D-2) and `RemoteAgent.slack_channel` hold a Slack channel identifier matching `^[CG][A-Z0-9]{8,20}$`, and `RemoteAgent.slack_account` holds the agent's Slack member identifier matching `^[UW][A-Z0-9]{8,20}$`; names are not accepted, because posting and recognising the agent's replies need identifiers and a name lookup would need a scope the HLD does not ask for. The Organisation Profile checks both forms when it reads the profile; a value outside its pattern reaching this adapter refuses the run with `STA_AGENT_UNKNOWN`. **SA-CLOSE-02** (closes G-STA-3) — two agent messages mean "the decision is being made elsewhere": a message carrying Slack buttons (an `actions` block, the form wrightrun agents use in the thread) and the hosted ordin agent's fixed text `This request is waiting for approval.` (03:1617; its approvers decide on cards in their direct chats, ADR-097 item 4). Either pauses the run with `DecisionElsewhere` and the message's link; a later agent message that is neither is the result. A button message that is not an approval is still treated as a decision in Slack: the person sees **Open in Slack**, which is the right place to answer any button.

**Reconciliation assumptions** (decision 80): **SA-RECON-01** — refusals return 2C `AdapterError::Refused { code }` or `InvalidRequest { code }` (2C C-13). **SA-RECON-02** — `SlackUserTokenSource` is implemented by the program over Identity's `access_token` and `report_refused` (row 487). **SA-RECON-03** — `abandon` is called by the program on the Session Supervisor's behalf (row 488).

A polling client of the Slack Web API, one follow task per run, over `reqwest` 0.13.5 (SA-HTTP-02): `chat.postMessage` to post and `conversations.replies` to read (Slack method names as wrightrun's ChatClient uses them, `agent-wright-runtime/docs/spec/components/ChatClient.md:25`, `:30`). The reply rule is fixed code: the first message in the thread from the agent's Slack account, posted after ordin's message, is the result, unless it carries Slack buttons, in which case it is an approval request made in Slack. The adapter keeps no table: the thread it posted in is carried in `RunHandle.remote_thread_id`, which the Session Supervisor journals. Error prefix *chosen here*: `STA_`; at the trait boundary every failure is an `AdapterError` (`RA_…`). Rejected: receiving Slack events (ADR-008: the application receives no Slack events and holds no bot token); asking the Approval Service from inside the adapter — the crate depends only on Identity and the Runtime Adapter interface (2B-3), so the approval to post as the person is obtained by the caller before `start` (MCP Front decision on Ordin's `delegate`, or the Crew Orchestrator's approval point).

## Shared Context

### Shared types used (copied from Phase 2C; crates `ordin-types`, `ordin-runtime-adapter`)

```rust
// ordin-types
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`
pub struct RunId(pub uuid::Uuid);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Github, Gitlab, Oidc, Slack, Confluence, Microsoft, Google,
    MailServerSingleSignOn, MailServerPassword, MailServerAppPassword,
    Jira,   // 2C, amended January 2026, ADR-106
}
```

`RemoteAgent` rule (2C domain): `route = slack` requires `slack_account` and `slack_channel` (each ≤ 80 characters); `deployment_address` absent (`RAG_SLACK_FIELDS_INVALID`).

```rust
// ordin-runtime-adapter (2C wire §2) — copied in full
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

// All 12 fields copied from RuntimeAdapterInterface.md. The Slack Thread Adapter ignores the last five
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
    /// Slack route: the decision is made in Slack (ADR-009); the link opens the thread.
    DecisionElsewhere { link: url::Url },
    /// Hosted agent: waiting for the hosted agent's approvers (2C C-14). This adapter never emits it.
    RemoteApprovers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunProgress {
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

2C rules applied: verbs not offered by `slack_thread`: `pause` and `resume` as structured verbs, `cancel`, `progress` (2C §2 table; 02:560–568, 09 ADR-003); `NoReply` only from `slack_thread`; `Abandoned` allowed; `remote_thread_id` 1–256 characters and required once started (`RA_HANDLE_INVALID`); `target` must be `remote_agent` (`RA_TARGET_MISMATCH`); `input.arguments` 0–16,384, `input.text` 1–16,384 characters (`RA_INPUT_TOO_LARGE`); one `started` first, one `ended` last.

### Configuration

| Name | Source | Value |
|---|---|---|
| `slack.workspace` | Organisation profile (2D-2) | the Slack workspace's domain (the `<domain>` of `https://<domain>.slack.com`), *assumed here* to be the domain form, 1–80 characters, `^[a-z0-9-]+$`; optional (amended January 2026, ADR-102): absent, with `slack.dedicated_channel`, means no Slack route — the program does not construct this adapter |
| `slack.dedicated_channel` | Organisation profile (2D-2) | the dedicated channel's Slack identifier, `^[CG][A-Z0-9]{8,20}$` (SA-CLOSE-01; a name would need a lookup the scopes allow but the HLD does not ask for); optional, present exactly when `slack.workspace` is (amended January 2026, ADR-102) |
| User-token scopes | SA-SLACK-01 | `chat:write`, `channels:history`, `groups:history`, `channels:read`, `groups:read` |
| Poll interval | SA-RATE-01 (reading taken over 2D-4's 5 s, see findings) | 10 seconds; `Retry-After` honoured on HTTP 429 |
| No-reply limit before the first agent reply | *chosen here* | 30 minutes |
| No-reply limit while the decision is in Slack | *chosen here* | 24 hours |
| Message text limit | 2D-4 *Slack limits* (Slack Channel Adapter's figure, applied here) | 4,000 characters |
| Request time limit per Web API call | *chosen here* | 15 seconds |

The adapter reads no environment variable.

### Slack Web API calls (method names from `agent-wright-runtime/docs/spec/components/ChatClient.md:25`, `:30`; field names *assumed here* from Slack's published Web API, checked by a recorded-response test at first build)

| Call | Request (form-encoded or JSON, `Authorization: Bearer <user token>`) | Fields read from the answer |
|---|---|---|
| `POST https://slack.com/api/chat.postMessage` | `channel`, `text`, `thread_ts` (when continuing a thread) | `ok`, `error`, `ts`, `channel` |
| `GET https://slack.com/api/conversations.replies` | `channel`, `ts` (thread parent), `oldest` (ordin's message `ts`), `limit=200`, `cursor` | `ok`, `error`, `messages[]` {`ts`, `user`, `bot_id`, `text`, `blocks[]` {`type`}}, `response_metadata.next_cursor` |

Slack reports most failures as HTTP 200 with `{"ok": false, "error": "<name>"}`; the names the adapter reacts to (*assumed here*): `invalid_auth`, `token_revoked`, `not_authed`, `account_inactive` (sign-in), `missing_scope`, `not_in_channel`, `channel_not_found`, `thread_not_found`, `msg_too_long`, `ratelimited` (also HTTP 429 with `Retry-After`).

## Public Interface

```rust
// crate ordin-slack-thread-adapter

/// A Slack remote agent resolved for posting (chosen here).
#[derive(Debug, Clone)]
pub struct SlackAgentTarget {
    pub remote_agent: RemoteAgent,   // route = Slack
    /// The agent's Slack member identifier, from `RemoteAgent.slack_account`, `^[UW][A-Z0-9]{8,20}$` (SA-CLOSE-01).
    pub agent_member_id: String,
    /// `RemoteAgent.slack_channel` when present, else the profile's `slack.dedicated_channel`.
    pub channel_id: String,
    /// The profile's `slack.workspace`.
    pub workspace_domain: String,
}

/// Implemented by the program from the Organisation Profile and the remote agent record (chosen here).
pub trait SlackAgentDirectory: Send + Sync + 'static {
    fn resolve_slack(&self, id: RemoteAgentId) -> Option<SlackAgentTarget>;
}

/// The person's Slack user token (CredentialKind::Slack). Owned here; implemented by the program over
/// `Identity::access_token(CredentialKind::Slack, None)` and `Identity::report_refused(credential_id, "STA_TOKEN_REFUSED")`
/// (IdentityAndCredentials.md); a `CredError` of a missing sign-in → `SignedOut`, any other → `Unavailable`.
#[async_trait::async_trait]
pub trait SlackUserTokenSource: Send + Sync + 'static {
    async fn slack_user_token(&self) -> Result<SlackUserToken, SlackTokenError>;
    /// Marks the token refused by Slack, so Identity asks the person to sign in again.
    async fn report_refused(&self);
}

/// `Debug`/`Display` print `SlackUserToken(***)`; never logged.
#[derive(Clone)]
pub struct SlackUserToken(String);
impl SlackUserToken { pub fn expose(&self) -> &str { &self.0 } }

#[derive(Debug, Clone, thiserror::Error)]
pub enum SlackTokenError {
    #[error("signed out of Slack")] SignedOut,
    #[error("keychain unavailable")] Unavailable,
}

#[derive(Debug, Clone)]
pub struct SlackThreadConfig {
    pub poll_interval: std::time::Duration,            // 10 s; values below 10 s are raised to 10 s
    pub no_reply_after: std::time::Duration,           // 30 min
    pub decision_in_slack_limit: std::time::Duration,  // 24 h
    pub text_limit_chars: usize,                       // 4,000
    pub request_timeout: std::time::Duration,          // 15 s
    pub api_base: url::Url,                            // https://slack.com/api/ (tests point it elsewhere)
}
impl Default for SlackThreadConfig { /* the values in the comments above */ }

pub struct SlackThreadAdapter { /* private */ }

impl SlackThreadAdapter {
    pub fn new(
        config: SlackThreadConfig,
        directory: std::sync::Arc<dyn SlackAgentDirectory>,
        tokens: std::sync::Arc<dyn SlackUserTokenSource>,
        http: reqwest::Client,
    ) -> Self;
}

#[async_trait::async_trait]
impl RuntimeAdapter for SlackThreadAdapter {
    fn kind(&self) -> AdapterKind { AdapterKind::SlackThread }
    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        &[AdapterVerb::Pause, AdapterVerb::Resume, AdapterVerb::Cancel, AdapterVerb::Progress]
    }
    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError>;
    fn events(&self, ctx: &AdapterContext, run: &RunHandle)
        -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError>;
    /// Always `Err(VerbNotOffered{Resume})`: the decision is made in Slack.
    async fn resume(&self, ctx: &AdapterContext, run: &RunHandle, decision: ResumeDecision)
        -> Result<(), AdapterError>;
    async fn read(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<RunSnapshot, AdapterError>;
    /// Always `Err(VerbNotOffered{Cancel})`: a question once asked cannot be withdrawn (11:424).
    async fn cancel(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError>;
    // Also exposed (not on the trait): stop following a run without contacting Slack.
}

impl SlackThreadAdapter {
    /// Stops following `run` and ends its stream with `Abandoned` (kill switch, 03:376). Contacts nothing.
    pub fn abandon(&self, run: RunId);
}
```

**Called by.**

| Item | Caller |
|---|---|
| `new` | Ordin Desktop and Ordin Server (personal profile) programs, with their `SlackAgentDirectory` (over the Organisation Profile's remote agents), the `SlackUserTokenSource` (over Identity and Credentials) and the HTTP client (row 487) |
| `RuntimeAdapter` methods | Session Supervisor, through the program's `AdapterRegistry` |
| `abandon` | the program, from its `SupervisorHost::run_abandoned(run_id, AdapterKind::SlackThread)` (SessionSupervisor.md), when the Supervisor marks a `slack_thread` run abandoned because this adapter offers no `cancel` (kill switch or a stop at a limit; row 488). The Supervisor holds only `dyn RuntimeAdapter`, so the program keeps the concrete adapter for this call |

`RunHandle` produced: `runtime_session_id` = ordin's message `ts` (the run's own message), `remote_thread_id` = the thread parent `ts` (equal to the message `ts` for a new thread). Both are Slack timestamps, `^[0-9]{10}\.[0-9]{6}$` (*assumed here*).

### Message text

- `Command { name, arguments }` → `<@{agent_member_id}> {name} {arguments}` (trailing space removed when `arguments` is empty).
- `FreeText { text }` → `<@{agent_member_id}> {text}`.
- The `<@…>` mention form is Slack's (*assumed here*). Text over 4,000 characters is refused, never cut.

### Reply rule (fixed code)

Among `messages[]` of the thread with `ts` greater than ordin's message `ts`, in ascending `ts`:
1. A message whose `user` equals `agent_member_id` and whose `blocks` contain an element with `type` = `actions`, or whose `text` trimmed equals `This request is waiting for approval.` (the fixed text a hosted ordin agent posts in the thread while its approvers decide, 03:1617), is an **approval request in Slack** (SA-CLOSE-02).
2. The first message whose `user` equals `agent_member_id`, which is not an approval request by rule 1, and whose `text` is not empty, is the **result**.
3. Messages from any other account (people in the channel) are ignored.

### Link to the thread

`https://{workspace_domain}.slack.com/archives/{channel_id}/p{ts with the dot removed}` for the agent's approval message (*assumed here*: Slack's permalink form, built without a call so no extra scope is needed).

### Example

```text
start(StartRun{ run_id: 0192a1c2-3344-7556-8677-8899aabbccdd,
  target: RemoteAgent{ remote_agent_id: 0192a0f3-…-0002 },
  input: FreeText{ text: "Is RC-12 ready to ship?" }, thread: None, … })

→ POST https://slack.com/api/chat.postMessage
  {"channel":"C07ORDINAG","text":"<@U05SUPPORT1> Is RC-12 ready to ship?"}
← 200 {"ok":true,"channel":"C07ORDINAG","ts":"1760952061.000200"}

returns RunHandle{"run_id":"0192a1c2-…","adapter":"slack_thread",
                  "runtime_session_id":"1760952061.000200","remote_thread_id":"1760952061.000200"}

GET https://slack.com/api/conversations.replies?channel=C07ORDINAG&ts=1760952061.000200&oldest=1760952061.000200&limit=200
← 200 {"ok":true,"messages":[{"ts":"1760952061.000200","user":"U02ASHA","text":"<@U05SUPPORT1> Is RC-12 ready to ship?"},
                            {"ts":"1760952139.004100","user":"U05SUPPORT1","bot_id":"B05SUP","text":"RC-12 is a go: all 7 checks pass.","blocks":[{"type":"section"}]}]}

events() yields:
  {"kind":"started","runtime_session_id":"1760952061.000200","remote_thread_id":"1760952061.000200"}
  {"kind":"ended","end":{"reason":"completed","final_message":"RC-12 is a go: all 7 checks pass.","artefacts":[]}}
```

## Internal Logic

**State in memory:** `followers: Mutex<HashMap<RunId, Follower>>` (target, message `ts`, thread `ts`, phase `Waiting | DecisionInSlack | Ended`, time of posting, time the phase began, event sender with a replay buffer of 16 events) and `bindings: Mutex<HashMap<RemoteAgentId, String>>` — the last thread `ts` used per remote agent in this process (02:569 "thread bindings per remote agent"), offered to callers through `last_thread(remote_agent_id) -> Option<String>` so Ordin can continue a conversation in the same thread.

**Precondition (not checked here):** the caller obtained the person's approval to post as them before calling `start` (11:295, ADR-008 consequence "posts are approval-gated"): the MCP Front's Rules Engine decision on Ordin's `delegate` call, or the Crew Orchestrator's approval point or charter pre-approval (03:378–395).

**`start(ctx, req)`**
1. `req.target` must be `RemoteAgent`; otherwise log `STA_TARGET_MISMATCH`, return `AdapterError::InvalidRequest { code: "RA_TARGET_MISMATCH" }`. `directory.resolve_slack(id)`: `None`, or an identifier outside its pattern → `Err(AdapterError::Refused { code: "STA_AGENT_UNKNOWN" })`.
2. Build the text (Message text). Over 4,000 characters or over 2C limits → `AdapterError::InvalidRequest { code: "RA_INPUT_TOO_LARGE" }` (`STA_TEXT_TOO_LONG` journalled). `attachments` non-empty → logged `STA_ATTACHMENTS_DROPPED`; nothing is uploaded (the user-token scopes include no file scope).
3. Token: `tokens.slack_user_token()`; `SignedOut` → `Err(AdapterError::SignedOut)`; `Unavailable` → `Err(AdapterError::Unreachable)`.
4. `chat.postMessage` with `channel`, `text`, and `thread_ts = req.thread` when given. Time limit `min(15 s, ctx.deadline)`. Outcomes:
   - `ok: true` → record `ts`; thread `ts` = `req.thread` or the new `ts`; update `bindings`; create the follower in `Waiting`; emit `Started{runtime_session_id: ts, remote_thread_id: thread}`; spawn the follow task; return the handle.
   - `invalid_auth`, `token_revoked`, `not_authed`, `account_inactive` → `tokens.report_refused()`, `Err(AdapterError::SignedOut)` (`STA_TOKEN_REFUSED`).
   - `missing_scope` → `STA_MISSING_SCOPE` (the needed scope is logged for Diagnostics, SA-SLACK-01 impact), `Err(AdapterError::Refused { code: "STA_MISSING_SCOPE" })`.
   - `not_in_channel`, `channel_not_found`, `thread_not_found` → `Err(AdapterError::Refused { code: "STA_CHANNEL_UNREACHABLE" })`.
   - `msg_too_long` → `Err(AdapterError::Refused { code: "STA_TEXT_TOO_LONG" })`.
   - HTTP 429 or `ratelimited` → wait `Retry-After` seconds (default 30 when absent) once if `ctx.deadline` allows, then retry; otherwise `Err(AdapterError::Unreachable)`.
   - Network, TLS, 5xx, time limit → `Err(AdapterError::Unreachable)` (`STA_UNREACHABLE`). Deadline → `Err(AdapterError::Deadline)`.
   A post is never repeated automatically after a response was not received: an unanswered `chat.postMessage` may have posted, so the adapter returns `Unreachable` and the external action is the Session Supervisor's to reconcile as *unknown outcome* (03:438).

**Follow task (per run)**
5. Every `poll_interval` (10 s, first read 10 s after posting): `conversations.replies` with `channel`, `ts` = thread `ts`, `oldest` = message `ts`, `limit=200`; follow `response_metadata.next_cursor` until empty (SA-PAGE-01: read to the end inside the adapter), at most 10 pages per poll (*assumed here*: 2,000 replies).
6. Apply the reply rule:
   - result found → emit `Resumed` if the phase was `DecisionInSlack`; emit `Ended{Completed, final_message: text}` (text cut to 32,768 characters; Slack mention markup left as text; control characters other than `\n`/`\t` removed); phase `Ended`; stop.
   - approval request found and phase `Waiting` → emit `Paused{DecisionElsewhere{link}}` with the link of that message; phase `DecisionInSlack`; restart the phase clock.
   - nothing → continue.
7. Limits: phase `Waiting` for 30 minutes since posting with no agent message at all → emit `Ended{NoReply, final_message: None}` and log `STA_NO_THREAD_REPLY` with field `expected = "delivery: thread"` when no agent message was ever seen on any run of this remote agent in this process (ADR-009: the missing configuration is reported) — otherwise `STA_NO_REPLY`. Phase `DecisionInSlack` for 24 hours → `Ended{NoReply}` (`STA_DECISION_NOT_SEEN`).
8. Errors while polling: token refused → `tokens.report_refused()`, emit `Paused{SignIn}` once (03:406 "the Slack token is revoked: the run pauses for sign-in"), then try `slack_user_token` every 30 s; when it succeeds emit `Resumed` and continue polling; the no-reply clock does not run while paused for sign-in. 429 → sleep `Retry-After` (minimum the poll interval). Network or 5xx → keep polling; 6 hours with no successful read → `Ended{Abandoned}` (`STA_ABANDONED`, *assumed here*). `channel_not_found`/`thread_not_found`/`not_in_channel` → `Ended{Failed}` (`STA_CHANNEL_UNREACHABLE`).
9. `abandon(run)` → emit `Ended{Abandoned}`, stop polling.

**`events(ctx, run)`** — replay buffered events then live ones. A `RunHandle` does not name its remote agent, so after a restart (03:409 "re-read thread (Slack)") the Session Supervisor re-attaches through `events_for(remote_agent_id, handle)` (below); `events` on a run this process does not follow returns `Err(AdapterError::UnknownRun)`.

```rust
impl SlackThreadAdapter {
    /// Recovery after restart: re-attach to a journalled run and continue polling its thread.
    pub fn events_for(&self, ctx: &AdapterContext, remote_agent_id: RemoteAgentId, run: &RunHandle)
        -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError>;
    /// Thread binding for the remote agent in this process, if any.
    pub fn last_thread(&self, remote_agent_id: RemoteAgentId) -> Option<String>;
}
```
`events_for` emits `Started` with the handle's identifiers, reads the thread once immediately, then polls as above; the no-reply clock starts at the message `ts` time.

**`read(ctx, run)`** — known follower → `Waiting` → `Running`; `DecisionInSlack` → `Paused`; `Ended` → its state and `RunEnd`. Unknown → `Err(AdapterError::UnknownRun)`. `checked_at` = time of the last successful `conversations.replies`.

**`resume`** → `Err(AdapterError::VerbNotOffered{verb: Resume})`. **`cancel`** → `Err(AdapterError::VerbNotOffered{verb: Cancel})`.

**Words shown to the person.** The adapter owns none. "Now: the support agent is working on it in Slack — last checked 20 seconds ago" (11:424), "No answer yet" with **Open the conversation** (11:1073), "Waiting for you in Slack" with **Open in Slack** (11:1074) and the *Needs you* entry (11:426) are drawn by the screens from `RunState`, `checked_at` and the `DecisionElsewhere` link.

## Data Model

N/A — no table. The crate depends only on Identity and the Runtime Adapter interface (2B-3), so it has no `Datastore` handle. The thread binding of each run is persisted in the journalled `RunHandle` (`remote_thread_id`, with ordin's message `ts` in `runtime_session_id`); the per-agent "last thread" binding is in memory and rebuilt from the journal by its owner of record (the Session Supervisor's runs) when a caller needs it after a restart.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Target is an agent instance | `Err(permanent)` `InvalidRequest { code: "RA_TARGET_MISMATCH" }` | `STA_TARGET_MISMATCH` (2C `RA_TARGET_MISMATCH`) | — |
| `StartRun.attachments` non-empty (step 2) | none: `Ok`, the run proceeds without them; logged `warn` | `STA_ATTACHMENTS_DROPPED` | — (nothing is uploaded to Slack) |
| Remote agent not resolved, or identifiers malformed | `Err(permanent)` `Refused { code }` | `STA_AGENT_UNKNOWN` | — |
| Text over 4,000 characters, or Slack `msg_too_long` | `Err(permanent)` `InvalidRequest` / `Refused { code }` | `STA_TEXT_TOO_LONG` | Slack 200 `{"ok":false,"error":"msg_too_long"}` |
| No Slack sign-in in Identity | `Err(permanent)` `SignedOut` | `STA_SIGNED_OUT` | — |
| Token refused at start | `Err(permanent)` `SignedOut` | `STA_TOKEN_REFUSED` | Slack 200 `{"ok":false,"error":"invalid_auth"}` / `token_revoked` |
| Token refused while polling | `Paused{SignIn}`, then `Resumed` | `STA_TOKEN_REFUSED` | as above |
| Scope missing | `Err(permanent)` `Refused { code }` | `STA_MISSING_SCOPE` | Slack 200 `{"ok":false,"error":"missing_scope"}` |
| Channel or thread not reachable | `Err(permanent)` `Refused { code }` at start; `Ended{Failed}` while polling | `STA_CHANNEL_UNREACHABLE` | Slack 200 `{"ok":false,"error":"channel_not_found"}` / `not_in_channel` / `thread_not_found` |
| Rate limited | wait `Retry-After`, retry | `STA_RATE_LIMITED` | HTTP 429, header `Retry-After: 30` |
| Network, TLS, 5xx, 15 s limit at start | `Err(retryable)` `Unreachable` | `STA_UNREACHABLE` | — |
| `ctx.deadline` passed | `Err(retryable)` `Deadline` | `STA_DEADLINE` | — |
| No agent reply within 30 minutes | `Ended{NoReply}` | `STA_NO_REPLY` / `STA_NO_THREAD_REPLY` (first ask never answered: `delivery: thread` expected) | — |
| Decision in Slack not followed by a result within 24 hours | `Ended{NoReply}` | `STA_DECISION_NOT_SEEN` | — |
| No successful read for 6 hours, or `abandon` | `Ended{Abandoned}` | `STA_ABANDONED` | — |
| `resume` called | `Err(permanent)` `VerbNotOffered{Resume}` | `RA_VERB_NOT_OFFERED` | — |
| `cancel` called | `Err(permanent)` `VerbNotOffered{Cancel}` | `RA_VERB_NOT_OFFERED` | — |
| `read`/`events` of an unknown run | `Err(permanent)` `UnknownRun` | `STA_RUN_UNKNOWN` | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Slack Thread Adapter

  Background:
    Given a remote agent "support-agent" with route slack, member "U05SUPPORT1" and channel "C07ORDINAG"
    And the person's Slack user token is valid

  Scenario: Attachments are dropped, not uploaded
    Given a start request for a slack_thread run with one attachment
    When the adapter starts the run
    Then the message is posted without the attachment
    And STA_ATTACHMENTS_DROPPED is logged

  Scenario: Happy path — post, poll, and take the agent's reply as the result
    When start is called with free text "Is RC-12 ready to ship?" and no thread
    Then the adapter posts "<@U05SUPPORT1> Is RC-12 ready to ship?" to channel "C07ORDINAG" with chat.postMessage
    And the RunHandle carries remote_thread_id equal to the posted ts
    When conversations.replies returns a message from "U05SUPPORT1" with text "RC-12 is a go."
    Then the events stream yields started and then ended with reason completed and final message "RC-12 is a go."

  Scenario: Edge — continue an existing thread
    When start is called with thread "1760952061.000200"
    Then chat.postMessage is sent with thread_ts "1760952061.000200"
    And replies are read only after the new message's ts

  Scenario: Edge — the agent asks for an approval in Slack
    When conversations.replies returns a message from "U05SUPPORT1" whose blocks contain an "actions" block
    Then the events stream yields paused with reason decision_elsewhere and a link "https://acme.slack.com/archives/C07ORDINAG/p1760952139004100"
    When a later message from "U05SUPPORT1" without an "actions" block arrives
    Then the events stream yields resumed and then ended with reason completed

  Scenario: Edge — messages from people in the channel are ignored
    When conversations.replies returns only a message from "U02PRIYA"
    Then no result is taken and polling continues

  Scenario: Edge — poll interval and rate limit
    When conversations.replies answers HTTP 429 with Retry-After 30
    Then the next read is sent no sooner than 30 seconds later
    And no two reads for one run are ever closer than 10 seconds

  Scenario: Error — no reply because the trigger does not deliver into the thread
    Given no message from "U05SUPPORT1" has been seen for this agent in this process
    When 30 minutes pass after posting with no reply from the agent
    Then the events stream ends with reason no_reply
    And the log line carries code STA_NO_THREAD_REPLY and expected "delivery: thread"

  Scenario: Error — the Slack token is revoked during a run
    When conversations.replies answers {"ok":false,"error":"token_revoked"}
    Then the events stream yields paused with reason sign_in
    When the person signs in to Slack again
    Then the events stream yields resumed and polling continues

  Scenario: Error — cancel and resume are not offered
    When cancel is called
    Then it returns AdapterError::VerbNotOffered with verb cancel
    When resume is called with Approve
    Then it returns AdapterError::VerbNotOffered with verb resume

  Scenario: Error — the post receives no answer
    When chat.postMessage times out after 15 seconds
    Then start returns AdapterError::Unreachable
    And the message is not posted a second time by the adapter

  Scenario: Recovery after restart
    Given a journalled RunHandle with runtime_session_id "1760952061.000200" and remote_thread_id "1760952061.000200"
    When events_for is called with that handle
    Then the adapter yields started, reads the thread at once, and continues polling

  Scenario: The program abandons a run the Supervisor stopped at a limit
    Given run R is followed in Slack
    When the program calls abandon for R
    Then R's stream ends with Ended reason Abandoned
    And no request is sent to Slack

  Scenario: A hosted ordin agent's approval wait pauses the run
    Given a run posted to "U05SUPPORT1" in channel "C07ORDINAG"
    When conversations.replies returns a message from "U05SUPPORT1" with text "This request is waiting for approval."
    Then the events stream emits Paused with reason decision_elsewhere and that message's link
    When a later message from "U05SUPPORT1" with text "Sent." arrives
    Then the events stream emits Resumed and Ended completed with final message "Sent."

  Scenario: A channel name instead of an identifier is refused
    Given the profile's slack.dedicated_channel is "ordin-agents"
    When start is called for a Slack remote agent without its own channel
    Then it fails with Refused STA_AGENT_UNKNOWN
```

## Performance / Security / Observability

- **Performance targets:** adapter overhead (excluding Slack) p95 ≤ 20 ms for `start` and `read`; one `conversations.replies` call per run per 10 s at most (≤ 6 a minute per run; 8 runs at once stay within 48 a minute, under Slack's per-method limits for user tokens, *assumed here*); a reply is reported within one poll interval plus 1 s of Slack returning it; memory per followed run ≤ 32 KiB; at most 50 runs followed at once.
- **Security:** the user token comes from `SlackUserTokenSource` per call, lives in `SlackUserToken` (redacted `Debug`), and is never logged, journalled, put in an error or written to disk (02:691: no credential reaches a log line or the journal); every call is `https` to `slack.com` through the platform trust store; redirects are not followed; posting needs the caller's approval first (precondition above); thread content is data — the reply text is passed on as text and never interpreted; a message is never re-posted without a new start; only the scopes of SA-SLACK-01 are needed.
- **Observability:** no metrics or spans (SA-XC-04/05). `tracing` events, target `ordin_slack_thread_adapter`, fields `run_id`, `remote_agent_id`, `channel_id`, `thread_ts`, `message_ts`, `correlation_id`, `code`, `slack_error`, `http_status`, `retry_after_secs`: `info` `slack_posted`, `slack_reply_taken`, `slack_decision_in_slack`; `warn` `slack_no_reply` (+ `expected` when `STA_NO_THREAD_REPLY`), `slack_token_refused`, `slack_rate_limited`, `slack_abandoned`; `debug` `slack_poll` (+ `messages_seen`). Never logged: token, message text, reply text.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

**Why.** The Organisation Profile's `SlackSection` now carries the Slack Service's keys (`tools_enabled`, `allowed_channels`, `rules_allowed`), and `workspace` and `dedicated_channel` become `Option<String>`, both present or both absent (`PROF_CONTENT_INVALID` otherwise). This adapter's behaviour is unchanged whenever both are present. The Slack Service (`SlackService.md`) shares no code or state with this adapter: it holds its own `reqwest::Client` and its own token source. The two services keep separate Web API clients (ADR-100: neither crate depends on the other).

**Shared Context — Configuration.** The two rows above are marked amended: `slack.workspace` and `slack.dedicated_channel` are read as `Option<String>`. When either is `None` the adapter is **not offered**: the program does not construct it, and remote agents whose route is Slack are shown as "This agent answers in Slack, which your company hasn't set up here." (message id `sta-route-not-set-up`, *chosen here*). `RemoteAgent.slack_channel` alone does not make the route available, because the workspace domain is needed for the thread link (§ *Link to the thread*).

**Internal Logic — no step changes.** The adapter never reads `slack.tools_enabled`, `slack.allowed_channels` or `slack.rules_allowed`; those govern the Slack Service only. The person's single `CredentialKind::Slack` credential is shared with the Slack Service; a scope added for the Slack Service by incremental consent (SA-SLACK-02) does not change any call this adapter makes.

**Error Table — no new row.** A missing route is an offer decision of the program, not an adapter error.

**Acceptance Criteria — added scenarios.**

```gherkin
  Scenario: No Slack route when the profile sets only the Slack Service keys
    Given the organisation profile's slack section has tools_enabled true and no workspace or dedicated_channel
    When the desktop program starts
    Then no Slack Thread Adapter is constructed
    And a remote agent whose route is Slack is shown with "This agent answers in Slack, which your company hasn't set up here."

  Scenario: Extra Slack scopes do not change the thread calls
    Given the person's Slack credential holds the SA-SLACK-01 scopes and users:read, users:read.email, im:write
    When a run asks a remote agent through Slack
    Then the adapter calls chat.postMessage and conversations.replies exactly as before

  Scenario: Route present as before
    Given slack.workspace "northwind" and slack.dedicated_channel "C0123456789"
    When a run asks a remote agent through Slack
    Then the adapter posts in channel C0123456789 and links the thread under https://northwind.slack.com/
```

**Gaps.** None.
