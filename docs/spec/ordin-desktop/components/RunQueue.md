# SPEC: Run Queue

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-run-queue` · **Build phase:** 2 · **Programs:** A
> **Dependencies:** Journal

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

The Run Queue keeps the record of every run and every thread of a hosted agent (agent profile only): who started it, its scope, its request, its state, its lease, its outcome and, while it is paused, the harness's saved session record (02:801–803, 04 *Run record*, ADR-098 item 5). A run that the Caller API, the Duty Timer, an intake or a chat connection accepts is recorded as queued. A run instance (the Session Supervisor in the runs role) takes one queued run in one statement, with a lease of 30 seconds that it renews, so one run is executed by one instance at a time; a run whose lease lapses is recorded as interrupted and never started again, because a tool call already made would be made twice (02:801, 04 *Run record* lifecycle, wrightrun ADR-047). A paused run is queued again when its decision arrives. The Run Queue also keeps each run's event list, which the Caller API's run event stream reads (ADR-098 item 2, "an event stream for one run"), records cancellation (ADR-098 item 2, "cancelling a run"), and counts and deletes records for the retention job and an operator's erasure (ADR-098 item 6).

## Approach

Three tables owned by this crate — `run_threads`, `run_records`, `run_events` — and one statement (or one short transaction on SQLite) for each change of state, written so that the test and the change are the same statement and the datastore picks the single winner: the take uses `UPDATE … WHERE id = (SELECT … FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING` on PostgreSQL and the same statement without the lock clause on SQLite, whose single writer serialises it (wrightrun `RunQueue.md` step 7, E-61). The guarantee that one schedule firing starts one run is a unique index on `run_records (started_by_id, firing_due)` for schedule runs, so `enqueue_firing` is one `INSERT … ON CONFLICT DO NOTHING` on this crate's own table and needs no table of another module (2B-1 rule 5). The storage trait `RunStore` is implemented twice, `SqliteRunStore` and `PgRunStore` (SA-DB-01). Error prefix: `RUN_` for queue operations (wrightrun's codes, kept because the Caller API returns them unchanged, SA-ENV-02), `DB_` for `DB_UNAVAILABLE` (also a wrightrun carried code, kept under its own name for the same reason and passed through unchanged by the Caller API, Duty Timer, Job Runner and Retention and Erasure, EH-D12), and the 2C `RREC_` codes for record validation. Rejected: `LISTEN`/`NOTIFY` to wake run instances — one take per second per free place costs less than a second channel to keep alive (wrightrun SA-77). Rejected: writing the Duty Timer's `schedule_firings` table in the same statement as wrightrun did — no module writes another's table here (2B-1 rule 5).

### Reconciliation assumptions

- **SA-RECON-01** The Run Queue adds `paused_with_command` because the Rules Engine's `RulesHost::paused_run_with_command` is answered from this crate in the agent profile and no other method gives it in one statement (reconciliation O43, ruling R6). `runs_in_progress` and `run_ended` are answered from the existing `in_progress` and `get`.
- **SA-RECON-02** There is no `cancel_requested` method; `renew` returning `CancelRequested` is the one cancel signal an executing run receives (reconciliation rows 438–439, N120, O110).
- **SA-CLOSE-01** *(K14)* `PendingItem` carries `approval_id: Option<ApprovalId>`, so the Caller API's resume route can pass it to `ApprovalService::decide_by_operator`. This crate stores and returns it unchanged in `outcome`; it never reads or checks the Approval Service.
- **SA-RECON-03** `RunRecord`, `Page<T>` and `Cursor` are copied from the corrected 2C (decision 80: C-15, C-28); the migrations are `MigrationStep` tuples returned by `migrations()` (ruling R3).

## Shared Context

### Shared types used (copied from Phase 2C, domain part, `ordin-types`)

```rust
// ordin-types — Timestamp, ids (phase-2c-shared-types-domain.md §Conventions)
/// RFC 3339, UTC, millisecond precision, `Z` suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
// serialises as "2026-01-10T08:15:30.123Z"; refuses any other form (TS_NOT_RFC3339_MS_UTC)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunRecordId(pub uuid::Uuid);   // UUIDv7 (ID_NOT_UUIDV7 otherwise)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScopeId(pub uuid::Uuid);       // UUIDv7

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskArgument { pub name: String, pub value: String }

// ordin-types — Agent profile (phase-2c-shared-types-domain.md §RunRecord)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunRecord {
    pub id: RunRecordId,
    pub run_id: String,
    pub thread_id: String,
    pub started_by: RunStarter,
    pub scope_id: ScopeId,
    pub request: RunRequest,
    /// Gathered context; its shape is wrightrun's (LD-38).
    pub gathered_context: serde_json::Value,
    pub state: RunRecordState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_kind: Option<RunEndKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_holder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    pub stretch_summaries: Vec<String>,
    pub queued_at: Timestamp,
    /// amended January 2026, decision 80: C-28.
    pub created_at: Timestamp,
    /// The first time an instance took the run; a resumed run keeps it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunStarter {
    Caller { caller_id: String },
    ScheduleEntry { entry: String },
    Intake { intake: String },
    ChatConnection { connection: String, asker: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunRequest {
    Command { command: String, arguments: Vec<TaskArgument> },
    Message { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunRecordState { Queued, Executing, Paused, Ended, Cancelled, Interrupted }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// `Control`: a control ended the run (wire state `ended-by-control`; 2C C-28).
pub enum RunEndKind { Completed, Control, Failed, RunLimit }
```

2C rules on `RunRecord` that this crate enforces (phase-2c-shared-types-domain.md §RunRecord):

| Field | Rule | Error code |
|---|---|---|
| (thread_id) | at most one run in `queued`, `executing` or `paused` | `RUN_THREAD_BUSY` (2C C-28; the carried wrightrun code callers parse, SA-ENV-02, SA-ERR-01) |
| lease_holder, lease_expires_at | present if and only if `state = executing` | `RREC_LEASE_MISMATCH` |
| end_kind | present if and only if `state = ended` | `RREC_END_KIND_MISMATCH` |
| state = interrupted | never started again | `RREC_INTERRUPTED_RESTART` |
| message text | 1–262,144 characters (2C assumed 32,000; raised here to the Caller API's limit, which ADR-098 item 2 keeps) | `RREC_MESSAGE_INVALID` |

State transitions (2C): `— → queued` (accepted); `queued → executing` (lease taken); `executing → paused` (approval needed, session record stored); `paused → queued` (decision); `executing → ended` (completed, failed, run limit); `queued | executing | paused → cancelled` (caller cancels); `executing → interrupted` (lease lapses).

### Datastore (owned by the Journal crate, 2B-1 rule 5)

Copied from JournalAndArtefactStore.md (ruling R3):

```rust
// ordin-journal
#[derive(Clone)]
pub enum Datastore {
    Sqlite(sqlx::SqlitePool),   // desktop, personal server, agent single shape
    Postgres(sqlx::PgPool),     // agent scaled shape
}
/// (version, description, SQL). Versions start at 1 and increase by exactly 1 within a module.
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
// The program passes this crate's `migrations()` to `MigrationRegistry::register(ModuleMigrations)`.
```

### Paging (copied from 2C domain, `ordin_types::paging`, decision 80: C-15)

```rust
pub const PAGE_SIZE: usize = 100;
pub const CURSOR_MAX_CHARS: usize = 1024;
pub struct Page<T> { pub items: Vec<T>, pub next_cursor: Option<Cursor> }   // next_cursor always serialised, null on the last page
pub struct Cursor(String);   // opaque: base64url of the JSON form of the last row's sort key
impl Cursor {
    pub fn encode<K: Serialize>(key: &K) -> Cursor;
    /// Refuses more than 1,024 characters, bad base64url, or JSON that is not a `K` (code SRV_BAD_CURSOR).
    pub fn decode<K: serde::de::DeserializeOwned>(&self) -> Result<K, TypesError>;
}
```

### Constants

| Constant | Value | Source |
|---|---|---|
| `LEASE_SECONDS` | 30 | 2D-4 *Run lease* (wrightrun `RunQueue.md:92`, SA-77) |
| `MAX_PROGRESS_LINE_CHARS` | 160 | LW-7 (`RunProgress.line`) |
| `MAX_EVENTS_PER_RUN` | 10,000 | *chosen here*: bounds one run's stream rows |
| `MAX_MESSAGE_CHARS` | 262,144 | wrightrun `RuntimeServer.md` `MAX_TEXT_CHARS` |
| `MAX_ARGUMENTS_BYTES` | 1,048,576 | *chosen here*: the largest intake body (wrightrun `EventIntake.md` `max-body-bytes` ceiling), which becomes a command's arguments |
| `MAX_STARTER_ID_CHARS` | 512 | wrightrun `0002_runs.sql` |
| `MAX_SESSION_RECORD_BYTES` | 67,108,864 (64 MiB) | *chosen here*: the harness session record of one paused run (C-80 measures the real size) |
| `MAX_DELETE_LIMIT` | 500 | wrightrun `RETENTION_BATCH` |

No environment variable and no configuration key is read by this crate.

## Public Interface

```rust
// crate ordin-run-queue
use async_trait::async_trait;
use ordin_types::{ApprovalId, RunRecord, RunRecordId, RunRecordState, RunEndKind, RunRequest, RunStarter,
                  ScopeId, TaskArgument, Timestamp};
use ordin_types::paging::Page;

/// One item awaiting a decision while the run is paused (wrightrun RunResult.pending).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingItem {
    pub kind: PendingKind,
    pub tool: Option<String>,
    pub gate: Option<String>,
    /// "approval-list" | "argument-rule:<name>" | "refused-ending"
    pub reason: String,
    /// The Approval Service card raised for this item (SA-CLOSE-01, K14): set by the Session
    /// Supervisor when it pauses a run on a tool call that has an approval card; `None` for a
    /// refused ending and for an item with no card. Absent from older rows (serde default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingKind {
    #[serde(rename = "tool-call")] ToolCall,
    #[serde(rename = "refused-ending")] RefusedEnding,
}

/// What a run produced, kept beside the 2C record (chosen here; wrightrun's RunResult keys,
/// stored as one JSON object in `run_records.outcome`). `RunRecord.result` holds `reply`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunOutcome {
    pub reply: Option<String>,
    pub ending: Option<String>,
    #[serde(default)] pub pending: Vec<PendingItem>,
    #[serde(default)] pub actions_asked: Vec<String>,
    #[serde(default)] pub actions_attached: Vec<String>,
    pub delivery: Option<Delivery>,
    #[serde(default)] pub opened_items: Vec<String>,
    pub error_code: Option<String>,
    #[serde(default)] pub safe_reply: bool,
    /// True when a control ended the run with no model call (wrightrun "ended-by-control").
    #[serde(default)] pub ended_by_control: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery { Asker, Thread }

/// A record with the times and outcome the Caller API and operator routes return.
#[derive(Debug, Clone, PartialEq)]
pub struct RunView {
    pub record: RunRecord,
    pub outcome: Option<RunOutcome>,
    pub created_at: Timestamp,          // equal to record.created_at (2C, decision 80: C-28)
    pub started_at: Option<Timestamp>,  // equal to record.started_at
    pub ended_at: Option<Timestamp>,    // equal to record.ended_at
    pub cancel_requested: bool,
}

/// What `take` hands the run instance.
#[derive(Debug, Clone)]
pub struct TakenRun {
    pub view: RunView,
    /// The decision given to the last `resume`; None on a first execution.
    pub decision: Option<serde_json::Value>,
    /// The harness session record stored at the last pause (C-80).
    pub session_record: Option<Vec<u8>>,
}

/// Input of `enqueue`.
#[derive(Debug, Clone)]
pub struct NewRun {
    pub started_by: RunStarter,
    pub scope_id: ScopeId,
    pub request: RunRequest,
    pub gathered_context: serde_json::Value,     // `{}` when none
    pub thread_id: Option<String>,               // None: a new thread
    pub run_id: Option<String>,                  // None: minted here (UUIDv7)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseRenewal { Kept, Lost, CancelRequested }

#[derive(Debug, Clone)]
pub struct RunEnd {
    pub end_kind: RunEndKind,
    pub outcome: RunOutcome,
    pub stretch_summary: Option<String>,
}
#[derive(Debug, Clone)]
pub struct RunPause {
    pub outcome: RunOutcome,               // `pending` non-empty
    pub session_record: Vec<u8>,
    pub stretch_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CancelOutcome {
    /// The run was queued or paused and is now cancelled.
    Cancelled(RunView),
    /// The run is executing; its instance stops it at its next renewal.
    Requested(RunView),
}

#[derive(Debug, Clone, Default)]
pub struct InProgressFilter {
    pub command: Option<String>,
    pub intake: Option<String>,
}

/// What a delete (or its count) of runs returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletedRuns {
    pub runs: u64,
    pub thread_ids: Vec<String>,
    pub run_ids: Vec<String>,
}

/// One stored event of a run's stream.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredRunEvent {
    pub run_id: String,
    pub seq: u64,                     // 1, 2, 3 … per run
    pub kind: RunEventKind,
    pub data: serde_json::Value,
    pub at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind { RunStateChanged, RunProgress }

/// Counter sink; the program forwards to the Telemetry crate in the agent profile.
pub trait MetricsSink: Send + Sync {
    fn add(&self, name: &'static str, labels: &[(&'static str, &str)], by: u64);
}

#[async_trait]
pub trait RunQueue: Send + Sync {
    async fn enqueue(&self, new: NewRun) -> Result<RunRecord, RunQueueError>;
    async fn enqueue_firing(&self, entry_key: &str, due: Timestamp, scope_id: ScopeId,
                            command: &str, arguments: &str) -> Result<Option<RunRecord>, RunQueueError>;
    async fn thread_owner(&self, thread_id: &str) -> Result<Option<RunStarter>, RunQueueError>;
    async fn take(&self, instance: &str) -> Result<Option<TakenRun>, RunQueueError>;
    async fn renew(&self, run_id: &str, instance: &str) -> Result<LeaseRenewal, RunQueueError>;
    async fn end(&self, run_id: &str, instance: &str, end: RunEnd) -> Result<(), RunQueueError>;
    async fn pause(&self, run_id: &str, instance: &str, pause: RunPause) -> Result<(), RunQueueError>;
    async fn resume(&self, run_id: &str, decision: serde_json::Value) -> Result<RunRecord, RunQueueError>;
    async fn cancel(&self, run_id: &str) -> Result<CancelOutcome, RunQueueError>;
    async fn record_cancelled(&self, run_id: &str, instance: &str, outcome: RunOutcome) -> Result<(), RunQueueError>;
    async fn decision(&self, run_id: &str) -> Result<Option<serde_json::Value>, RunQueueError>;
    async fn get(&self, run_id: &str) -> Result<Option<RunView>, RunQueueError>;
    async fn list(&self, states: Option<&[RunRecordState]>, cursor: Option<&str>) -> Result<Page<RunView>, RunQueueError>;
    async fn in_progress(&self, filter: &InProgressFilter) -> Result<u64, RunQueueError>;
    /// Whether a paused run other than `except_run_id` has one of `commands` (the Rules Engine's
    /// duty-overlap fact, through the program's `RulesHost`).
    async fn paused_with_command(&self, commands: &[String], except_run_id: &str) -> Result<bool, RunQueueError>;
    async fn mark_interrupted(&self) -> Result<Vec<String>, RunQueueError>;
    async fn append_progress(&self, run_id: &str, instance: &str, line: &str) -> Result<bool, RunQueueError>;
    async fn events_after(&self, run_id: &str, after_seq: u64, limit: u32) -> Result<Vec<StoredRunEvent>, RunQueueError>;
    // retention and erasure
    async fn count_ended_before(&self, cutoff: Timestamp, limit: u32) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_ended_before(&self, cutoff: Timestamp, limit: u32) -> Result<DeletedRuns, RunQueueError>;
    async fn count_started_by_asker(&self, connection: &str, asker: &str) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_started_by_asker(&self, connection: &str, asker: &str) -> Result<DeletedRuns, RunQueueError>;
    async fn count_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError>;
}

/// The one implementation; it holds a storage trait object chosen by the datastore.
pub struct DatastoreRunQueue { /* store: Box<dyn RunStore>, metrics: Arc<dyn MetricsSink> */ }
impl DatastoreRunQueue {
    pub fn new(datastore: &ordin_journal::Datastore, metrics: std::sync::Arc<dyn MetricsSink>) -> Self;
}
/// This crate's migrations (module name "run_queue"; version 1 is the Data Model below).
pub fn migrations() -> ordin_journal::ModuleMigrations;

#[derive(Debug, thiserror::Error)]
pub enum RunQueueError {
    #[error("No thread has this identifier.")]                          ThreadNotFound,
    #[error("The thread has a run that has not ended.")]                 ThreadBusy,
    #[error("A run has this identifier already.")]                       DuplicateId,
    #[error("A run starts with a command or with a message, not both.")] BadInput { field: &'static str },
    #[error("The run is not paused.")]                                   NotPaused,
    #[error("The run has ended and cannot be cancelled.")]               NotCancellable,
    #[error("The run is no longer executing under this instance's lease.")] LeaseLost,
    #[error("The session record exceeds 64 MiB.")]                       SessionRecordTooLarge,
    #[error("the cursor is not one this listing returned")]              BadCursor,
    #[error("The runtime database did not answer.")]                     Unavailable,
}
impl RunQueueError {
    pub fn code(&self) -> &'static str;     // see Error Table
    pub fn retryable(&self) -> bool;        // true for Unavailable only
}
```

`thiserror` is already a workspace dependency (`memory/Cargo.toml:32`); `async-trait` likewise (`memory/Cargo.toml:39`).

**Called by.**

| Public item | Called by |
|---|---|
| `DatastoreRunQueue::new`, `migrations`, `mark_interrupted` | the server program (OrdinServer, agent profile; `mark_interrupted` every 10 s in each run instance; the Session Supervisor never calls it) |
| `enqueue`, `thread_owner`, `get`, `resume`, `cancel`, `events_after`, `list` | Caller API (caller, intake and operator routes) |
| `enqueue` | Channel Gateway, through the program's `HostedAgentCore` (a chat message starts a run) |
| `enqueue_firing` | Duty Timer |
| `take`, `renew`, `end`, `pause`, `record_cancelled`, `append_progress` | Session Supervisor (agent-profile run worker; `append_progress` for each `Progress` event) |
| `resume` | Session Supervisor (an agent-profile approval decided: `{"approval_id", "state"}`) |
| `decision` | no component calls it today; the Session Supervisor reads the decision from `TakenRun.decision` |
| `in_progress`, `paused_with_command`, `get` | the program's `RulesHost` for the Rules Engine (`runs_in_progress(command)` = `in_progress(InProgressFilter { command: Some(command), intake: None })`; `paused_run_with_command`; `run_ended` from `get`) |
| `count_ended_before`, `delete_ended_before`, `count_started_by_asker`, `delete_started_by_asker`, `count_in_scope`, `delete_in_scope` | Retention and Erasure |
| `MetricsSink` | implemented by the program, forwarding to Telemetry; the Run Queue emits no journal events (reconciliation row 445) |

Interface rules fixed here:

- `enqueue` mints `run_id` and, for a new thread, `thread_id` as UUIDv7 strings (SA-ID-01) when not given. `RunRecord.id` is the same UUID as `run_id` (*chosen here*). A given `run_id` that is not a UUIDv7 string raises `BadInput { field: "run_id" }`; one a run already has raises `DuplicateId`.
- A run on an existing thread keeps the `started_by` given; the thread keeps the starter it was created with.
- `enqueue_firing` records the run of one schedule firing on a new thread, started by `RunStarter::ScheduleEntry { entry: entry_key }`, request `Command { command, arguments: [TaskArgument { name: "arguments", value: arguments }] }` (the argument name `arguments` is *chosen here*; wrightrun passes one text). It returns `None`, writing nothing, when a run for this `entry_key` and `due` exists. `entry_key` is 1–512 characters; `command` is 1–64 characters matching `^[a-z0-9]+(-[a-z0-9]+)*$`.
- `end` accepts `end_kind` ∈ {`Completed`, `Control`, `Failed`, `RunLimit`}. `Completed` with `outcome.ended_by_control` true is stored as `Control`; `Control` with `outcome.ended_by_control` false raises `BadInput { field: "end_kind" }`; `interrupted` is written by `mark_interrupted` only; `cancelled` by `cancel` and `record_cancelled` only.
- `end`, `pause`, `record_cancelled` and `append_progress` raise `LeaseLost` when the run is not executing under this instance's name.
- `renew` returns `CancelRequested` (and extends the lease) when a cancel was requested for an executing run; the instance then stops the harness and calls `record_cancelled`.
- `resume` raises `NotPaused` also when the run does not exist; callers that must tell the two apart call `get` first.
- `decision` returns the value given to the last `resume`, and `None` for a run never resumed or paused again since.
- `in_progress` counts runs in `queued`, `executing` or `paused` matching every filter given.
- `paused_with_command` returns `false` for an empty `commands` without reaching the datastore; `except_run_id` that is not a UUID excludes nothing.
- The queue offers no `cancel_requested(run_id)`: an executing run learns of a cancel from `renew`, which returns `LeaseRenewal::CancelRequested` (reconciliation row 439).
- `list` returns runs newest first (by `seq`), 100 to a page; `states = None` means every state; `Some(&[])` returns an empty page.
- The three `delete_…` methods delete whole threads with every run on them (a run in any state, an executing one included; its instance learns it at its next `renew`, which returns `Lost`). The three `count_…` methods return what the matching delete would delete at that moment and delete nothing.
- `delete_started_by_asker` finds what a chat user started through `connection` and nothing a caller's token started.
- Every method that takes a `run_id` or `thread_id` answers a string that is not a UUID as "not found" (`None`, `false`, or the method's named error) without reaching the datastore.

### Example

```rust
let queue = DatastoreRunQueue::new(&datastore, metrics.clone());
let rec = queue.enqueue(NewRun {
    started_by: RunStarter::Caller { caller_id: "00u8k2release-bot".into() },
    scope_id: agent_wide,                                   // ScopeId of the agent-wide scope
    request: RunRequest::Command {
        command: "run-go-no-go".into(),
        arguments: vec![TaskArgument { name: "arguments".into(), value: "RC-12".into() }],
    },
    gathered_context: serde_json::json!({}),
    thread_id: None, run_id: None,
}).await?;
// rec.run_id == "01929a3e-5f0c-7a4e-8d3b-2b1d6e0f9c11", rec.state == Queued

let taken = queue.take("runs-7c9d4-x2k8p").await?.unwrap();      // state Executing, lease 30 s
queue.pause(&taken.view.record.run_id, "runs-7c9d4-x2k8p", RunPause {
    outcome: RunOutcome { pending: vec![PendingItem { kind: PendingKind::ToolCall,
        tool: Some("create_ticket".into()), gate: None, reason: "approval-list".into(),
        approval_id: Some(ApprovalId(uuid::uuid!("0192a1c3-4455-7667-8889-aabbccddeeff"))) }], ..Default::default() },
    session_record: session_bytes, stretch_summary: Some(summary_json),
}).await?;
queue.resume(&rec.run_id, serde_json::json!({"decisions": [{"type": "approve"}], "set_aside": []})).await?;
// state Queued; the next take returns decision Some(...) and session_record Some(session_bytes)
```

The row of that run after `pause` (PostgreSQL):

```
state='paused' lease_holder=NULL lease_expires_at=NULL decision=NULL
outcome={"reply":null,"ending":null,"pending":[{"kind":"tool-call","tool":"create_ticket","gate":null,"reason":"approval-list","approval_id":"0192a1c3-4455-7667-8889-aabbccddeeff"}],
         "actions_asked":[],"actions_attached":[],"delivery":null,"opened_items":[],"error_code":null,"safe_reply":false,"ended_by_control":false}
session_record=<bytes>
```

## Internal Logic

Every numbered step is one statement unless it says "transaction". `R` stands for the full column list of `run_records`. A connection or I/O error from sqlx becomes `RunQueueError::Unavailable` (log `run_queue_unavailable` at WARN with `operation`; never the connection string); it is not repeated in the steps. On SQLite, `now` is `strftime('%Y-%m-%dT%H:%M:%fZ','now')`; on PostgreSQL `now()`. Every value is a bound parameter.

**Conversions.** `RunStarter` maps to three columns: `Caller` → (`caller`, caller_id, NULL); `ScheduleEntry` → (`schedule`, entry, NULL); `Intake` → (`intake`, intake, NULL); `ChatConnection` → (`chat`, connection, asker). `RunRequest` is stored as JSON in `request`, with `command` copied to its own column. `RunOutcome` is stored as JSON in `outcome`; a key the stored object lacks takes its default and a key the type does not know is ignored, so an older image reads a newer one's rows (ADR-098 item 5, additive change only).

**`enqueue(new)`**

1. Validate in code, writing nothing on failure (`BadInput { field }`): `started_by` ids 1–512 chars; `Message.text` 1–262,144 chars with at least one non-white-space char and no NUL; `Command.command` matches the slug pattern, ≤64 chars; the arguments' values joined total ≤1,048,576 bytes UTF-8 and hold no NUL; `run_id`, when given, parses as a UUIDv7; `gathered_context` is a JSON object.
2. With `thread_id = None`, one transaction: insert `run_threads (thread_id, started_by_kind, started_by_id, started_by_asker, created_at)`; insert `run_records` with `state='queued'`, `queued_at = created_at = now`; insert the first `run_events` row (`seq` 1, kind `run_state_changed`, data `{"state":"queued"}`). Return the record.
3. With a `thread_id`: the same transaction without the thread insert, preceded by `SELECT 1 FROM run_threads WHERE thread_id = ?`; no row → `ThreadNotFound`.
4. A unique violation: constraint `run_records_one_open_per_thread` → `ThreadBusy`; primary key → `DuplicateId`.
5. `metrics.add("wrightrun_runs_queued_total", [("started_by", kind)], 1)`; log `run_queued` (INFO; `run_id`, `thread_id`, `started_by` = kind, `command` or `"none"`). Arguments, message and context are never logged.

**`enqueue_firing(entry_key, due, scope_id, command, arguments)`**

6. Validate (`BadInput`): `entry_key` 1–512 chars; `command` a slug ≤64 chars; `arguments` ≤4,096 chars (wrightrun `ScheduleEntry.arguments`).
7. One transaction: `INSERT INTO run_records (…, started_by_kind, started_by_id, firing_due, …) VALUES (…, 'schedule', :entry_key, :due, …) ON CONFLICT (started_by_id, firing_due) WHERE started_by_kind = 'schedule' DO NOTHING RETURNING R`. No row returned → roll back, return `None` (nothing counted, nothing logged). A row → insert its `run_threads` row and the first `run_events` row in the same transaction, commit, count and log as step 5, return the record. On PostgreSQL the thread row is inserted first inside the transaction and the whole transaction is rolled back when the run insert returns no row, so no orphan thread remains.

**`thread_owner(thread_id)`**

8. `SELECT started_by_kind, started_by_id, started_by_asker FROM run_threads WHERE thread_id = ?`; map back to `RunStarter`; no row → `None`.

**`take(instance)`**

9. PostgreSQL:
   ```sql
   UPDATE run_records
   SET state = 'executing', lease_holder = $1, lease_expires_at = now() + make_interval(secs => 30),
       started_at = coalesce(started_at, now())
   WHERE id = (SELECT id FROM run_records WHERE state = 'queued'
               ORDER BY queued_at, seq FOR UPDATE SKIP LOCKED LIMIT 1)
   RETURNING R;
   ```
   SQLite: the same statement without `FOR UPDATE SKIP LOCKED`, inside `BEGIN IMMEDIATE`, with `lease_expires_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','+30 seconds')`. In the same transaction append a `run_state_changed` event `{"state":"executing"}`.
10. No row → `None`. A row → `metrics.add("wrightrun_runs_started_total", [("started_by", kind)], 1)`; log `run_taken` (INFO; `run_id`, `thread_id`, `instance`); return `TakenRun` with `decision` and `session_record` read from the same row. A resumed run keeps its first `started_at`.

**`renew(run_id, instance)`**

11. `UPDATE run_records SET lease_expires_at = now + 30 s WHERE id = ? AND lease_holder = ? AND state = 'executing' RETURNING cancel_requested_at`. No row → `Lost`. A row with `cancel_requested_at` set → `CancelRequested`; otherwise `Kept`. The statement does not test `lease_expires_at`: a late renewal before any instance recorded the run as interrupted keeps the run; whichever of this statement and step 18 commits first decides.

**`end(run_id, instance, end)`**

12. Validate: `end.end_kind = Control` with `end.outcome.ended_by_control` false → `BadInput { field: "end_kind" }`; `Completed` with `ended_by_control` true → write `Control`. Transaction: `UPDATE run_records SET state='ended', end_kind=?, result=:outcome.reply, outcome=?, ended_at=now, lease_holder=NULL, lease_expires_at=NULL, session_record=NULL, stretch_summaries = stretch_summaries || :summary WHERE id=? AND lease_holder=? AND state='executing'`; no row → `LeaseLost`. Append event `run_state_changed` with `{"state": <wire state>}` where the wire state is `finished` (Completed), `ended-by-control` (Control), `failed` (Failed) or `ended-at-limit` (RunLimit).
13. `metrics.add("wrightrun_runs_ended_total", [("state", <wire state>)], 1)`; log `run_ended` (INFO; `run_id`, `state`).

**`pause(run_id, instance, pause)`**

14. `pause.outcome.pending` empty → `BadInput { field: "pending" }`; an item of kind `refused-ending` with `approval_id` set → `BadInput { field: "pending.approval_id" }` (SA-CLOSE-01); `session_record` longer than 64 MiB → `SessionRecordTooLarge`. Transaction: `UPDATE … SET state='paused', outcome=?, decision=NULL, session_record=?, lease_holder=NULL, lease_expires_at=NULL, stretch_summaries = … || :summary WHERE id=? AND lease_holder=? AND state='executing'`; no row → `LeaseLost`. Append `run_state_changed` `{"state":"paused"}`.
15. For each pending item with reason `approval-list`: `metrics.add("wrightrun_tool_calls_paused_for_approval_total", [("tool", tool)], 1)`. Log `run_paused` (INFO; `run_id`, `pending` = count). **C-80 (bound assertion):** the stored session record is restored by whichever run instance takes the run. If C-80 shows the record is not portable, the Session Supervisor ends such a run with `end(Failed)` and the configuration's fixed text and never re-runs it (R-87); this crate stores and returns the bytes unchanged either way.

**`resume(run_id, decision)`**

16. Transaction: `UPDATE … SET state='queued', decision=?, queued_at=now WHERE id=? AND state='paused' RETURNING R`; no row → `NotPaused`. Append `run_state_changed` `{"state":"queued"}`. Log `run_resumed` (INFO; `run_id`), never the decision. The stored `outcome.pending` stays until the run pauses again or ends. Of two resumes at one instant one wins and the other gets `NotPaused`.

**`cancel(run_id)`**

17. Transaction: (a) `UPDATE … SET state='cancelled', ended_at=now, session_record=NULL WHERE id=? AND state IN ('queued','paused') RETURNING R` — a row → append `run_state_changed` `{"state":"cancelled"}`, `metrics.add("wrightrun_runs_ended_total", [("state","cancelled")], 1)`, log `run_cancelled` (INFO; `run_id`, `from`), return `Cancelled`. (b) Otherwise `UPDATE … SET cancel_requested_at = coalesce(cancel_requested_at, now) WHERE id=? AND state='executing' RETURNING R` — a row → log `run_cancel_requested` (INFO; `run_id`), return `Requested`. (c) Otherwise (the run has ended, is cancelled or interrupted, or does not exist) → `NotCancellable`. The Caller API calls `get` first to tell an absent run from an ended one.

**`record_cancelled(run_id, instance, outcome)`**

17a. `UPDATE … SET state='cancelled', outcome=?, ended_at=now, lease_holder=NULL, lease_expires_at=NULL, session_record=NULL WHERE id=? AND lease_holder=? AND state='executing' AND cancel_requested_at IS NOT NULL`; no row → `LeaseLost`. Append the event; count and log as step 17(a).

**`mark_interrupted()`**

18. Transaction: `UPDATE run_records SET state = CASE WHEN cancel_requested_at IS NULL THEN 'interrupted' ELSE 'cancelled' END, ended_at=now, lease_expires_at=NULL WHERE state='executing' AND lease_expires_at < now RETURNING id, thread_id, lease_holder, state` (*assumed here*: a run the caller asked to cancel whose instance then vanished is recorded as cancelled, the end the caller asked for). `lease_holder` keeps the instance name. Append one `run_state_changed` event per row. For each `interrupted` row: `metrics.add("wrightrun_runs_interrupted_total", [], 1)` and log `run_interrupted` (WARN; `run_id`, `thread_id`, `instance`). Return the run ids. Nothing is queued again (04 *Run record*: "never started again"). Called every 10 seconds by the server program in every run instance.

**`get(run_id)`** / **`decision(run_id)`**

19. `SELECT R FROM run_records WHERE id = ?` → `RunView` or `None`. `SELECT decision …` → value or `None`.

**`list(states, cursor)`**

20. `Some(&[])` → empty page. With a cursor: `cursor.decode::<i64>()`; an error → `BadCursor`. `SELECT R FROM run_records WHERE (:states IS NULL OR state IN (…)) AND (:before IS NULL OR seq < :before) ORDER BY seq DESC LIMIT 101`; return the first 100 and `next_cursor = Cursor::encode(&seq of the 100th)` when a 101st row came back. The filter on `RunRecordState::Ended` matches every ended row whatever its `end_kind`.

**`in_progress(filter)`**

21. `SELECT count(*) FROM run_records WHERE state IN ('queued','executing','paused') AND (:command IS NULL OR command = :command) AND (:intake IS NULL OR (started_by_kind='intake' AND started_by_id=:intake))`.

**`paused_with_command(commands, except_run_id)`**

21a. `SELECT EXISTS (SELECT 1 FROM run_records WHERE state = 'paused' AND command IN (:commands) AND id <> :except)`; `commands` is bound as one parameter per item (at most 100 items; more → `BadInput { field: "commands" }`).

**`append_progress(run_id, instance, line)`**

22. `line` is trimmed and cut to 160 characters (LW-7); an empty line → `Ok(false)` with no write. Transaction: confirm `state='executing' AND lease_holder=instance` (else `LeaseLost`); `SELECT coalesce(max(seq),0) FROM run_events WHERE run_id=?`; when it is ≥ 10,000 → `Ok(false)` and log `run_events_full` (WARN; `run_id`) once per run (the first refused line); else insert `(run_id, seq+1, 'run_progress', {"line": line}, now)` → `Ok(true)`. The line's text is never logged.

**`events_after(run_id, after_seq, limit)`**

23. `limit` is clamped to 1–500. `SELECT run_id, seq, kind, data, at FROM run_events WHERE run_id = ? AND seq > ? ORDER BY seq LIMIT ?`. An unknown run returns an empty list.

**Retention and erasure deletes** (wrightrun `RunQueue.md` steps 18–20a, with this crate's tables)

24. Each delete is one transaction of the form: select the thread ids `<S>` (ordered by `thread_id`, limited where stated); collect the run ids on those threads; `DELETE FROM run_threads WHERE thread_id IN (<S>)` (runs and their events go through `ON DELETE CASCADE`); return `DeletedRuns { runs, thread_ids, run_ids }`. The `count_…` twin runs the same selection and deletes nothing. `limit` is clamped to 1–500.
   - `<S>` for `…_ended_before(cutoff, limit)`: threads with at least one run whose `ended_at < cutoff` and no run with `ended_at IS NULL OR ended_at >= cutoff`; `ORDER BY thread_id LIMIT :limit`. A thread with a queued, executing or paused run is kept whole.
   - `<S>` for `…_started_by_asker(connection, asker)`: threads whose own starter, or any of whose runs' starter, is (`chat`, connection, asker).
   - `<S>` for `…_in_scope(scope_id)`: threads with any run whose `scope_id` = the given id.
25. Log `runs_deleted` (INFO; `reason` = `retention` | `erase-person` | `erase-scope`, `threads`, `runs`); the asker is never logged.

## Data Model

Migrations registered under module `run_queue` (SA-XC-07: forward-only, numbered). Nothing exists before this migration, so it destroys nothing; rollback before first use is dropping the three tables; after use, restoring the backup the migrator takes (SA-XC-07).

### SQLite (`MigrationStep` 1, `"run queue"`)

```sql
CREATE TABLE run_threads (
    thread_id        TEXT PRIMARY KEY,                                   -- UUIDv7 string
    started_by_kind  TEXT NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id    TEXT NOT NULL CHECK (length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker TEXT,
    created_at       TEXT NOT NULL,                                      -- RFC 3339 ms Z
    CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL))
);
CREATE INDEX run_threads_chat_asker_idx ON run_threads (started_by_id, started_by_asker)
    WHERE started_by_kind = 'chat';

CREATE TABLE run_records (
    id                  TEXT PRIMARY KEY,                                -- = run_id
    seq                 INTEGER NOT NULL UNIQUE,                         -- assigned as max(seq)+1 inside the insert transaction
    thread_id           TEXT NOT NULL REFERENCES run_threads (thread_id) ON DELETE CASCADE,
    state               TEXT NOT NULL CHECK (state IN ('queued','executing','paused','ended','cancelled','interrupted')),
    end_kind            TEXT CHECK (end_kind IN ('completed','control','failed','run_limit')),
    started_by_kind     TEXT NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id       TEXT NOT NULL CHECK (length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker    TEXT,
    scope_id            TEXT NOT NULL,                                   -- ScopeId; no foreign key (scopes belong to Retention and Erasure)
    command             TEXT,
    request             TEXT NOT NULL,                                   -- JSON RunRequest
    gathered_context    TEXT NOT NULL DEFAULT '{}',                      -- JSON
    firing_due          TEXT,                                            -- schedule runs only
    result              TEXT,                                            -- outcome.reply
    outcome             TEXT,                                            -- JSON RunOutcome
    decision            TEXT,                                            -- JSON
    session_record      BLOB,
    stretch_summaries   TEXT NOT NULL DEFAULT '[]',                      -- JSON array of strings
    lease_holder        TEXT,
    lease_expires_at    TEXT,
    cancel_requested_at TEXT,
    queued_at           TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    started_at          TEXT,
    ended_at            TEXT,
    CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL)),
    CHECK ((state = 'executing') = (lease_expires_at IS NOT NULL)),
    CHECK ((state = 'ended') = (end_kind IS NOT NULL)),
    CHECK ((state IN ('ended','cancelled','interrupted')) = (ended_at IS NOT NULL)),
    CHECK ((firing_due IS NULL) OR (started_by_kind = 'schedule'))
);
CREATE INDEX run_records_queued_idx   ON run_records (queued_at, seq) WHERE state = 'queued';
CREATE INDEX run_records_lease_idx    ON run_records (lease_expires_at) WHERE state = 'executing';
CREATE INDEX run_records_open_idx     ON run_records (command) WHERE state IN ('queued','executing','paused');
CREATE UNIQUE INDEX run_records_one_open_per_thread ON run_records (thread_id)
    WHERE state IN ('queued','executing','paused');
CREATE UNIQUE INDEX run_records_one_per_firing ON run_records (started_by_id, firing_due)
    WHERE started_by_kind = 'schedule';
CREATE INDEX run_records_thread_idx   ON run_records (thread_id);
CREATE INDEX run_records_ended_idx    ON run_records (ended_at) WHERE ended_at IS NOT NULL;
CREATE INDEX run_records_scope_idx    ON run_records (scope_id);

CREATE TABLE run_events (
    run_id  TEXT NOT NULL REFERENCES run_records (id) ON DELETE CASCADE,
    seq     INTEGER NOT NULL CHECK (seq >= 1),
    kind    TEXT NOT NULL CHECK (kind IN ('run_state_changed','run_progress')),
    data    TEXT NOT NULL,                                               -- JSON
    at      TEXT NOT NULL,
    PRIMARY KEY (run_id, seq)
);
```

SQLite foreign keys are enforced with `PRAGMA foreign_keys = ON`, which the Journal's pool sets on every connection (requirement on the Journal crate, recorded in findings).

### PostgreSQL (`MigrationStep` 1, `"run queue"`)

```sql
CREATE TABLE run_threads (
    thread_id        uuid        PRIMARY KEY,
    started_by_kind  text        NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id    text        NOT NULL CHECK (char_length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT run_threads_asker_with_chat CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL))
);
CREATE INDEX run_threads_chat_asker_idx ON run_threads (started_by_id, started_by_asker)
    WHERE started_by_kind = 'chat';

CREATE TABLE run_records (
    id                  uuid        PRIMARY KEY,
    seq                 bigint      GENERATED ALWAYS AS IDENTITY UNIQUE,
    thread_id           uuid        NOT NULL REFERENCES run_threads (thread_id) ON DELETE CASCADE,
    state               text        NOT NULL CHECK (state IN ('queued','executing','paused','ended','cancelled','interrupted')),
    end_kind            text        CHECK (end_kind IN ('completed','control','failed','run_limit')),
    started_by_kind     text        NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id       text        NOT NULL CHECK (char_length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker    text,
    scope_id            uuid        NOT NULL,
    command             text,
    request             jsonb       NOT NULL,
    gathered_context    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    firing_due          timestamptz,
    result              text,
    outcome             jsonb,
    decision            jsonb,
    session_record      bytea,
    stretch_summaries   jsonb       NOT NULL DEFAULT '[]'::jsonb,
    lease_holder        text,
    lease_expires_at    timestamptz,
    cancel_requested_at timestamptz,
    queued_at           timestamptz NOT NULL DEFAULT now(),
    created_at          timestamptz NOT NULL DEFAULT now(),
    started_at          timestamptz,
    ended_at            timestamptz,
    CONSTRAINT run_records_asker_with_chat CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL)),
    CONSTRAINT run_records_lease_with_executing CHECK ((state = 'executing') = (lease_expires_at IS NOT NULL)),
    CONSTRAINT run_records_end_kind_with_ended CHECK ((state = 'ended') = (end_kind IS NOT NULL)),
    CONSTRAINT run_records_ended_at CHECK ((state IN ('ended','cancelled','interrupted')) = (ended_at IS NOT NULL)),
    CONSTRAINT run_records_firing_schedule CHECK (firing_due IS NULL OR started_by_kind = 'schedule')
);
CREATE INDEX run_records_queued_idx   ON run_records (queued_at, seq) WHERE state = 'queued';
CREATE INDEX run_records_lease_idx    ON run_records (lease_expires_at) WHERE state = 'executing';
CREATE INDEX run_records_open_idx     ON run_records (command) WHERE state IN ('queued','executing','paused');
CREATE UNIQUE INDEX run_records_one_open_per_thread ON run_records (thread_id)
    WHERE state IN ('queued','executing','paused');
CREATE UNIQUE INDEX run_records_one_per_firing ON run_records (started_by_id, firing_due)
    WHERE started_by_kind = 'schedule';
CREATE INDEX run_records_thread_idx   ON run_records (thread_id);
CREATE INDEX run_records_ended_idx    ON run_records (ended_at) WHERE ended_at IS NOT NULL;
CREATE INDEX run_records_scope_idx    ON run_records (scope_id);

CREATE TABLE run_events (
    run_id  uuid        NOT NULL REFERENCES run_records (id) ON DELETE CASCADE,
    seq     bigint      NOT NULL CHECK (seq >= 1),
    kind    text        NOT NULL CHECK (kind IN ('run_state_changed','run_progress')),
    data    jsonb       NOT NULL,
    at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (run_id, seq)
);
```

A later state value is added by replacing a `CHECK` with a superset, an additive change the previous image still reads (ADR-098 item 5). Kept for: until the retention job deletes a thread whose runs all ended before the configured `runs-days`, or an operator erases a person or a scope (04 Data Lifecycle, *A hosted agent's runs…*).

## Error Table

In-process API; the Caller API maps codes to HTTP statuses (its own Error Table).

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `enqueue` on a `thread_id` no thread has | `Err(permanent)` | `RUN_THREAD_NOT_FOUND` | `RunQueueError::ThreadNotFound` — "No thread has this identifier." |
| `enqueue` on a thread with a queued, executing or paused run | `Err(permanent)` | `RUN_THREAD_BUSY` | `RunQueueError::ThreadBusy` — "The thread has a run that has not ended." |
| `enqueue` with a `run_id` a run already has | `Err(permanent)` | `RUN_DUPLICATE_ID` | `RunQueueError::DuplicateId` — "A run has this identifier already." |
| Input breaks a rule of step 1 or step 6, `pause` with no pending item, or `end` with `Control` and `ended_by_control` false (step 12) | `Err(permanent)` | `RUN_BAD_INPUT` | `RunQueueError::BadInput { field }` — "A run starts with a command or with a message, not both." (`field` names the part) |
| `resume` of a run that is not paused or does not exist | `Err(permanent)` | `RUN_NOT_PAUSED` | `RunQueueError::NotPaused` — "The run is not paused." |
| `cancel` of a run that has ended or does not exist | `Err(permanent)` | `RUN_NOT_CANCELLABLE` (*chosen here*) | `RunQueueError::NotCancellable` — "The run has ended and cannot be cancelled." |
| `end`, `pause`, `record_cancelled`, `append_progress` by an instance that no longer holds the lease | `Err(permanent)` | `RUN_LEASE_LOST` | `RunQueueError::LeaseLost` |
| `pause` with a session record over 64 MiB | `Err(permanent)` | `RUN_SESSION_RECORD_TOO_LARGE` (*chosen here*) | `RunQueueError::SessionRecordTooLarge` |
| `list` with a cursor that cannot be decoded or is not one integer | `Err(permanent)` | `SRV_BAD_CURSOR` | `RunQueueError::BadCursor` — "the cursor is not one this listing returned" |
| The datastore does not answer | `Err(retryable)` | `DB_UNAVAILABLE` (wrightrun carried, EH-D12) | `RunQueueError::Unavailable` — "The runtime database did not answer." |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Run Queue

  Background:
    Given the run_queue migrations applied to a SQLite datastore and, in scenarios marked [pg], to a PostgreSQL 18 container
    And a recording MetricsSink

  Scenario: Happy path — a run is recorded, taken, ended and read back
    When enqueue is called with Caller "svc-release-desk", the command "run-go-no-go" and the argument "release 4.12"
    And take is called with the instance "runs-a"
    And end is called by "runs-a" with Completed and an outcome whose reply is "Go."
    Then get returns the state Ended, end_kind Completed, result "Go." and started_at and ended_at set
    And events_after(run_id, 0, 10) returns run_state_changed events with states queued, executing, finished in that order
    And wrightrun_runs_queued_total{started_by="caller"}, wrightrun_runs_started_total{started_by="caller"} and wrightrun_runs_ended_total{state="finished"} each rose by 1

  Scenario: A run a control ended is recorded with the end kind Control
    Given a run taken by "runs-a"
    When end is called by "runs-a" with Completed and an outcome whose ended_by_control is true
    Then get returns the state Ended and end_kind Control
    And the last run_state_changed event has the state "ended-by-control"

  Scenario: Error path — the end kind Control without a control ending
    Given a run taken by "runs-a"
    When end is called by "runs-a" with Control and an outcome whose ended_by_control is false
    Then it fails with RUN_BAD_INPUT naming the field "end_kind" and the run stays executing

  Scenario: [pg] Fifty instances take one queued run and exactly one receives it
    Given one queued run
    When 50 connections call take at the same instant, in each of 100 rounds with a new run
    Then in every round exactly 1 call returns the run and 49 return None

  Scenario: Runs are taken oldest first and a resumed run joins the end of the line
    Given queued runs A, B and C recorded in that order
    And A was taken, paused and resumed after C was recorded
    When take is called three times
    Then the runs returned are B, C and A in that order

  Scenario: A paused run carries its session record to the next instance
    Given a run taken by "runs-a" and paused with one pending tool-call item and the session record bytes "S1"
    When resume is called with {"decisions":[{"type":"approve"}],"set_aside":[]}
    And take is called with the instance "runs-b"
    Then the TakenRun has the same run_id and started_at, decision equal to the resume value and session_record "S1"

  Scenario: A pending item keeps its approval id across pause and get
    Given a run taken by "runs-a"
    When it is paused with one pending tool-call item whose approval_id is "0192a1c3-4455-7667-8889-aabbccddeeff"
    Then get returns outcome.pending[0].approval_id equal to that id

  Scenario: A refused ending cannot carry an approval id
    When pause is called with a refused-ending item whose approval_id is set
    Then it fails with RUN_BAD_INPUT naming the field "pending.approval_id"

  Scenario: A run whose lease lapses is recorded as interrupted and never taken again
    Given a run taken by "runs-a" whose lease_expires_at is 1 second in the past
    When mark_interrupted is called
    Then it returns that run_id and get shows the state Interrupted with ended_at set
    And take returns None
    And wrightrun_runs_interrupted_total rose by 1

  Scenario: [pg] A renewal and the interruption of the same run have one outcome
    Given a run taken by "runs-a" whose lease_expires_at is 1 second in the past
    When renew by "runs-a" and mark_interrupted run on two connections at the same instant, in 100 rounds
    Then in every round either renew returns Kept and the state stays Executing, or renew returns Lost and the state is Interrupted

  Scenario: Error path — an instance that lost its lease cannot end the run
    Given a run taken by "runs-a" and then recorded as interrupted
    When "runs-a" calls end with Completed
    Then it fails with code RUN_LEASE_LOST and the state stays Interrupted

  Scenario: Error path — a second run on a thread whose run has not ended
    Given a thread with one paused run
    When enqueue is called on that thread
    Then it fails with code RUN_THREAD_BUSY
    And after the paused run is cancelled, enqueue on that thread succeeds

  Scenario: Error path — a run on a thread that does not exist
    When enqueue is called with thread_id "0192a000-0000-7000-8000-000000000000"
    Then it fails with code RUN_THREAD_NOT_FOUND and run_records holds no new row

  Scenario: Error path — two resumes at the same instant
    Given a paused run
    When resume is called on 10 tasks at the same instant
    Then exactly one call returns the record and nine fail with code RUN_NOT_PAUSED

  Scenario: Cancel a queued run, and request the cancel of an executing run
    Given a queued run Q and a run E taken by "runs-a"
    When cancel(Q) is called
    Then it returns Cancelled and get(Q) shows the state Cancelled
    When cancel(E) is called
    Then it returns Requested and renew(E, "runs-a") returns CancelRequested
    When record_cancelled(E, "runs-a", outcome) is called
    Then get(E) shows the state Cancelled with ended_at set

  Scenario: Error path — cancel of an ended run
    Given a run that ended Completed
    When cancel is called
    Then it fails with code RUN_NOT_CANCELLABLE

  Scenario: Edge case — an executing run asked to cancel whose instance vanished is recorded as cancelled
    Given a run E taken by "runs-a", cancel(E) called, and its lease 1 second in the past
    When mark_interrupted is called
    Then get(E) shows the state Cancelled and wrightrun_runs_interrupted_total did not rise

  Scenario: [pg] A firing and its run are recorded once across fifty instances
    When enqueue_firing("triage/run-triage/0 9 * * 1-5", 2026-01-10T09:00:00.000Z, agent_wide, "run-triage", "queue=support") runs on 50 connections at once, in 60 rounds with a new due instant
    Then in every round exactly 1 call returns a run and 49 return None
    And run_records holds 60 runs on 60 threads, each started_by schedule with that entry

  Scenario: Error path — a firing with an empty entry key writes nothing
    When enqueue_firing is called with the entry key ""
    Then it fails with code RUN_BAD_INPUT and no row is written

  Scenario: A paused run with a duty's command is found, the calling run excluded
    Given run P paused with command "watch-readiness" and run Q executing with command "watch-readiness"
    When paused_with_command(["watch-readiness"], P) is called
    Then it returns false
    And paused_with_command(["watch-readiness", "triage"], Q) returns true
    And paused_with_command([], Q) returns false without a statement

  Scenario: Progress lines are kept in order and bounded
    Given a run taken by "runs-a"
    When append_progress is called with "Reading 3 tickets" and then with a line of 400 characters
    Then events_after returns the two lines in order, the second cut to 160 characters
    And append_progress by "runs-b" fails with code RUN_LEASE_LOST

  Scenario: Retention deletes a thread only when all its runs ended before the cutoff
    Given thread T1 whose two runs ended 100 days ago, thread T2 whose run ended 100 days ago and whose second run is paused, and thread T3 whose run ended yesterday
    When delete_ended_before is called with a cutoff 90 days ago and limit 500
    Then it returns runs 2, thread_ids [T1] and the two run_ids of T1
    And every run of T2 and T3 is still read by get

  Scenario: Erasure by person deletes what a chat user started and nothing a caller started
    Given runs started by ChatConnection("team-slack","U024BE7LH"), ChatConnection("team-slack","U999") and Caller("U024BE7LH"), each on its own thread
    When count_started_by_asker("team-slack","U024BE7LH") is called
    Then it returns runs 1 and all three runs are still read by get
    When delete_started_by_asker("team-slack","U024BE7LH") is called
    Then it returns the same counts, the other two runs remain, and no log line holds "U024BE7LH"

  Scenario: Error path — a cursor of another listing
    When list is called with a cursor that encodes ["abc"]
    Then it fails with code SRV_BAD_CURSOR

  Scenario: Error path — the datastore does not answer
    Given the PostgreSQL container is paused [pg]
    When enqueue is called
    Then it fails with code DB_UNAVAILABLE and retryable() is true
    And no log line holds the connection string

  Scenario: Parity — SQLite and PostgreSQL return the same values
    Given the parity body that enqueues, takes, renews, pauses, resumes, cancels, ends, lists, counts and deletes
    When it runs against SqliteRunStore and PgRunStore
    Then every returned value and every error code is the same for both
```

## Performance / Security / Observability

- **Performance targets:** at A-48's scale (100 runs at once, 20,000 runs a day): p95 below 20 ms for `take`, `renew`, `end`, `pause`, `resume`, `cancel`, `get`, `in_progress`, `append_progress`, `events_after` on PostgreSQL, and below 200 ms for one page of `list` (wrightrun `RunQueue.md` targets; E-61 measured 1.3 ms take, 2.2 ms renewal). Single shape on SQLite: p95 below 10 ms for the same calls at 8 runs at once. Load: one `take` a second per free place, one `renew` every 10 s per executing run, one `mark_interrupted` every 10 s per run instance. At 90 days' retention the table holds about 1.8 million rows; the take and lease statements read partial indexes sized by open runs. Memory: no state held between calls; a session record is held only while it is written or returned.
- **Security:** the queue checks no identity; the Caller API checks ownership with `RunRecord.started_by` and `thread_owner` before calling `enqueue` on a thread, `get`, `resume`, `cancel` or `events_after`. Every statement is fixed text with bound parameters. `request`, `gathered_context`, `outcome`, `decision`, `session_record` and progress lines can hold what people wrote; no method logs any of them. `started_by_asker` is a person's chat identifier: returned only in records, read through the operator role only, and never logged. No agent tool reaches this crate.
- **Observability:** agent profile only (SA-XC-04). Counters through `MetricsSink`, names unchanged from wrightrun (SA-OBS-02): `wrightrun_runs_queued_total{started_by}`, `wrightrun_runs_started_total{started_by}`, `wrightrun_runs_ended_total{state}` (`state` ∈ `finished`, `ended-by-control`, `ended-at-limit`, `failed`, `cancelled`), `wrightrun_runs_interrupted_total`, `wrightrun_tool_calls_paused_for_approval_total{tool}`. `started_by` ∈ `caller`, `schedule`, `intake`, `chat`. Log events (`tracing`, target `ordin_run_queue`): `run_queued`, `run_taken`, `run_paused`, `run_resumed`, `run_ended`, `run_cancelled`, `run_cancel_requested`, `run_interrupted` (WARN), `run_events_full` (WARN), `runs_deleted`, `run_queue_unavailable` (WARN), with the fields of *Internal Logic*. No trace span (wrightrun ADR-013).

## Gaps

None.
