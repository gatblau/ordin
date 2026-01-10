# SPEC: Retention and Erasure

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-retention` · **Build phase:** 8 · **Programs:** A
> **Dependencies:** Knowledge Service, Memory Service, File Service, Run Queue, Settings Store, Journal

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

Retention and Erasure removes a hosted agent's stored content in two ways (02:825–827, ADR-098 item 6 *Retention and erasure*, 06 row *Personal data of everyone who talks to a hosted agent*). The **retention step** deletes each kind of content older than the period the agent configuration sets for it, in batches the Job Runner drives as the `retention` job; with no period set for a kind, that kind is kept. The **eraser** deletes, on an operator's request, the stored content of one person (named by a chat connection and the chat system's identifier), of one channel or direct-chat scope, or of the agent-wide scope, and returns counts with a fixed statement of what the erasure covers and what it does not; a dry run returns the same counts and deletes nothing. This crate also owns the **scopes** content is kept by (02:826 amendment; 2C *Scope*): it records each scope's kind, chat identifier and state, enables and disables it as the chat connection reports, and marks it erased so the agent stays silent there until it is removed and added again (04 *Scope* lifecycle).

## Approach

Every delete is a method of the module that owns the rows, so no statement here names another module's table (2B-1 rule 5): the Run Queue, Settings Store, Knowledge Service, Memory Service and Journal are called directly (they are this crate's dependencies); content kept by modules in later build phases — chat reply records and pending questions (Channel Gateway), job records (Job Runner), the Rules Engine's per-asker counts — reaches this crate through the trait `RetainedContent` it owns, implemented by those modules or by the program and registered at start (ADR-100, 2B-1 rule 4). The retention step works through the stages in a fixed order, at most 500 rows per call, and returns a checkpoint so the Job Runner can resume after a lease lapse. Error prefixes: `ERASE_` (wrightrun's) and the 2C `SCOPE_` codes. Rejected: cascading foreign keys across modules — modules do not share tables. Rejected: one transaction around a whole erasure — the stores use their own connections, and an erasure repeated after a failure finishes what was left because every delete is safe to repeat.

### Reconciliation assumptions

- **SA-RECON-01** Retention and Erasure owns the `scopes` table and `ScopeRegistry` (ruling R13, X4). The Channel Gateway enables, disables and reads scopes only through `ScopeRegistry`; the re-add-after-erasure rule (step 2) is the one rule (reconciliation rows 533–534).
- **SA-RECON-02** The `ControlState` content is provided by the program over `RulesEngine::forget_asker` (reconciliation O48, O123); its counts are 0 by design, as in wrightrun.
- **SA-RECON-03** This crate emits `wrightrun_retention_items_deleted_total` and `wrightrun_erased_items_total` (with the `memory-items` kind) through `MetricsSink`, which the program forwards to Telemetry; Telemetry names this crate as their source (reconciliation rows 448, 536). Telemetry defines `ErasedKind::MemoryItems` (`memory-items`, Telemetry.md SA-CLOSE-01, K17), which this crate emits for `ErasureCounts.memory_items`.
- **SA-RECON-04** The callees are used under their own names (ruling R6): the Knowledge Service's `KnowledgeIndex` (`ItemKind`, `chrono` cutoff, `Option<ScopeId>` with `None` for the agent-wide notes), the Memory Service's `erase_scope`/`count_scope` (the agent-wide scope by its `ScopeId`), and the Journal's `count_for_runs`/`delete_for_runs` over `RunId`. Migrations are `MigrationStep` tuples (R3).
- **SA-CLOSE-01** (*chosen here*) The Journal's `count_for_runs` is called by the Count step of both erasures (Internal Logic 13a), which reports what an erasure will remove: the journal rows of the counted runs, as `journal_rows` in the `erase_dry_run` and `erased` log lines (`ErasureCounts` and its metric kinds are unchanged). The retention step does not call it.

## Shared Context

### Shared types (Phase 2C, `ordin-types`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScopeId(pub uuid::Uuid);    // UUIDv7

pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);   // RFC 3339 ms Z

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Scope {
    pub id: ScopeId,
    pub kind: ScopeKind,
    /// The chat system's identifier; None for the agent-wide scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_identifier: Option<String>,
    /// The chat connection the scope belongs to; None for the agent-wide scope (2C, decision 80: C-28).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    pub state: ScopeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub erased_at: Option<Timestamp>,
    pub enabled_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind { Channel, DirectChat, AgentWide }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeState { Enabled, Disabled, Erased }
```

2C rules on `Scope`: `chat_identifier` present iff kind ∈ {channel, direct_chat}, ≤ 64 chars (`SCOPE_IDENTIFIER_MISMATCH`); exactly one `agent_wide` per agent (`SCOPE_AGENT_WIDE_DUPLICATE`); state `erased` stays silent until removed and added again (`SCOPE_ERASED_SILENT`). Transitions: — → enabled (account added, or first direct message from a listed user); enabled → disabled (removed); disabled → enabled (added again); enabled | disabled → erased (operator erasure); erased → enabled (removed and added again).

### Called modules (copied from each callee's spec)

