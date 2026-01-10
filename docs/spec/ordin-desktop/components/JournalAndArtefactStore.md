# SPEC: Journal and Artefact Store

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (Phase 1 build corrections: the stored report types are `ReportSection`, `ReportTableColumn`, `ReportKeyFigure` and `ReportFigureChange`, as 2C part 1; approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-journal` · **Build phase:** 1 · **Programs:** D P A
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
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

---

## Purpose

The Journal and Artefact Store is ordin's append-only record and the owner of its datastore (02:673–682, ADR-010, 2B-1 item 5). It opens the one datastore of each program — a SQLite file under the application data root on the desktop, the personal server and the agent profile's single shape; PostgreSQL in the agent profile's scaled shape (ADR-098 item 5) — and applies every module's registered migrations to it, forward only and after a backup copy (07 — *Migrations*). It gives every Service API event its backend sequence number and delivers the events, in order, to the modules that subscribe through the `EventSink` trait it owns (ADR-100 item 1; ADR-096 item 2). It keeps the records 2C assigns to it — artefacts and their versions, tool requests, external actions with their unique keys, start-up reports, reports and dashboards, each run's stated plan and step marks, and internal journal entries — and it is the source of truth for recovery: a resumed run reads which external actions completed and never repeats them (G8, 03:409–441). It applies the person's history retention (A-08, UX-A58) to its own tables and asks each module that registered a prune participant to apply the same cut-off to theirs.

## Approach

One crate with three layers: a `Datastore` handle that wraps a `sqlx` pool for either engine (SA-DB-01), a `MigrationRegistry` that every module feeds with plain tuples of versioned SQL, and a `Journal` service holding the records above, with event sequencing under one writer lock (SQLite) or one transaction-scoped advisory lock (PostgreSQL) so sequence numbers are contiguous, as the desktop's gap detection requires (2C wire §1 *EventEnvelope*). Events are stored as `kind` plus JSON `data`, because the typed `ServiceEvent` lives in `ordin-service-api`, which this crate may not depend on (2B-3). Rejected: `sqlx::migrate!` per module — sqlx's migrator keeps one version sequence per database, so two modules' version 1 would collide; a `tokio::sync::broadcast` channel shared by all subscribers — a slow subscriber would silently lose events with no point from which to re-read.

## Shared Context

### Identifiers, timestamps and hashes (2C domain, *Conventions*)

```rust
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// RFC 3339, UTC, millisecond precision, `Z` suffix; refused otherwise with TS_NOT_RFC3339_MS_UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub DateTime<Utc>);
// Serialize: self.0.to_rfc3339_opts(SecondsFormat::Millis, true)

// Every …Id is `pub struct XId(pub Uuid)`, serde(transparent), UUID version 7 (ID_NOT_UUIDV7).
// Used here: RunId, CrewRunId, WorkspaceId, AgentInstanceId, PlaceId, ArtefactId, ToolRequestId,
// ApprovalId, ExternalActionKey, ReportId, DashboardId, RunStepId, StartupReportId, AgentReleaseId,
// TaskId, WorkflowId, RemoteAgentId.

/// SHA-256 as 64 lowercase hexadecimal characters (HASH_NOT_SHA256_HEX).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String);

/// A base-10 decimal number written as text, `^-?[0-9]{1,28}(\.[0-9]{1,10})?$` (NUM_NOT_DECIMAL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecimalString(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentRef {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InputUsed { pub name: String, pub location: String, pub version_read: String }
// name 1–120, location 1–2048, version_read 1–256 characters (INPUT_NAME_INVALID, INPUT_LOCATION_INVALID, INPUT_VERSION_INVALID)
```

Storage convention (2C domain): field names are column names unchanged; a `Vec`, struct or data-carrying enum field is one JSON `TEXT` column of the same name (LD-01); every 2C table has exactly one writing component.

### Entities this component owns (2C domain, copied verbatim)

```rust
// ---- Artefact (2C domain "Artefact"; owner: Journal and Artefact Store; 02:677; 04:202–204)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Artefact {
    pub id: ArtefactId,
    pub location: ArtefactLocation,
    pub path: String,
    pub version: u32,
    pub hash: Sha256Hex,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producing_run_id: Option<RunId>,
    pub media_type: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtefactLocation {
    AgentFolder { agent_instance_id: AgentInstanceId },
    Place { place_id: PlaceId },
}

// ---- StartupReport (2C domain; held by the Journal pending bubble-up; 04:253–255)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartupReport {
    pub id: StartupReportId,
    pub subject: StartupSubject,
    pub enforced_rules: Vec<String>,
    pub rules_not_enforced: Vec<RuleNotEnforced>,
    pub unavailable_parts: Vec<String>,
    pub available_tools: Vec<String>,
    pub adapter_verbs_not_offered: Vec<String>,
    pub generated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartupSubject {
    Run { run_id: RunId },
    CrewRun { crew_run_id: CrewRunId },
    AgentRelease { agent_release_id: AgentReleaseId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RuleNotEnforced { pub rule: String, pub reason: String }

// ---- ToolRequest (2C domain; owner: Journal; decided by the Rules Engine; 04:190–192)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolRequest {
    pub id: ToolRequestId,
    pub run_id: RunId,
    pub tool: String,
    pub arguments_digest: Sha256Hex,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<ToolRequestDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deciding_rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    pub state: ToolRequestState,
    pub requested_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Renamed from `ToolDecision` (2C, decision 80: C-2).
pub enum ToolRequestDecision { Allow, Deny, Ask }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRequestState { Requested, Decided, Executed, Refused }

// ---- ExternalAction (2C domain; owner: Journal; 04:198–200; G8)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExternalAction {
    pub action_key: ExternalActionKey,
    /// Absent for an action the person started outside any run (a push, a handover, a post
    /// from a screen); 2C, decision 80: C-7.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,
    pub destination: String,
    pub state: ExternalActionState,
    pub sent_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind {
    SlackPost, ConnectionWrite, WrightrunRunStart, EmailSend, OriginPush, CopyPublish, TelegramPost,
    PullRequestOpened, HandoverWritten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionState { Sent, Confirmed, UnknownOutcome }

// ---- Report and Dashboard (2C domain; owner: Journal; 04:239–242; ADR-067; A-34)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Report {
    pub id: ReportId,
    pub workspace_id: WorkspaceId,
    pub run_id: RunId,
    pub title: String,
    pub as_of: Timestamp,
    pub parts: Vec<ReportSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportSection { pub heading: String, pub data: ReportData, pub sources: Vec<ReportSource> }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportData {
    Table { columns: Vec<ReportTableColumn>, rows: Vec<Vec<CellValue>> },
    Chart { chart_kind: ChartKind, columns: Vec<ReportTableColumn>, rows: Vec<Vec<CellValue>>,
            x_column: String, series_columns: Vec<String>, x_title: String, y_title: String,
            /// For bar charts only (2C, decision 80: C-23).
            #[serde(skip_serializing_if = "Option::is_none")]
            bar_arrangement: Option<BarArrangement> },
    KeyFigures { figures: Vec<ReportKeyFigure> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportTableColumn {
    pub name: String,
    pub column_type: ColumnType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Decimal places shown, for `number` only (2C, decision 80: C-23).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnType { Text, Number, Money, Percentage, Date, DocumentLink }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellValue { Text(String), Number(DecimalString), Null }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind { BarVertical, BarHorizontal, Line, Area, Scatter, Pie }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarArrangement { Grouped, Stacked }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportKeyFigure {
    pub label: String,
    pub value: DecimalString,
    pub unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_against: Option<ReportFigureChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportFigureChange { pub earlier_label: String, pub earlier_value: DecimalString }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportSource {
    Document { location: String, version_read: String },
    CompanySystem { system: String, read_at: Timestamp },
    EarlierResult { run_id: RunId },
    WorkedOutBy { agent: AgentRef, from: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Dashboard {
    pub id: DashboardId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub made_by: DashboardSource,
    pub inputs: Vec<InputUsed>,
    /// Each refresh's run, oldest first; the last one's report is shown.
    pub refresh_runs: Vec<RunId>,
    pub kept_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DashboardSource {
    Task { task_id: TaskId },
    Workflow { workflow_id: WorkflowId, version: u32 },
}

// ---- RunStep (2C domain; owner: Journal; 04:244–247; ADR-068)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunStep {
    pub id: RunStepId,
    pub run_id: RunId,
    pub plan_version: u32,
    pub position: u32,
    pub line: String,
    pub came_from: StepSource,
    pub mark: StepMark,
    pub marks_set: Vec<MarkSet>,
    /// Journal sequence numbers of the events seen while `now` (LD-35).
    pub events_seen: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepSource { Workflow, CrewPlan, AgentsPlan, HarnessTodoList, AddedByOrdin }

// `StepMark { Next, Now, Done, Skipped }` is defined once in 2C wire (`ordin_types::wire::run`)
// and re-exported by the domain module (decision 80: C-1, C-17).
pub use ordin_types::wire::run::StepMark;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MarkSet { pub mark: StepMark, pub at: Timestamp }

// ---- RunState (2C domain "Run"; owner Session Supervisor). The Journal reads it only from the
// `data.state` field of `run_state_changed` events, to keep its own run index (step 6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued, Running, Paused,
    Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply,
}
```

Validation rules of these entities, from 2C, enforced by this component:

| Entity · field | Rule | Error code |
|---|---|---|
| Artefact.path | relative to its folder, 1–4096 bytes, no `..` segment, no NUL | `ART_PATH_INVALID` |
| Artefact.media_type | RFC 6838 `type/subtype`, ≤ 255 chars | `ART_MEDIA_TYPE_INVALID` |
| Artefact deletion in a person's place | moved to the system's bin by its owner, never deleted by the Journal; the record stays (04:204) | `ART_DELETE_IN_PLACE` |
| StartupReport.subject | one report per subject, unless replaced on a resumed start | `START_DUPLICATE` |
| StartupReport lists | ≤ 500 items, each ≤ 500 chars (LD-42) | `START_LIST_TOO_LONG` |
| RuleNotEnforced.reason | 1–500 chars | `START_REASON_MISSING` |
| ToolRequest.tool | 1–200 chars (LD-43) | `TREQ_NAME_INVALID` |
| ToolRequest.decision, deciding_rule | present from `decided` on | `TREQ_UNDECIDED` |
| ToolRequest.approval_id | present only with `decision = ask` | `TREQ_APPROVAL_MISMATCH` |
| ToolRequest row | immutable after `executed` or `refused` | `TREQ_IMMUTABLE` |
| ExternalAction.action_key | unique; written before the action leaves ordin | `EXT_KEY_DUPLICATE` |
| ExternalAction.destination | 1–2048 chars (LD-45) | `EXT_DESTINATION_INVALID` |
| ExternalAction resend | never resent automatically (04:200) | `EXT_AUTO_RESEND` |
| Report.parts | 1–12 | `REPORT_PARTS_OUT_OF_RANGE` |
| Report serialised | ≤ 2 MB (2,000,000 bytes) | `REPORT_TOO_LARGE` |
| Table / chart data | ≤ 10,000 rows, ≤ 30 columns; chart ≤ 8 series, ≤ 5,000 points; pie ≤ 6 slices | `REPORT_TABLE_TOO_LARGE`, `REPORT_CHART_TOO_LARGE`, `REPORT_PIE_TOO_MANY_SLICES` |
| KeyFigures | 1–8 figures | `REPORT_FIGURES_OUT_OF_RANGE` |
| ReportSection.sources | ≥ 1 | `REPORT_PART_NO_SOURCE` |
| ReportTableColumn.currency | present if and only if `money`; ISO 4217 `[A-Z]{3}` | `REPORT_CURRENCY_MISMATCH` |
| Report.title, ReportSection.heading | 1–200 chars | `REPORT_TITLE_INVALID` |
| Dashboard.name | 1–80 chars; unique in its workspace | `REPORT_DASHBOARD_NAME_DUPLICATE` |
| RunStep.line | 1–80 chars | `RSTEP_LINE_TOO_LONG` |
| steps per (run_id, plan_version) | 3–7 | `RSTEP_PLAN_SIZE_OUT_OF_RANGE` |
| plan_version | consecutive from 1 | `RSTEP_PLAN_VERSION_GAP` |
| RunStep row | immutable once its run has ended | `RSTEP_IMMUTABLE` |

The Block Renderer's check (`REPORT_ACTIVE_CONTENT`, `REPORT_LINK_OUTSIDE_WORKSPACE`) runs before a report reaches this component (02:518, MCP Front **report** tool); the Journal does not repeat it.

### Wire types this component's events must match (2C wire §1)

The Journal cannot name `ordin-service-api` types. It stores each event as `kind` plus `data`, where `data` is the JSON object of the `ServiceEvent` variant **without** its `kind` key. The Journal itself emits two kinds and must produce exactly these shapes:

```json
{"run_id":"<RunId>","plan_version":2,"steps":[ <RunStep>, … ]}          // kind "plan_changed"
{"run_id":"<RunId>","plan_version":2,"position":3,"mark":"done"}           // kind "step_marked" (position u8, 1–7)
```

It reads one kind it does not emit: `run_state_changed`, `{"run_id": "<RunId>", "state": "<RunState>"}` (emitted by the Session Supervisor).

The fixed event kinds of Service API version 1.0 (2C wire *Names fixed by this spec*, item 7): `run_state_changed`, `run_progress`, `plan_changed`, `step_marked`, `start_up_report_ready`, `approval_requested`, `approval_decided`, `crew_round_changed`, `block_shown`, `reset`. `reset` is never stored; the Remote API sends it on `JRNL_RESUME_TOO_OLD`. Event `data` is at most 256 KiB serialised (LW-6).

### Configuration and constants

| Item | Value | Source |
|---|---|---|
| SQLite file | `<application data root>/journal.db` *(chosen here)* | ADR-010; 2D-1a `ORDIN_DATA_DIR` (server); the desktop's root is Tauri's per-user app data folder (ADR-072) |
| Backup copy before migrating | `<application data root>/backups/journal-<YYYYMMDDTHHMMSSZ>.db`, newest 2 kept *(chosen here)* | 07 — *Migrations*; SA-XC-07 |
| PostgreSQL connection string | read by the **program** from the file `ORDIN_DATABASE_URL_FILE` names and passed in; never logged | 2D-1a |
| History retention | 90 days default; whole days ≥ 1; the person's setting (Settings › History; server: setup command) | A-08; 2D-3; UX-A58 |
| Company maximum | `history.max_days` of the organisation profile, absent by default; passed in by the program (this crate has no Organisation Profile edge) | 2D-2; 1D *Configuration pass* item 2 |
| Journal size before pruning | 100,000 runs | 2D-4; 07 — *Scale* |
| Prune schedule | desktop and personal server: at start and every 24 hours *(chosen here)*; agent profile: off — the Retention and Erasure component calls `prune` | A-08; 02 Retention and Erasure |
| Compaction | ordin server: weekly, 04:00 server time, when no run is going (the program's Always-on scheduler calls `compact`) | ADR-076; 2D-4 |
| Page size | 100; cursor opaque base64, ≤ 1,024 characters | SA-PAGE-01 |
| Event data size | ≤ 262,144 bytes | LW-6 |
| Sink queue | 1,024 events per subscriber *(chosen here)* | — |

The crate reads **no environment variable**. Every path and connection string is passed in by the program.

### Errors registry form (SA-ENV-01, SA-ERR-01)

Codes are `SCREAMING_SNAKE_CASE`; this component's own prefix is `JRNL_` *(chosen here)*; it also raises the 2C entity codes listed above (`ART_`, `START_`, `TREQ_`, `EXT_`, `REPORT_`, `RSTEP_`) and `SRV_BAD_CURSOR` (SA-PAGE-01). The Service API reply shape is `{"error": "<sentence>", "code": "<CODE>", "action": "try_again" | "sign_in_again" | "send_to_it" | null}`; the person never sees a code.

### Reconciliation assumptions

- **SA-RECON-01** The migration contract is the tuple form `MigrationStep = (i64, &str, &str)` in `ModuleMigrations`; the write contract is `append_entry` and `append_event`; every consumer converts (R3).
- **SA-RECON-02** The Journal adds the reads its consumers need — run index, external action by key and by kind, tool request by id, per-run counts and deletes, diagnostics lines and support export — and nothing that belongs to another module's tables (R3, R6).
- **SA-RECON-03** Every table owner after phase 1 depends on the Journal and receives its `Datastore`; Organisation Profile and Identity and Credentials are handed the SQLite pool by the program (D4, R20).
- **SA-RECON-04** An external action may have no run (2C C-7); tool-request codes take the `TREQ_` prefix (D8).
- **SA-RECON-05** The Rules Engine is the only writer of tool-request rows; the MCP Front and Hook Relay report outcomes to it (row 21).
- **SA-RECON-06** Start-up reports are kept only by the Journal; the Session Supervisor stores none of its own (row 42).
- **SA-CLOSE-01** *(K6)* `dashboard(id)` reads one dashboard for the Ordin Host's refresh of a kept dashboard; absent → `Ok(None)`, never an error.
- **SA-CLOSE-02** *(K17)* The Connection Binder is not an external-action writer; the agent profile's outbound and HTTP writes are recorded through the Rules Engine's tool-request rows.
- **SA-CLOSE-03** *(K7)* The Telegram Adapter and the Slack Channel Adapter depend on the Journal (2B-3) and register their `migrations()` like every other table owner.
- **SA-RECON-07** The Journal records no metrics and exports no spans (row 64).

## Public Interface

All items are in crate `ordin_journal`. Every name below not found in 2C or the HLD is *chosen here*.

### Datastore

```rust
/// The program's one datastore. Cloning is cheap (pools are reference-counted).
#[derive(Clone)]
pub enum Datastore {
    Sqlite(sqlx::SqlitePool),
    Postgres(sqlx::PgPool),
}

pub enum DatastoreConfig {
    /// Desktop, personal server, agent single shape.
    Sqlite { path: std::path::PathBuf, backup_dir: std::path::PathBuf },
    /// Agent scaled shape.
    Postgres { url: DatabaseUrl, max_connections: u32 },
}

/// A PostgreSQL connection string. `Debug` and `Display` print `DatabaseUrl(<redacted>)`.
pub struct DatabaseUrl(String);
impl DatabaseUrl { pub fn new(s: String) -> Self; }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine { Sqlite, Postgres }

impl Datastore {
    pub async fn open(config: DatastoreConfig) -> Result<Datastore, JournalError>;
    pub fn engine(&self) -> Engine;
    /// The pool for modules that own SQLite tables (2B-1 item 5).
    pub fn sqlite(&self) -> Option<&sqlx::SqlitePool>;
    pub fn postgres(&self) -> Option<&sqlx::PgPool>;
    /// Closes every connection; called last at shutdown (SA-XC-13).
    pub async fn close(&self);
}
```

### Migration registry

```rust
/// (version, description, SQL). Versions start at 1 and increase by exactly 1 within a module.
pub type MigrationStep = (i64, &'static str, &'static str);

pub struct ModuleMigrations {
    /// `[a-z][a-z0-9_]{1,40}`: the crate name without `ordin-`, hyphens as underscores
    /// (for example `journal`, `organisation_profile`, `identity`).
    pub module: &'static str,
    pub sqlite: &'static [MigrationStep],
    /// Engine rule (DatabaseMigrations.md M-2): a module that runs in the agent profile's scaled
    /// shape (2B-3 Programs column includes A) supplies the same versions here as in `sqlite`;
    /// empty for a module that runs only on D or P, or whose data the scaled shape does not share.
    pub postgres: &'static [MigrationStep],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    /// Desktop, personal server, single shape, and the scaled shape's schema command.
    ApplyPending,
    /// Scaled-shape instances: refuse to start when anything is pending (ADR-098 item 5).
    CheckOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub applied: Vec<(String, i64)>,     // (module, version) applied by this call, in order
    pub backup_file: Option<std::path::PathBuf>,
}

pub struct MigrationRegistry { /* private */ }

impl MigrationRegistry {
    pub fn new() -> Self;
    /// Fails with JRNL_MIGRATION_DUPLICATE when the module is already registered or its
    /// versions are not 1, 2, 3, … without gaps.
    pub fn register(&mut self, migrations: ModuleMigrations) -> Result<(), JournalError>;
    pub async fn apply(&self, store: &Datastore, mode: ApplyMode) -> Result<MigrationReport, JournalError>;
}

/// This crate's own migrations; the program registers them first.
pub const MIGRATIONS: ModuleMigrations;
```

This tuple form is the one migration contract (reconciliation R3). Every table-owning module that depends on the Journal exports `pub fn migrations() -> ModuleMigrations` built from `MigrationStep` tuples with integer versions 1, 2, 3, … — not `sqlx::migrate::Migrator`, not a `Migration` struct, not text version names — and the program calls `register` once per module. The two phase-1 modules with tables and **no** Journal edge (Organisation Profile, Identity and Credentials; reconciliation R20, decision D4) export `pub const MODULE_NAME: &str` and `pub const SQLITE_MIGRATIONS: &[(i64, &str, &str)]`; the program builds a `ModuleMigrations` from them (empty `postgres`) and registers it. The program is handed the same `Datastore` to give to those two modules. Localisation, Brand and Setup Assistant depend on the Journal (D4) and use the first form.

**Called by:** `MigrationRegistry::new`, `register`, `apply` and `MIGRATIONS` — the desktop program and the server program only. `register` receives the `migrations()` value of every table-owning crate (Approval Service, Session Supervisor, Crew Orchestrator, Ordin Host, Usage Planner, Rules Engine, Memory Service, Mail Service, Telegram Service, Knowledge Service, Checker, Hook Relay, File Service, SharePoint Adapter, Confluence Adapter, Workspace Service, Home and Search, Craft Toolkit, Project Service, Push Service, Package Catalogue, Catalogue Service, Authoring Service, Handover Service, Run Queue, Settings Store, Duty Timer, Job Runner, Retention and Erasure, Agent Folder Manager, Server Connector, Remote API, Channel Gateway, Trigger Service, Slack Channel Adapter, Telegram Adapter, Localisation, Brand, Setup Assistant; *amended January 2026, ADR-113:* Knowledge Query Client, Knowledge Query Server, Knowledge Reader) and the `MODULE_NAME`/`SQLITE_MIGRATIONS` pair of Organisation Profile and Identity and Credentials. A module whose crate has no tables in the scaled shape passes an empty `postgres` slice. A module that tried to register a second time receives `JRNL_MIGRATION_DUPLICATE`. The server program's own record `ServerProfile` (2C owner: the ordin server) is not a Journal table: the server program registers its own `ModuleMigrations` for it, under module name `server` *(chosen here)*.

### Journal service

`Journal` is a concrete type, cheap to clone; consumers hold `ordin_journal::Journal` (or `Arc<Journal>`), never a trait object of their own (reconciliation row 41). A consumer's unit tests open a `Datastore` on a temporary SQLite file, apply the migrations and call `Journal::start` — the Journal publishes no test-double trait. The write vocabulary is `append_entry` (records) and `append_event` (events) only: there is no `record`, `emit`, `JournalWriter`, `ExternalActionLog`, `JournalReader` or `ArtefactStore` in this crate (R3).

```rust
use ordin_types::{Page, Cursor, CodedError, RepairAction};

#[derive(Clone)]
pub struct Journal { /* Arc<Inner> */ }

pub struct JournalOptions {
    /// `true` on the desktop and the personal server; `false` in the agent profile.
    pub automatic_pruning: bool,
    /// 24 hours unless a test shortens it.
    pub prune_interval: std::time::Duration,
    pub clock: std::sync::Arc<dyn Clock>,
}

pub trait Clock: Send + Sync { fn now(&self) -> Timestamp; }
pub struct SystemClock;
impl Clock for SystemClock { fn now(&self) -> Timestamp; }

impl Journal {
    /// After `MigrationRegistry::apply`. Reads or creates the installation's backend identifier,
    /// starts the delivery tasks and, when `automatic_pruning`, the prune timer.
    pub async fn start(store: Datastore, options: JournalOptions) -> Result<Journal, JournalError>;
    pub fn datastore(&self) -> &Datastore;
    /// The installation's backend identifier, minted once (UUIDv7). The program wraps it as
    /// the Service API's `BackendId` (`BackendId(journal.backend_uuid())`).
    pub fn backend_uuid(&self) -> uuid::Uuid;
    /// Stops the timer and delivery tasks after delivering what is queued (at most 2 s).
    /// There is no separate `flush` or `close` on the Journal: the program calls `shutdown`,
    /// then `Datastore::close`.
    pub async fn shutdown(&self);
    /// `true` when one `SELECT 1` on the datastore answers within `within`; never an error
    /// (step 16). For readiness and watchdog checks.
    pub async fn ping(&self, within: std::time::Duration) -> bool;

    // ---- events (EventSink hub; ADR-100)
    /// `kind` is `EventKind::as_str()` (2C wire); `data` is the `ServiceEvent` variant's
    /// fields without `kind`. There is no `emit(ServiceEvent)`: this crate cannot name it.
    pub async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>;
    pub async fn events_since(&self, after_seq: u64, limit: u32) -> Result<Vec<JournalEvent>, JournalError>;
    /// The highest sequence number appended so far (0 before the first event).
    pub fn last_seq(&self) -> u64;
    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId;
    pub fn unsubscribe(&self, id: SubscriptionId);

    // ---- internal journal entries (Diagnostics, help-request copies, setup results, repairs)
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    pub async fn list_entries(&self, filter: EntryFilter, cursor: Option<Cursor>) -> Result<Page<JournalEntry>, JournalError>;
    /// Newest first; the entries as Diagnostics lines (step 7.3).
    pub async fn diagnostic_lines(&self, cursor: Option<Cursor>) -> Result<Page<DiagnosticEntry>, JournalError>;
    /// The support bundle for at most 20 runs, as JSON Lines bytes; the program writes it
    /// where the person chose (step 7.4).
    pub async fn support_export(&self, run_ids: &[RunId]) -> Result<SupportBundle, JournalError>;

    // ---- artefacts (02:677 "the artefact index")
    /// Indexes content that the caller has already written; never writes or reads bytes.
    pub async fn record_artefact(&self, new: NewArtefact) -> Result<Artefact, JournalError>;
    pub async fn artefact_versions(&self, location: ArtefactLocation, path: &str) -> Result<Vec<Artefact>, JournalError>;
    pub async fn list_artefacts(&self, filter: ArtefactFilter, cursor: Option<Cursor>) -> Result<Page<Artefact>, JournalError>;

    // ---- tool requests (02:674)
    /// The Journal mints the `ToolRequestId`; there is no insert-or-replace.
    pub async fn record_tool_request(&self, run_id: RunId, tool: &str, arguments_digest: Sha256Hex) -> Result<ToolRequest, JournalError>;
    pub async fn decide_tool_request(&self, id: ToolRequestId, decision: ToolRequestDecision, deciding_rule: &str, approval_id: Option<ApprovalId>) -> Result<ToolRequest, JournalError>;
    pub async fn finish_tool_request(&self, id: ToolRequestId, executed: bool) -> Result<ToolRequest, JournalError>;
    pub async fn tool_request(&self, id: ToolRequestId) -> Result<Option<ToolRequest>, JournalError>;

    // ---- external actions (02:675; G8)
    pub async fn begin_external_action(&self, new: NewExternalAction) -> Result<ExternalAction, JournalError>;
    /// Sets `confirmed_at` from the Journal's clock; the caller passes no time.
    pub async fn confirm_external_action(&self, key: ExternalActionKey) -> Result<ExternalAction, JournalError>;
    /// The person's "It arrived — mark as done" on an unknown outcome (03:441).
    pub async fn mark_external_action_arrived(&self, key: ExternalActionKey) -> Result<ExternalAction, JournalError>;
    /// One action by its key; `None` when no row has it (step 9.5).
    pub async fn external_action(&self, key: ExternalActionKey) -> Result<Option<ExternalAction>, JournalError>;
    pub async fn external_actions_for_run(&self, run_id: RunId) -> Result<Vec<ExternalAction>, JournalError>;
    /// Every action of the given kinds still `sent` or `unknown_outcome`, oldest first,
    /// at most 1,000 (step 9.6).
    pub async fn unsettled_external_actions(&self, kinds: &[ExternalActionKind]) -> Result<Vec<ExternalAction>, JournalError>;
    pub async fn list_external_actions(&self, state: Option<ExternalActionState>, cursor: Option<Cursor>) -> Result<Page<ExternalAction>, JournalError>;
    /// Called once at start, before any session resumes (step 9).
    pub async fn recover_external_actions(&self) -> Result<u32, JournalError>;

    // ---- start-up reports (2C: the Journal holds them; no other table keeps them)
    pub async fn store_startup_report(&self, report: StartupReport, replace: bool) -> Result<(), JournalError>;
    pub async fn startup_report(&self, subject: StartupSubject) -> Result<Option<StartupReport>, JournalError>;

    // ---- reports and dashboards (ADR-067)
    /// The caller builds the 2C `Report`, including its `ReportId`.
    pub async fn store_report(&self, report: Report) -> Result<(), JournalError>;
    pub async fn report(&self, id: ReportId) -> Result<Report, JournalError>;
    pub async fn report_for_run(&self, run_id: RunId) -> Result<Option<Report>, JournalError>;
    pub async fn list_reports(&self, workspace_id: WorkspaceId, cursor: Option<Cursor>) -> Result<Page<ReportSummary>, JournalError>;
    pub async fn keep_dashboard(&self, new: NewDashboard) -> Result<Dashboard, JournalError>;
    pub async fn add_dashboard_refresh(&self, id: DashboardId, run_id: RunId) -> Result<Dashboard, JournalError>;
    pub async fn remove_dashboard(&self, id: DashboardId) -> Result<(), JournalError>;
    pub async fn list_dashboards(&self, workspace_id: WorkspaceId, cursor: Option<Cursor>) -> Result<Page<Dashboard>, JournalError>;
    /// One dashboard by id; `Ok(None)` when absent (SA-CLOSE-01, K6).
    pub async fn dashboard(&self, id: DashboardId) -> Result<Option<Dashboard>, JournalError>;
    /// Removes reports and dashboards of a removed workspace (04 Data Lifecycle).
    pub async fn forget_workspace(&self, workspace_id: WorkspaceId) -> Result<u64, JournalError>;

    // ---- plan and step marks (ADR-068)
    /// Emits `plan_changed`. The new plan version is `RunStep.plan_version` of the result.
    pub async fn state_plan(&self, run_id: RunId, steps: Vec<PlannedStep>) -> Result<Vec<RunStep>, JournalError>;
    /// Acts on the highest plan version; emits `step_marked`.
    pub async fn mark_step(&self, run_id: RunId, position: u32, mark: StepMark) -> Result<RunStep, JournalError>;
    /// The highest plan version's steps in position order; empty when no plan was stated.
    pub async fn current_plan(&self, run_id: RunId) -> Result<Vec<RunStep>, JournalError>;

    // ---- run index (read side, step 6.3)
    /// Runs still in progress, from the Journal's own run index.
    pub async fn runs_in_progress(&self) -> Result<Vec<RunId>, JournalError>;
    /// The same runs with their state and first-seen time, newest first, at most 500.
    pub async fn running_runs(&self) -> Result<Vec<RunIndexEntry>, JournalError>;
    /// Runs that ended at or after `since`, newest first, `limit` clamped to 1–500.
    pub async fn runs_ended_since(&self, since: Timestamp, limit: u32) -> Result<Vec<RunIndexEntry>, JournalError>;

    // ---- retention (A-08, UX-A58)
    pub async fn retention(&self) -> Result<RetentionSetting, JournalError>;
    pub async fn set_retention_days(&self, days: u32) -> Result<RetentionSetting, JournalError>;
    /// The program calls this at start and whenever the active profile changes.
    pub async fn set_company_max_days(&self, max_days: Option<u32>) -> Result<RetentionSetting, JournalError>;
    pub fn register_prune_participant(&self, participant: std::sync::Arc<dyn PruneParticipant>);
    pub async fn prune(&self, cutoff_override: Option<Timestamp>) -> Result<PruneSummary, JournalError>;
    /// Rows of this crate's tables linked to any of `run_ids` (step 14.4); nothing is removed.
    pub async fn count_for_runs(&self, run_ids: &[RunId]) -> Result<u64, JournalError>;
    /// Removes every row of this crate's tables linked to any of `run_ids`, at most 500 ids
    /// per call (step 14.4). Returns the number of rows removed.
    pub async fn delete_for_runs(&self, run_ids: &[RunId]) -> Result<u64, JournalError>;
    /// SQLite: `VACUUM` then `PRAGMA optimize`. PostgreSQL: `VACUUM (ANALYZE)` on the journal's tables.
    pub async fn compact(&self) -> Result<(), JournalError>;
}
```

#### Who calls each public item (reconciliation §3; R3, R6)

| Item | Called by |
|---|---|
| `Datastore::open`, `Datastore::close`, `Journal::start`, `shutdown` | desktop program, server program |
| `backend_uuid` | desktop program, server program, Remote API |
| `Datastore::sqlite`, `Datastore::postgres`, `Journal::datastore` | every table-owning module that depends on the Journal (list under *Migration registry*); the program hands the same `Datastore` to Organisation Profile and Identity and Credentials (R20) |
| `ping` | server program (readiness `ready()` and the systemd watchdog) |
| `append_event` | Workspace Service (`workspace_changed`), Session Supervisor (`run_state_changed`, `run_progress`, `start_up_report_ready`), Approval Service (`approval_requested`, `approval_decided`, `needs_you_changed`), Crew Orchestrator (`crew_round_changed`, `needs_you_changed`), MCP Front (`run_progress`, `block_shown`), Ordin Host (`block_shown`, `needs_you_changed`, `workspace_changed`), Push Service (`needs_you_changed`), Handover Service (`needs_you_changed`), Server Connector (`server_state_changed`), the desktop and server programs (forwarding `locale_changed` from Localisation, `brand_changed` from Brand, `setup_progress` from the Setup Assistant, `sign_in_changed` from Identity and Credentials); this crate itself (`plan_changed`, `step_marked`) |
| `events_since` | Remote API (resume after `Last-Event-ID`), Command Bridge, Workspace Service (catching up on approval decisions after a restart), and the sinks that re-read after `lagged`: Confluence Adapter, Push Service, Channel Gateway, Trigger Service |
| `last_seq` | Remote API, Command Bridge (the desktop's head sequence; there is no `head_seq`), Workspace Service, desktop and server programs (`service.hello`) |
| `datastore` | every table owner after phase 1, the Workspace Service, the Telegram Adapter and the Slack Channel Adapter included (K7) |
| `subscribe`, `unsubscribe` | desktop and server programs, registering the `event_sink()` of the Approval Service, Confluence Adapter, Push Service, Handover Service, Session Supervisor and Hook Relay, and the Workspace Service's sink; each of the Command Bridge, Remote API, Channel Gateway, Mail Service, Telegram Service, Sandbox and Trigger Service, which registers its own sink from its `start` |
| `append_entry` | Memory Service, Mail Service, Telegram Service, Knowledge Service, Sandbox, Agent Folder Manager, File Service, SharePoint Adapter, Confluence Adapter, Workspace Service, Package Catalogue, Authoring Service, Handover Service, Session Supervisor, Crew Orchestrator, Hook Relay, Rules Engine, Project Service, Craft Toolkit, Trigger Service, Channel Gateway (`chat_messages_ignored`), Usage Planner, Remote API (security entries); the programs (help-request copies, setup check results, `profile_activated`) |
| `list_entries`, `diagnostic_lines`, `support_export` | the programs, for `journal.list_entries`, `diagnostics.lines`, `diagnostics.support_export` |
| `record_artefact` | Sandbox, Ordin Host — each after writing the bytes itself |
| `artefact_versions` | the programs, for `artefacts.versions` |
| `list_artefacts` | the programs, for `artefacts.list`; Ordin Host (rehearsal evidence) |
| `record_tool_request`, `decide_tool_request`, `finish_tool_request`, `tool_request` | Rules Engine only (`decide_tool_call` records and decides; its `record_outcome` finishes). The MCP Front and the Hook Relay report a call's outcome to the Rules Engine, not to the Journal, so the row has one writer |
| `begin_external_action`, `confirm_external_action` | Confluence Adapter, SharePoint Adapter, Mail Service, Telegram Service, Push Service, Handover Service. The Connection Binder is not a writer: it has no Journal edge, and its outbound and HTTP writes are recorded as tool requests by the Rules Engine (SA-CLOSE-02, K17) |
| `external_action` | Mail Service, Telegram Service, Push Service (crash reconciliation by key) |
| `unsettled_external_actions` | Push Service, Handover Service, SharePoint Adapter (resume after a restart) |
| `external_actions_for_run` | the programs (`journal.run_external_actions`), Ordin Host (rehearsal evidence), Session Supervisor (the run summary's external actions at run end) |
| `list_external_actions`, `mark_external_action_arrived` | the programs (`journal.list_external_actions`, `journal.mark_arrived`); Home and Search's waiting-items composite reads `list_external_actions(Some(UnknownOutcome), …)` |
| `recover_external_actions`, `set_company_max_days` | desktop and server programs, at start (and `set_company_max_days` on each organisation-profile change) |
| `store_startup_report`, `startup_report` | Session Supervisor (runs), Crew Orchestrator (crew runs), the server program's `LoaderStore` for the Agent Loader (agent releases) |
| `store_report` | MCP Front (**report** tool) |
| `report`, `report_for_run`, `list_reports`, `keep_dashboard`, `remove_dashboard` | the programs, for the `reports.*` and `dashboards.*` commands |
| `add_dashboard_refresh`, `list_dashboards` | the programs, for the `dashboards.*` commands; Ordin Host (X-24) |
| `dashboard` | Ordin Host (refreshing one dashboard, K6) |
| `forget_workspace` | Workspace Service (workspace removal) |
| `state_plan` | MCP Front (`StepSource::AgentsPlan`), Session Supervisor (`StepSource::HarnessTodoList`, replacing its `run_harness_plan` journal entry) |
| `mark_step`, `current_plan` | MCP Front; the programs for `plans.get` |
| `runs_in_progress`, `running_runs`, `runs_ended_since` | Home and Search through the program's `JournalReader` composite (running and finished runs; titles come from the Session Supervisor); server program (`compact` only when `runs_in_progress` is empty) |
| `retention`, `set_retention_days` | the programs, for `history.get`, `history.set_days` |
| `register_prune_participant` | desktop and server programs, once per `PruneParticipant` (Session Supervisor, Approval Service, Crew Orchestrator, Ordin Host, Usage Planner, File Service, Mail Service, Authoring Service, Remote API) |
| `prune` | this crate's timer (desktop, personal server); Retention and Erasure in the agent profile with `Some(cutoff)` |
| `delete_for_runs` | Retention and Erasure (age stage `runs` and erasure of a person or a scope) |
| `count_for_runs` | Retention and Erasure (agent profile; imported beside `delete_for_runs` in its Shared Context, SA-RECON-04) |
| `compact` | server program (weekly, ADR-076) |

### The `EventSink` hub trait and the prune participant trait

```rust
/// Implemented by the Command Bridge, the Remote API, the Channel Gateway (ADR-100 item 1) and
/// by every module that reacts to events (Approval Service, Confluence Adapter, Mail Service,
/// Telegram Service, Sandbox, Workspace Service, Push Service, Handover Service, Session
/// Supervisor, Hook Relay, Trigger Service); registered with `Journal::subscribe` by the program
/// or by the module itself (see *Who calls each public item*). A sink decodes `JournalEvent.kind` with
/// `EventKind::parse` and `data` itself; there is no broadcast receiver and no `EventEnvelope`
/// in this crate (reconciliation row 15).
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    /// `[a-z_]{1,40}`, used in logs only.
    fn name(&self) -> &'static str;
    /// Called once per event, in `seq` order, never concurrently for one sink.
    async fn deliver(&self, event: &JournalEvent);
    /// The sink's queue overflowed; events from `next_seq` on were not delivered. The sink
    /// re-reads them with `Journal::events_since(next_seq - 1, …)`. Delivery resumes after this call.
    async fn lagged(&self, next_seq: u64);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JournalEvent {
    /// The installation's backend identifier (`Journal::backend_uuid`).
    pub backend: uuid::Uuid,
    /// Strictly increasing per backend, starting at 1, contiguous, never reused.
    pub seq: u64,
    pub at: Timestamp,
    /// `^[a-z][a-z0-9_]{0,63}$`.
    pub kind: String,
    /// The `ServiceEvent` variant's fields, without `kind`.
    pub data: serde_json::Value,
}

/// Implemented by modules that keep history in their own tables and depend on the Journal
/// (Session Supervisor, Approval Service, Crew Orchestrator, Ordin Host, Usage Planner,
/// File Service, Mail Service, Authoring Service, Remote API) and registered by the program.
#[async_trait::async_trait]
pub trait PruneParticipant: Send + Sync {
    /// The participant's `ModuleMigrations::module` name.
    fn module(&self) -> &'static str;
    /// Remove this module's history recorded before `request.cutoff`, never touching anything
    /// linked to a run in `request.runs_in_progress`, and never a pending approval (A-08).
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneRequest { pub cutoff: Timestamp, pub runs_in_progress: Vec<RunId> }

/// The participant's own error code (its registry prefix), e.g. `APPR_STORE_UNAVAILABLE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneFailure { pub code: &'static str }
```

### Supporting types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JournalEntryId(pub uuid::Uuid);           // UUIDv7 (SA-ID-01 "journal record")

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewJournalEntry {
    /// `^[a-z][a-z0-9_]{0,63}$`, for example `help_request_sent`, `setup_check_result`, `repair_done`.
    pub kind: String,
    /// The component that wrote it, `[a-z_]{1,40}`.
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")] pub crew_run_id: Option<CrewRunId>,
    #[serde(skip_serializing_if = "Option::is_none")] pub workspace_id: Option<WorkspaceId>,
    /// One plain sentence for Diagnostics, 1–300 characters; never a token or file content.
    pub summary: String,
    /// At most 65,536 bytes serialised.
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JournalEntry {
    pub id: JournalEntryId,
    pub kind: String,
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")] pub crew_run_id: Option<CrewRunId>,
    #[serde(skip_serializing_if = "Option::is_none")] pub workspace_id: Option<WorkspaceId>,
    pub summary: String,
    pub data: serde_json::Value,
    pub recorded_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EntryFilter {
    #[serde(skip_serializing_if = "Option::is_none")] pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")] pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewArtefact {
    pub location: ArtefactLocation,
    pub path: String,
    pub hash: Sha256Hex,
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub producing_run_id: Option<RunId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ArtefactFilter {
    #[serde(skip_serializing_if = "Option::is_none")] pub location: Option<ArtefactLocation>,
    #[serde(skip_serializing_if = "Option::is_none")] pub producing_run_id: Option<RunId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewExternalAction {
    /// Minted by the caller (UUIDv7) before the action leaves ordin.
    pub action_key: ExternalActionKey,
    /// `None` for an action started outside any run (an automatic push, a handover).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportSummary {
    pub id: ReportId, pub run_id: RunId, pub title: String, pub as_of: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewDashboard {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub made_by: DashboardSource,
    pub inputs: Vec<InputUsed>,
    /// The run whose report is kept; its report must exist.
    pub run_id: RunId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlannedStep { pub line: String, pub came_from: StepSource }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RetentionSetting {
    pub days: u32,
    /// The company maximum when the organisation profile sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_max_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pruned_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PruneSummary {
    pub cutoff: Timestamp,
    /// module → rows removed. `journal` holds this crate's own count.
    pub removed: std::collections::BTreeMap<String, u64>,
    /// modules whose participant failed, with its code; their rows stay until the next prune.
    pub failed: std::collections::BTreeMap<String, String>,
}

/// One row of the Journal's run index (`journal_runs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunIndexEntry {
    pub run_id: RunId,
    pub state: RunState,
    pub first_seen_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

/// A journal entry as one Diagnostics line. The program maps it field for field to the
/// Service API's `DiagnosticLine { reference, at, level, code, summary }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagnosticEntry {
    /// The first 8 characters of the entry's `JournalEntryId`, which the person reads out to IT.
    pub reference: String,
    pub at: Timestamp,
    /// `info`, `warning` or `error`.
    pub level: String,
    /// A registry code, or empty when the entry carries none.
    pub code: String,
    pub summary: String,
}

/// The bytes of a support export: JSON Lines, UTF-8, at most 10,000,000 bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SupportBundle {
    /// `ordin-support-<YYYYMMDDTHHMMSSZ>.jsonl` *(chosen here)*.
    pub suggested_file_name: String,
    pub bytes: Vec<u8>,
    /// `true` when the 10,000,000-byte limit cut the newest lines.
    pub truncated: bool,
}

// `Page<T>` and `Cursor` are `ordin_types::Page` and `ordin_types::Cursor` (2C domain, SA-PAGE-01);
// this crate defines no page type of its own.

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("{code}")]
    Invalid { code: &'static str },               // a 2C validation code, permanent
    #[error("JRNL_NOT_FOUND")] NotFound,
    #[error("JRNL_DATASTORE_UNAVAILABLE")] Unavailable,          // retryable
    #[error("JRNL_DATASTORE_OPEN_FAILED")] OpenFailed,
    #[error("JRNL_SCHEMA_NEWER")] SchemaNewer { module: String, found: i64, known: i64 },
    #[error("JRNL_SCHEMA_BEHIND")] SchemaBehind { pending: Vec<(String, i64)> },
    #[error("JRNL_MIGRATION_DUPLICATE")] MigrationDuplicate { module: String },
    #[error("JRNL_MIGRATION_CHANGED")] MigrationChanged { module: String, version: i64 },
    #[error("JRNL_MIGRATION_FAILED")] MigrationFailed { module: String, version: i64 },
    #[error("JRNL_BACKUP_FAILED")] BackupFailed,
    #[error("JRNL_EVENT_KIND_INVALID")] EventKindInvalid,
    #[error("JRNL_EVENT_TOO_LARGE")] EventTooLarge { bytes: usize },
    #[error("JRNL_ENTRY_INVALID")] EntryInvalid,
    #[error("JRNL_RESUME_TOO_OLD")] ResumeTooOld { first_kept_seq: u64 },
    #[error("JRNL_SEQ_AHEAD")] SeqAhead { last_seq: u64 },
    #[error("JRNL_RETENTION_INVALID")] RetentionInvalid,
    #[error("JRNL_RETENTION_ABOVE_COMPANY_MAX")] RetentionAboveCompanyMax { max_days: u32 },
    #[error("JRNL_EXT_STATE_INVALID")] ExternalActionState,
    #[error("JRNL_PLAN_NOT_FOUND")] PlanNotFound,
    #[error("JRNL_EXPORT_INVALID")] ExportInvalid,
    #[error("SRV_BAD_CURSOR")] BadCursor,
}

/// The one error contract of every module error (2C `CodedError`). Consumers match on
/// `code()` (for example `code() == "EXT_KEY_DUPLICATE"`), never on a local error enum of
/// their own.
impl CodedError for JournalError {
    /// The registry code (`JRNL_…`, a 2C code, or `SRV_BAD_CURSOR`).
    fn code(&self) -> &'static str;
    /// `true` only for `Unavailable`.
    fn retryable(&self) -> bool;
    /// `Some(TryAgain)` for `Unavailable` and `BackupFailed`; `Some(SendToIt)` for
    /// `OpenFailed`, `SchemaNewer`, `MigrationChanged` and `MigrationFailed`; `None` otherwise.
    fn repair(&self) -> Option<RepairAction>;
}
```

### Service API commands served from this component *(names chosen here; SA-API-01)*

The program implements `ServiceApi`; for each command it calls the method shown. Every command is `POST /v1/<module>/<verb>` on the Remote API and `<module>_<verb>` on Tauri IPC.

| Command | Input | Result | Method called |
|---|---|---|---|
| `journal.list_entries` | `{ "filter": EntryFilter, "cursor": string? }` | `Page<JournalEntry>` | `list_entries` |
| `journal.list_external_actions` | `{ "state": "unknown_outcome"?, "cursor": string? }` | `Page<ExternalAction>` | `list_external_actions` |
| `journal.mark_arrived` | `{ "action_key": ExternalActionKey }` | `ExternalAction` | `mark_external_action_arrived` |
| `journal.run_external_actions` | `{ "run_id": RunId }` | `{ "items": [ExternalAction] }` | `external_actions_for_run` |
| `diagnostics.lines` | `PageRequest` | `Page<DiagnosticLine>` (each from a `DiagnosticEntry`) | `diagnostic_lines` |
| `diagnostics.support_export` | `SupportExport { run_ids }` | `SavedCopy` | `support_export`; the desktop program writes `bytes` to the path the person chose in the save dialog and returns that path |
| `artefacts.list` | `{ "filter": ArtefactFilter, "cursor": string? }` | `Page<Artefact>` | `list_artefacts` |
| `artefacts.versions` | `{ "location": ArtefactLocation, "path": string }` | `{ "items": [Artefact] }` | `artefact_versions` |
| `reports.get` | `{ "report_id": ReportId }` | `Report` | `report` |
| `reports.list` | `{ "workspace_id": WorkspaceId, "cursor": string? }` | `Page<ReportSummary>` | `list_reports` |
| `dashboards.list` | `{ "workspace_id": WorkspaceId, "cursor": string? }` | `Page<Dashboard>` | `list_dashboards` |
| `dashboards.keep` | `NewDashboard` | `Dashboard` | `keep_dashboard` |
| `dashboards.remove` | `{ "dashboard_id": DashboardId }` | `{}` | `remove_dashboard` (after the person's confirmation in the screens, 04:242) |
| `plans.get` | `{ "run_id": RunId }` | `{ "items": [RunStep] }` | `current_plan` |
| `history.get` | `{}` | `RetentionSetting` | `retention` |
| `history.set_days` | `{ "days": u32 }` | `RetentionSetting` | `set_retention_days` |

These rows are the definitive names, inputs and results; the Service API catalogue takes them as written (R1): `journal.list_entries` takes `EntryFilter` and returns `Page<JournalEntry>` (not `EntryQuery`/`JournalEntryView`); `journal.list_external_actions` takes a state filter and a cursor (the per-run list is `journal.run_external_actions`); `artefacts.versions` takes `{location, path}` and returns `{items}`; `reports.list` returns `Page<ReportSummary>`; `plans.get` returns `{items: [RunStep]}`; `history.get` and `history.set_days` return `RetentionSetting`. `cursor` is the 2C `Cursor` (a string on the wire).

`dashboards.keep` input is assembled by the screens from the run the person chose (**Keep as dashboard**); `made_by` and `inputs` come from that run's task or workflow, which the screens already hold.

#### Example

Appending an event and delivering it:

```rust
let ev = journal.append_event(
    "run_state_changed",
    serde_json::json!({"run_id": "0192a1c2-3344-7556-8677-8899aabbccdd", "state": "running"}),
).await?;
assert_eq!(ev.seq, 48213);
```

The Remote API's sink receives `JournalEvent { backend: 0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b, seq: 48213, at: 2026-01-10T09:41:07.312Z, kind: "run_state_changed", data: {...} }` and writes the SSE frame `id: 48213` / `event: run_state_changed` / `data: {"api_version":{"major":1,"minor":0},"backend":"0192a1b4-…","seq":48213,"at":"2026-01-10T09:41:07.312Z","kind":"run_state_changed","run_id":"0192a1c2-…","state":"running"}`.

A desktop reconnecting with `Last-Event-ID: 40000` after the weekly compaction removed events up to 41,200:

```rust
match journal.events_since(40_000, 500).await {
    Err(JournalError::ResumeTooOld { first_kept_seq: 41_201 }) => { /* Remote API sends one `reset` event */ }
    _ => unreachable!(),
}
```

`history.set_days` over the Remote API:

```
POST /v1/history/set_days      {"days": 120}
200 {"days": 120, "company_max_days": 365, "last_pruned_at": "2026-01-10T04:00:01.004Z"}

POST /v1/history/set_days      {"days": 400}
200 {"kind":"error","error":{"code":"JRNL_RETENTION_ABOVE_COMPANY_MAX","message":"Your company keeps history for at most 365 days.","action":null}}
```

## Internal Logic

### 1. Opening the datastore — `Datastore::open`

1. **SQLite.** Create the parent folder of `path` with mode `0700` on macOS and Linux if absent. Build `SqliteConnectOptions` with `create_if_missing(true)`, `journal_mode(Wal)`, `synchronous(Full)` (a completed step must survive a crash, 07 — *Availability*), `foreign_keys(true)`, `busy_timeout(5 s)`, and a pool of at most 8 connections. On macOS and Linux set the file mode to `0600` after creation. Run `PRAGMA quick_check` once; a result other than `ok` → `JRNL_DATASTORE_OPEN_FAILED` (permanent), logged `journal.open.failed` with `reason = "integrity"`.
2. **PostgreSQL.** Connect with `PgPoolOptions::max_connections(config.max_connections)` (the program passes 10 per instance *(assumed here: wrightrun's per-instance pool)*), `acquire_timeout(10 s)`, TLS through `sqlx`'s rustls feature. A refused connection → `JRNL_DATASTORE_UNAVAILABLE` (retryable); the program retries with backoff 1 s doubling to 30 s for at most 5 minutes, then stops with a named error (2D-1a). The connection string never appears in an error or log; `DatabaseUrl`'s `Debug` prints `<redacted>`.
3. Log `journal.open.done` with `engine`.

### 2. Applying migrations — `MigrationRegistry::apply`

1. `register` keeps modules in registration order. It refuses a second registration of the same `module`, a version list that does not run 1, 2, 3 …, and a `module` name not matching `[a-z][a-z0-9_]{1,40}` (`JRNL_MIGRATION_DUPLICATE`).
2. Ensure the bookkeeping table `schema_migrations` exists (*Data Model*), outside the migration transaction (its `CREATE TABLE IF NOT EXISTS` is idempotent).
3. Read every row of `schema_migrations`. For each row:
   - module not registered → `JRNL_SCHEMA_NEWER` (the database was written by a newer ordin that has a module this program lacks);
   - version greater than the highest registered version of that module → `JRNL_SCHEMA_NEWER`;
   - `checksum` (SHA-256 hex of the step's SQL text for the current engine) differs from the registered step's → `JRNL_MIGRATION_CHANGED`.
   Each refusal is permanent and logged `journal.migrate.refused` with `module`, `version`. The desktop shows "This copy of ordin is older than your saved work. Update ordin to open it." † *(words chosen here; 11 §6 has no row)* with **Send to IT**; the server stops with exit status 78.
4. Compute the pending list: for each registered module in registration order, every step with a version above the module's highest applied version.
5. Pending list empty → return `MigrationReport { applied: [], backup_file: None }`.
6. `mode = CheckOnly` and pending non-empty → `JRNL_SCHEMA_BEHIND` with the list (the scaled-shape instance never reports ready, ADR-098 item 5).
7. **Backup (SQLite only).** `VACUUM INTO '<backup_dir>/journal-<YYYYMMDDTHHMMSSZ>.db'`, which makes a consistent copy while WAL is on. Delete older backups so the newest two remain. A failure (disk full, folder not writable) → `JRNL_BACKUP_FAILED` (permanent); nothing is migrated. PostgreSQL: no copy is made; the operator's database backup applies (SA-XC-07, sql-safety rule: migrations against shared databases are user actions — the schema command is that action).
8. **Apply in one transaction.** SQLite: `BEGIN IMMEDIATE`; PostgreSQL: `BEGIN`, then `SELECT pg_advisory_xact_lock(7243100001)` *(constant chosen here)* so two schema commands never run together. For each pending step, run its SQL with `sqlx::raw_sql` (multi-statement), then insert its `schema_migrations` row (`module`, `version`, `description`, `checksum`, `applied_at`). Any failure → `ROLLBACK`, `JRNL_MIGRATION_FAILED` with module and version, logged `journal.migrate.failed`; the SQLite file is unchanged and the backup copy stays. Commit.
9. Log `journal.migrate.applied` once per step with `module`, `version`; return the report.

Rollback consideration: migrations are forward-only. A broken release is undone by restoring the backup copy of step 7 with the previous program (SA-XC-07). Within one minor line a PostgreSQL migration only adds (ADR-098 item 5), so the previous image still reads the database.

### 3. Starting the Journal — `Journal::start`

1. Read `journal_meta` (id = 1). Absent → insert it with `backend_id` = a new UUIDv7, `last_seq = 0`, `retention_days = 90`, `created_at = now` (first start). Keep `backend_id` and `last_seq` in memory.
2. Rebuild nothing else in memory: the run index (step 6.3) is a table.
3. Start one delivery task per subscriber as subscribers register (step 5).
4. PostgreSQL: start the cross-instance poller (step 5.5).
5. `automatic_pruning` → schedule `prune(None)` 60 seconds after start *(chosen here: start-up work first)* and then every `prune_interval`.
6. Log `journal.started` with `engine`, `last_seq`.

### 4. Appending an event — `append_event`

1. Validate `kind` against `^[a-z][a-z0-9_]{0,63}$` → else `JRNL_EVENT_KIND_INVALID`; reject `reset` (never stored) with the same code. Serialise `data`; more than 262,144 bytes → `JRNL_EVENT_TOO_LARGE`; `data` not a JSON object → `JRNL_EVENT_KIND_INVALID`.
2. Take the writer lock: SQLite — an in-process `tokio::sync::Mutex<()>` held for the transaction (one writer per file); PostgreSQL — inside the transaction, `SELECT pg_advisory_xact_lock(7243100002)`.
3. In one transaction: read `last_seq` from `journal_meta` (PostgreSQL) or from memory (SQLite); `seq = last_seq + 1`; insert into `journal_events (seq, at, kind, run_id, data)` with `run_id` = `data.run_id` when it is a string that parses as a UUID, else NULL; update `journal_meta.last_seq = seq`; apply step 6 for the event's side effects on the Journal's own tables; commit. Any datastore failure → `JRNL_DATASTORE_UNAVAILABLE` (retryable), logged `journal.event.append_failed` with `kind`; the sequence number is not consumed.
4. After commit, update the in-memory `last_seq`, release the lock, and push the `JournalEvent` to every subscriber queue (step 5).
5. Log at `debug` `journal.event.appended` with `seq`, `kind`. Never log `data`.

### 5. Delivering events — `EventSink`

1. `subscribe(sink)` creates a bounded `tokio::sync::mpsc` channel of 1,024 events and a task that, for each received event, awaits `sink.deliver(&event)`. Delivery to one sink is sequential and in `seq` order; sinks are independent, so a slow sink never delays another.
2. When `try_send` to a sink's queue fails because it is full, mark the sink *lagged* with `next_seq` = that event's `seq`, drop that event and every later one for this sink, and log `journal.sink.lagged` with `sink`, `next_seq`.
3. When a lagged sink's queue has drained, the task calls `sink.lagged(next_seq)` once and clears the mark; the sink re-reads from `next_seq` with `events_since` and new events flow again from the next append.
4. `unsubscribe` closes the queue; the task ends after the event it is delivering.
5. **PostgreSQL only (scaled shape).** Events appended by other instances do not pass through this instance's `append_event`. A poller reads `events_since(delivered_seq, 500)` every 500 ms and pushes each event not yet pushed by this instance to the queues, so every instance's sinks see every event in order. The instance's own appends update `delivered_seq` only when no earlier `seq` is missing.
6. `events_since(after_seq, limit)`: `limit` is clamped to 1–1,000. `after_seq > last_seq` → `JRNL_SEQ_AHEAD` (the datastore was restored from an older copy; the Remote API sends `reset`). `after_seq + 1 < first_kept_seq` where `first_kept_seq = MIN(seq)` of `journal_events` (or `last_seq + 1` when the table is empty) → `JRNL_RESUME_TOO_OLD`. Otherwise `SELECT … WHERE seq > ? ORDER BY seq LIMIT ?`.

### 6. Side effects of an appended event on the Journal's own tables

1. Run inside the append transaction of step 4.3.
2. **Step marks see events.** When `run_id` is set, find the `run_steps` row of that run whose `mark = 'now'` in the highest `plan_version`; when one exists, append `seq` to its `events_seen` (at most 10,000 entries per step *(chosen here)*; beyond that the oldest are dropped).
3. **Run index.** When `kind = "run_state_changed"` and `data.state` parses as `RunState`: upsert `journal_runs (run_id, first_seen_at, state, ended_at)`, setting `first_seen_at` on insert, `state` always, and `ended_at = at` when the state is one of `completed`, `failed`, `stopped_at_limit`, `cancelled`, `interrupted`, `abandoned`, `no_reply`. A malformed `data` for this kind is logged `journal.run_index.skipped` and the event is still stored.
4. **Run index reads.** `runs_in_progress`: `run_id` of rows with `ended_at IS NULL`. `running_runs`: the same rows as `RunIndexEntry`, ordered `first_seen_at DESC`, at most 500. `runs_ended_since(since, limit)`: rows with `ended_at >= since`, ordered `ended_at DESC, run_id DESC`, `limit` clamped to 1–500. The index holds no title and no workspace: a reader that needs them joins with the Session Supervisor's run records (Home and Search does this in the program's composite).

### 7. Internal journal entries

1. `append_entry`: validate `kind` (as event kinds), `component` (`[a-z_]{1,40}`), `summary` (1–300 characters, NFC, no control characters) and `data` (JSON, ≤ 65,536 bytes serialised) → else `JRNL_ENTRY_INVALID`. Insert with a new `JournalEntryId` (UUIDv7) and `recorded_at = now`. No sequence number is consumed, so entries never create gaps in the event stream.
2. `list_entries`: keyset page over `(recorded_at DESC, id DESC)`, optional `run_id` and `kind` filters, 100 items; the cursor is `Cursor::encode(&(recorded_at, id))`; a cursor that `Cursor::decode` refuses → `SRV_BAD_CURSOR`.
3. `diagnostic_lines(cursor)`: the same page as `list_entries(EntryFilter::default(), cursor)`, each entry mapped to `DiagnosticEntry`: `reference` = the first 8 characters of `id`; `at` = `recorded_at`; `level` = `data.level` when it is the string `info`, `warning` or `error`, else `info`; `code` = `data.code` when it is a string matching `^[A-Z][A-Z0-9_]{2,63}$`, else the empty string; `summary` unchanged. A writer that records a failure puts `"level"` and `"code"` keys in `data` (convention for every `append_entry` caller).
4. `support_export(run_ids)`: `run_ids` empty or more than 20 → `JRNL_EXPORT_INVALID`. Read, for those runs, the journal entries, the events' `seq`, `at` and `kind` (never `data`), the tool requests (digests only), the external actions without `destination`, the start-up reports and the run steps without `line`; plus the newest 500 entries with no `run_id`. Write one JSON object per line, first a header line `{"format":"ordin-support","version":1,"backend":"<uuid>","made_at":"<Timestamp>"}`. Stop adding lines at 10,000,000 bytes and set `truncated = true`. Log `journal.support_export.done` with the line count. The bundle never holds a token, message content, file content or tool arguments, because none of these is stored (*Security*).

### 8. Artefacts

1. `record_artefact`: validate `path` (relative, 1–4096 bytes, no `..` segment, no leading `/`, no NUL → `ART_PATH_INVALID`), `media_type` (`^[a-z0-9][a-z0-9!#$&^_.+-]{0,126}/[a-z0-9][a-z0-9!#$&^_.+-]{0,126}$` → `ART_MEDIA_TYPE_INVALID`) and `hash` (`HASH_NOT_SHA256_HEX`).
2. In one transaction: `version` = 1 + the highest version of (`location_key`, `path`), or 1; insert with a new `ArtefactId` and `created_at = now`. When the highest existing version already has the same `hash`, return that row unchanged (no new version for unchanged content).
3. The Journal never reads, copies or deletes the file itself; content stays in the agent folder or the place (02:680).
4. `list_artefacts`: keyset page over `(created_at DESC, id DESC)`.

### 9. External actions and recovery (G8)

1. `begin_external_action` is called **before** the action leaves ordin. Validate `destination` (1–2048 characters → `EXT_DESTINATION_INVALID`). Insert with `state = sent`, `sent_at = now`. A primary-key conflict on `action_key` → `EXT_KEY_DUPLICATE` (permanent); the caller then reads the existing row with `external_action(key)` and does not act again. `run_id` is optional (2C C-7); an action with no run is never linked to a run in progress.
2. `confirm_external_action`: `sent` or `confirmed` → `confirmed` with `confirmed_at = now` (idempotent on `confirmed`); `unknown_outcome` → `JRNL_EXT_STATE_INVALID` (only the person settles an unknown outcome).
3. `recover_external_actions` (program start, after `Journal::start`, before any session resumes, 03:409): one statement `UPDATE external_actions SET state = 'unknown_outcome' WHERE state = 'sent'`; return the count; log `journal.recovery.unknown_outcomes` with `count`. Nothing is resent (`EXT_AUTO_RESEND` is never produced by this component; a caller that tries to begin a second action with a used key gets `EXT_KEY_DUPLICATE`).
4. `mark_external_action_arrived`: `unknown_outcome` → `confirmed` with `confirmed_at = now`; any other state → `JRNL_EXT_STATE_INVALID`. This transition extends 2C's table, following 03:441 (**It arrived — mark as done**). **Send it again** is a new approval card and a new action key, never this row.
5. `external_action(key)`: `SELECT … WHERE action_key = ?`; no row → `Ok(None)`.
6. `unsettled_external_actions(kinds)`: `kinds` empty → `Ok(vec![])`. `SELECT … WHERE state IN ('sent','unknown_outcome') AND kind IN (…) ORDER BY sent_at, action_key LIMIT 1000`. After `recover_external_actions` has run at start, every row this returns is `unknown_outcome`; the caller (Push Service, Handover Service) re-reads the destination to decide whether the action arrived, then calls `confirm_external_action` only for a row still `sent`, and otherwise leaves the row for the person.

### 10. Tool requests

1. `record_tool_request`: validate `tool` (1–200 characters → `TREQ_NAME_INVALID`); insert `state = requested`, `requested_at = now`.
2. `decide_tool_request`: row must be `requested` (else `TREQ_IMMUTABLE` when `executed`/`refused`, `JRNL_NOT_FOUND` when absent); `deciding_rule` 1–128 characters (LW-20); `approval_id` present only with `decision = ask` (`TREQ_APPROVAL_MISMATCH`); set `decision`, `deciding_rule`, `approval_id`, `state = decided`.
3. `finish_tool_request`: row must be `decided` (else `TREQ_UNDECIDED` from `requested`, `TREQ_IMMUTABLE` from `executed`/`refused`); set `executed` or `refused`.
4. `tool_request(id)`: one row by id; absent → `Ok(None)`.

### 11. Start-up reports

1. Validate each list (≤ 500 items, each ≤ 500 characters → `START_LIST_TOO_LONG`) and each `RuleNotEnforced.reason` (1–500 → `START_REASON_MISSING`).
2. `subject_key` = `"run:<id>"`, `"crew_run:<id>"` or `"agent_release:<id>"`. A row with that key exists and `replace = false` → `START_DUPLICATE`; `replace = true` (a resumed start) → overwrite.

### 12. Reports and dashboards

1. `store_report`: check the structural rules of the *Shared Context* table in this order — title, parts count, each part's heading and sources, table and chart sizes, pie slices, key figures count, currency — then the serialised size (2,000,000 bytes). First failure → its code. Insert with `recorded_at = now`.
2. `keep_dashboard`: the run's report must exist (`JRNL_NOT_FOUND`); `name` 1–80 characters, unique in the workspace (`REPORT_DASHBOARD_NAME_DUPLICATE`); insert with `refresh_runs = [run_id]`, `kept_at = now`.
3. `add_dashboard_refresh`: the run's report must exist; append `run_id` to `refresh_runs` (at most 1,000 kept; the oldest dropped *(chosen here)*).
4. `remove_dashboard`: delete the row; the reports stay until pruned with their runs.
5. `forget_workspace`: delete `dashboards` and `reports` rows of that workspace; return the count.
6. `dashboard(id)` (SA-CLOSE-01): `SELECT … FROM dashboards WHERE id = ?`; no row → `Ok(None)`; a datastore failure → `JRNL_DATASTORE_UNAVAILABLE` (retryable).

### 13. Plans and step marks (ADR-068)

1. `state_plan`: 3–7 steps (`RSTEP_PLAN_SIZE_OUT_OF_RANGE`), each line 1–80 characters (`RSTEP_LINE_TOO_LONG`); the run must not have ended according to `journal_runs` (`RSTEP_IMMUTABLE`); `plan_version` = highest existing + 1, or 1; at most 20 plan versions per run (LW-35) → `RSTEP_PLAN_VERSION_GAP` beyond 20 *(code reused; chosen here)*. This is the one code for a 21st version: the MCP Front maps it to its own reply and does not raise a second code for the same rule. Insert one row per step with `position` 1…n, `mark = next`, `marks_set = [{next, now}]`, `events_seen = []`.
2. In the same transaction, append the event `plan_changed` with `{"run_id", "plan_version", "steps"}` through step 4 (so the screens and the chat see it).
3. `mark_step`: on the highest plan version of the run; position must exist (`JRNL_PLAN_NOT_FOUND`); run not ended (`RSTEP_IMMUTABLE`). Allowed transitions: `next → now`, `now → done`, `next → skipped`, `now → skipped`; anything else → `RSTEP_IMMUTABLE`. At most one step of a plan is `now`: marking a second step `now` while another is `now` is refused with `RSTEP_IMMUTABLE`, so the caller marks the earlier step `done` or `skipped` first. Append to `marks_set`, then append the event `step_marked` with `position` as u8.

### 14. Retention and pruning (A-08, UX-A58)

1. `set_retention_days(days)`: `days < 1` → `JRNL_RETENTION_INVALID` (the screens show "Enter a whole number of days, 1 or more." from 11 §5.11 / UX-A58); `company_max_days` set and `days > company_max_days` → `JRNL_RETENTION_ABOVE_COMPANY_MAX` (words: "Your company keeps history for at most [n] days." † *chosen here*). Store; log `journal.retention.changed` with `days`.
2. `set_company_max_days(max)`: store it; when the stored `retention_days` exceeds the new maximum, lower `retention_days` to it at once (2D *Configuration sources*, item 4: a newer profile that lowers a maximum lowers the stored value at the next use).
3. `prune(cutoff_override)`:
   1. `age_cutoff = now − retention_days` (or `cutoff_override` when given, used by the agent profile's Retention and Erasure).
   2. `count = SELECT COUNT(*) FROM journal_runs`. When `count > 100,000`, `cap_cutoff` = `first_seen_at` of the 100,000th newest row; `cutoff = max(age_cutoff, cap_cutoff)`; else `cutoff = age_cutoff`.
   3. `runs_in_progress` = run ids in `journal_runs` with `ended_at IS NULL`.
   4. Delete in batches of 1,000 rows per transaction *(chosen here, keeps each write lock under 100 ms)*, each table filtered by its time column `< cutoff` and excluding rows whose `run_id` is in `runs_in_progress`: `journal_events` (`at`), `journal_entries` (`recorded_at`), `tool_requests` (`requested_at`), `external_actions` (`sent_at`, and never a row in `unknown_outcome` that the person has not settled *(chosen here: the person must see it)*), `startup_reports` (`generated_at`), `run_steps` (rows of runs whose `journal_runs.ended_at < cutoff`), `reports` (`recorded_at`, never the report of the last run in any dashboard's `refresh_runs`), `artefacts` (`created_at`, never the latest version of a (`location_key`, `path`), 04 *File versions*), `journal_runs` (`ended_at < cutoff`).
   5. Call each registered `PruneParticipant` in registration order with `PruneRequest { cutoff, runs_in_progress }`; record its count, or its `PruneFailure.code` in `failed` and continue with the next.
   6. Set `last_pruned_at = now`; log `journal.prune.done` with `cutoff`, `removed` per module and the failed module names. Return the summary.
   7. Two prunes never overlap: a second call while one runs waits for it and returns its summary.
4. `count_for_runs(run_ids)` and `delete_for_runs(run_ids)` (agent profile: Retention and Erasure's age stage and erasure). More than 500 ids → `JRNL_ENTRY_INVALID`; empty → `Ok(0)`. The rows linked to a run are: `journal_events` and `journal_entries` with that `run_id`, `tool_requests`, `external_actions` (whatever their state — erasure overrides the keep-unknown-outcomes rule of step 14.3.4), `startup_reports` whose `subject` is that run, `run_steps`, `reports`, `artefacts` with `producing_run_id` (index rows only, never the files), and the `journal_runs` row. `count_for_runs` returns the sum of those counts. `delete_for_runs` deletes them in one transaction per 500 ids, removes the run ids from every `dashboards.refresh_runs`, and logs `journal.erase.done` with the count (never the ids). Deleting events leaves gaps in `seq`; `events_since` returns the remaining rows in order, and a stream reader resumes across the gap without error. A run that is in progress is deleted too — the caller selects ended runs or the runs of an erased person.
5. `compact`: SQLite — `VACUUM` (needs free disk of the file's size; failure → `JRNL_DATASTORE_UNAVAILABLE`, logged, the file unchanged) then `PRAGMA optimize`; PostgreSQL — `VACUUM (ANALYZE)` on this crate's tables. The ordin server's Always-on schedule calls it weekly at 04:00 when `runs_in_progress` is empty (ADR-076).

### 15. Shutdown (SA-XC-13)

1. Stop the prune timer; wait for a running prune's current batch.
2. Close every subscriber queue and wait at most 2 seconds for delivery tasks to finish; log `journal.shutdown` with the count of undelivered events per sink.
3. The program calls `Datastore::close` last. SQLite runs a `PRAGMA wal_checkpoint(TRUNCATE)` before closing.

### 16. Liveness — `ping`

1. Run `SELECT 1` on the pool, bounded by `tokio::time::timeout(within)`. Answer within the bound → `true`; timeout, pool closed or any error → `false`, logged at `debug` `journal.ping.failed`. `ping` never returns an error and never retries.

## Data Model

All tables below are owned and written only by `ordin-journal` (ADR-096 item 1). The migration step texts are the content of `MIGRATIONS.sqlite[0]` and `MIGRATIONS.postgres[0]` (version 1, "create journal tables").

### SQLite (desktop, personal server, agent single shape)

```sql
-- Created by MigrationRegistry::apply itself, before any module's steps.
CREATE TABLE IF NOT EXISTS schema_migrations (
    module       TEXT    NOT NULL CHECK (length(module) BETWEEN 2 AND 41),
    version      INTEGER NOT NULL CHECK (version >= 1),
    description  TEXT    NOT NULL,
    checksum     TEXT    NOT NULL CHECK (length(checksum) = 64),
    applied_at   TEXT    NOT NULL,
    PRIMARY KEY (module, version)
);

-- journal v1
CREATE TABLE journal_meta (
    id                    INTEGER PRIMARY KEY CHECK (id = 1),
    backend_id            TEXT    NOT NULL CHECK (length(backend_id) = 36),
    last_seq              INTEGER NOT NULL DEFAULT 0 CHECK (last_seq >= 0),
    retention_days        INTEGER NOT NULL DEFAULT 90 CHECK (retention_days >= 1),
    company_max_days      INTEGER CHECK (company_max_days IS NULL OR company_max_days >= 1),
    last_pruned_at        TEXT,
    created_at            TEXT    NOT NULL
);

CREATE TABLE journal_events (
    seq     INTEGER PRIMARY KEY,            -- assigned by the Journal, never AUTOINCREMENT-reused
    at      TEXT    NOT NULL,
    kind    TEXT    NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
    run_id  TEXT    CHECK (run_id IS NULL OR length(run_id) = 36),
    data    TEXT    NOT NULL CHECK (json_valid(data) AND length(data) <= 262144)
);
CREATE INDEX journal_events_at  ON journal_events (at);
CREATE INDEX journal_events_run ON journal_events (run_id, seq) WHERE run_id IS NOT NULL;

CREATE TABLE journal_entries (
    id           TEXT PRIMARY KEY CHECK (length(id) = 36),
    kind         TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
    component    TEXT NOT NULL CHECK (length(component) BETWEEN 1 AND 40),
    run_id       TEXT,
    crew_run_id  TEXT,
    workspace_id TEXT,
    summary      TEXT NOT NULL CHECK (length(summary) BETWEEN 1 AND 300),
    data         TEXT NOT NULL CHECK (json_valid(data) AND length(data) <= 65536),
    recorded_at  TEXT NOT NULL
);
CREATE INDEX journal_entries_recorded ON journal_entries (recorded_at DESC, id DESC);
CREATE INDEX journal_entries_run      ON journal_entries (run_id, recorded_at) WHERE run_id IS NOT NULL;

CREATE TABLE journal_runs (
    run_id         TEXT PRIMARY KEY CHECK (length(run_id) = 36),
    first_seen_at  TEXT NOT NULL,
    state          TEXT NOT NULL CHECK (state IN ('queued','running','paused','completed','failed',
                        'stopped_at_limit','cancelled','interrupted','abandoned','no_reply')),
    ended_at       TEXT
);
CREATE INDEX journal_runs_first_seen ON journal_runs (first_seen_at DESC);
CREATE INDEX journal_runs_open       ON journal_runs (run_id) WHERE ended_at IS NULL;

CREATE TABLE artefacts (
    id                TEXT    PRIMARY KEY CHECK (length(id) = 36),
    location          TEXT    NOT NULL CHECK (json_valid(location)),
    location_key      TEXT    NOT NULL,      -- 'agent_folder:<uuid>' or 'place:<uuid>'; written by the Journal
    path              TEXT    NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    version           INTEGER NOT NULL CHECK (version >= 1),
    hash              TEXT    NOT NULL CHECK (length(hash) = 64),
    producing_run_id  TEXT,
    media_type        TEXT    NOT NULL CHECK (length(media_type) BETWEEN 3 AND 255),
    created_at        TEXT    NOT NULL,
    UNIQUE (location_key, path, version)
);
CREATE INDEX artefacts_created ON artefacts (created_at DESC, id DESC);
CREATE INDEX artefacts_run     ON artefacts (producing_run_id) WHERE producing_run_id IS NOT NULL;

CREATE TABLE tool_requests (
    id                TEXT PRIMARY KEY CHECK (length(id) = 36),
    run_id            TEXT NOT NULL,
    tool              TEXT NOT NULL CHECK (length(tool) BETWEEN 1 AND 200),
    arguments_digest  TEXT NOT NULL CHECK (length(arguments_digest) = 64),
    decision          TEXT CHECK (decision IS NULL OR decision IN ('allow','deny','ask')),
    deciding_rule     TEXT CHECK (deciding_rule IS NULL OR length(deciding_rule) BETWEEN 1 AND 128),
    approval_id       TEXT,
    state             TEXT NOT NULL CHECK (state IN ('requested','decided','executed','refused')),
    requested_at      TEXT NOT NULL,
    CHECK ((state = 'requested') = (decision IS NULL)),
    CHECK (approval_id IS NULL OR decision = 'ask')
);
CREATE INDEX tool_requests_run ON tool_requests (run_id, requested_at);
CREATE INDEX tool_requests_at  ON tool_requests (requested_at);

CREATE TABLE external_actions (
    action_key    TEXT PRIMARY KEY CHECK (length(action_key) = 36),
    run_id        TEXT CHECK (run_id IS NULL OR length(run_id) = 36),   -- NULL: no run (2C C-7)
    kind          TEXT NOT NULL CHECK (kind IN ('slack_post','connection_write','wrightrun_run_start',
                       'email_send','origin_push','copy_publish','telegram_post',
                       'pull_request_opened','handover_written')),
    destination   TEXT NOT NULL CHECK (length(destination) BETWEEN 1 AND 2048),
    state         TEXT NOT NULL CHECK (state IN ('sent','confirmed','unknown_outcome')),
    sent_at       TEXT NOT NULL,
    confirmed_at  TEXT,
    CHECK ((state = 'confirmed') = (confirmed_at IS NOT NULL))
);
CREATE INDEX external_actions_run   ON external_actions (run_id);
CREATE INDEX external_actions_state ON external_actions (state, sent_at DESC);
CREATE INDEX external_actions_kind  ON external_actions (kind, state, sent_at);

CREATE TABLE startup_reports (
    id                         TEXT PRIMARY KEY CHECK (length(id) = 36),
    subject                    TEXT NOT NULL CHECK (json_valid(subject)),
    subject_key                TEXT NOT NULL UNIQUE,
    enforced_rules             TEXT NOT NULL CHECK (json_valid(enforced_rules)),
    rules_not_enforced         TEXT NOT NULL CHECK (json_valid(rules_not_enforced)),
    unavailable_parts          TEXT NOT NULL CHECK (json_valid(unavailable_parts)),
    available_tools            TEXT NOT NULL CHECK (json_valid(available_tools)),
    adapter_verbs_not_offered  TEXT NOT NULL CHECK (json_valid(adapter_verbs_not_offered)),
    generated_at               TEXT NOT NULL
);
CREATE INDEX startup_reports_at ON startup_reports (generated_at);

CREATE TABLE reports (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id  TEXT NOT NULL,
    run_id        TEXT NOT NULL UNIQUE,
    title         TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    as_of         TEXT NOT NULL,
    parts         TEXT NOT NULL CHECK (json_valid(parts) AND length(parts) <= 2000000),
    recorded_at   TEXT NOT NULL
);
CREATE INDEX reports_workspace ON reports (workspace_id, as_of DESC, id DESC);
CREATE INDEX reports_recorded  ON reports (recorded_at);

CREATE TABLE dashboards (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id  TEXT NOT NULL,
    name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    made_by       TEXT NOT NULL CHECK (json_valid(made_by)),
    inputs        TEXT NOT NULL CHECK (json_valid(inputs)),
    refresh_runs  TEXT NOT NULL CHECK (json_valid(refresh_runs)),
    kept_at       TEXT NOT NULL,
    UNIQUE (workspace_id, name)
);

CREATE TABLE run_steps (
    id            TEXT    PRIMARY KEY CHECK (length(id) = 36),
    run_id        TEXT    NOT NULL,
    plan_version  INTEGER NOT NULL CHECK (plan_version BETWEEN 1 AND 20),
    position      INTEGER NOT NULL CHECK (position BETWEEN 1 AND 7),
    line          TEXT    NOT NULL CHECK (length(line) BETWEEN 1 AND 80),
    came_from     TEXT    NOT NULL CHECK (came_from IN ('workflow','crew_plan','agents_plan',
                       'harness_todo_list','added_by_ordin')),
    mark          TEXT    NOT NULL CHECK (mark IN ('next','now','done','skipped')),
    marks_set     TEXT    NOT NULL CHECK (json_valid(marks_set)),
    events_seen   TEXT    NOT NULL CHECK (json_valid(events_seen)),
    UNIQUE (run_id, plan_version, position)
);
CREATE INDEX run_steps_now ON run_steps (run_id, plan_version) WHERE mark = 'now';
```

SQLite `length()` counts characters for `TEXT`; the Rust validation is the authority and the `CHECK` constraints are a second line.

### PostgreSQL (agent scaled shape)

Same tables and columns, with these type changes: `TEXT` identifiers become `uuid`; timestamps become `timestamptz` (written and read through `Timestamp`); JSON columns become `jsonb`; `json_valid(x)` checks are dropped (the type enforces them) and size checks use `octet_length(x::text)`; `journal_events.seq` is `bigint PRIMARY KEY`; `journal_meta.last_seq` is `bigint`; partial indexes are kept with the same `WHERE` clauses. `schema_migrations.applied_at` is `timestamptz`.

```sql
CREATE TABLE journal_events (
    seq     bigint      PRIMARY KEY,
    at      timestamptz NOT NULL,
    kind    text        NOT NULL CHECK (char_length(kind) BETWEEN 1 AND 64),
    run_id  uuid,
    data    jsonb       NOT NULL CHECK (jsonb_typeof(data) = 'object' AND octet_length(data::text) <= 262144)
);
CREATE INDEX journal_events_at  ON journal_events (at);
CREATE INDEX journal_events_run ON journal_events (run_id, seq) WHERE run_id IS NOT NULL;

CREATE TABLE journal_meta (
    id                integer     PRIMARY KEY CHECK (id = 1),
    backend_id        uuid        NOT NULL,
    last_seq          bigint      NOT NULL DEFAULT 0 CHECK (last_seq >= 0),
    retention_days    integer     NOT NULL DEFAULT 90 CHECK (retention_days >= 1),
    company_max_days  integer     CHECK (company_max_days IS NULL OR company_max_days >= 1),
    last_pruned_at    timestamptz,
    created_at        timestamptz NOT NULL
);

CREATE TABLE journal_entries (
    id            uuid        PRIMARY KEY,
    kind          text        NOT NULL CHECK (char_length(kind) BETWEEN 1 AND 64),
    component     text        NOT NULL CHECK (char_length(component) BETWEEN 1 AND 40),
    run_id        uuid,
    crew_run_id   uuid,
    workspace_id  uuid,
    summary       text        NOT NULL CHECK (char_length(summary) BETWEEN 1 AND 300),
    data          jsonb       NOT NULL CHECK (octet_length(data::text) <= 65536),
    recorded_at   timestamptz NOT NULL
);
CREATE INDEX journal_entries_recorded ON journal_entries (recorded_at DESC, id DESC);
CREATE INDEX journal_entries_run      ON journal_entries (run_id, recorded_at) WHERE run_id IS NOT NULL;

CREATE TABLE journal_runs (
    run_id         uuid        PRIMARY KEY,
    first_seen_at  timestamptz NOT NULL,
    state          text        NOT NULL CHECK (state IN ('queued','running','paused','completed','failed',
                        'stopped_at_limit','cancelled','interrupted','abandoned','no_reply')),
    ended_at       timestamptz
);
CREATE INDEX journal_runs_first_seen ON journal_runs (first_seen_at DESC);
CREATE INDEX journal_runs_open       ON journal_runs (run_id) WHERE ended_at IS NULL;

CREATE TABLE artefacts (
    id                uuid        PRIMARY KEY,
    location          jsonb       NOT NULL,
    location_key      text        NOT NULL,
    path              text        NOT NULL CHECK (octet_length(path) BETWEEN 1 AND 4096),
    version           integer     NOT NULL CHECK (version >= 1),
    hash              text        NOT NULL CHECK (hash ~ '^[0-9a-f]{64}$'),
    producing_run_id  uuid,
    media_type        text        NOT NULL CHECK (char_length(media_type) BETWEEN 3 AND 255),
    created_at        timestamptz NOT NULL,
    UNIQUE (location_key, path, version)
);
CREATE INDEX artefacts_created ON artefacts (created_at DESC, id DESC);
CREATE INDEX artefacts_run     ON artefacts (producing_run_id) WHERE producing_run_id IS NOT NULL;

CREATE TABLE tool_requests (
    id                uuid        PRIMARY KEY,
    run_id            uuid        NOT NULL,
    tool              text        NOT NULL CHECK (char_length(tool) BETWEEN 1 AND 200),
    arguments_digest  text        NOT NULL CHECK (arguments_digest ~ '^[0-9a-f]{64}$'),
    decision          text        CHECK (decision IS NULL OR decision IN ('allow','deny','ask')),
    deciding_rule     text        CHECK (deciding_rule IS NULL OR char_length(deciding_rule) BETWEEN 1 AND 128),
    approval_id       uuid,
    state             text        NOT NULL CHECK (state IN ('requested','decided','executed','refused')),
    requested_at      timestamptz NOT NULL,
    CHECK ((state = 'requested') = (decision IS NULL)),
    CHECK (approval_id IS NULL OR decision = 'ask')
);
CREATE INDEX tool_requests_run ON tool_requests (run_id, requested_at);
CREATE INDEX tool_requests_at  ON tool_requests (requested_at);

CREATE TABLE external_actions (
    action_key    uuid        PRIMARY KEY,
    run_id        uuid,
    kind          text        NOT NULL CHECK (kind IN ('slack_post','connection_write','wrightrun_run_start',
                       'email_send','origin_push','copy_publish','telegram_post',
                       'pull_request_opened','handover_written')),
    destination   text        NOT NULL CHECK (char_length(destination) BETWEEN 1 AND 2048),
    state         text        NOT NULL CHECK (state IN ('sent','confirmed','unknown_outcome')),
    sent_at       timestamptz NOT NULL,
    confirmed_at  timestamptz,
    CHECK ((state = 'confirmed') = (confirmed_at IS NOT NULL))
);
CREATE INDEX external_actions_run   ON external_actions (run_id);
CREATE INDEX external_actions_state ON external_actions (state, sent_at DESC);
CREATE INDEX external_actions_kind  ON external_actions (kind, state, sent_at);

CREATE TABLE startup_reports (
    id                         uuid        PRIMARY KEY,
    subject                    jsonb       NOT NULL,
    subject_key                text        NOT NULL UNIQUE,
    enforced_rules             jsonb       NOT NULL,
    rules_not_enforced         jsonb       NOT NULL,
    unavailable_parts          jsonb       NOT NULL,
    available_tools            jsonb       NOT NULL,
    adapter_verbs_not_offered  jsonb       NOT NULL,
    generated_at               timestamptz NOT NULL
);
CREATE INDEX startup_reports_at ON startup_reports (generated_at);

CREATE TABLE reports (
    id            uuid        PRIMARY KEY,
    workspace_id  uuid        NOT NULL,
    run_id        uuid        NOT NULL UNIQUE,
    title         text        NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    as_of         timestamptz NOT NULL,
    parts         jsonb       NOT NULL CHECK (octet_length(parts::text) <= 2000000),
    recorded_at   timestamptz NOT NULL
);
CREATE INDEX reports_workspace ON reports (workspace_id, as_of DESC, id DESC);
CREATE INDEX reports_recorded  ON reports (recorded_at);

CREATE TABLE dashboards (
    id            uuid        PRIMARY KEY,
    workspace_id  uuid        NOT NULL,
    name          text        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    made_by       jsonb       NOT NULL,
    inputs        jsonb       NOT NULL,
    refresh_runs  jsonb       NOT NULL,
    kept_at       timestamptz NOT NULL,
    UNIQUE (workspace_id, name)
);

CREATE TABLE run_steps (
    id            uuid    PRIMARY KEY,
    run_id        uuid    NOT NULL,
    plan_version  integer NOT NULL CHECK (plan_version BETWEEN 1 AND 20),
    position      integer NOT NULL CHECK (position BETWEEN 1 AND 7),
    line          text    NOT NULL CHECK (char_length(line) BETWEEN 1 AND 80),
    came_from     text    NOT NULL CHECK (came_from IN ('workflow','crew_plan','agents_plan',
                       'harness_todo_list','added_by_ordin')),
    mark          text    NOT NULL CHECK (mark IN ('next','now','done','skipped')),
    marks_set     jsonb   NOT NULL,
    events_seen   jsonb   NOT NULL,
    UNIQUE (run_id, plan_version, position)
);
CREATE INDEX run_steps_now ON run_steps (run_id, plan_version) WHERE mark = 'now';

-- created by MigrationRegistry::apply before any step:
CREATE TABLE IF NOT EXISTS schema_migrations (
    module       text        NOT NULL CHECK (char_length(module) BETWEEN 2 AND 41),
    version      bigint      NOT NULL CHECK (version >= 1),
    description  text        NOT NULL,
    checksum     text        NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    applied_at   timestamptz NOT NULL,
    PRIMARY KEY (module, version)
);
```

### Retention and destructive operations

- Pruning deletes rows (A-08, decision 57); it is the only removal path and is the person's setting. Nothing in a pending approval or a run in progress is removed (A-08); unknown external-action outcomes stay until the person settles them.
- `forget_workspace` and `remove_dashboard` delete rows on the person's confirmed action.
- No migration in version 1 drops or narrows anything.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| SQLite file fails `PRAGMA quick_check`, or cannot be created or opened | `Err(permanent)` | `JRNL_DATASTORE_OPEN_FAILED` | `{"error":"ordin can't open its saved work on this computer.","code":"JRNL_DATASTORE_OPEN_FAILED","action":"send_to_it"}` † |
| A datastore call fails (connection lost, disk I/O, lock timeout after 5 s) | `Err(retryable)` | `JRNL_DATASTORE_UNAVAILABLE` | `{"error":"Something went wrong saving your work. Try again.","code":"JRNL_DATASTORE_UNAVAILABLE","action":"try_again"}` † |
| Database written by a newer ordin (unknown module or higher version) | `Err(permanent)`; server exit 78 | `JRNL_SCHEMA_NEWER` | `{"error":"This copy of ordin is older than your saved work. Update ordin to open it.","code":"JRNL_SCHEMA_NEWER","action":"send_to_it"}` † |
| Scaled-shape instance started on a schema with pending steps | `Err(permanent)`; instance not ready | `JRNL_SCHEMA_BEHIND` | log only: `{"code":"JRNL_SCHEMA_BEHIND","pending":[["journal",2]]}` |
| A module registered twice, or versions not 1, 2, 3 … | `Err(permanent)` (build defect) | `JRNL_MIGRATION_DUPLICATE` | log only |
| An applied step's SQL differs from the registered step | `Err(permanent)` | `JRNL_MIGRATION_CHANGED` | log only; the desktop shows the `JRNL_SCHEMA_NEWER` sentence with **Send to IT** |
| A migration step fails; transaction rolled back | `Err(permanent)` | `JRNL_MIGRATION_FAILED` | `{"error":"ordin couldn't update its saved work. Your work is unchanged.","code":"JRNL_MIGRATION_FAILED","action":"send_to_it"}` † |
| Backup copy before migrating cannot be written | `Err(permanent)` | `JRNL_BACKUP_FAILED` | `{"error":"ordin needs more free disk space to update.","code":"JRNL_BACKUP_FAILED","action":"try_again"}` † |
| Event kind malformed, `reset`, or `data` not an object | `Err(permanent)` | `JRNL_EVENT_KIND_INVALID` | in-process only |
| Event data larger than 256 KiB | `Err(permanent)` | `JRNL_EVENT_TOO_LARGE` | in-process only; the emitter sends a reference instead (LW-6) |
| Journal entry fails its rules | `Err(permanent)` | `JRNL_ENTRY_INVALID` | in-process only |
| `events_since` asked for a sequence older than kept | `Err(permanent)` | `JRNL_RESUME_TOO_OLD` | in-process; the Remote API sends one `reset` event |
| `events_since` asked for a sequence beyond `last_seq` | `Err(permanent)` | `JRNL_SEQ_AHEAD` | in-process; the Remote API sends one `reset` event |
| History days < 1, empty or not a whole number | Service API error reply | `JRNL_RETENTION_INVALID` | `{"error":"Enter a whole number of days, 1 or more.","code":"JRNL_RETENTION_INVALID","action":null}` |
| History days above the company maximum | Service API error reply | `JRNL_RETENTION_ABOVE_COMPANY_MAX` | `{"error":"Your company keeps history for at most 365 days.","code":"JRNL_RETENTION_ABOVE_COMPANY_MAX","action":null}` † |
| Confirm or mark-arrived on an external action in the wrong state | `Err(permanent)` | `JRNL_EXT_STATE_INVALID` | `{"error":"This was already settled.","code":"JRNL_EXT_STATE_INVALID","action":null}` † |
| Record, report, dashboard or plan not found | `Err(permanent)` | `JRNL_NOT_FOUND` / `JRNL_PLAN_NOT_FOUND` | `{"error":"This item is no longer available.","code":"JRNL_NOT_FOUND","action":null}` † |
| Malformed or over-long cursor | Remote API 422 (refused by the Service API's input check before dispatch, ServiceApi.md Internal Logic 2.9); in-process `Err(permanent)` | `SRV_BAD_CURSOR` | `{"error":"the server could not answer the request","code":"SRV_BAD_CURSOR","action":"try_again"}` |
| A 2C entity rule fails (artefact, tool request, external action, start-up report, report, run step) | `Err(permanent)` | the 2C code of the failed rule, as listed in the *Shared Context* validation table | in-process; reaches the person only through the calling component's sentence |
| A prune participant fails | recorded in `PruneSummary.failed`; prune continues | the participant's code | log only |
| Support export asked for no run or more than 20 runs | `Err(permanent)` | `JRNL_EXPORT_INVALID` | `{"error":"Choose between 1 and 20 runs to export.","code":"JRNL_EXPORT_INVALID","action":null}` † |
| `count_for_runs` / `delete_for_runs` given more than 500 run ids | `Err(permanent)` (caller defect) | `JRNL_ENTRY_INVALID` | in-process only |
| `ping` gets no answer within its bound | returns `false` (no error) | — | log `debug` `journal.ping.failed` |

Sentences marked † are *chosen here* (11 §6 has no row) and go to the error registry of Phase 4 for review.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Journal and Artefact Store

  Scenario: First start creates the database and the backend identifier
    Given no file exists at "<data>/journal.db"
    When the program opens the datastore, registers the journal's migrations and starts the Journal
    Then "<data>/journal.db" exists with mode 0600
    And schema_migrations holds ("journal", 1)
    And journal_meta holds one row with a UUIDv7 backend_id, last_seq 0 and retention_days 90
    And no backup file was written

  Scenario: Pending migrations are applied after a backup, in one transaction
    Given a journal.db with ("journal", 1) applied
    And the program registers module "approval" with steps 1 and 2
    When migrations are applied in ApplyPending mode
    Then a file "<data>/backups/journal-<UTC time>.db" exists
    And schema_migrations holds ("approval", 1) and ("approval", 2)

  Scenario: A failing step leaves the database unchanged
    Given module "approval" step 2 contains SQL that fails
    When migrations are applied
    Then the call fails with JRNL_MIGRATION_FAILED for ("approval", 2)
    And schema_migrations does not hold ("approval", 1)
    And the backup file is kept

  Scenario: A database written by a newer ordin is refused
    Given schema_migrations holds ("journal", 7)
    And this program registers journal steps 1 to 3
    When migrations are applied
    Then the call fails with JRNL_SCHEMA_NEWER
    And no table is changed

  Scenario: A changed migration is refused
    Given ("journal", 1) was applied with a checksum different from the registered step's
    When migrations are applied
    Then the call fails with JRNL_MIGRATION_CHANGED

  Scenario: A scaled-shape instance refuses a schema behind its program
    Given a PostgreSQL datastore missing ("journal", 2)
    When migrations are checked in CheckOnly mode
    Then the call fails with JRNL_SCHEMA_BEHIND naming ("journal", 2)

  Scenario: Events get contiguous sequence numbers and reach every sink in order
    Given sinks "command_bridge" and "remote_api" are subscribed
    When 3 events are appended concurrently
    Then their seq values are 1, 2 and 3
    And each sink receives seq 1, 2, 3 in that order

  Scenario: A slow sink is told it lagged and nothing else is affected
    Given sink "remote_api" does not return from deliver
    And sink "command_bridge" is subscribed
    When 1,100 events are appended
    Then "command_bridge" receives all 1,100 events
    And once "remote_api" returns, its lagged method is called once with the first undelivered seq

  Scenario: Resuming from a sequence older than the journal keeps
    Given pruning removed events up to seq 41200
    When events_since(40000, 500) is called
    Then it fails with JRNL_RESUME_TOO_OLD and first_kept_seq 41201

  Scenario: Resuming from a sequence beyond the last one
    Given last_seq is 500
    When events_since(900, 100) is called
    Then it fails with JRNL_SEQ_AHEAD

  Scenario: Oversized event data is refused without consuming a sequence number
    Given last_seq is 10
    When an event with 300,000 bytes of data is appended
    Then the call fails with JRNL_EVENT_TOO_LARGE
    And the next appended event has seq 11

  Scenario: An external action left unconfirmed by a crash becomes unknown outcome
    Given external action "k1" is in state sent
    When the program starts and calls recover_external_actions
    Then "k1" is in state unknown_outcome
    And beginning another action with key "k1" fails with EXT_KEY_DUPLICATE

  Scenario: The person settles an unknown outcome
    Given external action "k1" is in state unknown_outcome
    When the person chooses "It arrived — mark as done"
    Then "k1" is in state confirmed with confirmed_at set
    And confirming "k1" from unknown_outcome by an adapter fails with JRNL_EXT_STATE_INVALID

  Scenario Outline: Tool request transitions
    Given a tool request in state <from>
    When <call> is made
    Then the result is <outcome>

    Examples:
      | from      | call                         | outcome          |
      | requested | decide allow                 | state decided    |
      | requested | finish executed              | TREQ_UNDECIDED   |
      | decided   | finish executed              | state executed   |
      | executed  | decide deny                  | TREQ_IMMUTABLE   |
      | requested | decide allow with approval   | TREQ_APPROVAL_MISMATCH |

  Scenario: An unchanged artefact does not get a new version
    Given artefact "out/summary.md" in the agent folder has version 2 with hash H
    When record_artefact is called for the same path with hash H
    Then version 2 is returned and no row is added

  Scenario Outline: Report structural rules
    Given a report with <defect>
    When store_report is called
    Then it fails with <code>

    Examples:
      | defect                         | code                        |
      | 13 parts                       | REPORT_PARTS_OUT_OF_RANGE   |
      | a part with no source          | REPORT_PART_NO_SOURCE       |
      | a pie chart with 7 slices      | REPORT_PIE_TOO_MANY_SLICES  |
      | a money column without currency| REPORT_CURRENCY_MISMATCH    |
      | 2,100,000 bytes serialised     | REPORT_TOO_LARGE            |

  Scenario: A stated plan emits plan_changed and later events are seen by the current step
    Given run R has no plan
    When state_plan is called with 4 steps
    Then 4 run_steps rows exist with plan_version 1 and mark next
    And an event "plan_changed" is appended
    When step 1 is marked now and an event with run_id R is appended with seq 77
    Then step 1's events_seen contains 77

  Scenario: A plan of an ended run cannot change
    Given run R's last run_state_changed event has state completed
    When mark_step is called for run R
    Then it fails with RSTEP_IMMUTABLE

  Scenario: History retention is refused below one day
    When history.set_days is called with 0
    Then the reply is an error with code JRNL_RETENTION_INVALID and message "Enter a whole number of days, 1 or more."

  Scenario: A lower company maximum lowers the stored value
    Given retention_days is 200
    When set_company_max_days(90) is called
    Then retention_days is 90

  Scenario: Pruning keeps runs in progress and calls every participant
    Given retention_days is 90
    And run A ended 120 days ago and run B started 120 days ago and is still running
    And participants "approval" and "session_supervisor" are registered
    When prune runs
    Then events and tool requests of run A are removed
    And events and tool requests of run B remain
    And both participants received runs_in_progress containing B

  Scenario: Pruning applies the 100,000-run cap
    Given journal_runs holds 100,050 ended runs, all within the retention period
    When prune runs
    Then the 50 oldest runs' journal records are removed

  Scenario: An external action is read back by its key after a crash
    Given external action "k2" of kind telegram_post was begun and never confirmed
    And the program restarted and called recover_external_actions
    When external_action("k2") is called
    Then it returns "k2" in state unknown_outcome
    And external_action("k9") for a key never begun returns None

  Scenario: An external action with no run is accepted
    When begin_external_action is called with kind origin_push and no run_id
    Then the stored action has no run_id and state sent

  Scenario: Unsettled actions are listed by kind for the Push Service
    Given actions "a" (origin_push, unknown_outcome), "b" (copy_publish, confirmed) and "c" (email_send, unknown_outcome)
    When unsettled_external_actions([origin_push, copy_publish]) is called
    Then the result is exactly ["a"]

  Scenario: Runs ended since a time come from the run index
    Given run A ended at 2026-01-10T08:00:00.000Z and run B is running
    When runs_ended_since(2026-01-10T00:00:00.000Z, 50) is called
    Then the result is run A with its state and ended_at
    And running_runs returns run B with ended_at absent

  Scenario: Erasure removes every journal row of the given runs
    Given run A has 4 events, 2 entries, 1 tool request and 1 external action in unknown_outcome
    When count_for_runs([A]) is called
    Then the result is 9 including A's run index row
    When delete_for_runs([A]) is called
    Then no row linked to A remains
    And events of other runs keep their seq values

  Scenario: Diagnostics lines take level and code from the entry's data
    Given an entry with summary "Sign-in to GitHub failed" and data {"level":"error","code":"CRED_PROVIDER_REFUSED"}
    When diagnostic_lines is called
    Then its line has level "error", code "CRED_PROVIDER_REFUSED" and an 8-character reference

  Scenario: A support export of too many runs is refused
    When support_export is called with 21 run ids
    Then it fails with JRNL_EXPORT_INVALID

  Scenario: Ping answers false when the datastore is unreachable
    Given the PostgreSQL server stopped answering
    When ping(2 seconds) is called
    Then it returns false within 2 seconds and no error is raised

  Scenario: One dashboard is read by id
    Given a kept dashboard D in workspace W
    When dashboard(D) is called
    Then it returns Some with D's name and refresh_runs
    And dashboard(<an unknown id>) returns None

  Scenario: A failing participant does not stop pruning
    Given participant "approval" returns PruneFailure "APPR_STORE_UNAVAILABLE"
    When prune runs
    Then the summary lists "approval" under failed
    And the journal's own rows older than the cut-off are removed
```

## Performance / Security / Observability

- **Performance targets** (desktop SSD, SQLite, journal of 100,000 runs):
  - `append_event` p95 ≤ 5 ms; delivery to a sink's `deliver` call p95 ≤ 20 ms after commit, which leaves the rest of A-46's 500 ms to the Remote API and the network.
  - `events_since(_, 1000)` p95 ≤ 50 ms; any list command page p95 ≤ 40 ms, inside A-03's 150 ms.
  - `record_tool_request` and `decide_tool_request` p95 ≤ 5 ms each (they sit on the hook's 2 s allow/deny path, LW-21).
  - Applying an empty pending list ≤ 50 ms; start-up work of this component ≤ 300 ms excluding a backup, inside A-03's 3 s start.
  - Prune batches ≤ 100 ms of write lock each; a full prune of 1,000,000 rows ≤ 3 minutes in the background.
  - Memory: ≤ 1,024 queued events per sink (each ≤ 256 KiB; typical ≤ 2 KiB); the crate holds no other cache.
  - Scaled shape: append p95 ≤ 15 ms on PostgreSQL in the same cluster; cross-instance delivery ≤ 700 ms (500 ms poll plus query).
- **Security:**
  - No external entry point: the crate is called in-process only. Service API inputs reach it after the Command Bridge or Remote API validated them against their schemas (SA-API-02); the crate re-checks every field rule listed above.
  - Every query is parameterised (sqlx binds); `raw_sql` is used only for registered migration text compiled into the program.
  - The SQLite file and backups are mode `0600` in a `0700` folder on macOS and Linux; the file is not encrypted by ordin and relies on disk encryption (ADR-010, A-07).
  - The PostgreSQL connection string is never logged, printed or put in an error (2D-1a).
  - The journal never holds a token, a secret, message content, file content or tool arguments: tool calls are kept as digests (`arguments_digest`), artefacts as paths and hashes, and `journal_entries.summary` is limited to one plain sentence. The crate cannot detect a secret placed in `data` by a caller; callers are bound by 07 — *Logging* and SA-XC-03.
  - Pruning is the only deletion path for history, driven by the person's setting; it never removes a pending approval or work in progress.
- **Observability** (`tracing`, JSON lines; field `component = "journal"` on every line):
  - `journal.open.done` {engine}; `journal.open.failed` {engine, reason}
  - `journal.migrate.applied` {module, version}; `journal.migrate.refused` {module, version, code}; `journal.migrate.failed` {module, version}; `journal.backup.written` {file_name}
  - `journal.started` {engine, last_seq}; `journal.shutdown` {undelivered_by_sink}
  - `journal.event.appended` (debug) {seq, kind}; `journal.event.append_failed` {kind, code}
  - `journal.sink.lagged` {sink, next_seq}; `journal.sink.resumed` {sink}
  - `journal.recovery.unknown_outcomes` {count}
  - `journal.retention.changed` {days, company_max_days}
  - `journal.prune.done` {cutoff, removed, failed}; `journal.compact.done` {engine, duration_ms}
  - Never logged: event `data`, entry `data` and `summary`, destinations of external actions, report content, paths of artefacts (only counts), the database connection string.
  - `journal.support_export.done` {lines, truncated}; `journal.erase.done` {rows}; `journal.ping.failed` (debug)
  - **No metrics and no spans.** This crate has no Telemetry edge (2B-3), and Telemetry's metric list is closed and holds no journal family (reconciliation row 64, R4); the logs above are the record.

## Gaps

None.
