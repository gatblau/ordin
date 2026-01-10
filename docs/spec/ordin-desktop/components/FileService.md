# SPEC: File Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-104; promoted to Approved by the user January 2026) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-files` · **Build phase:** 4 · **Programs:** D P A
> **Dependencies:** Agent Folder Manager, Rules Engine, Journal

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
- [Amendment January 2026 — Moving a file (ADR-104)](#amendment-january-2026--moving-a-file-adr-104)

Line references: `02:N` is `docs/design/ordin-desktop/02-architecture.md` line N; `04:N`, `07:N`, `08:N`, `10:N`, `11:N` likewise; `09 ADR-0NN` is an ADR in `09-decisions.md`; `2C` is `docs/spec/ordin-desktop/phase-2c-shared-types-*.md`; `WR-FileTree` is `agent-wright-runtime/docs/spec/components/FileTree.md`.

## Purpose

The File Service is the person's friendly access to the file system (02:719, 09 ADR-015). It keeps **places** — folders the person added with the system's folder picker or by dropping them, each workspace's own folder, agent folders (never listed), and the working-copy folders of SharePoint and Confluence links — and shows them by friendly name, never by path. It lists folders in pages for places of up to 100,000 files (A-21), previews the common kinds, keeps a **version** of every file a run changes, imports dropped files, moves removed files to the system's bin, **watches** places and keeps a local **index** of names and document text that answers search across a workspace within 1 s (A-22). It hands the per-workspace folder grants that the Workspace Service owns to the Agent Folder Manager and the Rules Engine (02:718). It records every **published copy** of an original and holds the person's own edits to Markdown documents until they go back (02:719, 09 ADR-048, ADR-069). In the agent profile it keeps the hosted agent's **work folder** in the datastore under one namespace per agent, with the bundle read-only, writes limited to the configured paths, and a duty's `STOP` and `escalation.md` protected from the agent (02:720, 09 ADR-098).

## Approach

One module owns every table about files: places, the file index (SQLite FTS5, SA-DB-01), versions with content-addressed copies under the application data root (04 *Data Lifecycle*, "File versions"), published copies, held edits and the agent-profile work folder. Watching uses the `notify` crate with a start-up rescan that compares size, modification time and hash against the index, so changes made while ordin was closed are found (D-8, *assumed here*: a watcher alone misses offline changes). The work folder ports wrightrun's FileTree semantics (`WR-FileTree`) onto `sqlx` tables instead of a LangGraph store. Rejected: one index database file per place (cross-workspace search would open up to 20 files per query and miss A-21's 1 s); reading files on demand for search (A-22 rejects it as too slow for 100,000 files).

## Shared Context

### Identifiers, time and paging (2C part 1 *Conventions*; SA-PAGE-01)

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// RFC 3339, UTC, millisecond precision, `Z` suffix (TS_NOT_RFC3339_MS_UTC on read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub DateTime<Utc>);

// Each is a newtype over a UUIDv7 serialised as the 36-character lowercase string (ID_NOT_UUIDV7).
pub struct PlaceId(pub Uuid);           pub struct WorkspaceId(pub Uuid);
pub struct FolderGrantId(pub Uuid);     pub struct FileVersionId(pub Uuid);
pub struct PublishedCopyId(pub Uuid);   pub struct KnowledgePageId(pub Uuid);
pub struct ConfluenceLinkId(pub Uuid);  pub struct AgentInstanceId(pub Uuid);
pub struct CrewCharterId(pub Uuid);     pub struct RunId(pub Uuid);

/// SHA-256 as 64 lowercase hexadecimal characters (HASH_NOT_SHA256_HEX).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String);

// SA-PAGE-01 page form: `ordin_types::{Page, Cursor, PAGE_SIZE}` (2C domain, *Paging*), copied here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Always serialised; `null` on the last page.
    pub next_cursor: Option<Cursor>,
}
/// Opaque: base64url of the JSON form of the last row's sort key; at most 1,024 characters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(String);
impl Cursor {
    pub fn encode<K: Serialize>(key: &K) -> Cursor;
    pub fn decode<K: serde::de::DeserializeOwned>(&self) -> Result<K, TypesError>;
}
pub const PAGE_SIZE: usize = 100;
```

### Domain types this component owns or reads (2C part 1, copied unchanged)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Place {
    pub id: PlaceId,
    pub friendly_name: String,
    pub path: String,
    pub kind: PlaceKind,
    pub added_at: Timestamp,
}

/// 2C domain (amended under decision 80 with `WorkingCopy` and `CodingTaskWorktree`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind { PersonsFolder, WorkspaceFolder, AgentFolder, WorkingCopy, CodingTaskWorktree }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FolderGrant {
    pub id: FolderGrantId,
    pub workspace_id: WorkspaceId,
    pub grantee: Grantee,
    pub place_id: PlaceId,
    pub access: FolderAccess,
    pub granted_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Grantee {
    AgentInstance { agent_instance_id: AgentInstanceId },
    Crew { crew_charter_id: CrewCharterId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess { Read, ReadAndChange }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileVersion {
    pub id: FileVersionId,
    pub place_id: PlaceId,
    pub path: String,
    pub version: u32,
    pub hash: Sha256Hex,
    pub origin: FileVersionOrigin,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileVersionOrigin {
    Person,
    Run { run_id: RunId, tool: String },
    ArtCraftEdit { app: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PublishedCopy {
    pub id: PublishedCopyId,
    pub original: CopyOriginal,
    pub destination: CopyDestination,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_item_id: Option<String>,
    pub format: CopyFormat,
    pub maker: CopyMaker,
    pub published_via: PublishedVia,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followed_branch: Option<String>,
    pub keep_up_to_date: bool,
    pub audience: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_version_published: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_version_pushed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<Timestamp>,
    pub state: PublishedCopyState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyOriginal {
    PlaceFile { place_id: PlaceId, path: String },
    KnowledgePage { knowledge_page_id: KnowledgePageId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyDestination {
    ConfluencePage { space: String, parent_page_id: String },
    ConfluenceAttachment { space: String, parent_page_id: String },
    SharepointFile { site: String, library: String, folder: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyFormat { Page, Word, Pdf, Slides, ExactFile }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyMaker { Ordin, WordCraft, PdfCraft, DeckCraft, None }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublishedVia {
    ConfluenceLink { confluence_link_id: ConfluenceLinkId },
    PublishACopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishedCopyState {
    ReadyToPublish, AwaitingApproval, Current, OutOfDate, Updating,
    ChangedAtDestination, Failed, NotKeptUpToDate, Removed,
}
```

Validation carried from 2C (the codes are 2C's): `Place.friendly_name` 1–120 chars → `PLACE_NAME_INVALID`; `Place.path` absolute, ≤ 4096 bytes, unique → `PLACE_PATH_INVALID`; `FileVersion.path` relative, ≤ 4096 bytes → `FVER_PATH_INVALID`; `(place_id, path, version)` unique, versions 1, 2, 3 … without gaps → `FVER_VERSION_GAP`; the latest version of a path is never pruned → `FVER_LATEST_PRUNE`; `PublishedCopy`: `exact_file` only with `confluence_attachment` and `maker = none` → `PUB_FORMAT_MAKER_MISMATCH`; `page` only with `confluence_page` → `PUB_FORMAT_DESTINATION_MISMATCH`; `followed_branch` only for a repository original → `PUB_BRANCH_UNEXPECTED`; `audience` 1–500 chars → `PUSH_AUDIENCE_GREW` (the growth rule and its code are the Push Service's, EH-D4); `failure_reason` present iff `state = failed`, ≤ 500 chars → `PUB_FAILURE_REASON_MISMATCH`.

`PublishedCopy` state machine (2C, owned here):

| From | Event | To |
|---|---|---|
| — | new file in a linked folder | ready_to_publish |
| — | **Publish a copy…** | awaiting_approval |
| ready_to_publish | person checks it | awaiting_approval |
| awaiting_approval | approved and pushed | current |
| current | original gets a newer version | out_of_date |
| out_of_date | keep up to date on and change reached origin, or person approves | updating |
| updating | push succeeds | current |
| updating | push fails | failed |
| failed | **Try again** | updating |
| current, out_of_date | destination has a newer version than pushed | changed_at_destination |
| changed_at_destination | person brings change into original, or leaves it to be replaced | current |
| any non-removed | person turns updates off | not_kept_up_to_date |
| not_kept_up_to_date | person turns updates on | out_of_date |
| any non-removed | person removes it after the check | removed |

### Boundary interfaces of other components (each provider's current Public Interface; R3, R5, R6)

```rust
// ordin-journal (JournalAndArtefactStore.md *Datastore*, *Migration registry*, *Journal*, *Prune participants*)
#[derive(Clone)]
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
impl Datastore { pub fn sqlite(&self) -> Option<&sqlx::SqlitePool>; pub fn postgres(&self) -> Option<&sqlx::PgPool>; }
/// (version, description, SQL); versions 1, 2, 3, … within a module.
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations {
    pub module: &'static str,
    pub sqlite: &'static [MigrationStep],
    pub postgres: &'static [MigrationStep],
}
/// Concrete type; consumers hold `Arc<ordin_journal::Journal>` (reconciliation row 41).
impl Journal {
    pub fn datastore(&self) -> &Datastore;
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    pub fn register_prune_participant(&self, participant: std::sync::Arc<dyn PruneParticipant>);
}
pub struct NewJournalEntry {
    pub kind: String,                 // ^[a-z][a-z0-9_]{0,63}$
    pub component: String,            // "files"
    pub run_id: Option<RunId>,
    pub crew_run_id: Option<CrewRunId>,
    pub workspace_id: Option<WorkspaceId>,
    pub summary: String,              // one plain sentence, 1–300 characters, never file content or a path
    pub data: serde_json::Value,      // at most 65,536 bytes serialised
}
#[async_trait::async_trait]
pub trait PruneParticipant: Send + Sync {
    fn module(&self) -> &'static str;
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure>;
}
pub struct PruneRequest { pub cutoff: Timestamp, pub runs_in_progress: Vec<RunId> }
pub struct PruneFailure { pub code: &'static str }
// `JournalError` is the Journal's enum; this component matches `Unavailable` and maps every variant
// to `FILE_DATASTORE_UNAVAILABLE` (retryable when `Unavailable`, permanent otherwise).

// ordin-rules (RulesEngine.md *The engine*); concrete type, held as `Arc<ordin_rules::RulesEngine>`
pub struct GrantedPlace { pub grantee: Grantee, pub place_id: PlaceId, pub root: std::path::PathBuf, pub access: FolderAccess }
impl RulesEngine {
    /// Called by: File Service, at start and on every grant change.
    pub fn replace_grants(&self, workspace_id: WorkspaceId, grants: Vec<GrantedPlace>);
    /// Whether a folder may never become a place (H.4, K.6).
    pub fn is_protected_location(&self, path: &std::path::Path) -> bool;
}

// ordin-agent-folders (AgentFolderManager.md); concrete type, held as `Arc<ordin_agent_folders::AgentFolderManager>`
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceGrant { pub grant: FolderGrant, pub place_path: std::path::PathBuf, pub place_kind: PlaceKind }
impl AgentFolderManager {
    pub async fn set_workspace_grants(&self, workspace_id: WorkspaceId, grants: Vec<WorkspaceGrant>)
        -> Result<(), AfmError>;
}
// `AfmError` is the Agent Folder Manager's enum (`AFM_*` codes); `retryable()` decides the retry class.
```