```rust
// ordin-run-queue (defined in the Run Queue spec)
pub struct DeletedRuns { pub runs: u64, pub thread_ids: Vec<String>, pub run_ids: Vec<String> }
trait RunQueue {
    async fn count_ended_before(&self, cutoff: Timestamp, limit: u32) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_ended_before(&self, cutoff: Timestamp, limit: u32) -> Result<DeletedRuns, RunQueueError>;
    async fn count_started_by_asker(&self, connection: &str, asker: &str) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_started_by_asker(&self, connection: &str, asker: &str) -> Result<DeletedRuns, RunQueueError>;
    async fn count_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError>;
}
// ordin-settings-store (defined in the Settings Store spec)
trait SettingsStore { async fn count(&self, scope: ScopeId) -> Result<u64, SettingsError>;
                      async fn erase(&self, scope: ScopeId) -> Result<u64, SettingsError>; }

// ordin-knowledge (copied from KnowledgeService.md; the program passes `KnowledgeService::index()`, Some in the agent profile)
pub enum ItemKind { ChatHistory, Entries, Notes }
trait KnowledgeIndex {   // the methods used here
    async fn delete_older_than(&self, kind: ItemKind, cutoff: chrono::DateTime<chrono::Utc>, limit: u32) -> Result<u64, KnowledgeError>;
    async fn count_by_sender(&self, connection: &str, sender: &str) -> Result<u64, KnowledgeError>;
    async fn delete_by_sender(&self, connection: &str, sender: &str) -> Result<u64, KnowledgeError>;
    /// `None` is the agent-wide scope (its notes), never a `ScopeId`.
    async fn count_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError>;
    async fn delete_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError>;
}
// ordin-memory-service (copied from MemoryService.md; agent profile; memory scope key `scope:<ScopeId>`, the agent-wide scope included)
impl MemoryService {
    pub async fn erase_scope(&self, scope_id: ScopeId) -> Result<ScopeErasureReceipt, MemoryError>;   // receipt.rows_removed
    pub async fn count_scope(&self, scope_id: ScopeId) -> Result<u64, MemoryError>;
}
// ordin-journal (copied from JournalAndArtefactStore.md; ruling R3)
impl Journal {
    /// Rows linked to any of `run_ids`; nothing removed. More than 500 ids → JRNL_ENTRY_INVALID; empty → Ok(0).
    pub async fn count_for_runs(&self, run_ids: &[RunId]) -> Result<u64, JournalError>;
    pub async fn delete_for_runs(&self, run_ids: &[RunId]) -> Result<u64, JournalError>;
}
#[derive(Clone)] pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
```

Each callee error type exposes `code() -> &'static str` and `retryable() -> bool` (the convention of every ordin crate's error enum, SA-ERR-01).

The File Service (a listed dependency) is not called: the agent's work folder is outside retention and erasure (04 Data Lifecycle, *A hosted agent's work folder*: "until removed by the agent … or by an operator"), and both statements say so.

### Retention periods (owned here; the program fills them from the agent configuration's `retention` key, wrightrun's `RetentionConfig`)

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetentionPeriods {
    pub chat_history_days: Option<u32>,   // retention.chat-history-days
    pub entries_days: Option<u32>,        // retention.entries-days
    pub notes_days: Option<u32>,          // retention.notes-days
    pub chat_replies_days: Option<u32>,   // retention.chat-replies-days (with pending questions)
    pub runs_days: Option<u32>,           // retention.runs-days (with threads, saved sessions, journal entries)
    pub jobs_days: Option<u32>,           // retention.jobs-days
}
```

`None` = kept. Memory pages have no period in the configuration and are removed only by erasure *(assumed here: wrightrun's configuration names no memory period and ADR-097 adds none)*. The `retention.cron` timer is the Job Runner's (02:822 "the configuration's job timers").

### Constants

| Constant | Value | Source |
|---|---|---|
| `RETENTION_BATCH` | 500 | wrightrun `RetentionAndErasure.md` |
| `CHECKPOINT_VERSION` | 1 | wrightrun |
| `RETENTION_STAGES` | `chat-history`, `entries`, `notes`, `chat-replies`, `runs`, `jobs` | wrightrun |
| `MAX_CONNECTION_CHARS` | 64, slug `^[a-z0-9]+(-[a-z0-9]+)*$` | wrightrun |
| `MAX_CHAT_ID_CHARS` | 64 (2C `Scope.chat_identifier`), no white space or control character | 2C; wrightrun allows 128 for a person |
| `MAX_USER_CHARS` | 128, no white space or control character | wrightrun `USER` |

## Public Interface

```rust
// crate ordin-retention
use async_trait::async_trait;
use ordin_types::{Scope, ScopeId, ScopeKind, ScopeState, Timestamp};

/// A scope with the chat connection it belongs to; `connection` equals `scope.connection` (2C now carries
/// the field, decision 80: C-28; the wrapper stays because the Caller API reads it).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScopeEntry { pub scope: Scope, pub connection: Option<String> }

/// How a person or an operator names a scope: "agent-wide" or "<connection>:<chat identifier>".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeAddress { AgentWide, Chat { connection: String, chat_identifier: String } }
pub fn parse_scope_text(text: &str) -> Result<ScopeAddress, RetentionError>;   // ERASE_BAD_TARGET
pub fn scope_text(entry: &ScopeEntry) -> String;

#[async_trait]
pub trait ScopeRegistry: Send + Sync {
    /// The one agent-wide scope, created on first call.
    async fn agent_wide(&self) -> Result<ScopeEntry, RetentionError>;
    /// The agent's account was added to a channel, or a listed user sent a first direct message.
    async fn enable(&self, kind: ScopeKind, connection: &str, chat_identifier: &str) -> Result<ScopeEntry, RetentionError>;
    /// The agent's account was removed from the channel.
    async fn disable(&self, connection: &str, chat_identifier: &str) -> Result<Option<ScopeEntry>, RetentionError>;
    async fn find(&self, connection: &str, chat_identifier: &str) -> Result<Option<ScopeEntry>, RetentionError>;
    async fn resolve(&self, address: &ScopeAddress) -> Result<Option<ScopeEntry>, RetentionError>;
    async fn get(&self, id: ScopeId) -> Result<Option<ScopeEntry>, RetentionError>;
    async fn enabled(&self, connection: &str) -> Result<Vec<ScopeEntry>, RetentionError>;
    async fn is_enabled(&self, id: ScopeId) -> Result<bool, RetentionError>;
}

