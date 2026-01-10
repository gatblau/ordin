# SPEC: Job Runner

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-job-runner` · **Build phase:** 9 · **Programs:** A
> **Dependencies:** Knowledge Service, Retention and Erasure, Journal, Telemetry

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

The Job Runner executes a hosted agent's work that makes no model call and is not a run: knowledge ingest and the retention job (02:821–823, ADR-098 item 6). A job is requested by writing its record; one instance takes it with a lease and resumes from its checkpoint after a lapse; a second job for the same key is refused while one is requested or running; progress is readable by operators and chat users. It owns the job records and the configuration's job timers (`knowledge[].ingest-cron`, `retention.cron`), each fired once across instances. It runs in instances started in the `jobs` or `chat-jobs` role and in the single shape, and in the `ordin-server agent job run-one` command (08 *ordin server command, agent profile*).

## Approach

The job records are one table; every guarantee is one statement: "one requested or running job per key" is a partial unique index with `INSERT … ON CONFLICT DO NOTHING`; the take is `UPDATE … WHERE id = (SELECT … FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING` on PostgreSQL and the same statement under `BEGIN IMMEDIATE` on SQLite (wrightrun `JobRunner.md`, E-61). One instance executes one job at a time, with one task renewing the lease and watching for a stall beside the job's batches. Unlike wrightrun, where ingest and retention register job functions with the runner, here the runner calls the Knowledge Service's ingest step and the Retention step directly, because it depends on them (2B-3); job timer firings are recorded in this crate's own `job_timer_firings` table. Error prefix: `JOB_` (wrightrun's). Rejected: database advisory locks for the lease — a pool returns connections between statements, which ends a session lock. Rejected: a second table for checkpoints — a job has one current checkpoint, and saving it with the progress in the job's own row keeps the save one statement.

### Reconciliation assumptions

- **SA-RECON-01** Knowledge ingest of chat history (ruling R14): the Knowledge Service owns the ingest logic and the index; the Job Runner owns the job record, the timers, the lease and the checkpoint, and calls the Knowledge Service for each step.
- **SA-RECON-02** The Job Runner owns the outcome wording (`outcome_text`, wrightrun's) because the Channel Gateway's `notify_job_ended` sends the text it is given and adds nothing (ChannelGateway step 13).
- **SA-RECON-03** Counters go through `Telemetry::record(TelemetryEvent::Job { … })` (ruling R4); migrations are `MigrationStep` tuples (R3); paging is 2C `Page<T>`/`Cursor`. Telemetry names the Knowledge Service's ingest step as the source of families 36–37 (Telemetry.md SA-CLOSE-01, K17); this crate emits neither.

## Shared Context

### Shared types (Phase 2C, `ordin-types`)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);   // RFC 3339 ms Z
#[serde(transparent)] pub struct ScopeId(pub uuid::Uuid);  // UUIDv7

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunStarter {
    Caller { caller_id: String },
    ScheduleEntry { entry: String },
    Intake { intake: String },
    ChatConnection { connection: String, asker: String },
}
```

`RunStarter` names who asked for a job: an operator through the operator routes is `Caller { caller_id: <token subject> }`; a chat user is `ChatConnection`; a timer is no one (`None`).

Paging (2C `ordin_types::paging`, decision 80: C-15): `Page<T> { items: Vec<T>, next_cursor: Option<Cursor> }`, `PAGE_SIZE = 100`; `Cursor::encode(&key)`, `cursor.decode::<K>()` (a refusal → `SRV_BAD_CURSOR`).

### From Knowledge Service (`ordin-knowledge`, copied from KnowledgeService.md; ruling R14)

```rust
/// One page of an ingest of one chat-history source in one scope (wrightrun KnowledgeIngest).
pub struct IngestStep {
    pub checkpoint: serde_json::Value,    // differs from every earlier checkpoint of this job
    pub items_read: u64,                  // cumulative
    pub total: Option<u64>,
    pub done: bool,
}
#[async_trait]
pub trait KnowledgeIngest: Send + Sync {
    /// Reads one page of the scope's chat history and indexes it; refuses with KNOW_SCOPE_NOT_ENABLED
    /// when the scope has no enabled chat of the source's connection.
    async fn ingest_step(&self, source: &str, scope: ScopeId, checkpoint: Option<&serde_json::Value>)
        -> Result<IngestStep, KnowledgeError>;
}
// KnowledgeError::code(), retryable(). The program passes `KnowledgeService::ingest()` (Some in the agent profile).
```

### From Retention and Erasure (`ordin-retention`)

```rust
pub struct RetentionStep { pub checkpoint: serde_json::Value, pub items_deleted: u64, pub done: bool }
impl Retention {
    pub async fn retention_step(&self, checkpoint: Option<&serde_json::Value>) -> Result<RetentionStep, RetentionError>;
    pub fn scopes(&self) -> Arc<dyn ScopeRegistry>;    // find(connection, chat_identifier), enabled(connection), is_enabled(id)
    pub fn register(&self, provider: Arc<dyn RetainedContent>) -> Result<(), RetentionError>;
}
// RetainedContent (hub trait owned by Retention): content() -> ProvidedContent::Jobs;
// delete_older_than, count_for_asker, delete_for_asker, count_scope, delete_scope
```

### From Telemetry (`ordin-telemetry`, copied from Telemetry.md; ruling R4)

`Telemetry::record(&self, event: TelemetryEvent)`; the variant used here is `TelemetryEvent::Job { what: JobCounter, kind: ordin_telemetry::JobKind }` with `JobCounter::{Started, Finished, Failed}` (families 21–23) and `ordin_telemetry::JobKind::{Ingest, Retention}`.

### From Channel Gateway, through the program

`ChannelGateway::notify_job_ended(&self, connection: &str, user: &str, text: &str) -> Result<(), GatewayError>` (codes `CHAT_LINK_UNKNOWN`, `CHAT_POST_FAILED`). The program implements `JobOutcomeNotifier::job_ended(job)` as: `requested_by = ChatConnection { connection, asker }` → `notify_job_ended(connection, asker, &outcome_text(job)?)`; a gateway error is returned to the notifier and logged there (step 19).

### From Agent Configuration, passed by the program (this crate does not depend on it)

The timers are built by the program from `knowledge[]` entries of kind `chat-history` with `ingest-cron` and from `retention.cron` (wrightrun configuration keys), and passed as `JobTimer` values (below).

### From the Journal (`ordin-journal`, copied from JournalAndArtefactStore.md; ruling R3)

`#[derive(Clone)] pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }`; `pub type MigrationStep = (i64, &'static str, &'static str)`; `pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }`. The program passes this crate's `migrations()` to `MigrationRegistry::register`.

### Constants

| Constant | Value | Source |
|---|---|---|
| `LEASE_SECONDS` | 30 | wrightrun |
| `LEASE_RENEW_SECONDS` | 10 | wrightrun |
| `JOB_POLL_SECONDS` | 15 | wrightrun (A-21) |
| `JOB_STALL_SECONDS` | 600 | wrightrun |
| `JOB_STOP_GRACE_SECONDS` | 20 | wrightrun |
| `MAX_CHECKPOINT_BYTES` | 1,048,576 | wrightrun |
| `MAX_KEY_CHARS` | 400 | wrightrun |
| `RETENTION_KEY` | `retention` | wrightrun |

## Public Interface

```rust
// crate ordin-job-runner
use ordin_types::{RunStarter, ScopeId, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind { Ingest, Retention }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState { Requested, Running, Finished, Failed }

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobProgress { pub items_read: u64, pub total: Option<u64>, pub started_at: Option<Timestamp> }

/// The Job record entity (wrightrun's fields; owned here, 02:822 amendment).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobRecord {
    pub job_id: String,                  // UUIDv7
    pub kind: JobKind,
    pub key: String,                     // ingest "<source>/<connection>/<chat identifier>"; retention "retention"
    pub state: JobState,
    pub requested_by: Option<RunStarter>,
    pub progress: JobProgress,
    pub error_code: Option<String>,
    pub created_at: Timestamp,
    pub ended_at: Option<Timestamp>,
}

/// Share done and time left (wrightrun's one formula, A-37).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProgressView { pub items_read: u64, pub total: Option<u64>, pub share_done: Option<f64>, pub seconds_left: Option<u64> }
pub fn progress_view(job: &JobRecord, now: chrono::DateTime<chrono::Utc>) -> ProgressView;

/// A configured timer that asks for jobs when its cron is due, once across instances.
#[derive(Debug, Clone)]
pub enum JobTimer {
    Retention { cron: String },                              // name "retention"
    Ingest { source: String, connection: String, cron: String },   // name "ingest-<source>"
}

/// Tells the requester how a job ended (implemented by the program from the Channel Gateway;
/// a job whose requester is not a chat user, or whose asker was erased, is not told).
#[async_trait::async_trait]
pub trait JobOutcomeNotifier: Send + Sync {
    async fn job_ended(&self, job: &JobRecord);
}

#[async_trait::async_trait]
pub trait JobStore: Send + Sync {
    async fn request(&self, kind: JobKind, key: &str, requested_by: Option<RunStarter>) -> Result<(JobRecord, bool), JobError>;
    async fn get(&self, job_id: &str) -> Result<Option<JobRecord>, JobError>;
    async fn latest(&self, key: &str) -> Result<Option<JobRecord>, JobError>;
    async fn list(&self, cursor: Option<&str>) -> Result<ordin_types::paging::Page<JobRecord>, JobError>;
}

pub struct JobRunner { /* store backend, knowledge, retention, telemetry, notifier, timers, instance */ }
impl JobRunner {
    /// Fails with JOB_BAD_CRON for a timer cron that is not five valid fields, JOB_DUPLICATE for two
    /// timers of one name.
    pub fn new(datastore: &ordin_journal::Datastore,
               knowledge: std::sync::Arc<dyn ordin_knowledge::KnowledgeIngest>,
               retention: std::sync::Arc<ordin_retention::Retention>,
               telemetry: std::sync::Arc<ordin_telemetry::Telemetry>,
               notifier: Option<std::sync::Arc<dyn JobOutcomeNotifier>>,
               timers: Vec<JobTimer>,
               instance: String) -> Result<Self, JobError>;
    pub fn store(&self) -> std::sync::Arc<dyn JobStore>;
    /// The Retention provider for job records (registered by the program with Retention::register).
    pub fn retained_content(&self) -> std::sync::Arc<dyn ordin_retention::RetainedContent>;
    pub async fn run_forever(&self, stop: tokio_util::sync::CancellationToken);
    /// `ordin-server agent job run-one`: 0 finished, 1 any other outcome.
    pub async fn run_one(&self, kind: JobKind, key: &str, stop: Option<tokio_util::sync::CancellationToken>) -> i32;
}
pub fn check_key(kind: JobKind, key: &str) -> Result<(), JobError>;
pub fn migrations() -> ordin_journal::ModuleMigrations;   // module "job_runner"; version 1 is the Data Model
/// The one wording of a job's outcome sent to the chat user who asked (wrightrun ChatConnector `outcome_text`);
/// None for a job that is neither finished nor failed.
pub fn outcome_text(job: &JobRecord) -> Option<String>;

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("the key of an ingest job is \"<source>/<connection>/<channel>\"")]   BadKey,
    #[error("\"{0}\" is not a five-field cron expression")]                       BadCron(String),
    #[error("the timer {0} is configured twice")]                                 Duplicate(String),
    #[error("the job changed state three times while it was asked for; ask again")] RequestConflict,
    #[error("the job is running in another instance")]                            Busy,
    #[error("the job is no longer running under this instance's lease")]          LeaseLost,
    #[error("a checkpoint holds at most 1048576 bytes")]                          CheckpointTooLarge,
    #[error("the job function saved a checkpoint it had saved before")]           Revisited,
    #[error("the job saved no checkpoint for 600 seconds")]                       Stalled,
    #[error("the job failed")]                                                    Failed,
    #[error("the cursor is not one this listing returned")]                       BadCursor,
    #[error("the job records could not be read or written")]                      DbUnavailable,
}
impl JobError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

**Called by.**

| Public item | Called by |
|---|---|
| `JobRunner::new`, `run_forever`, `migrations` | the server program (OrdinServer, agent profile, `jobs` and `chat-jobs` roles and the single shape) |
| `run_one` | the server program's `ordin-server agent job run-one --kind KIND --key KEY` |
| `store()` → `JobStore::{request, list}`, `progress_view` | Caller API routes 16–17, through the `JobDesk` the program implements over `JobStore` (`JobKindText` → `JobKind`; `requested_by_subject` → `RunStarter::Caller { caller_id }`; `JobRecord` and `progress_view` → `JobRow`) |
| `store()` → `JobStore::{request, latest, get}`, `check_key` | Channel Gateway, through the program's `HostedAgentCore::job_command` (a chat user asks for an ingest) |
| `retained_content()` | the server program, which passes it to `Retention::register` (`ProvidedContent::Jobs`) |
| `JobOutcomeNotifier`, `outcome_text` | implemented by the program over `ChannelGateway::notify_job_ended(connection, asker, outcome_text(job))`; called by this crate at a job's end |

The ingest job's step is the Knowledge Service's: this crate schedules and leases the job and calls the Knowledge Service once per step; reading the chat history and writing the index are the Knowledge Service's (ruling R14, SA-RECON-01).

`check_key`: for `Ingest` the key has three non-empty parts split at the first two `/`; the first two match `^[a-z0-9]+(-[a-z0-9]+)*$`; the third is 1–64 characters without white space or control characters (2C `Scope.chat_identifier`); for `Retention` the key is exactly `retention`; at most 400 characters.

`progress_view`: `share_done = min(1.0, items_read / total)` when `total > 0`, else `None`; `seconds_left = 0` when `total` is known and `items_read ≥ total`; `ceil((total − items_read) × elapsed / items_read)` when `total` is known, `started_at` is set, `items_read > 0` and `elapsed > 0`; else `None`.

### Example

```rust
let store = runner.store();
let asker = RunStarter::ChatConnection { connection: "team-slack".into(), asker: "U02ABCDEF".into() };
let (rec, created) = store.request(JobKind::Ingest, "channel-history/team-slack/C0456XYZ", Some(asker)).await?;
// created == true; rec.state == Requested
let (again, created) = store.request(JobKind::Ingest, "channel-history/team-slack/C0456XYZ", None).await?;
// created == false; again.job_id == rec.job_id
```

The log line of a finished job (JSON lines, SA-OBS-01):

```json
{"event":"job_finished","job_id":"01929b10-3f6c-7a52-9c1d-2f0e8d1b7a10","kind":"ingest","key":"channel-history/team-slack/C0456XYZ","instance":"jobs-7c9d","items_read":1000,"seconds":187}
```

## Internal Logic

**Store statements** (one each; `R` = `id, kind, key, state, requested_by, items_read, total, started_at, error_code, created_at, ended_at`). A sqlx connection error → `DbUnavailable`.

1. `request(kind, key, requested_by)`: `check_key`. For ingest, `scope_id` = `retention.scopes().find(connection, chat_identifier)` (absent → the job is still recorded with `scope_id` NULL; the ingest step then refuses it). J1: `INSERT INTO jobs (id, kind, key, state, requested_by, scope_id, created_at) VALUES (…, 'requested', …) ON CONFLICT (key) WHERE state IN ('requested','running') DO NOTHING RETURNING R`. A row → `(rec, true)`. None → J2: `SELECT R FROM jobs WHERE key = ? AND state IN ('requested','running')` → `(rec, false)`. None again (the open job ended between) → repeat J1, at most 3 attempts in all, then `RequestConflict`.
2. `fail_stalled(kinds)`: `UPDATE jobs SET state='failed', error_code='JOB_STALLED', ended_at=now, lease_holder=NULL, lease_expires_at=NULL WHERE state='running' AND kind IN (…) AND lease_expires_at < now AND advanced_at < now − 600 s RETURNING R`.
3. `take(kinds)`: PostgreSQL `UPDATE jobs SET state='running', lease_holder=:instance, lease_expires_at=now()+30s, started_at=coalesce(started_at, now()), advanced_at=coalesce(advanced_at, now()), attempts=attempts+1 WHERE id = (SELECT id FROM jobs WHERE kind = ANY(:kinds) AND (state='requested' OR (state='running' AND lease_expires_at < now() AND advanced_at >= now() − 600s)) ORDER BY created_at, id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING R`; SQLite the same without the lock clause under `BEGIN IMMEDIATE`. `take_job(job_id)` narrows the inner select to `id = :job_id`.
4. `renew`: `UPDATE jobs SET lease_expires_at = now+30s WHERE id=? AND state='running' AND lease_holder=?` → rows affected == 1.
5. `save(job_id, checkpoint, progress)`: serialise the checkpoint canonically (`serde_json` with sorted keys); over 1,048,576 bytes → `CheckpointTooLarge`. `UPDATE jobs SET checkpoint=?, items_read=?, total=?, advanced_at=now, lease_expires_at=now+30s WHERE id=? AND state='running' AND lease_holder=?`; 0 rows → `LeaseLost`.
6. `checkpoint(job_id)`: `SELECT checkpoint FROM jobs WHERE id = ?`.
7. `finish` / `fail(code)`: `UPDATE jobs SET state='finished'|'failed', [error_code=?,] ended_at=now, lease_holder=NULL, lease_expires_at=NULL WHERE id=? AND state='running' AND lease_holder=?`; 0 rows → `LeaseLost`.
8. `get`, `latest(key)` (`ORDER BY created_at DESC, id DESC LIMIT 1`), `list(cursor)` (newest first, `(created_at, id) < (:c, :i)`, 101 rows; cursor = `Cursor::encode(&(created_at, id))`; `cursor.decode::<(Timestamp, String)>()` failing → `BadCursor`).
9. Retention provider (`retained_content()`, content `Jobs`): `delete_older_than(cutoff, limit)` deletes at most `limit` jobs in `finished`/`failed` with `ended_at < cutoff` (never an open job); `count_scope` / `delete_scope(scope_id)` count or delete every ingest job of that scope in any state; `count_for_asker` / `delete_for_asker(connection, asker)` count, or rewrite, the records whose requester is `ChatConnection { connection, asker }`: the asker is replaced by the fixed text `"erased"` in `requested_by` and in `requested_by_asker` *(chosen here: 2C's `RunStarter::ChatConnection.asker` is a required string, so it cannot become null; the notifier sends nothing for `"erased"`)*; the records stay (wrightrun ADR-069). Log `job_records_changed` (INFO; `reason`, `count`), never the asker.

**`run_forever(stop)`**

10. `started = now`. Log `job_runner_started` (INFO; `instance`, `timers` = names).
11. Loop until `stop`; each turn waits for `stop` at most 15 s, then steps 12–14. A `DbUnavailable` in a turn is logged `job_db_unavailable` (WARN; `code="JOB_DB_UNAVAILABLE"`) and ends the turn; the runner does not end while the datastore is away.
12. **Timers.** For each timer, `due` = the latest instant of its cron at or before now (`croner`, UTC; *see Duty Timer spec for the library pick*). When `due > started` and differs from the last `due` this runner fired for the timer: `INSERT INTO job_timer_firings (timer, due, recorded_at) VALUES (…) ON CONFLICT DO NOTHING RETURNING timer`. A row → ask for the jobs: `Retention` → `request(Retention, "retention", None)`; `Ingest { source, connection }` → for each `retention.scopes().enabled(connection)` scope, `request(Ingest, "<source>/<connection>/<chat identifier>", None)`. Log `job_timer_fired` (INFO; `timer`, `due`, `created`, `already_open`). No row → another instance fired it. Remember `due` either way. A firing due before `started` is not made up.
13. **Take.** `fail_stalled(all kinds)`; for each record: `telemetry.record(TelemetryEvent::Job { what: JobCounter::Failed, kind: k })`, log `job_failed` (`code="JOB_STALLED"`), notify (step 19). Then `take(all kinds)`; `None` ends the turn.
14. **Execute** (steps 15–19), then start the next turn at once, without the wait.

**Executing one job**

15. `saved = checkpoint(job_id)`. `telemetry.record(TelemetryEvent::Job { what: JobCounter::Started, kind: k })`. Log `job_taken` (INFO; `job_id`, `kind`, `key`, `instance`, `resumed` = `saved.is_some()`).
16. Spawn the lease task: every 10 s `renew`; `false` → cancel the job and mark `lease-lost`. When `now − last_advance > 600 s` (`last_advance` starts at the take) → cancel and mark `stalled`.
17. The job loop, until done, cancelled or `stop`:
    - `Ingest`: parse the key; `scope = scopes.find(connection, chat_identifier)`; none or not enabled → fail with `KNOW_SCOPE_NOT_ENABLED` (*the Knowledge Service's code*). Else `step = knowledge.ingest_step(source, scope.id, checkpoint)`.
    - `Retention`: `step = retention.retention_step(checkpoint)`.
    - `step.done` → leave the loop with outcome `finished`. Else compute the SHA-256 of the canonical checkpoint; equal to a digest saved in this execution or to `saved`'s digest → outcome `failed` with `JOB_REVISITED`. Else `save(job_id, step.checkpoint, progress)` (items: ingest `items_read`/`total`; retention `items_deleted`/`None`), remember the digest, `last_advance = now`. After each save: `stop` cancelled → outcome `interrupted`.
    - An error from a step: outcome `failed` with the error's `code()` (`KNOW_…`, `ERASE_…`, `DB_UNAVAILABLE`, …).
    - `stop` cancelled while a step runs: wait for it at most 20 s, then cancel it; outcome `interrupted`.
18. Outcome: `lease-lost` → log `job_lease_lost`; write nothing (another instance has the job). `interrupted` → log `job_interrupted`; write nothing (the lease ends within 30 s and another instance resumes from the checkpoint). `stalled` → `fail("JOB_STALLED")`. `failed` → `fail(code)`; a panic of a step is caught by the spawned task and recorded as `JOB_FAILED` with the panic's type only. `finished` → `finish`. After `fail`: `telemetry.record(TelemetryEvent::Job { what: JobCounter::Failed, kind: k })`, log `job_failed` (WARN; `job_id`, `kind`, `key`, `instance`, `code`, `items_read`). After `finish`: `telemetry.record(TelemetryEvent::Job { what: JobCounter::Finished, kind: k })`, log `job_finished` (INFO; `items_read`, `seconds`). A `LeaseLost` from `fail`/`finish` → log `job_lease_lost`, nothing changes. Stop the lease task.
19. **Notify.** After a successful `finish` or `fail`, and for each stalled record of step 13: when a notifier is given and `requested_by` is `ChatConnection` with an asker other than `"erased"`, `notifier.job_ended(&get(job_id))`; an error or panic in it is logged `job_notify_failed` and changes nothing.

**`outcome_text(job)`** (wrightrun ChatConnector `outcome_text`, kept): `Finished` → `"The ingest you asked for finished: <items_read> items read."`; `Failed` → `"The ingest you asked for failed (<error_code>). An operator can read the cause in the log."`; for kind `Retention` the text begins `"The retention job"` in place of `"The ingest you asked for"`; any other state → `None`. The text names no channel and no content.

**`run_one(kind, key, stop)`**

20. `fail_stalled([kind])` (as step 13) so a job no instance runs is not answered `JOB_BUSY`. `request(kind, key, None)`; an error → log, return 1. `take_job(job_id)`; `None` → log `JOB_BUSY`, return 1. Execute as steps 15–19. Return 0 for `finished`, 1 otherwise. A `DbUnavailable` → log, return 1.

No step calls a model, the chat system or any address outside the datastore; ingest's reads of Slack happen inside the Knowledge Service's step.

## Data Model

Module `job_runner`. New tables; nothing to roll back before use; after use, restore the migrator's backup (SA-XC-07).

### SQLite (`MigrationStep` 1, `"job runner"`)

```sql
CREATE TABLE jobs (
    id               TEXT PRIMARY KEY,                                      -- UUIDv7
    kind             TEXT NOT NULL CHECK (kind IN ('ingest','retention')),
    key              TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 400),
    state            TEXT NOT NULL CHECK (state IN ('requested','running','finished','failed')),
    requested_by     TEXT,                                                  -- JSON RunStarter; NULL for a timer
    requested_by_kind TEXT,                                                 -- RunStarter kind ('caller', 'schedule_entry', 'intake', 'chat_connection'); copied for the asker queries
    requested_by_connection TEXT,
    requested_by_asker TEXT,
    scope_id         TEXT,                                                  -- ingest: the scope (no foreign key)
    lease_holder     TEXT,
    lease_expires_at TEXT,
    checkpoint       TEXT,                                                  -- JSON
    advanced_at      TEXT,
    items_read       INTEGER NOT NULL DEFAULT 0 CHECK (items_read >= 0),
    total            INTEGER CHECK (total IS NULL OR total >= 0),
    started_at       TEXT,
    attempts         INTEGER NOT NULL DEFAULT 0,
    error_code       TEXT,
    created_at       TEXT NOT NULL,
    ended_at         TEXT,
    CHECK ((state IN ('finished','failed')) = (ended_at IS NOT NULL)),
    CHECK (state <> 'running' OR (lease_holder IS NOT NULL AND lease_expires_at IS NOT NULL))
);
CREATE UNIQUE INDEX jobs_one_open_per_key ON jobs (key) WHERE state IN ('requested','running');
CREATE INDEX jobs_open_by_age  ON jobs (created_at, id) WHERE state IN ('requested','running');
CREATE INDEX jobs_by_key       ON jobs (key, created_at);
CREATE INDEX jobs_newest_first ON jobs (created_at, id);
CREATE INDEX jobs_ended        ON jobs (ended_at) WHERE ended_at IS NOT NULL;
CREATE INDEX jobs_scope        ON jobs (scope_id) WHERE scope_id IS NOT NULL;
CREATE INDEX jobs_asker        ON jobs (requested_by_connection, requested_by_asker) WHERE requested_by_kind = 'chat_connection';

CREATE TABLE job_timer_firings (
    timer       TEXT NOT NULL CHECK (length(timer) BETWEEN 1 AND 200),       -- "retention" | "ingest-<source>"
    due         TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (timer, due)
);
```

### PostgreSQL (`MigrationStep` 1, `"job runner"`)

```sql
CREATE TABLE jobs (
    id                      uuid        PRIMARY KEY,
    kind                    text        NOT NULL CHECK (kind IN ('ingest','retention')),
    key                     text        NOT NULL CHECK (char_length(key) BETWEEN 1 AND 400),
    state                   text        NOT NULL CHECK (state IN ('requested','running','finished','failed')),
    requested_by            jsonb,
    requested_by_kind       text,
    requested_by_connection text,
    requested_by_asker      text,
    scope_id                uuid,
    lease_holder            text,
    lease_expires_at        timestamptz,
    checkpoint              jsonb,
    advanced_at             timestamptz,
    items_read              bigint      NOT NULL DEFAULT 0 CHECK (items_read >= 0),
    total                   bigint      CHECK (total IS NULL OR total >= 0),
    started_at              timestamptz,
    attempts                integer     NOT NULL DEFAULT 0,
    error_code              text,
    created_at              timestamptz NOT NULL DEFAULT now(),
    ended_at                timestamptz,
    CONSTRAINT jobs_ended_at CHECK ((state IN ('finished','failed')) = (ended_at IS NOT NULL)),
    CONSTRAINT jobs_lease CHECK (state <> 'running' OR (lease_holder IS NOT NULL AND lease_expires_at IS NOT NULL))
);
CREATE UNIQUE INDEX jobs_one_open_per_key ON jobs (key) WHERE state IN ('requested','running');
CREATE INDEX jobs_open_by_age  ON jobs (created_at, id) WHERE state IN ('requested','running');
CREATE INDEX jobs_by_key       ON jobs (key, created_at DESC);
CREATE INDEX jobs_newest_first ON jobs (created_at DESC, id DESC);
CREATE INDEX jobs_ended        ON jobs (ended_at) WHERE ended_at IS NOT NULL;
CREATE INDEX jobs_scope        ON jobs (scope_id) WHERE scope_id IS NOT NULL;
CREATE INDEX jobs_asker        ON jobs (requested_by_connection, requested_by_asker) WHERE requested_by_kind = 'chat_connection';

CREATE TABLE job_timer_firings (
    timer       text        NOT NULL CHECK (char_length(timer) BETWEEN 1 AND 200),
    due         timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (timer, due)
);
```

The asker columns are copies of `requested_by` so erasure by person is an indexed statement; `delete_for_asker` updates the JSON and the copy together. A job record holds who asked for it (06): retention deletes job records by `jobs-days`, erasure by scope deletes ingest records of the scope, erasure by person removes the asker. `job_timer_firings` holds no content; kept until the organisation deletes rows.

## Error Table

Status: exit status of `ordin-server agent job run-one`, or the in-process variant.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A key not of its kind's form, or over 400 chars | 1; `Err(permanent)` | `JOB_BAD_KEY` | `error[JOB_BAD_KEY]: the key of an ingest job is "<source>/<connection>/<channel>"` |
| A timer cron that is not five valid fields | exit 1 at start | `JOB_BAD_CRON` | `error[JOB_BAD_CRON]: "61 * * * *" is not a five-field cron expression` |
| Two timers of one name | exit 1 at start | `JOB_DUPLICATE` | `error[JOB_DUPLICATE]: the timer ingest-channel-history is configured twice` |
| `request` found no open job after three refused inserts | `Err(retryable)` | `JOB_REQUEST_CONFLICT` | "the job changed state three times while it was asked for; ask again" |
| `run_one`: the job runs under another instance's live lease | 1 | `JOB_BUSY` | `error[JOB_BUSY]: the job is running in another instance` |
| `save`, `finish`, `fail` by an instance that lost the lease | logged; nothing written | `JOB_LEASE_LOST` | log `job_lease_lost` |
| A checkpoint over 1,048,576 bytes | job failed | `JOB_CHECKPOINT_TOO_LARGE` | recorded as the job's `error_code` |
| A step returned a checkpoint already saved | job failed | `JOB_REVISITED` | recorded as the job's `error_code` |
| A running job saved no checkpoint for 600 s | job failed | `JOB_STALLED` | recorded as the job's `error_code` |
| A step panicked | job failed | `JOB_FAILED` | recorded; the log names the panic's type only |
| A step returned a named error | job failed | that code (`KNOW_SCOPE_NOT_ENABLED`, `ERASE_STORE_FAILED`, `DB_UNAVAILABLE`, …) | recorded unchanged as `error_code` |
| `list` with a cursor of another shape | `Err(permanent)` | `SRV_BAD_CURSOR` | "the cursor is not one this listing returned" |
| The datastore cannot be reached | 1; `Err(retryable)` | `JOB_DB_UNAVAILABLE` | log `job_db_unavailable`; `run_forever` retries next turn |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Job Runner

  Background:
    Given the job_runner migrations on SQLite, or PostgreSQL 18 where marked [pg]
    And a KnowledgeIngest double whose steps return checkpoints {"page":1}, {"page":2}, then done
    And a Retention whose scope registry has "team-slack" channel C0456XYZ enabled

  Scenario: Happy path — an ingest job is requested, taken, saved and finished
    When request(Ingest, "channel-history/team-slack/C0456XYZ", ChatConnection("team-slack","U02ABCDEF")) is called
    And run_forever runs one turn
    Then the job is Finished with items_read from the last step
    And wrightrun_jobs_started_total{kind="ingest"} and wrightrun_jobs_finished_total{kind="ingest"} each rose by 1
    And the notifier was called once with the finished record
    And outcome_text of that record is "The ingest you asked for finished: 1000 items read." when items_read is 1000
    And no log line holds "U02ABCDEF"

  Scenario: Edge case — a second request for an open key returns the open job
    Given an ingest job for the key is Requested
    When request is called again for the same key
    Then it returns the same job_id and created false

  Scenario: [pg] Two requests at the same instant create one job
    When request for one key runs on 10 connections at once
    Then exactly one returns created true and all return the same job_id

  Scenario: A job resumes from its checkpoint after a lapsed lease
    Given a running ingest job with checkpoint {"page":1} whose lease ended 5 seconds ago and that advanced 40 seconds ago
    When another instance's turn runs
    Then it takes the job and the first ingest_step receives {"page":1}

  Scenario: Error path — a job that stopped advancing is failed and not taken again
    Given a running job whose lease ended and that advanced 601 seconds ago
    When a turn runs
    Then the job is Failed with JOB_STALLED and take did not return it

  Scenario: Error path — a step that repeats a checkpoint fails the job
    Given a KnowledgeIngest double that returns {"page":1} twice
    When the job executes
    Then it is Failed with JOB_REVISITED

  Scenario: Error path — ingest of a scope that is not enabled
    Given the scope C0456XYZ was erased
    When the ingest job executes
    Then it is Failed with KNOW_SCOPE_NOT_ENABLED

  Scenario: A stop during a job leaves it for another instance
    Given an ingest job executing on "jobs-a"
    When stop is cancelled after the first save
    Then nothing more is written by "jobs-a" and the job is still Running
    And after 30 seconds another instance takes it from {"page":1}

  Scenario: [pg] A job timer fires once across instances
    Given two runners with the timer Retention { cron "30 2 * * *" } started before 02:30
    When both run a turn just after 02:30 UTC
    Then job_timer_firings holds one row for "retention" and jobs holds one retention job

  Scenario: Ingest timers ask for one job per enabled scope
    Given the timer Ingest { source "channel-history", connection "team-slack", cron "15 * * * *" } and scopes C1, C2 enabled and C3 disabled
    When the timer is due
    Then ingest jobs exist for C1 and C2 and none for C3

  Scenario: The retention job runs retention steps until done
    Given a Retention whose steps delete 500, 500, 200, then report done
    When run_one(Retention, "retention", None) is called
    Then it returns 0 and the job is Finished with items_read 1200

  Scenario: Error path — run_one for a job running under a live lease
    Given the retention job Running on "jobs-a" with a live lease
    When run_one(Retention, "retention", None) is called on "jobs-b"
    Then it returns 1 and logs JOB_BUSY

  Scenario: Erasure by person removes the asker and keeps the record
    Given two job records requested by ChatConnection("team-slack","U02ABCDEF")
    When retained_content().delete_for_asker("team-slack","U02ABCDEF") is called
    Then it returns 2, both records remain, and their asker reads "erased"

  Scenario: Error path — the datastore is away
    Given the datastore is closed during a turn
    Then run_forever logs job_db_unavailable with code JOB_DB_UNAVAILABLE and keeps running
```

## Performance / Security / Observability

- **Performance targets:** `request`, `take`, `renew`, `save` one or two statements each; p95 of `take` with 1,000 open jobs under 5 ms on PostgreSQL. A requested job starts within 15 s plus one take when a jobs instance is idle (A-21). One job at a time per instance; a deployment adds instances for more. An idle instance makes two statements per 15 s plus one firing insert per due timer. Memory per job: one 32-byte digest per saved checkpoint plus the checkpoint (≤ 1 MiB).
- **Security:** no route and no agent tool reaches the runner; the operator routes reach `JobStore::request` and `list` only (through the Caller API). Statements are bound. Keys and instance names are logged; the asker's identifier, a checkpoint's content and an error's text are never logged.
- **Observability:** agent profile only. Counters (names unchanged, SA-OBS-02): `wrightrun_jobs_started_total{kind}`, `wrightrun_jobs_finished_total{kind}`, `wrightrun_jobs_failed_total{kind}`; the failure code is in the record and the log line, not a label. Log events (target `ordin_job_runner`; each with `job_id`, `kind`, `key`, `instance` where a job is involved): `job_runner_started`, `job_timer_fired`, `job_taken`, `job_finished`, `job_failed` (WARN), `job_interrupted`, `job_lease_lost`, `job_notify_failed` (WARN), `job_db_unavailable` (WARN), `job_records_changed`, `job_runner_stopped`. No trace span.

## Gaps

None.