### Reconciliation assumptions

- **SA-RECON-01** The File Service reaches its tables through the Journal's `Datastore` (`journal.datastore()`), not a pool handed over by the program (decision D4, ruling R20); its migrations are the tuple form `pub fn migrations() -> ModuleMigrations` (R3).
- **SA-RECON-02** The Journal, Rules Engine and Agent Folder Manager are concrete types; the File Service calls their inherent methods (`append_entry`, `replace_grants`, `is_protected_location`, `set_workspace_grants`) and declares no trait of its own for them (reconciliation rows 41, 199–201, 600; R5, R6).
- **SA-RECON-03** History pruning reaches this module only through its `PruneParticipant`, which the program registers with `Journal::register_prune_participant` (reconciliation row O3).
- **SA-RECON-04** The Workspace Service owns `working_copy_items` and their states (R12); the File Service holds only the bytes and versions of working-copy places. The Push Service's `WorkingCopyStore` is implemented by the program over the Workspace Service and this component's `read_file`, `versions`, `read_version` and `restore_version` (decision D3).
- **SA-RECON-05** The File Service does not implement the MCP Front's `ToolProvider`; the Agent Loader wraps `WorkFolder` and registers it (2B-2 D4, R2). Tool names follow R9.
- **SA-RECON-06** In the agent profile the program implements the Rules Engine's `RulesHost` work-folder facts (`path_exists`, `path_contains`, `read_text`, `append_line`, `remove`) over `WorkFolder::operator_exists`, `operator_read`, `operator_write` and `operator_remove` (decision D3).
- **SA-RECON-07** Service API command names are this module's own list, adopted by the catalogue (R1). The File Service appends no Service API event: a tree-visible change is announced as `workspace_changed` by the Workspace Service, which calls every place-changing method here (reconciliation row 673). ServiceApi.md's `workspace_changed` emitter list no longer names the File Service (closing decision K3).
- **SA-RECON-08** The work-folder codes `WF_*` are wrightrun's and keep their names; only ordin's workflow-file codes take `WFF_` (decision D8).
- **SA-RECON-09** `notify` is accepted as a named licence exception (decision D1); the other new libraries are accepted under decision D2.

### Constants (chosen here unless cited)

| Constant | Value | Source |
|---|---|---|
| `MAX_PLACE_FILES` | 100,000 files per place indexed; files beyond are listed but not indexed | A-21 |
| `MAX_FOLDER_DEPTH` | 20 levels indexed and watched | A-21 |
| `PAGE_SIZE` | 100 rows | SA-PAGE-01 |
| `READ_LIMIT_BYTES` | 64 MiB for `read_file` and `read_version`; larger is refused | *assumed here*: bounds core memory per call |
| `PREVIEW_TEXT_BYTES` | first 256 KiB of a text or Markdown file | *assumed here* |
| `INDEX_TEXT_BYTES` | first 1 MiB of extracted text per file | *assumed here*: bounds the index at 100,000 files |
| `INDEX_MAX_SOURCE_BYTES` | files above 50 MiB are indexed by name only | *assumed here* |
| `WATCH_DEBOUNCE` | 500 ms per path | *assumed here* |
| `SNIPPET_CHARS` | 160 characters around the first match | *assumed here*; one line in the search dialog |
| `WF_MAX_FILE_BYTES` | 10,485,760 bytes | WR-FileTree `MAX_FILE_BYTES`; 07 *Scale* (agent profile) |
| `WF_MAX_PATH_CHARS` | 1,024 | WR-FileTree |
| `WF_BUNDLE_DIR_PREFIXES` | `/.apm/`, `/knowledge/`, `/acceptance/`, `/brand/` | WR-FileTree |
| `WF_BUNDLE_FILES` | `/AGENTS.md`, `/wrightrun-manifest.json`, `/wrightrun-declaration.yaml` | WR-FileTree; ADR-098 keeps the declaration's file name |
| `WF_NAMESPACE_PATTERN` | `^[A-Za-z0-9_@+:~-]{1,64}$` | WR-FileTree |
| `INCOMING_REFUSED_EXTENSIONS` | `exe`, `msi`, `bat`, `cmd`, `com`, `scr`, `pif`, `ps1`, `vbs`, `vbe`, `js`, `jse`, `wsf`, `jar`, `app`, `dmg`, `pkg`, `sh`, `command`, `dll`, `so`, `dylib`, `lnk`, `docm`, `xlsm`, `pptm` | *assumed here*: the HLD names the check (02:399, 09:1576) without its rules |

### Libraries (1A, 05, and B7 contract D-11)

`sqlx` 0.9.0 (SA-DB-01, SQLite with FTS5; PostgreSQL for the work folder in the scaled shape), `tokio`, `serde`, `serde_json`, `sha2`, `base64`, `thiserror`, `tracing` (SA-OBS-01), `uuid` 1.28.0 (SA-ID-01). Accepted under decisions D1 and D2 (05 rows): `notify` 8.2.0 (CC0-1.0, named licence exception, D1), `trash` 5.2.9 (MIT), `pulldown-cmark` 0.13.4 (MIT), `zip` 9.0.5 (MIT), `quick-xml` 0.42.0 (MIT), `pdf-extract` 0.12.1 (MIT).

## Public Interface

Error prefix: `FILE_` for this component's own codes; the 2C validation codes above; `WF_` for the work folder (codes carried from wrightrun, SA-ERR-01).

### Construction and registration

```rust
pub struct FileServiceDeps {
    /// The datastore is `journal.datastore()` (SA-RECON-01).
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    pub rules: std::sync::Arc<ordin_rules::RulesEngine>,
    pub agent_folders: std::sync::Arc<ordin_agent_folders::AgentFolderManager>,
    /// The application data root (ADR-072); versions live under `<root>/versions/`.
    pub data_root: std::path::PathBuf,
}

pub struct FileService { /* private */ }

impl FileService {
    /// This module's migrations: `ModuleMigrations { module: "files", sqlite: &[(1, …), (2, …)], postgres: &[(1, …)] }`.
    /// The program passes the value to `MigrationRegistry::register` before start (R3).
    pub fn migrations() -> ModuleMigrations;
    pub async fn new(deps: FileServiceDeps) -> Result<std::sync::Arc<FileService>, FileError>;
    /// Starts the watcher task and the start-up rescan of every watched place. Returns at once.
    pub async fn start(self: &std::sync::Arc<Self>) -> Result<(), FileError>;
    /// Flushes pending index writes and stops the watcher (SA-XC-13).
    pub async fn shutdown(&self);
}
```

### Places

```rust
pub struct AddPlace { pub path: std::path::PathBuf, pub friendly_name: Option<String>, pub kind: PlaceKind }

impl FileService {
    pub async fn add_place(&self, req: AddPlace) -> Result<Place, FileError>;
    pub async fn rename_place(&self, place_id: PlaceId, friendly_name: String) -> Result<Place, FileError>;
    /// Deletes the row and its index, versions bookkeeping stays (see Internal Logic 3); never touches the folder.
    pub async fn remove_place(&self, place_id: PlaceId) -> Result<(), FileError>;
    pub async fn place(&self, place_id: PlaceId) -> Result<Place, FileError>;
    /// Places in `ids`, in the given order; an unknown id is skipped. Agent folders are never returned
    /// unless `include_hidden` is true (02:722: "Agent folders are places too but are never listed").
    pub async fn places(&self, ids: &[PlaceId], include_hidden: bool) -> Result<Vec<Place>, FileError>;
    /// The place holding an absolute path, and the path relative to it (deepest place root wins).
    /// Used by the Craft Toolkit to record a tool's output file as a version; `None` when no place holds it.
    pub async fn locate(&self, absolute: &std::path::Path) -> Result<Option<(PlaceId, String)>, FileError>;
    /// Hands the grants of one workspace to the Agent Folder Manager and the Rules Engine (B7 contract D-7).
    pub async fn publish_grants(&self, workspace_id: WorkspaceId, grants: Vec<FolderGrant>) -> Result<(), FileError>;
}
```

### Listing, reading, previews

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderSort { NameAsc, NameDesc, KindAsc, ChangedDesc, ChangedAsc, ChangedByAsc }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileEntry {
    pub place_id: PlaceId,
    /// Relative to the place, `/`-separated; a folder ends with `/`.
    pub path: String,
    pub name: String,
    pub is_folder: bool,
    /// Lower-case extension without the dot; empty for a folder or no extension.
    pub kind: String,
    pub size_bytes: u64,
    pub changed_at: Timestamp,
    /// "Ordin", an agent instance's display name supplied by the writer, "you", or the
    /// origin's author recorded by `set_attribution`; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    pub has_versions: bool,
}