/// Content kept by modules this crate cannot depend on (ADR-100 hub trait).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProvidedContent { ChatReplies, PendingQuestions, Jobs, ControlState }
#[async_trait]
pub trait RetainedContent: Send + Sync {
    fn content(&self) -> ProvidedContent;
    async fn delete_older_than(&self, cutoff: Timestamp, limit: u32) -> Result<u64, ProviderFailed>;
    async fn count_for_asker(&self, connection: &str, asker: &str) -> Result<u64, ProviderFailed>;
    /// Jobs: removes the asker's identifier from the records (they stay). ControlState: deletes the asker's counts.
    async fn delete_for_asker(&self, connection: &str, asker: &str) -> Result<u64, ProviderFailed>;
    async fn count_scope(&self, scope: ScopeId) -> Result<u64, ProviderFailed>;
    async fn delete_scope(&self, scope: ScopeId) -> Result<u64, ProviderFailed>;
}
#[derive(Debug, Clone)]
pub struct ProviderFailed { pub code: String }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ErasureCounts {
    pub knowledge_items: u64, pub memory_items: u64, pub chat_replies: u64, pub pending_questions: u64,
    pub runs: u64, pub threads: u64, pub settings: u64, pub jobs: u64, pub channels: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErasureTarget { Person, Scope }
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErasureReport {
    pub target: ErasureTarget,
    pub dry_run: bool,
    pub counts: ErasureCounts,
    pub covers: Vec<&'static str>,
    pub does_not_cover: Vec<&'static str>,
}

/// One call of the retention job (the Job Runner saves `checkpoint` after each returned step).
#[derive(Debug, Clone, PartialEq)]
pub struct RetentionStep { pub checkpoint: serde_json::Value, pub items_deleted: u64, pub done: bool }

pub trait MetricsSink: Send + Sync {
    fn add(&self, name: &'static str, labels: &[(&'static str, &str)], by: u64);
}

pub struct Retention { /* stores, providers, periods, datastore backend, metrics */ }
impl Retention {
    pub fn new(datastore: &ordin_journal::Datastore,
               periods: RetentionPeriods,
               runs: std::sync::Arc<dyn ordin_run_queue::RunQueue>,
               settings: std::sync::Arc<dyn ordin_settings_store::SettingsStore>,
               knowledge: std::sync::Arc<dyn ordin_knowledge::KnowledgeIndex>,
               memory: ordin_memory_service::MemoryService,
               journal: std::sync::Arc<ordin_journal::Journal>,
               metrics: std::sync::Arc<dyn MetricsSink>) -> Self;
    /// Called by the program at start, before any route or job is served; one provider per content.
    pub fn register(&self, provider: std::sync::Arc<dyn RetainedContent>) -> Result<(), RetentionError>;
    pub fn scopes(&self) -> std::sync::Arc<dyn ScopeRegistry>;
    pub async fn retention_step(&self, checkpoint: Option<&serde_json::Value>) -> Result<RetentionStep, RetentionError>;
    pub async fn erase_person(&self, connection: &str, user: &str, dry_run: bool, operator: &str) -> Result<ErasureReport, RetentionError>;
    pub async fn erase_scope(&self, address: &ScopeAddress, dry_run: bool, operator: &str) -> Result<ErasureReport, RetentionError>;
}
pub fn migrations() -> ordin_journal::ModuleMigrations;   // module "retention"; version 1 is the Data Model

#[derive(Debug, thiserror::Error)]
pub enum RetentionError {
    #[error("a person is named by a chat connection and the chat system's identifier of 1 to 128 characters without white space")] BadTarget,
    #[error("the erasure stopped at the step \"{step}\" after {done}; repeat it to finish")] Incomplete { step: &'static str, done: String },
    #[error("the checkpoint has the version {0}; this ordin reads version 1")] BadCheckpoint(u64),
    #[error("the retention job stopped at the stage \"{0}\"; the next job goes on from what is left")] StoreFailed(&'static str),
    #[error("a scope's chat identifier is required for a channel or direct chat and absent for the agent-wide scope")] IdentifierMismatch,
    #[error("a provider for this content is registered already")] DuplicateProvider,
    #[error("the stored scopes could not be read or written")] Unavailable,
}
impl RetentionError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

**Called by.**

