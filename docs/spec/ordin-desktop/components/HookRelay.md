# SPEC: Hook Relay

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-hook-relay` · **Build phase:** 4 · **Programs:** D P A
> **Dependencies:** Rules Engine, Approval Service, Session Supervisor, Journal

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

The Hook Relay is the core's end of the ordin hook's local channel (02:524–529, ADR-045). It mints a secret for every harness session, listens on one local channel, authenticates each hook call by its session's secret, turns it into a tool request for the Rules Engine, and answers allow, deny, or — when the engine asks a person — holds the hook with an interim `holding` frame until the person decides or the hook's deadline passes. Past the deadline it answers deny, and when the person decides later it resumes the session with the decision through the Session Supervisor, so a slow approval never runs a tool unapproved. It denies on any error, so the hook fails closed. It also carries the harness's after-tool and stop events (ADR-045 *Other events*), reports each session's hook events to the adapter that started it, and gives the Copilot Local Adapter the same decision path in process, since the Copilot CLI runs no hook program (X3b). MCP-bridge connections do not come here: the ordin hook's `mcp-bridge` mode connects to the MCP Front's own socket with the session's MCP token (R8, X2; SA-MCP-01 as amended).

## Approach

One listener task (Unix domain socket on macOS and Linux, named pipe on Windows) and one short-lived task per connection, each reading exactly one request line and writing one or two reply lines (2C wire §4: one request per connection). Session secrets and replay sets live in memory only; the only stored state is the list of approvals whose hook gave up waiting, so a late decision still resumes the session after a restart. Error prefix: `HOOK_` (2C). Rejected: a long-lived connection per session multiplexing calls — a crashed hook would leave the relay holding state for calls nobody waits on, and the per-connection form gives each call its own deadline. Rejected: TCP on `127.0.0.1` — any local user's process could connect; a `0600` socket in the application data root is reachable by the person's own processes only (07, ADR-072).

## Shared Context

### From `ordin-types` (2C wire §3, §4; copied)

```rust
// Environment the adapters set in the harness's environment (2C HookEnvironment, 2D-1c)
// ORDIN_HOOK_ENDPOINT  <app data root>/run/hook.sock (mode 0600); Windows \\.\pipe\ordin-hook-<installation id>
// ORDIN_HOOK_SECRET    32 random bytes, base64url without padding (43 characters), per session; never journalled or logged
// ORDIN_SESSION_ID     the RunId the session belongs to
// ORDIN_HOOK_PROTOCOL  1

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HookRequest {
    pub protocol: u16,
    pub call_id: HookCallId,
    pub session: RunId,
    pub agent_instance_id: AgentInstanceId,
    /// HMAC-SHA256 of the canonical JSON of `message`, keyed with ORDIN_HOOK_SECRET, base64url without padding.
    pub mac: String,
    pub message: HookMessage,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookMessage {
    PreToolUse { harness: HarnessKind, tool: String, origin: ToolOrigin, arguments: serde_json::Value,
                 #[serde(skip_serializing_if = "Option::is_none")] harness_tool_use_id: Option<String>,
                 /// The sub-agent that asked (2C C-20). Conditional on C-78: filled only once the spike
                 /// confirms the harness payload names it; absent until then.
                 #[serde(skip_serializing_if = "Option::is_none")] subagent: Option<String> },
    PostToolUse { harness: HarnessKind, tool: String,
                  #[serde(skip_serializing_if = "Option::is_none")] harness_tool_use_id: Option<String>, succeeded: bool },
    Stop { harness: HarnessKind },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }

/// The relay writes zero or one `Holding` line, then exactly one final line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookReply {
    Holding { call_id: HookCallId, approval_id: ApprovalId, deadline: Timestamp },
    Allow { call_id: HookCallId },
    Deny { call_id: HookCallId, reason: String },
    Ack { call_id: HookCallId },
}
pub struct HookCallId(uuid::Uuid);    // UUIDv7
pub enum ToolOrigin { BuiltIn, OrdinMcpFront { server: String }, PackageMcpServer { server: String } }
pub struct ToolCall { pub run_id: RunId, pub agent_instance_id: AgentInstanceId, pub workspace_id: WorkspaceId,
    pub point: DecisionPoint, pub tool: String, pub origin: ToolOrigin, pub arguments: serde_json::Value }
pub enum DecisionPoint { RunStart, BeforeToolCall, RunEnd }
```

Validation and timing copied from 2C: request line one JSON object ≤ 1 MiB ending in `\n` (`HOOK_MALFORMED` → deny); `protocol` = 1, greater → deny "ordin needs updating" (`HOOK_PROTOCOL_UNSUPPORTED`); `mac` constant-time match (`HOOK_UNAUTHENTICATED` → deny, journalled without the payload); `session` live with a secret (`HOOK_SESSION_UNKNOWN`); `call_id` UUIDv7 not seen before in the session (`HOOK_REPLAYED`); Unix peer user id equals the core's (`HOOK_PEER_REFUSED`). Allow or deny without *ask* within 2 s (LW-21, else the hook denies with `HOOK_CORE_TIMEOUT`); the hook's own deadline 50 s (LW-22, 2D-4); the harness hook timeout ordin registers 60 s (LW-23); `Deny.reason` 1–500 characters; the hook never returns `ask` or `defer` to the harness. Accepted protocol numbers: the current and the previous (2C *Versioning*); with protocol 1 current, only 1 is accepted.

### Other crates used

- `ordin-rules` (Rules Engine spec): `decide_tool_call(&ToolCall, &CallContext) -> RulesOutcome { decision: ToolDecision /* 2C wire */ { verdict: Verdict::{Allow, Deny{reason}, Ask{proposal}}, rule, decided_at }, tool_request_id, approval_id: Option<ApprovalId> }`; `record_outcome(tool_request_id, executed: bool)`; `decide_run_end(run_id) -> EndDecision::{End, Continue{control, message}, Ask{gates, approval_id}}`; `settle_end_gates(run_id, approved) -> EndDecision`; `wait_for_approval(approval_id, until: Option<tokio::time::Instant>) -> Result<ApprovalWait::{Approved, Rejected, Expired, Pending}, RulesError>` (spends the approved-once record on `Approved`); `CallContext { subagent: Option<String>, class: Option<ToolClass> }`.
- `ordin-approval` (Approval Service spec): `wait(approval_id, deadline: Option<Instant>) -> Result<WaitOutcome::{Decided(ApprovalState), DeadlinePassed}, ApprovalError>`; `ApprovalState { Pending, Approved, Rejected, Expired }`.
- `ordin-session-supervisor` (Session Supervisor spec): `is_live(run_id) -> bool`; `resume_with_decision(run_id, ResumeDecision) -> Result<(), SupervisorError>`; `ResumeDecision::{Approve { approval_id }, Reject { approval_id, reason }}`.
- `ordin-journal` (Journal spec, B2 form, R3): `Datastore` (the program passes `journal.datastore().clone()`), `ModuleMigrations` of `MigrationStep` tuples, `EventSink`, `JournalEvent`; `Journal::append_entry(NewJournalEntry { kind, component: "hook_relay", run_id, crew_run_id: None, workspace_id, summary, data }) -> Result<JournalEntry, JournalError>` for `hook_refused` and `hook_stop` ("journal X" in this spec means one such entry).

### Fixed constants

| Constant | Value | Source |
|---|---|---|
| Hook protocol version | 1 | 2C, 08:27 |
| Request read timeout (connection open, no complete line) | 5 s | *chosen here* |
| Decision budget for allow or deny | 1.5 s, inside the hook's 2 s | *chosen here* (LW-21 minus margin) |
| Hook deadline (in `Holding.deadline`) | request receipt + 50 s | 2D-4, LW-22 |
| Replay set per session | the last 10,000 `call_id` values | *chosen here* |
| Concurrent connections | 64; the 65th waits in the accept backlog | *chosen here* |
| Socket directory | `<app data root>/run`, mode `0700`; socket `hook.sock`, mode `0600` | 2C, 2D-1c |

## Public Interface

Crate `ordin-hook-relay`; names *chosen here* unless marked 2C.

```rust
#[derive(Debug, Clone)]
pub struct HookRelayConfig {
    pub app_data_root: std::path::PathBuf,
    /// Windows only: used in the pipe name `\\.\pipe\ordin-hook-<installation id>`.
    pub installation_id: String,
}

#[derive(Debug, Clone)]
pub struct HookSessionSpec {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub harness: HarnessKind,
}

/// What the adapter places in the harness's environment. `secret` is shown nowhere else.
pub struct HookSessionEnv { pub endpoint: String, pub secret: String, pub session_id: RunId, pub protocol: u16 }
impl HookSessionEnv {
    /// [("ORDIN_HOOK_ENDPOINT", …), ("ORDIN_HOOK_SECRET", …), ("ORDIN_SESSION_ID", …), ("ORDIN_HOOK_PROTOCOL", "1")]
    pub fn env_vars(&self) -> [(&'static str, String); 4];
}
impl std::fmt::Debug for HookSessionEnv { /* prints the endpoint and session only; never the secret */ }

/// What one session's hook calls did, for the adapter that started the session (it turns them
/// into `Paused { Approval }`, `Resumed` and progress). One stream per session; it ends when the
/// session is closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookSessionEvent {
    /// The engine asked a person; the hook is held (D.5.1).
    Holding { approval_id: ApprovalId, tool: String },
    /// The hook's deadline passed with no decision; the call was denied and a late decision will
    /// resume the session (D.5.2, G).
    DeniedAtDeadline { approval_id: ApprovalId, tool: String },
    /// The final answer to a tool call: allowed or denied (D.3, D.4, D.5.2).
    Decided { tool: String, allowed: bool },
    /// An after-tool event (E.1); `origin` is the one the call's `PreToolUse` carried.
    ToolUsed { tool: String, origin: ToolOrigin, succeeded: bool },
    /// A stop event arrived (E.2), before the end decision is made.
    TurnStopping,
}

#[derive(Debug, thiserror::Error)]
pub enum HookRelayError {
    #[error("channel could not be opened: {0}")] Listen(#[source] std::io::Error),
    #[error("endpoint path too long")] EndpointTooLong,
    #[error("session already open")] SessionExists,
    #[error("random source unavailable")] Random,
    #[error("datastore unavailable")] Store(#[source] sqlx::Error),
}
impl HookRelayError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }

pub struct HookRelay { /* private */ }
impl HookRelay {
    pub const MODULE: &'static str = "hook_relay";
    pub fn migrations() -> ModuleMigrations;   // module "hook_relay" (ModuleMigrations.module)
    /// Called by: the desktop and server programs at start.
    pub fn new(datastore: Datastore, journal: Arc<Journal>, rules: Arc<RulesEngine>, approvals: Arc<ApprovalService>,
               supervisor: Arc<SessionSupervisor>, config: HookRelayConfig) -> Arc<Self>;
    /// Opens the local channel and starts accepting. Called by: the desktop and server programs,
    /// once at start, before any session (there is no `listen`).
    pub async fn start(self: &Arc<Self>) -> Result<(), HookRelayError>;
    /// Synchronous. Called by: Claude Local Adapter (before starting the harness process; it places
    /// `HookSessionEnv::env_vars()` in the child's environment) and Copilot Local Adapter (before
    /// starting a session; it passes nothing to the CLI, the session's state serves `decide_in_process`).
    pub fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError>;
    /// Synchronous. Called by: both local adapters, when the harness session has ended or paused
    /// for good.
    pub fn close_session(&self, run_id: RunId);
    pub fn endpoint(&self) -> &str;
    /// The session's hook events (F.1). Called by: both local adapters, right after `open_session`.
    /// An unknown run gives a stream that ends at once.
    pub fn session_events(&self, run_id: RunId) -> futures::stream::BoxStream<'static, HookSessionEvent>;
    /// The decision path of one hook call, in process: no socket, no MAC, no replay set (F.2).
    /// Returns the final reply (never `Holding`; holding is reported through `session_events`).
    /// Called by: Copilot Local Adapter, for each `PreToolUse`, `PostToolUse` and the end of a turn
    /// (`Stop`) the Copilot SDK reports.
    pub async fn decide_in_process(&self, run_id: RunId, message: HookMessage) -> HookReply;
    /// The Journal subscription that resumes sessions after a late decision across a restart.
    /// Called by: the desktop and server programs (`journal.subscribe(hook_relay.event_sink())`).
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink>;
    /// Stops accepting and removes the socket file. Called by: the desktop and server programs on
    /// quit (there is no `close`).
    pub async fn shutdown(&self);
}
```

### Example

A Claude Code session of run `0192a1c2-…` asks to call `mcp__supplier-db__update_rate`. The hook writes:

```json
{"protocol":1,"call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00","session":"0192a1c2-3344-7556-8677-8899aabbccdd","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff","mac":"x3mQ9Jt0b2pV8sYk1LwZr6NcA4fH7eDuIq5oGvRnTyE","message":{"kind":"pre_tool_use","harness":"claude_code","tool":"mcp__supplier-db__update_rate","origin":{"kind":"package_mcp_server","server":"supplier-db"},"arguments":{"supplier":"Kestrel","rate":412.5},"harness_tool_use_id":"toolu_01AbCdEf"}}
```

The Rules Engine asks a person (a package tool). The relay writes at once:

```json
{"kind":"holding","call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00","approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f","deadline":"2026-01-10T09:41:57.000Z"}
```

The person approves after 12 s; the relay writes `{"kind":"allow","call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00"}` and closes the connection.

## Internal Logic

**A. `start()`.**
1. Unix: create `<app data root>/run` with mode `0700` when absent (an existing directory with wider permissions is changed to `0700`); the socket path must be ≤ 104 bytes on macOS and ≤ 108 on Linux, else `HOOK_ENDPOINT_TOO_LONG` (the program then cannot run harness sessions and the Setup Assistant reports "the ordin hook answers" as failed, 07 Health checks). Remove a stale `hook.sock` (a socket file no process accepts on: a connect attempt fails with `ECONNREFUSED`); a socket a live process accepts on → `HOOK_LISTEN_FAILED` (a second core is running). Bind `tokio::net::UnixListener`, then `chmod 0600`.
2. Windows: `tokio::net::windows::named_pipe::ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true)` on `\\.\pipe\ordin-hook-<installation id>`; the pipe takes the default security descriptor of the person's process (owner, Administrators, LocalSystem; *assumed here* sufficient, since every call is also authenticated by its HMAC). One new pipe instance is created after each accepted connection.
3. Reload `hook_late_decisions` rows (Data Model) into memory.
4. Spawn the accept loop; at most 64 connection tasks at once (a semaphore).

**B. `open_session(spec)`.** An open session for `run_id` → `HOOK_SESSION_EXISTS` (a resumed session closes the old one first). Draw 32 bytes from the operating system (`getrandom`; failure → `HOOK_RANDOM_UNAVAILABLE`), encode base64url without padding; store `{spec, secret bytes, seen call ids, tool use map}` in memory; return the env. Log `hook_session_opened` {run_id, harness}. The secret is never journalled, logged or stored.

**C. One connection.**
1. Unix: `peer_cred()`; a user id other than the core's → write `Deny { call_id: nil, reason: "refused" }` and close; journal `hook_refused` {code: `HOOK_PEER_REFUSED`}. (The nil UUID `00000000-0000-0000-0000-000000000000` is used wherever the call id cannot be read.)
2. Read one line within 5 s, at most 1 MiB + 1 byte: timeout, a longer line, invalid UTF-8 or invalid JSON → `Deny` with reason "ordin could not read this request" and `HOOK_MALFORMED`.
3. Parse `HookRequest`; failure (unknown field, wrong kind, or a line from an older ordin hook carrying a `bridge` key) → `HOOK_MALFORMED` deny. MCP-bridge connections belong to the MCP Front's socket, never this one.
4. `protocol > 1` → deny "ordin needs updating" (`HOOK_PROTOCOL_UNSUPPORTED`); `protocol < 1` → `HOOK_PROTOCOL_UNSUPPORTED`.
5. Session: no open session for `session`, or `supervisor.is_live(session)` false → `HOOK_SESSION_UNKNOWN` deny.
6. MAC: compute HMAC-SHA256 over the canonical JSON of `message` (H) with the session's secret; decode `mac` from base64url (decoding failure counts as a mismatch); compare in constant time (`hmac::Mac::verify_slice`). Mismatch → `HOOK_UNAUTHENTICATED` deny; journal `hook_refused` {run_id, code} without the payload.
7. `agent_instance_id` different from the session's → `HOOK_SESSION_MISMATCH` deny.
8. `call_id` not a version-7 UUID, or in the seen set → `HOOK_REPLAYED` deny. Add it to the set (oldest dropped beyond 10,000).
9. Dispatch on `message` (D, E). Any error not named in D or E → deny "ordin could not decide" (fail closed), logged `hook_internal` {run_id, code}.
10. Write the final line, flush, close. A write failure (the hook went away) is logged `hook_peer_gone` {run_id, call_id}; nothing else changes — a held approval stays open and G resumes the session when it is decided.

**D. `PreToolUse`.**
1. Build `ToolCall { run_id: session, agent_instance_id, workspace_id: session's, point: BeforeToolCall, tool, origin, arguments }` and `CallContext { subagent, class: None }`; the class is left to the Rules Engine for harness tools. `CallContext.subagent` = the message's `subagent` (2C `HookMessage::PreToolUse`, C-20). Conditional on C-78: the hook fills `subagent` only once the spike shows the harness payload names the sub-agent; until then it is absent, `CallContext.subagent` is `None`, and the Rules Engine lists the reviewer's restriction as not enforced (its fallback).
2. `timeout(1.5 s, rules.decide_tool_call(&call, &ctx))`. Timeout → deny "ordin could not decide in time" (`HOOK_DECISION_TIMEOUT`).
3. `Allow` → remember `harness_tool_use_id → tool_request_id` (when the id is present) for E.1; reply `Allow`.
4. `Deny { reason }` → reply `Deny { reason }` (cut to 500 characters).
5. `Ask` with `approval_id = Some(id)`:
   1. `deadline = received_at + 50 s`; write `Holding { call_id, approval_id: id, deadline }` and flush (an interim line, 2C).
   2. `rules.wait_for_approval(id, Some(deadline))` (the engine spends its approved-once record when the call runs now):
      - `Approved` → reply `Allow`; remember the tool use id as in D.3.
      - `Rejected` → reply `Deny { reason: "The person rejected this action." }` (the model is told so, 03 golden path).
      - `Expired` → reply `Deny { reason: "The run ended before the person decided." }`.
      - `Pending` → reply `Deny { reason: "waiting for the person's approval" }` (09 ADR-045, exact words); insert `(approval_id, run_id)` into `hook_late_decisions` and start the late watcher (G).
      - `RULE_APPROVAL_UNAVAILABLE` → deny "ordin could not decide" (`HOOK_APPROVAL_UNAVAILABLE`).
   3. `Ask` without an approval id is an engine defect: deny with `HOOK_INTERNAL`.

**E. `PostToolUse` and `Stop`.**
1. `PostToolUse { succeeded }` → look up the tool request by `harness_tool_use_id`; found → `rules.record_outcome(id, succeeded)` and drop the entry; absent → log `hook_post_unmatched` {run_id}. Reply `Ack`.
2. `Stop` → `rules.decide_run_end(run_id)`:
   - `End` → `Ack`;
   - `Continue { message }` → `Deny { reason: message }` (the ordin hook turns a deny to a stop event into the harness's "do not stop yet, with this reason"; see the ordin hook spec);
   - `Ask { approval_id }` → `Holding`, then `approvals.wait(id, Some(deadline))`: `Decided(Approved)` → `rules.settle_end_gates(run_id, true)` → `Ack` when it returns `End`; `Rejected` → `settle_end_gates(run_id, false)` → `Deny { reason: message }`; `DeadlinePassed` → `Deny { reason: "waiting for the person's approval" }` and the late watcher (G) settles later.
3. ADR-036's check of a result by Ordin before the turn ends is made by the Ordin Host on the run's result (*assumed here*, SA-HOOK-01: holding a stop event for a model turn would exceed the 50 s deadline); the relay journals `hook_stop` {run_id} for it.

**F. Session events and the in-process path.**
1. `session_events(run_id)`: each open session holds a `tokio::sync::broadcast` sender of capacity 64 (*chosen here*); the call subscribes and returns the receiver as a stream; lagged events are dropped and logged `hook_events_lagged` {run_id, dropped}. Events are sent at: D.5.1 (`Holding`), D.5.2 `Pending` (`DeniedAtDeadline`), every final answer to a `PreToolUse` (`Decided`), E.1 (`ToolUsed`, with the origin remembered from the call's `PreToolUse` in the tool-use map; an unmatched after-tool event carries `BuiltIn`), and E.2 on receipt (`TurnStopping`). `close_session` drops the sender, which ends every stream.
2. `decide_in_process(run_id, message)`: no open session for `run_id`, or `supervisor.is_live(run_id)` false → `Deny { reason: "this session is not known to ordin" }` (`HOOK_SESSION_UNKNOWN`). Mint a `call_id` (UUIDv7) for the reply and the journal; skip C.1–C.8 (no peer, no line, no MAC, no replay set: the caller is in process); run D and E exactly as for a hook call, with the same 50 s deadline measured from the call; a `Holding` answer is not returned, only sent as a session event, and the call returns the final reply. `PostToolUse` and `Stop` return `Ack` or, for a refused ending, `Deny { reason }`. Any internal error → deny "ordin could not decide" (`HOOK_INTERNAL`). Log `hook_decided` as for a hook call, with `route: in_process`.

**G. Late decisions.**
1. A watcher task per row: `approvals.wait(id, None)`; on `Approved` → `supervisor.resume_with_decision(run_id, Approve { approval_id })`; on `Rejected` → `Reject { approval_id, reason: Some("The person chose not to do this.") }`; on `Expired` → nothing. A stop-gate approval (from E.2) settles through `rules.settle_end_gates` before the resume. Then delete the row.
2. A resume error (run ended, verb not offered) → log `hook_late_resume_failed` {run_id, code}; delete the row (the run is gone or cannot be resumed; the Rules Engine's approved-once record still lets the call through if the session repeats it).
3. After a restart, watchers are rebuilt from the table (A.3); the Journal sink also delivers `approval_decided`: an id in the table triggers step 1 directly.

**H. Canonical JSON** (SA-HOOK-02): `ordin_types::encoding::canonical_json` of the object's `serde_json::Value` — the workspace's one definition (SharedTypes.md `encoding.rs`, Logging R-3): the value serialised with object keys sorted by their UTF-8 bytes, no insignificant white space, strings escaped as `serde_json` escapes them, numbers as `serde_json` writes them. `serde_json` without the `preserve_order` feature already keeps `Map` keys sorted; the workspace must not enable `preserve_order` for `ordin-types` (a test asserts the order). The ordin hook computes the same bytes from the same value.

**I. `close_session(run_id)` and `shutdown()`.** Close drops the secret, the replay set and the tool-use map; connections already being served finish with the state they hold. Shutdown stops the accept loop, waits up to 2 s for open connections (each then denied), and removes `hook.sock`.

## Data Model

Module `hook_relay`. One table, holding no secret and no payload.

```sql
-- SQLite, migration 1
CREATE TABLE hook_late_decisions (
    approval_id TEXT PRIMARY KEY CHECK (length(approval_id) = 36),
    run_id TEXT NOT NULL CHECK (length(run_id) = 36),
    kind TEXT NOT NULL CHECK (kind IN ('tool_call', 'stop_gate')),
    created_at TEXT NOT NULL
);
-- PostgreSQL, migration 1 (scaled shape)
CREATE TABLE hook_late_decisions (approval_id uuid PRIMARY KEY, run_id uuid NOT NULL,
  kind text NOT NULL CHECK (kind IN ('tool_call', 'stop_gate')), created_at timestamptz NOT NULL);
```

Rows are deleted when resolved (G.1, G.2) and rows older than 30 days at start (*chosen here*). Session secrets, replay sets and pending calls are memory only: after a restart every harness session is new (the adapters open sessions again and set a new secret).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Socket peer is another user (C.1) | hook reply `deny` | `HOOK_PEER_REFUSED` | `{"kind":"deny","call_id":"00000000-0000-0000-0000-000000000000","reason":"refused"}` |
| Line too long, not JSON, unknown fields, read timeout (C.2, C.3) | `deny` | `HOOK_MALFORMED` | reason "ordin could not read this request" |
| Protocol other than 1 (C.4) | `deny` | `HOOK_PROTOCOL_UNSUPPORTED` | reason "ordin needs updating" |
| No open, live session (C.5) | `deny` | `HOOK_SESSION_UNKNOWN` | reason "this session is not known to ordin" |
| MAC mismatch (C.6) | `deny` | `HOOK_UNAUTHENTICATED` | reason "refused"; journalled without payload |
| Agent instance does not match the session (C.7) | `deny` | `HOOK_SESSION_MISMATCH` | reason "refused" |
| Repeated or malformed call id (C.8) | `deny` | `HOOK_REPLAYED` | reason "refused" |
| The engine does not decide within 1.5 s (D.2) | `deny` | `HOOK_DECISION_TIMEOUT` | reason "ordin could not decide in time" |
| The Approval Service fails while holding (D.5) | `deny` | `HOOK_APPROVAL_UNAVAILABLE` | reason "ordin could not decide" |
| The person did not decide within 50 s (D.5) | `deny` | `HOOK_APPROVAL_PENDING` | reason "waiting for the person's approval" |
| Any other internal error (C.9) | `deny` | `HOOK_INTERNAL` | reason "ordin could not decide" |
| Socket path too long (A.1) | `Err(permanent)` | `HOOK_ENDPOINT_TOO_LONG` | `HookRelayError::EndpointTooLong` |
| Another core holds the socket, or bind fails (A.1, A.2) | `Err(permanent)` | `HOOK_LISTEN_FAILED` | `HookRelayError::Listen` |
| Session opened twice (B) | `Err(permanent)` | `HOOK_SESSION_EXISTS` | `HookRelayError::SessionExists` |
| No randomness from the operating system (B) | `Err(retryable)` | `HOOK_RANDOM_UNAVAILABLE` | `HookRelayError::Random` |
| Store unavailable (A.3, D.5) | `Err(retryable)`; in a call, deny | `HOOK_STORE_UNAVAILABLE` | `HookRelayError::Store` |

The person never sees these codes. Deny reasons reach the model and Diagnostics only. A socket on which another live core accepts is `HOOK_LISTEN_FAILED`; there is no separate "endpoint in use" code.

`HOOK_` is the 2C hook-protocol prefix, raised at both ends of the protocol: by this relay and by the ordin hook program (OrdinHook.md). `HOOK_APPROVAL_PENDING` is the one code both raise, with the same meaning — the person has not decided yet: the relay raises it at its 50 s hold, the hook at its own deadline (Error Handling, EH-D10). Every other `HOOK_` code has one raiser.

Reconciliation assumptions (January 2026):
- **SA-RECON-01** — R8, X2: the relay carries no MCP-bridge traffic; `BridgeHello`, `BridgeOpen`, `BridgeReply`, `BridgeHandler`, `LocalStream` and `BridgeError` are removed, and the programs implement no bridge handler.
- **SA-RECON-03** — R5: a held tool call waits through the Rules Engine's `wait_for_approval`; the stop-gate wait (E.2) and late decisions (G) wait on the Approval Service directly, since neither runs the held call now.
- **SA-RECON-04** — R3, R20: journal records go through `append_entry`; the table lives in the Journal's `Datastore`.
- **SA-RECON-02** — X3b: the Copilot CLI runs no hook program; its tool calls, after-tool events and turn ends reach the relay through `decide_in_process`, which applies D and E unchanged.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Hook Relay

  Scenario: Happy path — an allowed call
    Given a session opened for run "R1" and its secret placed in the hook's environment
    When the hook sends a pre_tool_use for Read with a valid MAC
    Then the relay answers allow within 2 seconds
    And the Rules Engine journalled the tool request

  Scenario: Ask and approve within the deadline
    Given the Rules Engine asks a person for the call
    When the hook sends the request
    Then the relay writes a holding line with the approval id and a deadline 50 seconds after receipt
    When the person approves after 12 seconds
    Then the relay writes allow and closes the connection

  Scenario: Ask and the deadline passes, then the person approves
    Given the Rules Engine asks a person for the call
    When 50 seconds pass with no decision
    Then the relay writes deny with reason "waiting for the person's approval"
    And a late-decision row exists for the approval
    When the person approves 3 minutes later
    Then the Session Supervisor resumes run "R1" with Approve for that approval
    And the late-decision row is deleted

  Scenario: Late decision survives a restart
    Given a late-decision row for approval "A1" of run "R1"
    When the application restarts and the person then approves "A1"
    Then the relay resumes run "R1" with Approve "A1"

  Scenario: Wrong MAC
    When a request arrives whose MAC was made with another secret
    Then the relay answers deny with code HOOK_UNAUTHENTICATED
    And the journal records hook_refused without the arguments

  Scenario: Replay
    Given a request with call id C was answered
    When the same request is sent again
    Then the relay answers deny with code HOOK_REPLAYED

  Scenario: Unknown session
    When a request names a run with no open session
    Then the relay answers deny with code HOOK_SESSION_UNKNOWN

  Scenario: Malformed line
    When the hook sends "not json\n"
    Then the relay answers deny with the nil call id and code HOOK_MALFORMED

  Scenario: Newer protocol
    When a request carries protocol 2
    Then the relay answers deny with reason "ordin needs updating"

  Scenario: The engine is slow
    Given the Rules Engine takes 3 seconds to decide
    When the hook sends a request
    Then the relay answers deny "ordin could not decide in time" within 1.6 seconds

  Scenario: Stop refused by a judged loop
    Given the Rules Engine's end decision is Continue with "The run cannot end yet. The control judged-loop:draft requires a call to the sub-agent checker before the run ends."
    When the hook sends a stop message
    Then the relay answers deny with that reason

  Scenario: Post tool use is acknowledged
    Given an allowed call with harness tool use id "toolu_01AbCdEf"
    When the hook sends post_tool_use with succeeded true for that id
    Then the relay answers ack and the tool request is recorded as executed

  Scenario: A line from an older bridge is refused
    Given a session for run "R1"
    When a connection sends a line with a top-level "bridge" key
    Then the relay answers deny with code HOOK_MALFORMED and closes

  Scenario: Session events reach the adapter
    Given the Claude Local Adapter subscribed to session_events for run "R1"
    When the Rules Engine asks a person for a call of "mcp__supplier-db__update_rate" and 50 seconds pass
    Then the stream yields Holding then DeniedAtDeadline for that approval and tool
    When the hook later sends post_tool_use for an allowed call of Read with succeeded true
    Then the stream yields ToolUsed for Read with origin built_in

  Scenario: Copilot decision in process
    Given a session opened for Copilot run "R2"
    When the Copilot Local Adapter calls decide_in_process with pre_tool_use for "bash" whose engine verdict is deny
    Then it returns deny with the engine's reason without any socket connection
    And the stream of session_events yields Decided for "bash" with allowed false

  Scenario: In-process call for an unknown session
    When decide_in_process is called for a run with no open session
    Then it returns deny "this session is not known to ordin"

  Scenario: Peer from another user
    Given a process of another operating-system user connects to the socket
    Then the relay answers deny with code HOOK_PEER_REFUSED

  Scenario: Socket permissions
    When the relay starts
    Then "<app data root>/run" has mode 0700 and "hook.sock" has mode 0600
```

## Performance / Security / Observability

- **Performance targets:** allow or deny without *ask*: p95 ≤ 20 ms and p99 ≤ 100 ms from line received to reply written (including the Rules Engine's ≤ 5 ms p95), far inside the hook's 2 s; *holding* written within 30 ms of receipt. Up to 64 concurrent connections; 8 local sessions × a few calls a second is the desktop load. Memory per session under 1 MiB (10,000 call ids at 16 bytes plus the tool-use map).
- **Security:** Local channel only: a `0600` socket in a `0700` folder in the person's application data root, with a peer user-id check; on Windows a per-installation pipe refusing remote clients. Every request is authenticated by HMAC-SHA256 under a 256-bit per-session secret that exists only in memory and in the harness child's environment (whose shell tool is off, 02:507), compared in constant time; each call id is single use per session. Fails closed on every error and timeout. The relay never returns *ask* to a harness. Never logged or journalled: the secret, the MAC, tool arguments, deny reasons with values (reasons hold names only, Rules Engine).
- **Observability:** log events (target `ordin_hook_relay`): `hook_session_opened` {run_id, harness}; `hook_session_closed` {run_id}; `hook_decided` {run_id, call_id, verdict, elapsed_ms}; `hook_holding` {run_id, approval_id}; `hook_refused` {run_id?, code}; `hook_peer_gone` {run_id, call_id}; `hook_post_unmatched` {run_id}; `hook_stop` {run_id, outcome}; `hook_late_resume_failed` {run_id, code}; `hook_events_lagged` {run_id, dropped}; `hook_internal` {run_id, code}. `hook_decided` carries `route` (`socket`, `in_process`). Agent profile only: no metric of its own (Metrics.md N-1; refusals and pauses are counted by the Rules Engine's families 12 and 13); for each built-in tool call (`origin = "built_in"`) this crate opens the exported span `ordin.tool_call` with fields `tool`, `origin`, `outcome`, `control` only (Tracing.md allow list; T-D1).

## Gaps

None.