#[derive(Debug, Clone)]
pub struct FileBytes { pub place_id: PlaceId, pub path: String, pub bytes: Vec<u8>, pub hash: Sha256Hex, pub media_type: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preview {
    /// The WebView draws it from the bytes (png, jpeg, gif, webp, svg as an image, never as markup).
    Image { media_type: String, bytes_b64: String },
    /// The WebView's own PDF viewer draws it.
    Pdf { bytes_b64: String },
    /// Plain text or Markdown, the first PREVIEW_TEXT_BYTES; `truncated` when longer.
    Text { text: String, markdown: bool, truncated: bool },
    /// A platform thumbnail (macOS Quick Look); PNG.
    Thumbnail { png_b64: String },
    /// No preview for this kind on this platform; the screens show the kind's icon.
    None,
}

impl FileService {
    pub async fn list_folder(&self, place_id: PlaceId, folder: &str, sort: FolderSort,
                             cursor: Option<Cursor>) -> Result<Page<FileEntry>, FileError>;
    pub async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError>;
    pub async fn preview(&self, place_id: PlaceId, path: &str) -> Result<Preview, FileError>;
    /// Records who changed a file at its origin, for the "changed by" column (working-copy places).
    pub async fn set_attribution(&self, place_id: PlaceId, path: &str, changed_by: String) -> Result<(), FileError>;
}
```

### Versions, writes, import, bin

```rust
pub struct WriteVersion {
    pub place_id: PlaceId,
    pub path: String,
    pub bytes: Vec<u8>,
    pub origin: FileVersionOrigin,
    /// When present, the write is refused if the file's current hash differs (optimistic check).
    pub expected_hash: Option<Sha256Hex>,
}

impl FileService {
    pub async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError>;
    /// Writes bytes read from an origin into a `working_copy` place (the Workspace Service's reads,
    /// ADR-029). No version row and no PlaceChange: fetching is nobody's change. Returns the hash.
    pub async fn store_fetched(&self, place_id: PlaceId, path: &str, bytes: Vec<u8>,
                               changed_by: Option<String>, changed_at: Timestamp) -> Result<Sha256Hex, FileError>;
    pub async fn versions(&self, place_id: PlaceId, path: &str) -> Result<Vec<FileVersion>, FileError>;
    pub async fn read_version(&self, version_id: FileVersionId) -> Result<FileBytes, FileError>;
    /// Writes an earlier version's bytes back as a new version with origin Person.
    pub async fn restore_version(&self, version_id: FileVersionId) -> Result<FileVersion, FileError>;
    /// Copies dropped files into a place folder; returns the new entries in drop order.
    pub async fn import_files(&self, place_id: PlaceId, folder: &str, sources: Vec<std::path::PathBuf>)
        -> Result<Vec<FileEntry>, FileError>;
    /// The latest version row of a path; `None` when the path has no version history.
    /// Called by: Knowledge Service (freshness of a knowledge page's source).
    pub async fn current_version(&self, place_id: PlaceId, path: &str) -> Result<Option<FileVersion>, FileError>;
    /// Keeps the file's bytes as a version, then moves the file to the system bin (Internal Logic 7.2).
    /// The ticket undoes it through `restore_from_bin`. Called by: Service API `files.move_to_bin`, Knowledge Service.
    pub async fn move_to_bin(&self, place_id: PlaceId, path: &str) -> Result<BinTicket, FileError>;
    /// Writes the kept bytes back at the ticket's path as a new version with origin Person (Internal Logic 7.3).
    /// Called by: Knowledge Service (undo of a removed knowledge page), Service API `files.restore_from_bin`.
    pub async fn restore_from_bin(&self, ticket: &BinTicket) -> Result<FileVersion, FileError>;
    /// Deletes version copies older than `before`, keeping the latest version of every path (04 *Data Lifecycle*).
    /// Called by this crate's `PruneParticipant` (below) with the Journal's cut-off (A-08, D3).
    pub async fn prune_versions(&self, before: Timestamp) -> Result<PruneReport, FileError>;
    /// The Journal's prune participant for module "files" (D3; JournalAndArtefactStore `PruneParticipant`).
    /// The program passes it to `Journal::register_prune_participant` at start.
    pub fn prune_participant(self: &std::sync::Arc<Self>) -> std::sync::Arc<dyn ordin_journal::PruneParticipant>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PruneReport { pub versions_removed: u64, pub bytes_freed: u64 }

/// Undo record of one `move_to_bin`; the bytes are the version `file_version_id` in the version store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BinTicket { pub place_id: PlaceId, pub path: String, pub file_version_id: FileVersionId }
```

### Watching, changes, search

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlaceChange {
    pub place_id: PlaceId,
    pub path: String,
    pub change: PlaceChangeKind,
    pub observed_at: Timestamp,
    /// True when the change is a write this service made for a run, a Craft output or a restore;
    /// false for the person's own edits, in ordin or in another app.
    pub by_ordin: bool,
    /// The version recorded for this change (Internal Logic 5.5 and 6.7); absent when the path has no
    /// version history and none was written. Added for the Knowledge Service (reconciliation row 362).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// The origin of that version; absent exactly when `version` is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<FileVersionOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaceChangeKind { Created, Modified, Removed, Renamed { from: String } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchResults { pub hits: Vec<SearchHit>, pub coverage: IndexCoverage }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchHit {
    pub place_id: PlaceId,
    pub path: String,
    pub name_match: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    pub changed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IndexCoverage { pub indexed: u64, pub total: u64 }

impl FileService {
    pub async fn watch(&self, place_ids: &[PlaceId]) -> Result<(), FileError>;
    pub async fn unwatch(&self, place_ids: &[PlaceId]);
    pub fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<PlaceChange>;
    /// `query` 1–200 chars; `limit` 1–200. Name matches first, then text matches by FTS5 rank.
    pub async fn search(&self, place_ids: &[PlaceId], query: &str, limit: u32) -> Result<SearchResults, FileError>;
    pub async fn coverage(&self, place_ids: &[PlaceId]) -> Result<IndexCoverage, FileError>;
}
```

### Published copies and held edits

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HeldEdit {
    pub place_id: PlaceId,
    pub path: String,
    pub file_version_id: FileVersionId,
    pub saved_at: Timestamp,
}

impl FileService {
    /// Inserts, or replaces the row with the same id; checks the 2C rules and the state transition.
    pub async fn record_published_copy(&self, c: PublishedCopy) -> Result<PublishedCopy, FileError>;
    pub async fn set_published_copy_state(&self, id: PublishedCopyId, state: PublishedCopyState,
                                          failure_reason: Option<String>) -> Result<PublishedCopy, FileError>;
    pub async fn published_copy(&self, id: PublishedCopyId) -> Result<PublishedCopy, FileError>;
    pub async fn published_copies_of(&self, original: &CopyOriginal) -> Result<Vec<PublishedCopy>, FileError>;
    pub async fn published_copies_via_link(&self, link: ConfluenceLinkId) -> Result<Vec<PublishedCopy>, FileError>;
    /// The person's Save on a Markdown document (ADR-048): a new version with origin Person, held until it goes back.
    pub async fn save_person_edit(&self, place_id: PlaceId, path: &str, bytes: Vec<u8>) -> Result<FileVersion, FileError>;
    pub async fn held_edits(&self, place_ids: &[PlaceId]) -> Result<Vec<HeldEdit>, FileError>;
    pub async fn mark_edit_sent(&self, place_id: PlaceId, path: &str) -> Result<(), FileError>;
}
```

### Incoming files and grant-checked reads

```rust
/// Result of `save_incoming`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SavedFile {
    pub place_id: PlaceId,
    /// Relative to the place; the file name, numbered " (2)", " (3)" … when the name is taken.
    pub path: String,
    /// True when the safety check found hidden text (Internal Logic 11.3); the file is saved and the
    /// caller reports it as for documents (02:404).
    pub hidden_content_found: bool,
}

impl FileService {
    /// Runs the incoming-file safety check (Internal Logic 11), then writes a new file in `place_id`,
    /// which must be a place of kind `workspace_folder` (the workspace's own folder, which the caller reads
    /// from `Workspace.own_folder_place_id`). Never overwrites: a taken name gets a number.
    /// Called by: Mail Service (`email_save_attachment`), Telegram Service (`telegram_save_file`),
    /// Channel Gateway (files received in chat).
    pub async fn save_incoming(&self, place_id: PlaceId, file_name: &str, bytes: Vec<u8>,
                               origin_note: &str) -> Result<SavedFile, FileError>;
    /// Reads one file for an agent instance, only when the workspace's last published grants give that
    /// instance a grant on `place_id` (Internal Logic 12). `max_bytes` ≤ `READ_LIMIT_BYTES`.
    /// Called by: the program's `PlaceFiles` implementation for the Sandbox, Mail Service, Telegram Service.
    pub async fn read_for_agent(&self, agent_instance_id: AgentInstanceId, workspace_id: WorkspaceId,
                                place_id: PlaceId, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, FileError>;
    /// "<friendly name> › <folder> › <file>" for the person (Mail's `place_display_path`); no absolute path.
    pub async fn display_path(&self, place_id: PlaceId, path: &str) -> Result<String, FileError>;
}
```

A person's own read (Mail's `read_for_person`) is `read_file` followed by the caller's size test; no separate method is added.

### Work folder (agent profile only; ADR-098, WR-FileTree)

```rust
pub struct WorkFolderConfig {
    /// `^[A-Za-z0-9_@+:~-]{1,64}$`; the agent configuration's work namespace, else the package name.
    pub namespace: String,
    /// The obtained release's bundle folder on local disk (read-only).
    pub bundle_dir: std::path::PathBuf,
    /// None: every work-folder path writable; Some(empty): none writable (WR-FileTree step 4).
    pub writable_paths: Option<Vec<String>>,
    /// `<duty folder>STOP` and `<duty folder>escalation.md` of each declared duty.
    pub protected_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkFileEntry { pub path: String, pub size_bytes: u64, pub modified_at: Timestamp, pub bundle: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GrepMatch { pub path: String, pub line: u32, pub text: String }

/// What the agent's file tools call (the Agent Loader wraps it as the `files` tool provider and
/// registers it with the MCP Front; see *Agent file tools*).
/// Every method returns `Ok` with data, or `Err(AgentRefusal)` whose text is given to the model verbatim.
pub struct WorkFolder { /* private */ }

impl WorkFolder {
    pub fn new(datastore: Datastore, config: WorkFolderConfig) -> Result<WorkFolder, FileError>;
    pub async fn ls(&self, path: &str) -> Result<Vec<WorkFileEntry>, AgentRefusal>;
    pub async fn read(&self, path: &str, offset: u32, limit: u32) -> Result<String, AgentRefusal>;
    pub async fn write(&self, path: &str, content: &str) -> Result<(), AgentRefusal>;
    pub async fn edit(&self, path: &str, old: &str, new: &str, replace_all: bool) -> Result<u32, AgentRefusal>;
    pub async fn delete(&self, path: &str) -> Result<(), AgentRefusal>;
    pub async fn glob(&self, pattern: &str, path: Option<&str>) -> Result<Vec<String>, AgentRefusal>;
    pub async fn grep(&self, pattern: &str, path: Option<&str>, max_count: Option<u32>) -> Result<Vec<GrepMatch>, AgentRefusal>;
    // Operator routes (no writable-path or protected-path test; WR-FileTree `WorkFolder`)
    pub async fn operator_list(&self, prefix: &str, cursor: Option<Cursor>) -> Result<Page<WorkFileEntry>, FileError>;
    pub async fn operator_read(&self, path: &str) -> Result<Vec<u8>, FileError>;
    pub async fn operator_write(&self, path: &str, content: Vec<u8>) -> Result<(), FileError>;
    pub async fn operator_remove(&self, path: &str) -> Result<(), FileError>;
    /// True when a work-folder file exists at the path (bundle paths → `WF_BUNDLE_PATH`). Called by: Caller API.
    pub async fn operator_exists(&self, path: &str) -> Result<bool, FileError>;
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct AgentRefusal(pub String);

/// WR-FileTree `MAX_FILE_BYTES`; equals the constant `WF_MAX_FILE_BYTES`. Called by: Caller API.
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
/// The work-folder path rule of Internal Logic 10.2; `Err(FileError::WorkFolder("WF_INVALID_PATH"))`.
/// Called by: Caller API, and every `WorkFolder` method.
pub fn normalise_path(path: &str) -> Result<String, FileError>;
/// True for a normalised path in `WF_BUNDLE_FILES` or under a `WF_BUNDLE_DIR_PREFIXES` entry. Called by: Caller API.
pub fn is_bundle_path(normalised: &str) -> bool;
```

### Agent file tools (agent profile only; reconciliation row 360, R2, R9)

The File Service does not implement the MCP Front's `ToolProvider`: it has no dependency edge to the MCP Front, which sits in the same build phase (2B-2 D4). The **Agent Loader** wraps a `WorkFolder` built with `WorkFolder::new` as the provider (`provider_id` "files", one server `files`) and registers it. This table is the File Service's contract for that wrapper: tool names, arguments, the `WorkFolder` method each one calls, and the `plain` sentence.

| Tool (server `files`) | Arguments | `WorkFolder` method | `plain` |
|---|---|---|---|
| `ls` | `{ "path": string }` | `ls` | "Listed files in the work folder" |
| `read_file` | `{ "path": string, "offset": u32?, "limit": u32? }` | `read` | "Read a file in the work folder" |
| `write_file` | `{ "path": string, "content": string }` | `write` | "Wrote a file in the work folder" |
| `edit_file` | `{ "path": string, "old": string, "new": string, "replace_all": bool? }` | `edit` | "Changed a file in the work folder" |
| `delete_file` | `{ "path": string }` | `delete` | "Removed a file from the work folder" |
| `glob` | `{ "pattern": string, "path": string? }` | `glob` | "Looked for files by name in the work folder" |
| `grep` | `{ "pattern": string, "path": string?, "max_count": u32? }` | `grep` | "Searched text in the work folder" |

Every tool is `sandbox_callable = false`; the names fit both the MCP Front pattern and wrightrun's (R9). An `Err(AgentRefusal(text))` becomes `ToolFailure { code: "FILE_AGENT_REFUSED", message: text, retryable: false }`, the text verbatim (WR-FileTree). Called by: Agent Loader (step L-16).

### Errors

```rust
#[derive(Debug, Clone, thiserror::Error)]
pub enum FileError {
    #[error("FILE_PLACE_NOT_FOUND")] PlaceNotFound,
    #[error("FILE_NOT_FOUND")] NotFound,
    #[error("FILE_PROTECTED_LOCATION")] ProtectedLocation,
    #[error("FILE_FOLDER_UNREADABLE")] FolderUnreadable,
    #[error("FILE_PLACE_EXISTS")] PlaceExists { existing: PlaceId },
    #[error("FILE_PATH_ESCAPES_PLACE")] PathEscapesPlace,
    #[error("FILE_TOO_LARGE")] TooLarge { limit_bytes: u64 },
    #[error("FILE_CHANGED_MEANWHILE")] ChangedMeanwhile { current: Sha256Hex },
    #[error("FILE_WRITE_FAILED")] WriteFailed,
    #[error("FILE_BIN_UNAVAILABLE")] BinUnavailable,
    #[error("FILE_VERSION_NOT_FOUND")] VersionNotFound,
    #[error("FILE_VERSION_COPY_MISSING")] VersionCopyMissing,
    #[error("FILE_COPY_NOT_FOUND")] CopyNotFound,
    #[error("FILE_COPY_TRANSITION_INVALID")] CopyTransitionInvalid { from: PublishedCopyState, to: PublishedCopyState },
    #[error("FILE_QUERY_INVALID")] QueryInvalid,
    #[error("FILE_GRANT_HANDOFF_FAILED")] GrantHandoffFailed,
    #[error("FILE_WATCH_FAILED")] WatchFailed,
    #[error("FILE_DATASTORE_UNAVAILABLE")] DatastoreUnavailable,
    #[error("FILE_NOT_GRANTED")] NotGranted,
    #[error("FILE_INCOMING_REFUSED")] IncomingRefused { reason: String },
    #[error("FILE_RESTORE_TARGET_EXISTS")] RestoreTargetExists,
    /// A 2C validation code (PLACE_*, FVER_*, PUB_*), carried as its string.
    #[error("{0}")] Invalid(&'static str),
    /// Work folder (WR-FileTree codes).
    #[error("{0}")] WorkFolder(&'static str),
}
impl FileError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

### Service API commands (*chosen here*; `ordin-service-api`, SA-API-01)

| Command | Input | Result |
|---|---|---|
| `files.add_place` | `{ "path": string (from the system folder picker), "friendly_name": string? }` | `Place` |
| `files.rename_place` | `{ "place_id", "friendly_name" }` | `Place` |
| `files.list_folder` | `{ "place_id", "folder": string, "sort": FolderSort, "cursor": string? }` | `Page<FileEntry>` |
| `files.preview` | `{ "place_id", "path" }` | `Preview` |
| `files.versions` | `{ "place_id", "path" }` | `Vec<FileVersion>` |
| `files.restore_version` | `{ "file_version_id" }` | `FileVersion` |
| `files.import` | `{ "place_id", "folder", "sources": [string] }` (dropped paths) | `Vec<FileEntry>` |
| `files.move_to_bin` | `{ "place_id", "path" }` | `BinTicket` |
| `files.restore_from_bin` | `{ "ticket": BinTicket }` | `FileVersion` |
| `files.download` | `{ "place_id", "path" }` | `{ "name": string, "media_type": string, "bytes_b64": string }` (over `read_file`; the desktop's **Save a copy** writes it) |
| `files.save_edit` | `{ "place_id", "path", "text": string }` | `FileVersion` |
| `files.published_copies` | `{ "original": CopyOriginal }` | `Vec<PublishedCopy>` |
| `files.search` | `{ "place_ids": [PlaceId], "query", "limit" }` | `SearchResults` |

These names are the catalogue's names (R1: the catalogue adopts each module's command list). Callers: the Service API catalogue; the screens through it.

The screens never pass a typed path except `files.add_place` and `files.import`, whose paths come from the system folder picker or a drop event (02:722 "A path is never typed").

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| `add_place`, `rename_place`, `remove_place`, `places`, `watch`, `unwatch`, `publish_grants`, `store_fetched`, `held_edits` | Workspace Service |
| `place` | Craft Toolkit, Project Service, Push Service |
| `read_file` | Craft Toolkit, Project Service, Knowledge Service, Confluence Adapter, Push Service, Home and Search (version check), Ordin Host (immediate-start `PlaceFile` input), Telegram Service, Mail Service |
| `write_version` | Craft Toolkit, Project Service, Knowledge Service, Workspace Service |
| `locate` | Craft Toolkit |
| `list_folder` | Confluence Adapter |
| `add_place` (kind `coding_task_worktree`), `remove_place`, `watch`, `unwatch`, `coverage`, `subscribe_changes` | Project Service |
| `subscribe_changes` (`PlaceChange`) | Push Service (originals saved, ADR-069), Home and Search, Knowledge Service, Project Service, Workspace Service; the desktop and personal-server programs, which forward each event to `MemoryService::source_changed` (OrdinDesktop.md, OrdinServer.md *Program-side implementations and calls*) |
| `current_version` | Knowledge Service, Push Service |
| `move_to_bin`, `restore_from_bin` | Knowledge Service |
| `record_published_copy`, `set_published_copy_state` | Push Service, Confluence Adapter (linked-folder copies) |
| `published_copy`, `published_copies_of`, `read_version`, `save_person_edit`, `mark_edit_sent` | Push Service |
| `published_copies_via_link` | Confluence Adapter |
| `versions` | Confluence Adapter (linked-folder sync), Push Service (reading an original at a version) |
| `read_file`, `versions`, `read_version`, `restore_version` (working-copy places) | the desktop and personal-server programs' `WorkingCopyStore` for the Push Service (`content`: `read_file`, or `versions` then `read_version` for a `local_version`; `restore_version`: `versions` then `restore_version`) |
| `search`, `coverage` | Home and Search |
| `save_incoming` | Mail Service, Telegram Service |
| `read_for_agent`, `display_path` | program (`PlaceFiles` for the Sandbox), Mail Service, Telegram Service |
| `WorkFolder` operator methods, `normalise_path`, `is_bundle_path`, `MAX_FILE_BYTES` | Caller API |
| `WorkFolder::new` and its agent methods (the *Agent file tools* table) | Agent Loader |
| `prune_participant` | program (registers it with the Journal) |

### Example

```rust
let place = files.add_place(AddPlace {
    path: "/Users/asha/Documents/Supplier quotes".into(), friendly_name: None, kind: PlaceKind::PersonsFolder,
}).await?;
// Place { id: 0192a0f2-2222-7333-8444-555566667777, friendly_name: "Supplier quotes",
//         path: "/Users/asha/Documents/Supplier quotes", kind: PersonsFolder, added_at: 2026-01-10T09:12:04.511Z }

let page = files.list_folder(place.id, "Q3/", FolderSort::ChangedDesc, None).await?;
// Page { items: [FileEntry { path: "Q3/Kestrel quote.pdf", name: "Kestrel quote.pdf", is_folder: false,
//         kind: "pdf", size_bytes: 184320, changed_at: 2026-01-10T16:40:00.000Z, changed_by: Some("you"),
//         has_versions: false }, …], next_cursor: None }

let v = files.write_version(WriteVersion {
    place_id: place.id, path: "Q3/summary.md".into(), bytes: b"# Q3 summary\n".to_vec(),
    origin: FileVersionOrigin::Run { run_id, tool: "write_file".into() }, expected_hash: None,
}).await?;
// FileVersion { version: 2, origin: Run { .. }, .. }   (version 1 is the person's file as it was before)

let r = files.search(&[place.id], "Kestrel day rate", 20).await?;
// SearchResults { hits: [SearchHit { path: "Q3/Kestrel quote.pdf", name_match: true,
//   snippet: Some("…day rate of £640 for senior consultants…"), .. }], coverage: { indexed: 41200, total: 58000 } }
```

`files.list_folder` over the Service API:

```json
{"api_version":{"major":1,"minor":0},"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","command":"files.list_folder",
 "backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b",
 "input":{"place_id":"0192a0f2-2222-7333-8444-555566667777","folder":"Q3/","sort":"changed_desc","cursor":null}}
```

## Internal Logic

### 1. Start-up

1. `migrations()` returns module `files` with SQLite steps 1 (places, file index, versions, published copies, held edits) and 2 (work folder), and PostgreSQL step 1 (work folder, the only table of the scaled shape). The program registers the value; `MigrationRegistry::apply` runs it (SA-XC-07). Every "Journal `<kind>`" below is `journal.append_entry(NewJournalEntry { kind, component: "files", workspace_id, run_id, summary, data })` with a summary that names no path and no content; a `JournalError` from it is logged and does not fail the operation that already completed.
2. `new` checks `<data_root>/versions/` exists, creating it with mode `0700` on macOS and Linux; failure → `FILE_WRITE_FAILED` (permanent; the program stops with Diagnostics).
3. `start` loads the watched set from `places` rows with `watched = 1`, starts one `notify::RecommendedWatcher` per place root (recursive, depth ≤ `MAX_FOLDER_DEPTH` enforced on events), and spawns the start-up rescan task (step 5.3) for each. A place root that no longer exists is logged `place_missing` (fields `place_id`) and left unwatched; it is not removed.

### 2. Adding, renaming and removing places

1. `add_place`: canonicalise `path` (resolve symbolic links); refuse when not absolute or > 4096 bytes (`PLACE_PATH_INVALID`).
2. Call `rules.is_protected_location(path)`; true → `FILE_PROTECTED_LOCATION` (words, 11 §5.6: "This folder belongs to the system and can't be added.").
3. Check that the folder exists and its entries can be read by the person's account (`std::fs::read_dir`); failure → `FILE_FOLDER_UNREADABLE` (sequence *Add a folder*, error posture: "the person is told plainly, and the place is not added").
4. A row with the same canonical path exists → `FILE_PLACE_EXISTS { existing }`; callers treat it as success and reuse the existing place (a place may belong to several workspaces, 04:65).
5. `friendly_name` defaults to the folder's last path segment; trimmed, 1–120 chars, no control characters → else `PLACE_NAME_INVALID`.
6. Insert the row (UUIDv7 id), `watched = 0`. Journal `place_added` with `place_id`, `kind` (no path in the journal entry data shown to agents; the path is stored in the row only).
7. `kind = agent_folder` and `kind = coding_task_worktree` rows are hidden: `places(.., include_hidden = false)` and every Service API listing skip them.
8. `rename_place`: validate as step 5; update; journal `place_renamed`.
9. `locate(absolute)`: canonicalise; find the place rows whose `path` is a prefix of it at a path-segment boundary; choose the longest; return its id and the relative `/`-separated remainder (normalised as step 4.1). No match → `None`.
10. `remove_place`: unwatch; delete the place's `file_index` and `file_index_fts` rows and its `held_edits`; keep `file_versions` rows until pruned (they hold the journal's history); delete the `places` row. The folder on disk is never touched (04:57). Removing a place that holds an original with published copies is allowed; those copies' rows are left unchanged, and the Push Service reports such an original as unreadable when it next updates them. Journal `place_removed`.

### 3. Grant hand-off (`publish_grants`, D-7)

1. For each grant, read the place row; a grant whose place no longer exists is dropped and logged `grant_place_missing`.
2. Build `WorkspaceGrant { grant, place_path, place_kind }` for the Agent Folder Manager and `GrantedPlace { grantee: grant.grantee, place_id, root: place_path, access: grant.access }` for the Rules Engine.
3. Call `agent_folders.set_workspace_grants(workspace_id, workspace_grants)`, then `rules.replace_grants(workspace_id, granted_places)`. The Rules Engine call is made even when the first call fails, so enforcement is never more permissive than the stored grants; an `AfmError` from the first call returns `FILE_GRANT_HANDOFF_FAILED` (retryable when `AfmError::retryable()`, else permanent).
4. Journal `grants_published` with `workspace_id` and the count.

### 4. Listing, reading, previews

1. Every relative path argument is normalised: `/`-separated, no leading `/`, no `..` segment, no NUL, ≤ 4096 bytes; the joined absolute path, after resolving symbolic links, must lie under the place root → else `FILE_PATH_ESCAPES_PLACE`.
2. `list_folder` reads from `file_index` for indexed places (keyset query on the sort key, `LIMIT 101`; the 101st row only sets `next_cursor`), so the first page of a 100,000-file place answers from SQLite, not the disk (A-21). For a place whose index is still being built (`index_state != 'ready'`), it reads the folder from disk with `read_dir`, sorts in memory by the sort key and pages the same way.
3. The cursor is `Cursor::encode(&(sort_key_value, path))`, ≤ 1,024 chars; `Cursor::decode` failing → `SRV_BAD_CURSOR` (SA-PAGE-01).
4. `read_file`: size > `READ_LIMIT_BYTES` → `FILE_TOO_LARGE`; missing → `FILE_NOT_FOUND`. The hash is computed while reading. `media_type` from the extension table of the File Service (`png` → `image/png`, … *chosen here*, the IANA type for each extension the index knows; unknown → `application/octet-stream`).
5. `preview`: images (`png`, `jpg`, `jpeg`, `gif`, `webp`, `svg`) ≤ 20 MiB → `Image`; `pdf` ≤ 64 MiB → `Pdf`; text-like kinds (`txt`, `md`, `markdown`, `csv`, `json`, `yaml`, `yml`, `toml`, `xml`, `log`, source files the index lists as text) → `Text` with the first `PREVIEW_TEXT_BYTES` decoded as UTF-8 with replacement; office kinds (`docx`, `xlsx`, `pptx`, `doc`, `xls`, `ppt`, `odt`, `ods`, `odp`, `rtf`) → on macOS run `/usr/bin/qlmanage -t -s 1024 -o <tmp> <file>` with a 10 s limit and return `Thumbnail`; on Windows and Linux, and when `qlmanage` fails, → `None` *(assumed here: 02:724 "where the platform offers a preview"; a Windows shell-thumbnail call needs a new library)*. An SVG is returned as image bytes and the WebView draws it in an `<img>`, never inline (ADR-028).

### 5. Watching and the index

1. `watch(place_ids)`: set `watched = 1`, start a watcher on each root, enqueue a full rescan when `index_state` is `none`. `unwatch`: stop watchers, set `watched = 0`; the index is kept for search until the place is removed.
2. Watcher events are debounced per path for `WATCH_DEBOUNCE`, then for each path: `stat` it; if gone → delete its index row and emit `PlaceChange::Removed`; if new or `(size, mtime)` differ from the index → hash it, update the row, re-extract text (step 5.4), and emit `Created` or `Modified`. A rename pair from the watcher emits `Renamed { from }`. Paths deeper than `MAX_FOLDER_DEPTH` and files beyond `MAX_PLACE_FILES` in a place are skipped and counted in `index_skipped`.
3. Start-up rescan (and the first index build): walk the place breadth-first; for each file compare `(size, mtime)` with the index; on difference hash and update as in step 2, emitting `PlaceChange` with `observed_at` = now. Index rows whose file is gone emit `Removed`. Progress (`indexed`, `total`) is kept in `places.index_indexed` and `places.index_total` for `coverage`. The walk yields to other tasks every 500 files.
4. Text extraction for the index, by kind: text-like kinds as UTF-8 with replacement; `md`/`markdown` through `pulldown-cmark`, keeping text events only; `docx`, `pptx`, `xlsx` by reading `word/document.xml`, `ppt/slides/slide*.xml`, `xl/sharedStrings.xml` from the archive (`zip`) and keeping text nodes (`quick-xml`); `pdf` through `pdf-extract`; other kinds and files above `INDEX_MAX_SOURCE_BYTES`: name only. Extracted text is cut at `INDEX_TEXT_BYTES`. Extraction runs on a blocking thread with a 30 s limit per file *(assumed here)*; a failure or timeout indexes the name only and logs `index_extract_failed` (fields `place_id`, `kind`, `error_class`; never the path's content).
5. Every changed path whose file has version history and whose new hash differs from the latest version's hash gets a new `file_versions` row with origin `Person`, unless the change was made by this service within the last 2 s for the same hash (`by_ordin` writes are recorded in step 6, not here).
6. `PlaceChange.by_ordin` is true when the path and hash match a write this service made in step 6.4 within the last 10 s (an in-memory map of recent writes, *chosen here*); otherwise false. `PlaceChange.version` and `.origin` carry the version row written for the change (step 5.5 or step 6.7, the recent-writes map holding the version written by step 6); both are absent when no row was written. Subscribers: the Push Service (a person's new version of a local original, ADR-069), the Confluence Adapter's linked-folder sync, Home and Search (changed marks), the Knowledge Service (freshness), the Project Service (the engineer's edits), the Workspace Service (working-copy item states).
7. The broadcast channel holds 4,096 changes *(chosen here)*; a lagging subscriber receives `RecvError::Lagged` and must re-read state through queries.

### 6. Writing versions (`write_version`)

1. Normalise the path (step 4.1). `bytes.len() > READ_LIMIT_BYTES` → `FILE_TOO_LARGE`.
2. When the place is not of kind `persons_folder`, `workspace_folder`, `working_copy` or `coding_task_worktree` → `FILE_PROTECTED_LOCATION` (agent folders are written by their sessions directly, not through this service).
3. Read the current file, if any, and its hash. `expected_hash` present and different → `FILE_CHANGED_MEANWHILE { current }` (retryable after re-read).
4. Baseline: when the file exists and has no `file_versions` row, store its current bytes as version 1 with origin `Person`, so every file a run changes keeps its earlier content (ADR-015).
5. Store the new bytes' copy at `<data_root>/versions/<hash[0..2]>/<hash>` if not already present (write to a temporary file in the same folder, `fsync`, rename).
6. Write the file in the place atomically: temporary file in the same folder, `fsync`, rename over the target; record the path and hash in the recent-writes map (step 5.6). Failure → `FILE_WRITE_FAILED` (retryable), and the version row is not inserted.
7. Insert `file_versions` with `version = max(version) + 1` in one transaction with the `file_index` update; a unique violation (concurrent writer) retries once from step 3.
8. Journal `file_version_written` with `place_id`, `version`, `origin.kind`, `run_id` when present (no content, no path text beyond the place-relative path).
9. `restore_version`: read the version's copy (missing → `FILE_VERSION_COPY_MISSING`), then step 1–8 with origin `Person`.
10. `store_fetched`: the place must be of kind `working_copy` (else `FILE_PROTECTED_LOCATION`); normalise the path; write atomically as step 6.6 but record the path in a *fetched-writes* map instead of the recent-writes map, so the watcher event that follows is dropped and no `PlaceChange` is broadcast; when the path already has version history, store the fetched bytes as a new version with origin `Person` and the origin author kept in `file_index.changed_by` (the origin's new version is a version of the document); update `file_index` (`changed_at` = the origin's `changed_at`, `changed_by`) and its text. Journal nothing (the Workspace Service journals the read).

### 7. Import and bin

1. `import_files`: for each source, refuse a protected location (rules) with `FILE_PROTECTED_LOCATION`. A dropped folder becomes a place through the Workspace Service, so this method accepts files only and refuses a folder source with `FILE_QUERY_INVALID`. Copy each file into `place/folder/` under its name; an existing name gets " (2)", " (3)" … before the extension. The copy is `Created` with `by_ordin = false` (the person dropped it). Journal `files_imported` with the count.
2. `move_to_bin`: normalise the path (step 4.1); missing → `FILE_NOT_FOUND`. Keep the file's current bytes as a version (steps 6.4–6.5 and 6.7 with origin `Person`; when the latest version already holds the same hash, reuse it). Then call `trash::delete(path)`; failure (no bin on the platform, permission) → `FILE_BIN_UNAVAILABLE` and nothing is deleted. Return `BinTicket { place_id, path, file_version_id }`. The application never deletes a file in a place permanently (02:725). Journal `file_binned` with `place_id`, `file_version_id`.
3. `restore_from_bin(ticket)`: the place must exist (`FILE_PLACE_NOT_FOUND`); a file now at `ticket.path` → `FILE_RESTORE_TARGET_EXISTS` (nothing overwritten); read the version copy (missing → `FILE_VERSION_COPY_MISSING`); write it with steps 6.1–6.8 and origin `Person`. The item in the system bin is left there (the `trash` crate restores only on Windows and Linux, so one rule serves all three systems). Journal `file_restored` with `place_id`, `file_version_id`.

### 8. Search

1. `query`: trimmed, 1–200 chars, else `FILE_QUERY_INVALID`. The FTS5 query is built from the words of `query`, each quoted as a string token with `*` appended to the last (prefix match); no FTS5 syntax from the person reaches the engine.
2. Name matches: `file_index.name LIKE '%' || ? || '%'` over `place_id IN (…)`, ordered by `changed_at DESC`, up to `limit`.
3. Text matches: `file_index_fts MATCH ?` joined to `file_index` with `place_id IN (…)`, ordered by `bm25(file_index_fts)`, up to `limit` minus name matches; `snippet(file_index_fts, 1, '', '', '…', 24)` cut to `SNIPPET_CHARS`.
4. Return name matches then text matches, de-duplicated by `(place_id, path)`, with `coverage` summed over the places.

### 9. Published copies and held edits

1. `record_published_copy`: validate the 2C rules; for an existing id, check the transition against the state table (else `FILE_COPY_TRANSITION_INVALID`); upsert; journal `published_copy_recorded` with `id`, `state`.
2. `set_published_copy_state`: same transition check; `failure_reason` required iff `failed`; `last_updated_at = now` when the new state is `current`.
3. `published_copies_of` and `published_copies_via_link` read by indexed columns `original_key` and `confluence_link_id`.
4. `save_person_edit`: `write_version` with origin `Person`, then upsert `held_edits(place_id, path, file_version_id, saved_at)`. A held edit is listed until `mark_edit_sent` deletes it (the Push Service calls it after the push). For a place of kind `persons_folder` or `workspace_folder` the saved file is the origin itself, so no held edit is recorded and the `PlaceChange` (by_ordin = false) is what the Push Service uses for copies (ADR-069 "a new version saved there by the person").

### 10. Work folder (agent profile)

Ported from WR-FileTree steps 1–26 with these mappings:

1. `new`: `namespace` must match `WF_NAMESPACE_PATTERN` → else `WF_INVALID_NAMESPACE`; `bundle_dir` must be a folder; each writable path is normalised and must not be a bundle path → `FileError::WorkFolder("WF_INVALID_PATH")`.
2. Path rules: `normalise_path` refuses a path that does not start with `/`, holds NUL, a `..` segment or `~`, or exceeds 1,024 chars; collapses `//` and `.`. Bundle path: in `WF_BUNDLE_FILES`, or under a `WF_BUNDLE_DIR_PREFIXES` entry, or equal to one without its trailing `/`.
3. Reads (`ls`, `read`, `glob`, `grep`): a bundle path is served from `bundle_dir` (read-only file access, `read` with `offset` and `limit` in lines, default limit 2,000); any other path from `work_folder_files`. `ls("/")` joins the bundle's bundle paths and the work folder's top level, sorted by path. `glob` uses shell-style patterns (`*`, `?`, `**`) implemented in this crate; `grep` matches a Rust `regex` (linear time, as ADR-098 item 4) line by line, `max_count` default 100.
4. Writes (`write`, `edit`, `delete`), first failing test wins: bundle path → `Error: <path> is read-only (bundle path)`; `delete` of `/` → `Error: / cannot be deleted`; `delete` of a protected path or a folder holding one → `Error: <path> cannot be removed by the agent; a person removes <protected> with the operator's remove command`; not writable → `Error: <path> is outside the writable paths of this agent`; content above `WF_MAX_FILE_BYTES` → `Error: <path> exceeds the limit of 10485760 bytes`. Writing `STOP` or `escalation.md` is allowed; only their removal is refused.
5. Storage errors → `Error: the work folder cannot be reached`, log `work_folder_unreachable` (fields `operation`, `error_class`).
6. Text content is stored as UTF-8 with `encoding = 'utf-8'`; bytes that are not UTF-8 (operator writes) as base64 with `encoding = 'base64'`.
7. Operator methods: no writable-path or protected-path test; a bundle path → `WF_BUNDLE_PATH`; missing → `WF_NOT_FOUND`; too large → `WF_TOO_LARGE`; storage error → `WF_STORE_UNAVAILABLE`; `operator_list` pages by path with `PAGE_SIZE`.
8. The run's ledger flag `changed_work_folder` (WR-FileTree step 18) is reported by returning `true` from a successful write; the Session Supervisor records it (Interaction).

### 11. Incoming-file safety check and `save_incoming`

1. `file_name`: take the last path segment, trim, refuse empty, NUL, control characters, `/`, `\`, a leading `.`, or more than 255 bytes → `FILE_INCOMING_REFUSED { reason: "The file's name can't be used." }`. `bytes.len() > READ_LIMIT_BYTES` → `FILE_TOO_LARGE`. The place must be of kind `workspace_folder` → else `FILE_PROTECTED_LOCATION`.
2. Refuse (`FILE_INCOMING_REFUSED`, nothing written) when the lower-case extension is in `INCOMING_REFUSED_EXTENSIONS`, or the first bytes are a program header (`MZ`; `0x7F 'E' 'L' 'F'`; Mach-O `FE ED FA CE`, `FE ED FA CF`, `CE FA ED FE`, `CF FA ED FE`, `CA FE BA BE`), reason "This file can run as a program, so ordin didn't save it."; or the file is an Office archive holding `vbaProject.bin` (read with `zip`), reason "This document holds macros, so ordin didn't save it.".
3. Hidden text: for text-like kinds, Markdown and the Office kinds of step 5.4, extract text as step 5.4 and set `hidden_content_found` when it holds a zero-width character (U+200B–U+200D, U+2060, U+FEFF) or a Unicode tag character (U+E0000–U+E007F). The file is still saved.
4. Choose the name: when `<name>` exists in the place root, try `<stem> (2).<ext>`, `(3)` … up to `(999)`, else `FILE_WRITE_FAILED`.
5. Write with steps 6.1–6.8, origin `Person` (the person is the receiver; `origin_note` is kept in the journal entry, not in the version). Journal `file_received` with `place_id`, `path`, `bytes`, `hidden_content_found`, `origin_note` (at most 200 characters).

### 12. Grant-checked reads (`read_for_agent`, `display_path`)

1. `publish_grants` (section 3) also replaces the in-memory map `granted: HashMap<WorkspaceId, Vec<WorkspaceGrant>>` with the grants it handed on. The Workspace Service publishes every workspace's grants at its start, so the map is complete once the Workspace Service has started; before that every `read_for_agent` returns `FILE_NOT_GRANTED`.
2. `read_for_agent`: a grant in `granted[workspace_id]` with `grantee = Grantee::AgentInstance { agent_instance_id }` and `place_id` equal must exist → else `FILE_NOT_GRANTED`. Normalise the path (step 4.1); `max_bytes` above `READ_LIMIT_BYTES` is lowered to it; file larger than `max_bytes` → `FILE_TOO_LARGE`; missing → `FILE_NOT_FOUND`. Return the bytes. Nothing is journalled here (the tool call is journalled by the MCP Front).
3. `display_path`: the place's `friendly_name`, then each folder segment and the file name, joined with " › ".

### 13. Agent file tools (served through the Agent Loader)

1. The Agent Loader builds the `WorkFolder` with `WorkFolderConfig { namespace, bundle_dir, writable_paths, protected_paths }` and the agent datastore; `WorkFolder::new`'s error is returned to it unchanged.
2. Each tool of the *Agent file tools* table calls exactly its `WorkFolder` method with the arguments named there; `read_file` defaults `offset` 0 and `limit` 2,000 lines; `edit_file` defaults `replace_all` false; `grep` defaults `max_count` 100.
3. A refusal text from Internal Logic 10.4–10.5 is given to the model verbatim as `FILE_AGENT_REFUSED`.

### 14. Prune participant

1. `prune_participant().module()` is `"files"`; the program passes it to `journal.register_prune_participant` at start. `prune(store, request)` calls `prune_versions(request.cutoff)` and returns `versions_removed`; a `FileError` becomes `PruneFailure` with its code. Version rows are not linked to runs in progress beyond the latest version per path, which is always kept, so `runs_in_progress` needs no test here.

## Data Model

SQLite (desktop, personal server, agent single shape). Migration 1:

```sql
CREATE TABLE places (
    id              TEXT PRIMARY KEY,                 -- UUIDv7
    friendly_name   TEXT NOT NULL CHECK (length(friendly_name) BETWEEN 1 AND 120),
    path            TEXT NOT NULL UNIQUE CHECK (length(path) BETWEEN 1 AND 4096),
    kind            TEXT NOT NULL CHECK (kind IN ('persons_folder','workspace_folder','agent_folder',
                                                  'working_copy','coding_task_worktree')),
    added_at        TEXT NOT NULL,
    watched         INTEGER NOT NULL DEFAULT 0 CHECK (watched IN (0,1)),
    index_state     TEXT NOT NULL DEFAULT 'none' CHECK (index_state IN ('none','building','ready')),
    index_indexed   INTEGER NOT NULL DEFAULT 0,
    index_total     INTEGER NOT NULL DEFAULT 0,
    index_skipped   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE file_index (
    place_id    TEXT NOT NULL REFERENCES places(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,                        -- relative, '/'-separated; folders end with '/'
    parent      TEXT NOT NULL,                        -- the folder part, '' for the root
    name        TEXT NOT NULL,
    is_folder   INTEGER NOT NULL CHECK (is_folder IN (0,1)),
    kind        TEXT NOT NULL,
    size_bytes  INTEGER NOT NULL,
    mtime_ns    INTEGER NOT NULL,
    hash        TEXT,                                 -- NULL for folders
    changed_at  TEXT NOT NULL,
    changed_by  TEXT,
    text_state  TEXT NOT NULL DEFAULT 'pending' CHECK (text_state IN ('pending','text','name_only')),
    PRIMARY KEY (place_id, path)
);
CREATE INDEX file_index_parent_name    ON file_index (place_id, parent, name);
CREATE INDEX file_index_parent_changed ON file_index (place_id, parent, changed_at);
CREATE INDEX file_index_name           ON file_index (name);

CREATE VIRTUAL TABLE file_index_fts USING fts5 (
    place_id UNINDEXED, path UNINDEXED, body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE file_versions (
    id          TEXT PRIMARY KEY,
    place_id    TEXT NOT NULL,                        -- no FK: versions outlive a removed place until pruned
    path        TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    version     INTEGER NOT NULL CHECK (version >= 1),
    hash        TEXT NOT NULL CHECK (length(hash) = 64),
    origin      TEXT NOT NULL,                        -- JSON FileVersionOrigin
    created_at  TEXT NOT NULL,
    UNIQUE (place_id, path, version)
);
CREATE INDEX file_versions_created ON file_versions (created_at);
CREATE INDEX file_versions_hash    ON file_versions (hash);

CREATE TABLE published_copies (
    id                          TEXT PRIMARY KEY,
    original                    TEXT NOT NULL,        -- JSON CopyOriginal
    original_key                TEXT NOT NULL,        -- 'place:<place_id>:<path>' or 'page:<knowledge_page_id>'
    destination                 TEXT NOT NULL,        -- JSON CopyDestination
    destination_item_id         TEXT,
    format                      TEXT NOT NULL CHECK (format IN ('page','word','pdf','slides','exact_file')),
    maker                       TEXT NOT NULL CHECK (maker IN ('ordin','word_craft','pdf_craft','deck_craft','none')),
    published_via               TEXT NOT NULL,        -- JSON PublishedVia
    confluence_link_id          TEXT,                 -- set when published_via is confluence_link
    followed_branch             TEXT,
    keep_up_to_date             INTEGER NOT NULL CHECK (keep_up_to_date IN (0,1)),
    audience                    TEXT NOT NULL CHECK (length(audience) BETWEEN 1 AND 500),
    original_version_published  TEXT,
    destination_version_pushed  TEXT,
    last_updated_at             TEXT,
    state                       TEXT NOT NULL CHECK (state IN ('ready_to_publish','awaiting_approval','current',
                                   'out_of_date','updating','changed_at_destination','failed',
                                   'not_kept_up_to_date','removed')),
    failure_reason              TEXT CHECK (failure_reason IS NULL OR length(failure_reason) <= 500),
    CHECK ((state = 'failed') = (failure_reason IS NOT NULL))
);
CREATE INDEX published_copies_original ON published_copies (original_key);
CREATE INDEX published_copies_link     ON published_copies (confluence_link_id);

CREATE TABLE held_edits (
    place_id         TEXT NOT NULL,
    path             TEXT NOT NULL,
    file_version_id  TEXT NOT NULL REFERENCES file_versions(id),
    saved_at         TEXT NOT NULL,
    PRIMARY KEY (place_id, path)
);
```

`maker` values: serde writes `CopyMaker::WordCraft` as `word_craft` under `rename_all = "snake_case"`; the CHECK lists exactly those strings.

Migration 2 (work folder). SQLite:

```sql
CREATE TABLE work_folder_files (
    namespace    TEXT NOT NULL,
    path         TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 1024),
    content      TEXT NOT NULL,
    encoding     TEXT NOT NULL CHECK (encoding IN ('utf-8','base64')),
    size_bytes   INTEGER NOT NULL CHECK (size_bytes <= 10485760),
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL,
    PRIMARY KEY (namespace, path)
);
```

PostgreSQL (agent profile, scaled shape — the only table this module keeps there):

```sql
CREATE TABLE work_folder_files (
    namespace    TEXT NOT NULL,
    path         TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 1024),
    content      TEXT NOT NULL,
    encoding     TEXT NOT NULL CHECK (encoding IN ('utf-8','base64')),
    size_bytes   BIGINT NOT NULL CHECK (size_bytes <= 10485760),
    created_at   TIMESTAMPTZ NOT NULL,
    modified_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (namespace, path)
);
CREATE INDEX work_folder_files_prefix ON work_folder_files (namespace, path text_pattern_ops);
```

Rollback: both migrations only create tables; rollback is the backup copy of the SQLite file (SA-XC-07). In the scaled shape a release that drops the table is never shipped within one minor line (ADR-098 item 5). Removing a place deletes its index rows only; files on disk are never deleted by any statement of this module.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Place id unknown | `Err(permanent)` | `FILE_PLACE_NOT_FOUND` | `{"error":"That folder is no longer in ordin.","code":"FILE_PLACE_NOT_FOUND","action":null}` |
| File missing at the path | `Err(permanent)` | `FILE_NOT_FOUND` | `{"error":"That document is no longer there.","code":"FILE_NOT_FOUND","action":null}` |
| Folder is a protected system location | `Err(permanent)` | `FILE_PROTECTED_LOCATION` | `{"error":"This folder belongs to the system and can't be added.","code":"FILE_PROTECTED_LOCATION","action":null}` |
| Folder cannot be read by the person's account | `Err(permanent)` | `FILE_FOLDER_UNREADABLE` | `{"error":"ordin can't open this folder with your account.","code":"FILE_FOLDER_UNREADABLE","action":null}` |
| Folder already a place | `Err(permanent)`; callers reuse `existing` | `FILE_PLACE_EXISTS` | `{"error":"This folder is already in ordin.","code":"FILE_PLACE_EXISTS","action":null}` |
| Friendly name empty, too long or with control characters | `Err(permanent)` | `PLACE_NAME_INVALID` | `{"error":"Give the folder a name of up to 120 characters.","code":"PLACE_NAME_INVALID","action":null}` |
| Path not absolute or over 4096 bytes | `Err(permanent)` | `PLACE_PATH_INVALID` | `{"error":"ordin can't use that folder.","code":"PLACE_PATH_INVALID","action":null}` |
| Relative path escapes its place (`..`, link outside) | `Err(permanent)` | `FILE_PATH_ESCAPES_PLACE` | `{"error":"That document isn't in this folder.","code":"FILE_PATH_ESCAPES_PLACE","action":null}` |
| Read or write above 64 MiB | `Err(permanent)` | `FILE_TOO_LARGE` | `{"error":"This document is too large to open in ordin.","code":"FILE_TOO_LARGE","action":null}` |
| `expected_hash` differs from the file on disk | `Err(retryable)` | `FILE_CHANGED_MEANWHILE` | `{"error":"This document changed while ordin was working on it.","code":"FILE_CHANGED_MEANWHILE","action":"try_again"}` |
| Disk write or rename fails | `Err(retryable)` | `FILE_WRITE_FAILED` | `{"error":"ordin couldn't save the document.","code":"FILE_WRITE_FAILED","action":"try_again"}` |
| Version number gap or concurrent insert after one retry | `Err(retryable)` | `FVER_VERSION_GAP` | `{"error":"ordin couldn't save the document.","code":"FVER_VERSION_GAP","action":"try_again"}` |
| Prune would remove a path's latest version | `Err(permanent)` (defect guard) | `FVER_LATEST_PRUNE` | — (internal; never reaches the person) |
| System bin unavailable | `Err(permanent)` | `FILE_BIN_UNAVAILABLE` | `{"error":"This computer's Bin isn't available, so nothing was moved.","code":"FILE_BIN_UNAVAILABLE","action":null}` |
| Version id unknown | `Err(permanent)` | `FILE_VERSION_NOT_FOUND` | `{"error":"That version is no longer kept.","code":"FILE_VERSION_NOT_FOUND","action":null}` |
| Version copy file missing under the data root | `Err(permanent)` | `FILE_VERSION_COPY_MISSING` | `{"error":"That version is no longer kept.","code":"FILE_VERSION_COPY_MISSING","action":"send_to_it"}` |
| Published copy id unknown | `Err(permanent)` | `FILE_COPY_NOT_FOUND` | `{"error":"That copy is no longer listed.","code":"FILE_COPY_NOT_FOUND","action":null}` |
| Published copy state change not in the state table | `Err(permanent)` (caller defect) | `FILE_COPY_TRANSITION_INVALID` | — (internal) |
| Published copy fields break a 2C rule | `Err(permanent)` | `PUB_FORMAT_MAKER_MISMATCH`, `PUB_FORMAT_DESTINATION_MISMATCH`, `PUB_BRANCH_UNEXPECTED`, `PUB_FAILURE_REASON_MISMATCH` | — (internal; the Push Service builds rows) |
| Search query empty or over 200 chars; folder given to `import_files` | `Err(permanent)` | `FILE_QUERY_INVALID` | `{"error":"Type what to look for.","code":"FILE_QUERY_INVALID","action":null}` |
| Malformed cursor | `Err(permanent)` | `SRV_BAD_CURSOR` | `{"error":"ordin couldn't load more of this list.","code":"SRV_BAD_CURSOR","action":"try_again"}` |
| Agent Folder Manager refused the grants (`AfmError`) | `Err(retryable)` when `AfmError::retryable()`, else `Err(permanent)` | `FILE_GRANT_HANDOFF_FAILED` | — (internal; the Workspace Service retries) |
| Watcher cannot be started on a place | `Err(retryable)` | `FILE_WATCH_FAILED` | — (logged; search still answers from the last index) |
| Agent instance has no grant on the place (`read_for_agent`) | `Err(permanent)` | `FILE_NOT_GRANTED` | `{"error":"This agent hasn't been given that folder.","code":"FILE_NOT_GRANTED","action":null}` |
| Incoming file fails the safety check | `Err(permanent)` | `FILE_INCOMING_REFUSED` | `{"error":"<reason sentence of Internal Logic 11>","code":"FILE_INCOMING_REFUSED","action":null}` |
| A file already exists where a binned file would be restored | `Err(permanent)` | `FILE_RESTORE_TARGET_EXISTS` | `{"error":"A document with that name is already there, so nothing was put back.","code":"FILE_RESTORE_TARGET_EXISTS","action":null}` |
| Agent file tool refused by the work folder | tool failure | `FILE_AGENT_REFUSED` | the refusal text of Internal Logic 10.4–10.5, verbatim |
| Datastore unreachable | `Err(retryable)` | `FILE_DATASTORE_UNAVAILABLE` | `{"error":"ordin couldn't reach its own records.","code":"FILE_DATASTORE_UNAVAILABLE","action":"try_again"}` |
| Work folder: invalid path (operator) | 400 | `WF_INVALID_PATH` | `{"error":"a work-folder path starts with / and holds no .. segment","code":"WF_INVALID_PATH"}` |
| Work folder: bundle path to operator method | 400 | `WF_BUNDLE_PATH` | `{"error":"a bundle path is not part of the work folder","code":"WF_BUNDLE_PATH"}` |
| Work folder: no file (operator) | 404 | `WF_NOT_FOUND` | `{"error":"no file exists at this path","code":"WF_NOT_FOUND"}` |
| Work folder: above 10,485,760 bytes (operator) | 413 | `WF_TOO_LARGE` | `{"error":"a work-folder file holds at most 10485760 bytes","code":"WF_TOO_LARGE"}` |
| Work folder: store unreachable (operator) | 503 | `WF_STORE_UNAVAILABLE` | `{"error":"the work folder cannot be reached","code":"WF_STORE_UNAVAILABLE"}` |
| Work folder: namespace does not match its pattern | exit 1 at load | `WF_INVALID_NAMESPACE` | `error[WF_INVALID_NAMESPACE]: the work namespace holds 1 to 64 of the characters A-Z a-z 0-9 _ @ + : ~ - (work-namespace)` |
| Work folder: agent write refused (bundle, writable paths, protected, size, store) | tool result with `is_error` | none | `Error: /AGENTS.md is read-only (bundle path)` (texts of Internal Logic 10.4–10.5) |

Status for HTTP surfaces is the Caller API operator-route status (SA-ENV-02); Service API refusals reply with `kind: "error"` and status 200 on the Remote API (LW-4). The sentences are this component's proposals for the Phase 4 error registry; "This folder belongs to the system and can't be added." is quoted from 11 §5.6.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: File Service

  Scenario: Happy path — add a place and list its first page
    Given the folder "/Users/asha/Documents/Supplier quotes" holds 250 files
    When add_place is called with that path and kind persons_folder
    Then a Place named "Supplier quotes" is returned and journalled as place_added
    And list_folder with sort name_asc returns 100 entries and a next_cursor
    And the third page returns 50 entries and next_cursor null

  Scenario: Error path — a protected system location is refused
    Given the Rules Engine reports "/System" as protected
    When add_place is called with "/System"
    Then it fails with FILE_PROTECTED_LOCATION and no place row exists

  Scenario: Error path — an unreadable folder is refused
    Given a folder the person's account cannot list
    When add_place is called with it
    Then it fails with FILE_FOLDER_UNREADABLE

  Scenario: Edge case — the same folder added twice is one place
    Given "Supplier quotes" is already a place
    When add_place is called with the same canonical path through a symbolic link
    Then it fails with FILE_PLACE_EXISTS naming the existing place id

  Scenario: Happy path — a run's write keeps the earlier content as version 1
    Given "Q3/summary.md" exists with no version history
    When write_version is called with origin run "0192a1c2-3344-7556-8677-8899aabbccdd" and tool "write_file"
    Then versions("Q3/summary.md") returns version 1 with origin person and version 2 with origin run
    And the file on disk holds the new bytes
    And a PlaceChange Modified with by_ordin true is broadcast

  Scenario: Error path — a write after the file changed is refused
    Given the person edited "Q3/summary.md" after the agent read hash "aa…"
    When write_version is called with expected_hash "aa…"
    Then it fails with FILE_CHANGED_MEANWHILE and the file is unchanged

  Scenario: Error path — a path that escapes the place is refused
    When read_file is called with path "../secrets.txt"
    Then it fails with FILE_PATH_ESCAPES_PLACE

  Scenario: Edge case — a change made while ordin was closed is found at start
    Given "Q3/rates.md" was edited in another app while ordin was not running
    When the File Service starts and rescans the watched place
    Then a PlaceChange Modified with by_ordin false is broadcast for "Q3/rates.md"
    And, because the file has version history, a new version with origin person is recorded

  Scenario: Happy path — search finds a name and a text match within one second
    Given 20 workspaces whose places hold 100,000 indexed files in all
    When search is called for "Kestrel day rate" over every place
    Then the first results are returned within 1 second
    And name matches come before text matches, each text match with a snippet of at most 160 characters

  Scenario: Edge case — search while the index is still being built reports coverage
    Given a place whose index holds 41,200 of 58,000 files
    When search is called over that place
    Then coverage is indexed 41200 and total 58000

  Scenario: Error path — the system bin is unavailable
    Given the platform reports no bin for the volume
    When move_to_bin is called
    Then it fails with FILE_BIN_UNAVAILABLE and the file still exists

  Scenario: Happy path — grants reach the Agent Folder Manager and the Rules Engine
    When publish_grants is called for workspace W with one read-and-change grant on place P
    Then the Agent Folder Manager receives one WorkspaceGrant and the Rules Engine one GrantedPlace, each with P's path

  Scenario: Error path — a published copy cannot skip a state
    Given a published copy in state ready_to_publish
    When set_published_copy_state is called with current
    Then it fails with FILE_COPY_TRANSITION_INVALID

  Scenario: Edge case — a person's saved edit to a working-copy document is held
    Given "Supplier summary.md" in a working_copy place
    When save_person_edit is called
    Then held_edits lists it until mark_edit_sent is called

  Scenario: Error path — the agent cannot remove a duty's STOP file
    Given a WorkFolder with protected path "/.agent-run/watch-readiness/STOP" and that file stored
    When the agent calls delete on "/.agent-run/watch-readiness"
    Then the refusal text holds "cannot be removed by the agent" and the file is still stored
    And operator_remove of the same path succeeds

  Scenario: Error path — a bundle path is read-only for the agent
    When the agent calls write on "/AGENTS.md"
    Then the refusal is "Error: /AGENTS.md is read-only (bundle path)"

  Scenario: Edge case — an empty writable-paths list makes the work folder read-only for the agent
    Given a WorkFolder with writable_paths Some([])
    When the agent calls write on "/plans/a.md"
    Then the refusal is "Error: /plans/a.md is outside the writable paths of this agent"

  Scenario: Happy path — an email attachment is saved after the safety check
    Given workspace W's own folder is place P and holds "quote.pdf"
    When save_incoming is called with P, "quote.pdf" and a PDF of 180 KB
    Then the file is written as "quote (2).pdf" and SavedFile.hidden_content_found is false
    And the journal holds file_received for P

  Scenario: Error path — a program received by email is refused
    When save_incoming is called with file name "invoice.pdf" and bytes starting with "MZ"
    Then it fails with FILE_INCOMING_REFUSED and no file is written

  Scenario: Edge case — hidden text is reported but the file is kept
    Given a Markdown file whose text holds U+200B
    When save_incoming is called with it
    Then the file is written and hidden_content_found is true

  Scenario: Error path — an agent without a grant cannot read a place
    Given workspace W's published grants give agent A a grant on place P only
    When read_for_agent is called for agent A, workspace W and place Q
    Then it fails with FILE_NOT_GRANTED

  Scenario: Happy path — a binned file is put back from its ticket
    Given "notes/plan.md" in place P has no version history
    When move_to_bin is called and then restore_from_bin with the returned ticket
    Then "notes/plan.md" holds its earlier bytes and versions lists a version with origin person

  Scenario: Error path — restore does not overwrite a new file
    Given a ticket for "notes/plan.md" and a new file now at that path
    When restore_from_bin is called
    Then it fails with FILE_RESTORE_TARGET_EXISTS and the new file is unchanged

  Scenario: Happy path — a run's write tells subscribers its version
    When write_version is called for "Q3/summary.md" with origin run R
    Then the PlaceChange broadcast carries version 2 and origin run R

  Scenario: Happy path — the operator checks a work-folder file exists
    Given a WorkFolder holding "/plans/a.md"
    When operator_exists is called with "/plans/a.md" and then with "/plans/b.md"
    Then it returns true and then false
    And operator_exists on "/AGENTS.md" fails with WF_BUNDLE_PATH
```

## Performance / Security / Observability

- **Performance targets.** `list_folder` first page ≤ 300 ms p95 at 100,000 files in a place (A-21); `search` first results ≤ 1 s p95 across all of a person's workspaces at A-21's limits (A-21, C-36); `place`, `versions`, `published_copies_of` ≤ 50 ms p95 (*chosen here*, part of A-03's 150 ms); a watcher event reaches subscribers ≤ 1 s after the debounce window; start-up rescan of 100,000 unchanged files ≤ 60 s in the background, never blocking start (A-03's 3 s); resident memory for indexing ≤ 200 MiB (*chosen here*). Index text extraction runs on at most 2 blocking threads.
- **Security.** Paths come only from the system folder picker or a drop (02:722); every relative path is checked to stay under its place after resolving links; protected locations are refused through the Rules Engine; agent folders and worktrees are never listed; previews never interpret markup (SVG as image only, Markdown as text); the version store is mode `0700`; files in places are never deleted, only moved to the system bin; the work folder enforces the bundle, writable-path and protected-path rules before any storage call. Service API inputs are typed with unknown fields refused (SA-API-02).
- **Never logged.** File content, extracted text, search queries, snippets, absolute paths of the person's folders (logs carry `place_id` and the extension only), work-folder content.
- **Observability (desktop and personal server: logs only, SA-XC-04/05).** `tracing` events with `component = "files"`: `place_added {place_id, kind}`, `place_removed {place_id}`, `place_missing {place_id}`, `index_build_started {place_id}`, `index_build_finished {place_id, indexed, total, skipped, elapsed_ms}`, `index_extract_failed {place_id, kind, error_class}`, `watch_failed {place_id, error_class}`, `file_version_written {place_id, version, origin_kind, run_id?}`, `grants_published {workspace_id, count}`, `versions_pruned {removed, bytes_freed}`, `work_folder_unreachable {operation, error_class}`. **Agent profile:** no metric of its own (Metrics.md N-1: only the 50 families of the closed list exist (49 before ADR-107); work-folder changes are counted by the Caller API's `wrightrun_work_folder_changes_total{action}`); refusals are logged with their `code`; `work_folder.<operation>` with no path or content is a local `tracing` span only, never exported (Tracing.md T-9: the exported spans are the five of the Telemetry allow list).

## Gaps

None.

---

## Amendment January 2026 — Moving a file (ADR-104)

> Source: decision 84 (the user's decisions K1–K9 of January 2026); HLD revision 0.52.0, ADR-104 (a knowledge page's file follows its title, keeping every earlier version). Every rule below adds to the sections above; nothing above changes.

### Purpose (addition)

`move_file` renames or moves one file inside a place and keeps its version history under the new path, so a renamed knowledge page keeps **Use this version** for every earlier version (G11). It is the only way this service moves a file; a move made by another app is still seen by the watcher as `Renamed { from }`, whose version rows this amendment also carries to the new path.

### Public Interface (additions)

```rust
impl FileService {
    /// Moves `from` to `to` inside `place_id` (places of kind `workspace_folder` only); the version rows,
    /// the index row and any held edit follow the file. Returns the latest version, now at `to`.
    pub async fn move_file(&self, place_id: PlaceId, from: &str, to: &str) -> Result<FileVersion, FileError>;
}
// FileError gains:
    #[error("FILE_MOVE_TARGET_EXISTS")] MoveTargetExists,
```

| Item | Called by |
|---|---|
| `move_file` | Knowledge Service (a page renamed in ordin, KnowledgeService.md amendment January 2026 — Page links, §Q) |
| `versions`, `list_folder` (additional caller) | Knowledge Service (`Removed` of a page file; the start-up scan of `knowledge/`) |

No Service API command is added: the person renames a page through `knowledge.save_edit`.

### Internal Logic (additions)

**15. `move_file(place_id, from, to)`.**

1. Normalise both paths (step 4.1); either escaping the place → `FILE_PATH_ESCAPES_PLACE`. The place must exist (`FILE_PLACE_NOT_FOUND`) and be of kind `workspace_folder` (else `FILE_PROTECTED_LOCATION`). `from` missing → `FILE_NOT_FOUND`. `from` equal to `to` → return the latest version, nothing done.
2. A file at `to` that is not `from` itself (paths compared case-insensitively on macOS and Windows, exactly on Linux) → `FILE_MOVE_TARGET_EXISTS`, nothing moved. A move that changes only letter case is done as two renames through a temporary name in the same folder.
3. Baseline as step 6.4 when `from` has no version row, so the history starts before the move.
4. Create `to`'s missing folders; `std::fs::rename(from, to)`; failure → `FILE_WRITE_FAILED` (retryable), nothing else changed. Record both paths in the recent-writes map, so the watcher's rename pair is broadcast once as `PlaceChange { change: Renamed { from }, by_ordin: true, version: <latest>, … }`.
5. In one transaction: `UPDATE file_versions SET path = :to WHERE place_id = :p AND path = :from`; move the `file_index` row (`path`, `parent`, `name`) and its `file_index_fts` row; `UPDATE held_edits SET path = :to …` for the same key; `UPDATE published_copies SET original_key = 'place:<place_id>:<to>' WHERE original_key = 'place:<place_id>:<from>'` (copies of a knowledge page are keyed `page:<id>` and are not touched). A unique violation on `file_versions` (versions already recorded at `to`) → the transaction is rolled back, the file is renamed back, `FILE_MOVE_TARGET_EXISTS`.
6. Journal `file_moved` with `place_id` and the version count (no path text).

**5a. Renames seen by the watcher (addition to step 5.2).** A rename pair not made by `move_file` (another app renamed the file) also runs step 15.5 for its two paths before `Renamed { from }` is broadcast, so the history follows a rename made outside ordin; a unique violation keeps both histories where they are and is logged `file_move_history_kept {place_id}`.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `move_file` onto a path where another file exists, or whose versions already exist there | `Err(permanent)` | `FILE_MOVE_TARGET_EXISTS` | — (Knowledge Service picks the next name) |
| `move_file` in a place that is not a workspace's own folder | `Err(permanent)` | `FILE_PROTECTED_LOCATION` | — (caller defect, logged) |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: File Service — moving a file

  Scenario: Happy path — the history follows the file
    Given "knowledge/Supplier rates.md" in workspace folder place P with versions 1 to 3
    When move_file(P, "knowledge/Supplier rates.md", "knowledge/Supplier day rates.md") is called
    Then versions(P, "knowledge/Supplier day rates.md") lists versions 1 to 3
    And versions(P, "knowledge/Supplier rates.md") is empty
    And one PlaceChange Renamed from "knowledge/Supplier rates.md" with by_ordin true is broadcast

  Scenario: Edge — only the letter case changes
    When move_file(P, "knowledge/rates.md", "knowledge/Rates.md") is called on macOS
    Then the file is named "Rates.md" and keeps its versions

  Scenario: Error — the target exists
    Given "knowledge/Day rates.md" exists in P
    When move_file(P, "knowledge/Supplier rates.md", "knowledge/Day rates.md") is called
    Then it fails with FILE_MOVE_TARGET_EXISTS and both files are unchanged

  Scenario: Error — not a workspace folder
    Given place Q of kind persons_folder
    When move_file(Q, "a.md", "b.md") is called
    Then it fails with FILE_PROTECTED_LOCATION

  Scenario: Edge — another app renames a file
    Given "knowledge/a.md" in P with versions 1 to 2
    When another app renames it to "knowledge/b.md"
    Then versions(P, "knowledge/b.md") lists versions 1 to 2 and a PlaceChange Renamed with by_ordin false is broadcast
```

### Performance, Security, Observability (additions)

- **Performance:** `move_file` p95 ≤ 50 ms for a file with 100 versions, excluding the operating system's rename.
- **Security:** both paths are normalised and confined to the place (step 4.1); only a workspace's own folder is moved in; nothing is overwritten.
- **Observability:** journal `file_moved {place_id, versions}`; log `file_moved {place_id}`, `file_move_history_kept {place_id}` (warn).

### Gaps

None.