| Public item | Called by |
|---|---|
| `Retention::new`, `register`, `migrations` | the server program (OrdinServer, agent profile, every role) |
| `scopes()` | the server program (OrdinServer); Caller API (a run start, step 12; the operator routes with `{scope}`, step 31); Job Runner (`request` for ingest, the ingest timers) |
| `register` providers | the program, before any route or job is served: `ChatReplies` and `PendingQuestions` — the Channel Gateway's two `RetainedContent` objects over its `chat_replies` and `chat_pending_questions` tables; `Jobs` — `JobRunner::retained_content()`; `ControlState` — an object the program builds over `RulesEngine::forget_asker` (see below) |
| `ScopeRegistry::{agent_wide, enable, disable, find, enabled}` | Channel Gateway (`new` → `agent_wide`; `Joined` → `enable`; `Left` → `disable`; a message → `find`, then it reads `Scope.state`; the comparison with the chat system's list → `enabled`; ruling R13, X4). The gateway keeps no scopes table of its own |
| `ScopeRegistry::{find, enabled}` | Job Runner (`find` for an ingest job's scope; `enabled(connection)` for each ingest timer) |
| `ScopeRegistry::is_enabled` | no component calls it today |
| `ScopeRegistry::{agent_wide, resolve, get}`, `parse_scope_text`, `scope_text` | Caller API (operator routes with `{scope}`), the program (the agent-wide `ScopeId` it gives the Duty Timer and the Caller API), Memory Service and Knowledge Service (through the program, to read a scope's state) |
| `retention_step` | Job Runner (`retention` job, one call per step) |
| `erase_person`, `erase_scope` | Caller API route 25 (`POST /operator/erase`) |

**The `ControlState` provider.** The program implements `RetainedContent` with `content() = ControlState` over the Rules Engine: `delete_for_asker(connection, asker)` = `RulesEngine::forget_asker(connection, asker)`; `count_for_asker` returns 0, because the control rows are covered but not counted (step 14 and wrightrun's `not counted:` line); `delete_older_than`, `count_scope` and `delete_scope` return 0 (control counts carry their own windows in the Rules Engine). A `RulesError` maps to `ProviderFailed { code }` with the Rules Engine's code (reconciliation O48, O123).

**What an erasure by person covers** (`ERASE_PERSON_COVERS`, in this order and wording; wrightrun's lines with ordin's saved sessions and journal entries in place of checkpoints):

1. `Knowledge items of chat history that the chat system attributes to this person, in every channel of the connection.`
2. `The records of replies to this person's questions, with the text of each question and the content of each reply, and this person's questions that still await a reply.`
3. `The runs this person started through the chat connection, with their input, their result, their threads, their saved sessions and their journal entries.`
4. `This person's identifier in the records of the jobs this person asked for, and the counts the agent keeps of this person's recent questions.`

**Does not cover** (`ERASE_PERSON_NOT_COVERED`):

1. `A note of the agent, or a message of another person, that repeats this person's words.`
2. `Memory pages of a channel that hold this person's words.`
3. `This person's messages inside the transcript gathered for a run another person started.`
4. `A run started with a caller's token, by a schedule or by an intake.`
5. `An entry this person added to a list setting.`
6. `Work-folder files.`
7. `Log lines, database backups, and what a model provider keeps.`
8. `The chat system itself, where the messages stay.`

**What an erasure by scope covers** (`ERASE_SCOPE_COVERS`):

1. `Every knowledge item of the channel: its chat history, its added entries and the agent's notes of the channel.`
2. `The memory pages of the channel.`
3. `The records of replies delivered in the channel, and the questions asked there that still await a reply.`
4. `The runs asked in the channel, with their input, their result, their threads, their saved sessions and their journal entries.`
5. `The setting values of the channel.`
6. `The job records of the channel's ingests.`
7. `The record that the agent is enabled in the channel. The agent stays silent there until its account is removed from the channel and added again.`

**Does not cover** (`ERASE_SCOPE_NOT_COVERED`): 1. `The agent's notes and memory pages of the agent-wide scope.` 2. `Work-folder files.` 3. `Log lines, database backups, and what a model provider keeps.` 4. `The chat system itself, where the messages stay.`

**Agent-wide scope covers** (`ERASE_AGENT_WIDE_COVERS`): 1. `The agent's notes of the agent-wide scope, which every run reads.` 2. `The memory pages of the agent-wide scope.` **Does not cover** (`ERASE_AGENT_WIDE_NOT_COVERED`): 1. `The notes, the memory pages, the chat history and the entries of every channel.` 2. `The records of runs that no channel holds.` 3. `Work-folder files.` 4. `Log lines, database backups, and what a model provider keeps.` 5. `The chat system itself, where the messages stay.`

A direct-chat scope uses the scope lists with "channel" read as the direct chat; the lines are not reworded *(assumed here: one statement per target keeps the operator's text stable)*.

These lines reach operators only through the operator routes and the `ordin-server agent erase` command (Diagnostics-level text; 11 §1 keeps technical text out of business screens). They are English constants; localisation of operator text is not in scope of this crate.

### Example

```yaml
retention:
  chat-history-days: 365
  chat-replies-days: 90
  runs-days: 30
  cron: "30 2 * * *"
```

```rust
let report = retention.erase_person("team-slack", "U02ABCDEF", true, "00u3ops-anna").await?;
// ErasureReport { target: Person, dry_run: true,
//   counts: ErasureCounts { knowledge_items: 412, memory_items: 0, chat_replies: 37, pending_questions: 1,
//                           runs: 37, threads: 37, settings: 0, jobs: 0, channels: 0 },
//   covers: [4 lines], does_not_cover: [8 lines] }

let step = retention.retention_step(None).await?;
// RetentionStep { checkpoint: {"v":1,"started":"2026-01-10T02:30:14.000Z","stage":0,
//                 "deleted":{"chat-history":500,"entries":0,"notes":0,"chat-replies":0,"runs":0,"jobs":0}},
//                 items_deleted: 500, done: false }
```

## Internal Logic

A sqlx error on this crate's own tables becomes `Unavailable` (log `retention_unavailable` WARN, `operation`).

### Scope registry

1. **`agent_wide()`** — `SELECT … FROM scopes WHERE kind = 'agent_wide'`; none → insert one (UUIDv7, `state='enabled'`, `enabled_at=now`, `chat_identifier` NULL, `connection` NULL) guarded by the partial unique index `scopes_one_agent_wide`; on its violation re-read (another instance won). Cached in memory after the first read.
2. **`enable(kind, connection, chat_identifier)`** — `kind = AgentWide` → `IdentifierMismatch`. Validate `connection` (slug ≤ 64) and `chat_identifier` (1–64 chars, no white space or control character) → else `BadTarget`. One transaction: read the row by (`connection`, `chat_identifier`); none → insert `enabled`; `disabled` → set `enabled`, `enabled_at=now`; `erased` with `removed_after_erasure = true` → set `enabled`, `enabled_at=now`, `erased_at` kept, `removed_after_erasure = false`; `erased` with `removed_after_erasure = false` → unchanged, log `scope_erased_silent` (INFO; `scope_id`) (`SCOPE_ERASED_SILENT` is a state, not an error: the caller reads `state == Erased` and stays silent); `enabled` → unchanged. Log `scope_enabled` (INFO; `scope_id`, `kind`) on a change.
3. **`disable(connection, chat_identifier)`** — `enabled` → `disabled`; `erased` → set `removed_after_erasure = true` (state stays `erased`); log `scope_disabled`. Unknown → `None`.
4. **`find`, `resolve`, `get`, `enabled(connection)`, `is_enabled(id)`** — single `SELECT`s; `resolve(AgentWide)` = `agent_wide()`; `is_enabled` is true only for state `enabled`.
5. **`parse_scope_text`** — `"agent-wide"` → `AgentWide`; else split at the first `:`; the left part must be a connection slug ≤ 64, the right part 1–64 chars without white space or control characters → `Chat`; else `BadTarget`. `scope_text` writes the inverse.

### Retention step (called by the Job Runner's `retention` job)

6. **State.** `checkpoint = None` → `started = now` (UTC, ms), `stage = 0`, all `deleted` counts 0. `Some(c)` → `c["v"]` must be 1 (else `BadCheckpoint(v)`); `started`, `stage`, `deleted` continue from it. Each stage's cutoff is `started − period days`, so a job taken again deletes against the same instants.
7. Advance `stage` past every stage whose period is `None`, and past `chat-replies` when no `ChatReplies` provider is registered. If `stage` reaches 6 → log `retention_finished` (INFO; `started`, the six counts as `chat_history`, `entries`, `notes`, `chat_replies`, `runs`, `jobs`) and return `RetentionStep { checkpoint: <final>, items_deleted: 0, done: true }`.
8. Run one batch of the current stage:
   - `chat-history` / `entries` / `notes`: `n = knowledge.delete_older_than(<ItemKind of the stage>, cutoff.0, 500)`.
   - `chat-replies`: `n = provider(ChatReplies).delete_older_than(cutoff, 500)`; then, when a `PendingQuestions` provider is registered, `n += provider(PendingQuestions).delete_older_than(cutoff, 500)` (a pending question older than the period is deleted with the replies; whoever asked gets no reply).
   - `runs`: `found = runs.count_ended_before(cutoff, 500)`; `found.run_ids` empty → batch empty. Else `journal.delete_for_runs(<found.run_ids as RunId>)` first, then `gone = runs.delete_ended_before(cutoff, 500)`; `journal.delete_for_runs(<gone.run_ids − found.run_ids, as RunId>)` for runs that joined between the calls; `n = gone.runs`. A stop between the two calls leaves run records whose journal entries are gone, never journal entries no record names; the next step deletes those records. Saved sessions go with their run rows (Run Queue `session_record` column).
   - `jobs`: `n = provider(Jobs).delete_older_than(cutoff, 500)`; the provider deletes ended jobs only, so the running retention job is never among them.
9. Batch empty (`n == 0` and nothing selected) → the stage is done: `stage += 1`, go to step 7 within the same call. `n == 0` with rows selected (every selected thread gained a run between the two Run Queue calls) → repeat the batch at most 3 times, then treat the stage as done for this job *(chosen here: bounds a call; the next job deletes the rest)*.
10. `n > 0` → `deleted[stage] += n`; `metrics.add("wrightrun_retention_items_deleted_total", [("kind", stage_name)], n)`; return `RetentionStep { checkpoint: {"v":1,"started":…,"stage":…,"deleted":{…}}, items_deleted: <sum of deleted>, done: false }`. Each returned checkpoint differs from the one before (a count rose), which the Job Runner requires.
11. A callee error with `retryable() == true` (a datastore away) is returned unchanged in code: `StoreFailed(stage)` is used only for a callee error that is not retryable; the message names the stage and never the callee's text. In both cases the Job Runner records the job failed with that code, and the next firing starts a new job that deletes what is left.

### Erasure

12. **Validation.** Person: `connection` slug ≤ 64, `user` 1–128 chars, no white space or control character → else `BadTarget`. Scope: `address` from `parse_scope_text`. An unknown person or scope is not an error: every count is 0.
13a. **Journal rows the erasure will remove** (SA-CLOSE-01; the Count step of 14 and 15): `journal.count_for_runs(<the counted runs' run_ids as RunId>)` in batches of at most 500 ids (the Journal refuses more with `JRNL_ENTRY_INVALID`), summed as `journal_rows`; no run ids → 0 without a call. `ErasureCounts` has no journal field, so `journal_rows` goes in the `erase_dry_run` and `erased` log lines, as `control_rows` does. A Journal error while counting → `Incomplete { step: "journal", done: "nothing removed" }`: nothing deleted, no `erasure_records` row.
13. **Run deletes, journal first** (both erasures): `found = runs.count_…()`; `journal.delete_for_runs(<found.run_ids as RunId>)`; `gone = runs.delete_…()`; `journal.delete_for_runs(<gone.run_ids − found.run_ids, as RunId>)`.
14. **`erase_person(connection, user, dry_run, operator)`**
    - Count: `knowledge.count_by_sender`, `provider(ChatReplies)?.count_for_asker` (0 if absent), `provider(PendingQuestions)?.count_for_asker`, `runs.count_started_by_asker` (runs and threads), then `journal.count_for_runs` over those runs (step 13a), `provider(Jobs)?.count_for_asker`. The control-state rows are not counted (the statement says they are covered).
    - `dry_run` → log `erase_dry_run` (INFO; `request="person"`, `connection`, counts, `journal_rows`); return the report. Nothing deleted, nothing counted in metrics.
    - Else, in this order: `knowledge.delete_by_sender`; `ChatReplies.delete_for_asker`; `PendingQuestions.delete_for_asker` (before the runs, so no chat instance delivers a reply for a run about to go); runs per step 13; `Jobs.delete_for_asker` (removes the identifier, records stay); `ControlState.delete_for_asker` (its count is logged as `control_rows`, not a report field).
    - Build counts (`settings`, `channels`, `memory_items` = 0). `metrics.add("wrightrun_erased_items_total", [("kind", k), ("request", "person")], count)` once per field. Insert an `erasure_records` row (step 17). Log `erased` (WARN; `request="person"`, `connection`, counts, `journal_rows`, `operator`); the person's identifier is never logged.
15. **`erase_scope(address, dry_run, operator)`** for a channel or direct chat (`resolve(address)`; none → all counts 0):
    - Count: `knowledge.count_scope(Some(id))`, `memory.count_scope(id)`, `ChatReplies.count_scope`, `PendingQuestions.count_scope`, `runs.count_in_scope`, then `journal.count_for_runs` over those runs (step 13a), `settings.count`, `Jobs.count_scope`, `channels` = 1 when the scope is `enabled`, else 0.
    - `dry_run` → log `erase_dry_run` (INFO; `request="scope"`, `scope_text`, counts, `journal_rows`) and return.
    - Else, in this order: (a) mark the scope `erased`, `erased_at = now`, `removed_after_erasure = false` — first, so no new question there starts a run and no ingest of it starts while the rest is deleted; (b) `Jobs.delete_scope` (a running ingest loses its record and stops at its next save with `JOB_LEASE_LOST`); (c) `knowledge.delete_scope(Some(id))`; (d) `memory.erase_scope(id)` (`memory_items` = `rows_removed`); (e) `ChatReplies.delete_scope`, then `PendingQuestions.delete_scope`; (f) runs per step 13 (`count_in_scope` / `delete_in_scope`); (g) `settings.erase`; (h) a second `knowledge.delete_scope(Some(id))`, added to `knowledge_items`, removing what an ingest wrote between (b) and (c).
    - Metrics with `request="scope"`, an `erasure_records` row, log `erased` with `scope_text` and `journal_rows`.
16. **`erase_scope(AgentWide, …)`** — counts `knowledge.count_scope(None)` (notes) and `memory.count_scope(agent_wide.id)`; every other field 0. Not dry → `knowledge.delete_scope(None)`, `memory.erase_scope(agent_wide.id)`; the agent-wide scope is not marked erased (the agent keeps answering; its notes and memory start empty). Metrics `request="scope"`, a record, log `erased` with `agent-wide`.
17. **Failure.** A callee error during the deletes → stop, insert the `erasure_records` row with `outcome='incomplete'` and the counts reached, and return `Incomplete { step, done }` naming the step (`knowledge`, `memory`, `chat-replies`, `pending-questions`, `runs`, `journal`, `settings`, `jobs`, `control-state`, `scope`) and the counts reached. A repeat finishes what was left.
18. **Erasure record.** On a non-dry run: insert (`id` UUIDv7, `target`, `connection` for a person or `scope_id` for a scope, `counts` JSON, `operator` = token subject, `requested_at`, `completed_at`, `outcome` `done`|`incomplete`). The person's chat identifier is not stored.

## Data Model

Module `retention`. New tables; nothing to roll back before use. Every delete this crate drives is destructive by purpose and has no undo inside ordin: what a period or an erasure removed comes back only from the organisation's database backup; the dry run and the statements are the safeguard before an erasure, and an unset period is the safeguard for retention.

### SQLite (`MigrationStep` 1, `"retention"`)

```sql
CREATE TABLE scopes (
    id                    TEXT PRIMARY KEY,                                  -- ScopeId (UUIDv7)
    kind                  TEXT NOT NULL CHECK (kind IN ('channel','direct_chat','agent_wide')),
    connection            TEXT CHECK (connection IS NULL OR length(connection) BETWEEN 1 AND 64),
    chat_identifier       TEXT CHECK (chat_identifier IS NULL OR length(chat_identifier) BETWEEN 1 AND 64),
    state                 TEXT NOT NULL CHECK (state IN ('enabled','disabled','erased')),
    erased_at             TEXT,
    removed_after_erasure INTEGER NOT NULL DEFAULT 0 CHECK (removed_after_erasure IN (0,1)),
    enabled_at            TEXT NOT NULL,
    CHECK ((kind = 'agent_wide') = (chat_identifier IS NULL)),
    CHECK ((kind = 'agent_wide') = (connection IS NULL)),
    CHECK ((state = 'erased') <= (erased_at IS NOT NULL))
);
CREATE UNIQUE INDEX scopes_one_agent_wide ON scopes (kind) WHERE kind = 'agent_wide';
CREATE UNIQUE INDEX scopes_by_chat ON scopes (connection, chat_identifier) WHERE kind <> 'agent_wide';
CREATE INDEX scopes_enabled ON scopes (connection) WHERE state = 'enabled';

CREATE TABLE erasure_records (
    id            TEXT PRIMARY KEY,
    target        TEXT NOT NULL CHECK (target IN ('person','scope')),
    connection    TEXT,                                                      -- person target
    scope_id      TEXT,                                                      -- scope target
    counts        TEXT NOT NULL,                                             -- JSON ErasureCounts
    operator      TEXT NOT NULL CHECK (length(operator) BETWEEN 1 AND 255),
    requested_at  TEXT NOT NULL,
    completed_at  TEXT NOT NULL,
    outcome       TEXT NOT NULL CHECK (outcome IN ('done','incomplete')),
    CHECK ((target = 'person') = (connection IS NOT NULL)),
    CHECK ((target = 'scope') = (scope_id IS NOT NULL))
);
CREATE INDEX erasure_records_requested ON erasure_records (requested_at);
```

### PostgreSQL (`MigrationStep` 1, `"retention"`)

```sql
CREATE TABLE scopes (
    id                    uuid        PRIMARY KEY,
    kind                  text        NOT NULL CHECK (kind IN ('channel','direct_chat','agent_wide')),
    connection            text        CHECK (connection IS NULL OR char_length(connection) BETWEEN 1 AND 64),
    chat_identifier       text        CHECK (chat_identifier IS NULL OR char_length(chat_identifier) BETWEEN 1 AND 64),
    state                 text        NOT NULL CHECK (state IN ('enabled','disabled','erased')),
    erased_at             timestamptz,
    removed_after_erasure boolean     NOT NULL DEFAULT false,
    enabled_at            timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT scopes_identifier_with_kind CHECK ((kind = 'agent_wide') = (chat_identifier IS NULL)),
    CONSTRAINT scopes_connection_with_kind CHECK ((kind = 'agent_wide') = (connection IS NULL)),
    CONSTRAINT scopes_erased_at CHECK (state <> 'erased' OR erased_at IS NOT NULL)
);
CREATE UNIQUE INDEX scopes_one_agent_wide ON scopes (kind) WHERE kind = 'agent_wide';
CREATE UNIQUE INDEX scopes_by_chat ON scopes (connection, chat_identifier) WHERE kind <> 'agent_wide';
CREATE INDEX scopes_enabled ON scopes (connection) WHERE state = 'enabled';

CREATE TABLE erasure_records (
    id            uuid        PRIMARY KEY,
    target        text        NOT NULL CHECK (target IN ('person','scope')),
    connection    text,
    scope_id      uuid,
    counts        jsonb       NOT NULL,
    operator      text        NOT NULL CHECK (char_length(operator) BETWEEN 1 AND 255),
    requested_at  timestamptz NOT NULL,
    completed_at  timestamptz NOT NULL DEFAULT now(),
    outcome       text        NOT NULL CHECK (outcome IN ('done','incomplete')),
    CONSTRAINT erasure_records_person CHECK ((target = 'person') = (connection IS NOT NULL)),
    CONSTRAINT erasure_records_scope CHECK ((target = 'scope') = (scope_id IS NOT NULL))
);
CREATE INDEX erasure_records_requested ON erasure_records (requested_at);
```

`scopes` and `erasure_records` hold no content and no person's identifier; they are not reached by retention. An erasure record is kept until the organisation deletes it *(assumed here: it is the evidence that an erasure was carried out, 06)*.

## Error Table

In-process API; the Caller API maps codes to HTTP statuses on the erase route.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A connection that is no slug, or a person's or scope's identifier empty, too long or holding white space; a scope text of another form | `Err(permanent)` | `ERASE_BAD_TARGET` | `RetentionError::BadTarget` — "a person is named by a chat connection and the chat system's identifier of 1 to 128 characters without white space" |
| The Journal's count of the runs' rows failed during an erasure's Count step (13a); nothing deleted | `Err(retryable)` | `ERASE_INCOMPLETE` | `RetentionError::Incomplete` — "the erasure stopped at the step \"journal\" after nothing removed; repeat it to finish" |
| A delete of an erasure failed part-way | `Err(retryable)` | `ERASE_INCOMPLETE` | `RetentionError::Incomplete` — "the erasure stopped at the step \"runs\" after 412 knowledge items and 37 chat reply records; repeat it to finish" |
| The retention checkpoint has a version other than 1 | `Err(permanent)` | `ERASE_BAD_CHECKPOINT` | `RetentionError::BadCheckpoint` — "the checkpoint has the version 2; this ordin reads version 1" |
| A store's count or delete fails with a non-retryable error during the retention step | `Err(permanent)` | `ERASE_STORE_FAILED` | `RetentionError::StoreFailed` — "the retention job stopped at the stage \"runs\"; the next job goes on from what is left" |
| A store's datastore is away during the retention step | `Err(retryable)` | the callee's code (`DB_UNAVAILABLE`, `SET_UNAVAILABLE`, …) | passed through unchanged |
| `enable` with kind agent-wide, or a chat scope without identifier | `Err(permanent)` | `SCOPE_IDENTIFIER_MISMATCH` | `RetentionError::IdentifierMismatch` |
| A second provider for one content | `Err(permanent)` (start-up) | `ERASE_DUPLICATE_PROVIDER` (*chosen here*) | `RetentionError::DuplicateProvider` |
| This crate's own tables cannot be read or written | `Err(retryable)` | `ERASE_UNAVAILABLE` (*chosen here*) | `RetentionError::Unavailable` — "the stored scopes could not be read or written" |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Retention and Erasure

  Background:
    Given the retention migrations on SQLite, or PostgreSQL 18 where marked [pg]
    And in-memory doubles of the Knowledge Service index, the Memory Service and the Journal's run-linked counts and deletes
    And a real Run Queue and Settings Store on the same datastore
    And registered providers for ChatReplies, PendingQuestions, Jobs and ControlState that record their calls

  Scenario: Happy path — a dry run counts and deletes nothing
    Given the person U02ABCDEF of "team-slack" has 412 knowledge items, 37 reply records, 1 pending question and 37 runs on 37 threads
    When erase_person("team-slack", "U02ABCDEF", true, "ops-1") is called
    Then the counts are knowledge_items 412, chat_replies 37, pending_questions 1, runs 37, threads 37
    And covers holds 4 lines and does_not_cover 8 lines, in the stated wording
    And every store still holds its rows and no erasure_records row exists
    And count_for_runs was called once with the 37 run ids and the erase_dry_run line holds its journal_rows

  Scenario: Happy path — an erasure by person deletes in order and records it
    Given the same person
    When erase_person("team-slack", "U02ABCDEF", false, "ops-1") is called
    Then the providers saw delete_for_asker in the order ChatReplies, PendingQuestions, then Jobs, then ControlState
    And the journal entries of the 37 runs were deleted before the runs
    And erasure_records holds one row with target person, connection "team-slack", outcome done, operator "ops-1"
    And no log line and no erasure_records column holds "U02ABCDEF"

  Scenario: Edge case — erasure by person leaves a caller-started run
    Given a run started by Caller "U02ABCDEF"
    When erase_person("team-slack", "U02ABCDEF", false, "ops-1") is called
    Then that run is still read by get

  Scenario: Erasure by scope marks the scope first and covers memory
    Given an enabled channel scope C of "team-slack" with knowledge, memory pages, setting values, an ingest job record and two runs
    When erase_scope(Chat { "team-slack", "C0123" }, false, "ops-1") is called
    Then the scope's state became erased before any store was called
    And the counts hold channels 1, settings 3, memory_items 5 and runs 2
    And is_enabled(C) is false

  Scenario: An erased scope stays silent until removed and added again
    Given scope C erased
    When enable(Channel, "team-slack", "C0123") is called
    Then the state is still erased
    When disable then enable are called
    Then the state is enabled

  Scenario: Agent-wide erasure deletes notes and memory only
    When erase_scope(AgentWide, false, "ops-1") is called
    Then only knowledge.delete_scope(None) and memory.erase_scope(<agent-wide ScopeId>) were called
    And the agent-wide scope is still enabled

  Scenario: Error path — a bad target
    When erase_person("Team Slack", "U0", true, "ops-1") is called
    Then it fails with code ERASE_BAD_TARGET

  Scenario: Error path — the journal count fails before anything is removed (SA-CLOSE-01)
    Given the Journal double fails count_for_runs
    When erase_person("team-slack", "U02ABCDEF", true, "ops-1") is called
    Then it fails with code ERASE_INCOMPLETE naming the step "journal"
    And every store still holds its rows and no erasure_records row exists

  Scenario: Error path — a store fails part-way
    Given the Memory Service double fails erase_scope
    When erase_scope for C is called with dry_run false
    Then it fails with code ERASE_INCOMPLETE naming the step "memory"
    And an erasure_records row with outcome incomplete exists
    When the double recovers and the erasure is repeated
    Then it completes and a second record with outcome done exists

  Scenario: Retention deletes by stage with distinct checkpoints
    Given periods chat-history 365, chat-replies 90, runs 30 and 1,200 chat-history items older than 365 days
    When retention_step is called with None and then with each returned checkpoint until done
    Then the steps deleted 500, 500 and 200 chat-history items, then the replies, then the runs
    And no two returned checkpoints are equal
    And wrightrun_retention_items_deleted_total{kind="chat-history"} rose by 1200

  Scenario: Edge case — no period set deletes nothing
    Given RetentionPeriods::default()
    When retention_step(None) is called
    Then it returns done true with items_deleted 0 and no store was called

  Scenario: Edge case — retention keeps a thread with an open run
    Given thread T1 whose runs ended 100 days ago and thread T2 with an ended run and a paused run
    And runs-days 90
    When the retention steps run to the end
    Then T1's runs and journal entries are gone and every run of T2 remains

  Scenario: Error path — a checkpoint of another version
    When retention_step is called with {"v": 2}
    Then it fails with code ERASE_BAD_CHECKPOINT

  Scenario: [pg] Two instances create the agent-wide scope at once
    When agent_wide() is called on two connections at the same instant
    Then both return the same ScopeId and scopes holds one agent_wide row
```

## Performance / Security / Observability

- **Performance targets:** one retention batch of 500 knowledge items within 2 s, of 500 threads (with their journal entries) within 10 s on PostgreSQL — far below the Job Runner's 600 s stall limit. An erasure makes one count and one delete per store; for 100,000 knowledge items the target is 30 s, within the operator command's 300 s limit (wrightrun `OperatorRoutes.md`). Scope registry calls: one primary-key or unique-index statement each, p95 under 5 ms.
- **Security:** the eraser is reached only through `POST /operator/erase` (operator role) and the operator command. The person's identifier is passed to stores as a parameter, never logged, never a metric label, never stored in `erasure_records`, never in an error message. Deleted content cannot be restored by ordin. A dry run is available for every target and the statements say what is not covered (wrightrun R-45). No agent tool reaches this crate.
- **Observability:** agent profile only. Counters (names unchanged, SA-OBS-02): `wrightrun_retention_items_deleted_total{kind}` (`chat-history`, `entries`, `notes`, `chat-replies`, `runs`, `jobs`); `wrightrun_erased_items_total{kind, request}` (`kind` ∈ `knowledge-items`, `memory-items`, `chat-replies`, `pending-questions`, `runs`, `threads`, `settings`, `jobs`, `channels`; `request` ∈ `person`, `scope`); a dry run counts nothing. Log events (target `ordin_retention`): `retention_finished`, `erase_dry_run`, `erased` (WARN), `scope_enabled`, `scope_disabled`, `scope_erased_silent`, `retention_unavailable` (WARN). No trace span.

## Gaps

None.
