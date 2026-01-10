# SPEC: Agent Folder Manager

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-agent-folders` · **Build phase:** 2 · **Programs:** D P A
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
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)

## Purpose

The Agent Folder Manager creates, locks and removes agent folders: one agent folder per agent instance, under one application data root (02:482–484). It applies, for each session, the folder grants the person made in the workspace the run started in, so a session reaches a folder outside its agent folder only through such a grant (ADR-025); it records each application (02:482 *Owns*: "agent folders and the session-level application of folder grants"). From revision 0.48.0 the grants themselves are recorded by the Workspace Service; this component applies them and records none (04:64). It keeps a working copy of each workspace's grant set as the File Service publishes it (`set_workspace_grants`) and derives every session's grants from that copy, so a grant the person withdraws reaches running sessions without a separate revocation call. The agent folder is the harness session's working directory (ADR-038) and the place the Installers write a bundle into (ADR-082). Agent folders are never listed to the person (04:56).

## Approach

A small module over the file system and two tables of its own in the journal database, reached through the Journal's `Datastore` (2B-1 item 5). Each instance gets a directory pair under `<application data root>/agents/<agent instance id>/`: the agent folder itself (`folder/`) and ordin's own files for that instance (`meta/`), kept outside the agent folder so a session, which may write anywhere inside its own folder, cannot alter the install record, backups or locks. Locks combine an in-process reader–writer table with an operating-system file lock on `meta/folder.lock`, so a second core started on the same data root cannot write a folder a session is using. Rejected: (1) one shared folder per package version — 04:45 makes the folder the instance's own; (2) recording grants here as well as in the Workspace Service — 04:64 (revision 0.48.0) gives one recorder.

### Reconciliation assumptions

- **SA-RECON-01** The Workspace Service, through the File Service, pushes each workspace's whole grant set with `set_workspace_grants`; this component derives each session's grants from the stored set, and `revoke` is internal (reconciliation rows 600, 603; ruling R6).
- **SA-RECON-02** The crate exports no trait; the File Service and the Package Catalogue call the inherent methods of `AgentFolderManager` and match `AfmError` (reconciliation rows 601, N173, N197; ruling R6).
- **SA-RECON-03** Journal calls use the B2 form: `Journal::append_entry(NewJournalEntry)`, migrations as `ModuleMigrations`, and the tables through `Journal::datastore()` (ruling R3, D4).
- **SA-RECON-04** The programs call `recover()` once at start, after migrations and before any session starts (reconciliation row 604, O161).

### Closing-round assumptions

- **SA-CLOSE-01** (*chosen here*, K22; closes the `check_access` gap) — `check_access` and its types (`AccessOp`, `AccessDecision`, `AccessWithin`, `DenyReason`) are dropped: no component calls them. Per-call path decisions belong to the Rules Engine (path rule H.2, lexical), and this component's part in link safety is step 6: each granted place's root is canonicalised at session start, so a root that is, or leads through, a link into ordin's data root or an agent folder is never applied. `AFM_PATH_INVALID`, raised only by `check_access`, goes with it.

## Shared Context

### Types from Phase 2C (`ordin-types`), used unchanged

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct AgentInstanceId(pub Uuid);   // UUIDv7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct WorkspaceId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct PlaceId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct RunId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct FolderGrantId(pub Uuid);
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);   // RFC 3339, ms, Z

/// 2C §FolderGrant: "can't use" is the absence of a grant (04:61).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess { Read, ReadAndChange }

/// 2C §Place (amended January 2026, decision 80: C-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind { PersonsFolder, WorkspaceFolder, AgentFolder, WorkingCopy, CodingTaskWorktree }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct CrewCharterId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct CrewRunId(pub Uuid);

/// 2C §FolderGrant.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Grantee {
    AgentInstance { agent_instance_id: AgentInstanceId },
    Crew { crew_charter_id: CrewCharterId },
}
```

2C shared rule: any local file-system path is absolute, 1–4096 bytes UTF-8, no NUL (LD-03); a grant path that breaks it is left out of the stored set (step 6a), never an error.

### Journal (`ordin-journal`) shapes this component calls

Copied from the Journal and Artefact Store spec (ruling R3, the B2 form; D4: this module reaches its tables through the Journal's `Datastore`):

```rust
#[derive(Clone)]
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
impl Datastore { pub fn sqlite(&self) -> Option<&sqlx::SqlitePool>; pub fn postgres(&self) -> Option<&sqlx::PgPool>; }
/// (version, description, SQL). Versions start at 1 and increase by exactly 1 within a module.
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str,
    pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }

#[derive(Clone)]
pub struct Journal { /* Arc<Inner> */ }
impl Journal {
    pub fn datastore(&self) -> &Datastore;
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
}
pub struct NewJournalEntry {
    pub kind: String,                    // ^[a-z][a-z0-9_]{0,63}$
    pub component: String,               // [a-z_]{1,40}; this component writes "agent_folders"
    pub run_id: Option<RunId>,
    pub crew_run_id: Option<CrewRunId>,
    pub workspace_id: Option<WorkspaceId>,
    pub summary: String,                 // one plain sentence, 1–300 characters, no paths
    pub data: serde_json::Value,         // at most 65,536 bytes serialised
}
pub enum JournalError { /* JRNL_* codes; `code()` and `retryable()` per 2C `CodedError` */ }
```

### Configuration

| Name | Value | Source |
|---|---|---|
| Application data root | desktop: the Tauri application data directory; server: `ORDIN_DATA_DIR` (2D-1a) | constructor argument from the program |
| Agents directory | `<data root>/agents` (*chosen here*) | constant `AGENTS_DIR = "agents"` |
| Write-lock wait | 30 s (*assumed here*: longer than one file copy of a 200-part bundle, shorter than a person's patience at **Add it**) | constant `WRITE_LOCK_WAIT` |

## Public Interface

Crate `ordin_agent_folders`. Names are *chosen here* unless traced.

```rust
use std::path::{Path, PathBuf};

pub const MODULE: &str = "agent_folders";
pub fn migrations() -> ModuleMigrations;          // registered by the program with the Journal; module "agent_folders"

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentFolder {
    pub agent_instance_id: AgentInstanceId,
    /// `<data root>/agents/<id>/folder` — the harness working directory; 2C `AgentInstance.agent_folder_path`.
    pub folder_path: PathBuf,
    /// `<data root>/agents/<id>/meta` — ordin's own files for the instance; never given to a session.
    pub meta_path: PathBuf,
    pub state: AgentFolderState,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFolderState { Created, Removing, Removed }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// A harness session uses the folder; many at once.
    Session,
    /// Install, update, repair or removal; excludes every other lease.
    Write,
    /// The pack-choice rebuild during a session (ADR-089 decision 64, C-64): allowed beside
    /// `Session` leases, excludes `Write` and another `SessionUpdate`.
    SessionUpdate,
}

/// Held for as long as the lock is needed; dropping it releases the lock.
pub struct FolderLease { /* private: instance id, mode, OS file handle, in-process guard */ }
impl FolderLease { pub fn mode(&self) -> LockMode; pub fn folder(&self) -> &Path; pub fn meta(&self) -> &Path; }

/// One grant of a workspace as the File Service publishes it (B7 contract D-7): the 2C grant plus
/// the place's absolute path and kind, read by the File Service from its `places` row, never from a
/// tool argument.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceGrant {
    pub grant: FolderGrant,
    pub place_path: PathBuf,
    pub place_kind: PlaceKind,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AppliedGrant { pub place_id: PlaceId, pub path: PathBuf, pub access: FolderAccess }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionAccess {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub agent_folder: PathBuf,
    pub grants: Vec<AppliedGrant>,
}

pub struct AgentFolderManager { /* private */ }

impl AgentFolderManager {
    pub async fn new(journal: ordin_journal::Journal, data_root: PathBuf) -> Result<Self, AfmError>;
    /// Finishes interrupted removals and clears session applications left by a previous start.
    pub async fn recover(&self) -> Result<RecoveryReport, AfmError>;

    pub async fn create(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError>;
    pub async fn get(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError>;
    pub async fn lock(&self, id: AgentInstanceId, mode: LockMode) -> Result<FolderLease, AfmError>;
    /// Deletes the agent folder and its meta folder; refused while any lease is held.
    pub async fn remove(&self, id: AgentInstanceId) -> Result<(), AfmError>;

    /// Replaces the stored grant set of one workspace and re-derives the grants of every live
    /// session in that workspace (step 6a). An empty `grants` clears the workspace.
    pub async fn set_workspace_grants(&self, workspace_id: WorkspaceId, grants: Vec<WorkspaceGrant>)
        -> Result<(), AfmError>;
    /// Session start and resume: derives the session's grants from the stored set of `workspace_id`
    /// — every grant whose grantee is the instance, or the crew charter when `crew_charter_id` is set.
    pub async fn apply_grants(&self, run_id: RunId, id: AgentInstanceId, workspace_id: WorkspaceId,
        crew_charter_id: Option<CrewCharterId>) -> Result<SessionAccess, AfmError>;
    pub async fn session_access(&self, run_id: RunId) -> Result<SessionAccess, AfmError>;
    pub async fn end_session(&self, run_id: RunId) -> Result<(), AfmError>;
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RecoveryReport { pub removals_finished: u32, pub sessions_cleared: u32, pub unknown_directories: u32 }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AfmError {
    #[error("AFM_ROOT_UNWRITABLE")] RootUnwritable,
    #[error("AFM_FOLDER_EXISTS")] FolderExists { id: AgentInstanceId },
    #[error("AFM_FOLDER_NOT_FOUND")] FolderNotFound { id: AgentInstanceId },
    #[error("AFM_FOLDER_BUSY")] FolderBusy { id: AgentInstanceId, held: &'static str },
    #[error("AFM_GRANT_INVALID")] GrantInvalid { folder_grant_id: FolderGrantId },
    #[error("AFM_SESSION_UNKNOWN")] SessionUnknown { run_id: RunId },
    #[error("AFM_IO_FAILED")] IoFailed { retryable: bool },
    #[error("AFM_DATASTORE_FAILED")] DatastoreFailed { retryable: bool },
}
impl AfmError { pub fn retryable(&self) -> bool; }
```

The crate exports no trait. Consumers hold `Arc<AgentFolderManager>` and use the inherent methods above with `AfmError`; a consumer that needs a test double declares a port trait in its own crate under its own name (this settles the two different `AgentFolders` traits that the File Service and the Package Catalogue each placed in this crate).

### Who calls each public item

| Item | Called by |
|---|---|
| `migrations` | Ordin Desktop and Ordin Server, registered with the Journal at start (OrdinDesktop row 3.6, OrdinServer row 5.6) |
| `new` | Ordin Desktop and Ordin Server at start, with the Journal and the data root |
| `recover` | Ordin Desktop and Ordin Server once at start, after `new` and before `session_supervisor.recover()`, so no stale session row survives into a resumed run (03 *Recovery after restart*) |
| `create`, `remove` | Package Catalogue (*Add an agent*, *Removal*; `create` returns `AgentFolder`, whose `folder_path` is the instance's agent folder) |
| `get` | Ordin Desktop program (its Sandbox `PlaceFiles` implementation, to find an instance's agent folder) |
| `lock` | Installers (`Write`, `SessionUpdate`, `Session` for verify); the program's `SessionSetupSource` (`Session`, held for the run's life) |
| `set_workspace_grants` | File Service `publish_grants` (B7 contract D-7), each time a workspace's grant set changes |
| `apply_grants` | The program's `SessionSetupSource` (Claude Local Adapter only; `CopilotSessionSetup` carries no granted folders) at session start and on resume, on behalf of the Session Supervisor; `SessionAccess.grants` fills `SessionSetup.extra_dirs` |
| `session_access` | The program's `SessionSetupSource`, when a resumed session's setup is rebuilt |
| `end_session` | The program, when the Session Supervisor reports the run ended (`Ended`) |

### Example

```rust
let afm = AgentFolderManager::new(journal, PathBuf::from("/Users/asha/Library/Application Support/ordin")).await?;
let folder = afm.create(AgentInstanceId(uuid!("0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"))).await?;
// folder.folder_path = ".../ordin/agents/0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff/folder"
afm.set_workspace_grants(workspace_id, vec![WorkspaceGrant {
    grant: FolderGrant { id: folder_grant_id, workspace_id, place_id, access: FolderAccess::Read, granted_at,
        grantee: Grantee::AgentInstance { agent_instance_id: folder.agent_instance_id } },
    place_path: "/Users/asha/Documents/Suppliers".into(), place_kind: PlaceKind::PersonsFolder }]).await?;
let access = afm.apply_grants(run_id, folder.agent_instance_id, workspace_id, None).await?;
// → SessionAccess { grants: [AppliedGrant { place_id, path: "/Users/asha/Documents/Suppliers", access: Read }], .. }
```

## Internal Logic

1. **Start (`new`).** Create `<data root>/agents` when missing (mode `0700` on macOS and Linux); confirm it is writable by creating and deleting `agents/.probe-<UUIDv7>`; failure → `AFM_ROOT_UNWRITABLE` (the program refuses to start, 2D-1a `ORDIN_DATA_DIR`). Canonicalise the data root once (`std::fs::canonicalize`) and keep it.
2. **Recover.** Rows in state `removing` → repeat step 5's deletion and set `removed`; delete every `session_folder_access` row (sessions do not survive a restart without the Session Supervisor applying grants again on resume, 03 *Recovery after restart*); count directories under `agents/` with no row and leave them untouched (never delete what ordin did not record), logging `agent_folders.unknown_directory` with the directory name only. Journal nothing.
3. **Create.** In one datastore transaction insert `agent_folders` with state `created`; a row with the same id in state `created` or `removing` → `AFM_FOLDER_EXISTS`; a row in state `removed` is replaced (an instance id is never reused by the Package Catalogue, so this arises only in tests). Then create `folder/` and `meta/` (mode `0700`); `folder/` must be empty or absent — non-empty → `AFM_FOLDER_EXISTS`. An I/O failure rolls back the row and removes what was created → `AFM_IO_FAILED { retryable: true }`. Append journal entry `agent_folder_created` `{agent_instance_id}`; a journal failure is logged at `error` and the folder stays (the journal is the record of actions, the folder is the state; *assumed here*: a missing journal line must not leave a half-created instance).
4. **Lock.** Per instance an in-process state `{ sessions: u32, write: bool, session_update: bool }` behind a `tokio::sync::Mutex` with a `Notify`. `Session`: granted when `!write`. `SessionUpdate`: granted when `!write && !session_update`. `Write`: granted when all three are zero/false. A request that cannot be granted waits on the `Notify` up to `WRITE_LOCK_WAIT` for `Write` and `SessionUpdate`, and does not wait for `Session` (a session never queues behind an install: the Session Supervisor reports the agent busy). Timeout or refusal → `AFM_FOLDER_BUSY { held }` (retryable). After the in-process grant, take the OS lock on `meta/folder.lock` with `std::fs::File::lock_shared` for `Session` and `SessionUpdate` and `std::fs::File::try_lock` (exclusive) for `Write`, retried every 100 ms within the same wait; failure → release the in-process grant and `AFM_FOLDER_BUSY { held: "another ordin process" }`. Unknown instance or state not `created` → `AFM_FOLDER_NOT_FOUND`. `FolderLease::drop` releases both and notifies waiters.
5. **Remove.** Take a `Write` lease (busy → `AFM_FOLDER_BUSY`). Set state `removing`; delete every `session_folder_access` row of the instance; delete `folder/` recursively without following symbolic links (a link is removed, its target untouched); delete `meta/`; delete the instance directory; set state `removed` with `removed_at`. An I/O failure leaves state `removing` (finished by `recover`) → `AFM_IO_FAILED { retryable: true }`. Append `agent_folder_removed` `{agent_instance_id}`. The person's confirmation and the Installers' restore happen before this call (Package Catalogue *Removal*, 02:427).
6a. **Set workspace grants (`set_workspace_grants`).** Every `grant.grant.workspace_id` must equal `workspace_id`, else `AFM_GRANT_INVALID { folder_grant_id }` and nothing is stored. Each grant's `place_path` must be absolute, ≤ 4,096 bytes, UTF-8, no NUL; a path equal to or inside the data root, a `place_kind` of `AgentFolder`, or a path inside any agent folder is left out of the stored set and logged `agent_folders.grant_skipped` (warn) with `folder_grant_id` and `place_id` — the rest of the set is still stored, so a withdrawal always lands. In one datastore transaction: delete every `workspace_folder_grants` row of `workspace_id` and insert one row per kept grant; then, for every live session of that workspace (distinct `run_id` in `session_folder_access` with that `workspace_id`, plus each run in the in-memory list of sessions opened by `apply_grants` with no grant yet), re-derive its rows exactly as step 6 does. Grants that no longer appear are therefore gone from the next `session_access`. Append `workspace_folder_grants_set` `{workspace_id, grants: <count kept>, skipped: <count>, sessions_updated: <count>}`. Return `Ok(())`.
6. **Apply grants (session start and resume).** Instance must exist in state `created` (else `AFM_FOLDER_NOT_FOUND`). Read the `workspace_folder_grants` rows of `workspace_id` whose grantee is `agent_instance` with this instance, or `crew` with `crew_charter_id` when it is `Some`. For each: the path must exist as a directory, else it is left out of `SessionAccess` and logged `agent_folders.grant_unreachable` (warn, `place_id` only) — a place on an unplugged drive does not stop the session; canonicalise it; a canonical path equal to or inside the data root, or inside any agent folder → left out and logged `agent_folders.grant_skipped` (a link that resolves into ordin's data is never applied). Replace the run's rows in one transaction (delete then insert one row per applied grant) and remember the run in the in-memory live-session list with its `crew_charter_id`. Return `SessionAccess`. Append `folder_grants_applied` `{run_id, agent_instance_id, workspace_id, grants: [{place_id, access}]}` — no paths in the journal record (paths are personal data, 07 *Logging*; *assumed here*). Calling it again for the same run (resume) repeats the derivation and replaces the rows.
7. *(Withdrawn in the closing round, SA-CLOSE-01: per-call path decisions are the Rules Engine's.)*
8. **Revocation (internal).** Revocation is not a separate call: a grant withdrawn by the person, a place leaving the workspace, and an instance removed from a workspace all reach this component as a new grant set through `set_workspace_grants` (step 6a), which removes the session rows that no longer have a grant. `remove` (step 5) deletes the removed instance's session rows. Effective on the next `session_access` and the next session setup of those sessions.
9. **End session.** Delete the run's rows and drop the run from the live-session list; unknown run → `Ok(())` (idempotent).
10. **Datastore errors.** Any `sqlx` error → `AFM_DATASTORE_FAILED`, retryable when the error is a busy or locked database, permanent otherwise; logged at `error` with the statement name, never parameter values.

## Data Model

Tables in the journal database, written only by this component (2B-1 item 5). Registered as `MODULE = "agent_folders"`, migration version 1.

SQLite (desktop, personal server, agent single shape):

```sql
CREATE TABLE agent_folders (
    agent_instance_id TEXT PRIMARY KEY CHECK (length(agent_instance_id) = 36),
    folder_path       TEXT NOT NULL UNIQUE CHECK (length(folder_path) BETWEEN 1 AND 4096),
    meta_path         TEXT NOT NULL UNIQUE CHECK (length(meta_path) BETWEEN 1 AND 4096),
    state             TEXT NOT NULL CHECK (state IN ('created', 'removing', 'removed')),
    created_at        TEXT NOT NULL,   -- RFC 3339, ms, Z
    removed_at        TEXT
) STRICT;

CREATE TABLE session_folder_access (
    run_id            TEXT NOT NULL CHECK (length(run_id) = 36),
    agent_instance_id TEXT NOT NULL REFERENCES agent_folders(agent_instance_id),
    workspace_id      TEXT NOT NULL CHECK (length(workspace_id) = 36),
    folder_grant_id   TEXT NOT NULL CHECK (length(folder_grant_id) = 36),
    place_id          TEXT NOT NULL CHECK (length(place_id) = 36),
    path              TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    applied_at        TEXT NOT NULL,
    PRIMARY KEY (run_id, place_id)
) STRICT;
CREATE INDEX session_folder_access_instance ON session_folder_access (agent_instance_id);
CREATE INDEX session_folder_access_workspace_place ON session_folder_access (workspace_id, place_id);

-- Working copy of each workspace's grant set, replaced by set_workspace_grants (the Workspace
-- Service's table stays the record of grants, 04:64).
CREATE TABLE workspace_folder_grants (
    folder_grant_id   TEXT PRIMARY KEY CHECK (length(folder_grant_id) = 36),
    workspace_id      TEXT NOT NULL CHECK (length(workspace_id) = 36),
    grantee_kind      TEXT NOT NULL CHECK (grantee_kind IN ('agent_instance', 'crew')),
    grantee_id        TEXT NOT NULL CHECK (length(grantee_id) = 36),
    place_id          TEXT NOT NULL CHECK (length(place_id) = 36),
    place_kind        TEXT NOT NULL CHECK (place_kind IN ('persons_folder', 'workspace_folder', 'working_copy', 'coding_task_worktree')),
    path              TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    granted_at        TEXT NOT NULL
) STRICT;
CREATE INDEX workspace_folder_grants_workspace ON workspace_folder_grants (workspace_id, grantee_kind, grantee_id);
```

PostgreSQL (agent profile scaled shape):

```sql
CREATE TABLE agent_folders (
    agent_instance_id UUID PRIMARY KEY,
    folder_path       TEXT NOT NULL UNIQUE CHECK (char_length(folder_path) BETWEEN 1 AND 4096),
    meta_path         TEXT NOT NULL UNIQUE CHECK (char_length(meta_path) BETWEEN 1 AND 4096),
    state             TEXT NOT NULL CHECK (state IN ('created', 'removing', 'removed')),
    created_at        TIMESTAMPTZ NOT NULL,
    removed_at        TIMESTAMPTZ
);
CREATE TABLE session_folder_access (
    run_id            UUID NOT NULL,
    agent_instance_id UUID NOT NULL REFERENCES agent_folders(agent_instance_id),
    workspace_id      UUID NOT NULL,
    folder_grant_id   UUID NOT NULL,
    place_id          UUID NOT NULL,
    path              TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    applied_at        TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (run_id, place_id)
);
CREATE INDEX session_folder_access_instance ON session_folder_access (agent_instance_id);
CREATE INDEX session_folder_access_workspace_place ON session_folder_access (workspace_id, place_id);
CREATE TABLE workspace_folder_grants (
    folder_grant_id   UUID PRIMARY KEY,
    workspace_id      UUID NOT NULL,
    grantee_kind      TEXT NOT NULL CHECK (grantee_kind IN ('agent_instance', 'crew')),
    grantee_id        UUID NOT NULL,
    place_id          UUID NOT NULL,
    place_kind        TEXT NOT NULL CHECK (place_kind IN ('persons_folder', 'workspace_folder', 'working_copy', 'coding_task_worktree')),
    path              TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    granted_at        TIMESTAMPTZ NOT NULL
);
CREATE INDEX workspace_folder_grants_workspace ON workspace_folder_grants (workspace_id, grantee_kind, grantee_id);
```

In the scaled shape each instance (pod) has its own data root, so `folder_path` uniqueness holds per database only when one pod writes agent folders; *assumed here*: in the agent profile the hosted agent's single agent folder is created by the instance that runs it, keyed by `agent_instance_id`, and other pods read nothing from this table.

Journal entry kinds (*chosen here*), each written with `journal.append_entry(NewJournalEntry { kind, component: "agent_folders", run_id, crew_run_id: None, workspace_id, summary, data })`: `agent_folder_created`, `agent_folder_removed`, `folder_grants_applied`, `workspace_folder_grants_set` (the Journal's kind pattern `^[a-z][a-z0-9_]{0,63}$` admits no dot). `summary` is a fixed sentence per kind with no path, for example "Agent folder created.". A journal failure on any of the four is logged at `error` with the `JRNL_*` code and does not undo the operation it records (SA-RECON-03).

Rollback: migration 1 only creates; rolling back is dropping the three tables, which forgets which folders exist — never run on a populated database without a backup (SA-XC-07).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Data root cannot be created or written | `Err(permanent)` | `AFM_ROOT_UNWRITABLE` | `AfmError::RootUnwritable` |
| Folder or row already exists for the instance | `Err(permanent)` | `AFM_FOLDER_EXISTS` | `AfmError::FolderExists { id }` |
| No row in state `created` for the instance | `Err(permanent)` | `AFM_FOLDER_NOT_FOUND` | `AfmError::FolderNotFound { id }` |
| A lease conflicts and the wait ran out, or another process holds the OS lock | `Err(retryable)` | `AFM_FOLDER_BUSY` | `AfmError::FolderBusy { id, held }` |
| A grant's path is missing, not a directory, or inside ordin's data root or an agent folder | not an error: the grant is left out of the stored set or of the session and logged (steps 6, 6a) | — | — |
| `set_workspace_grants` with a grant of another workspace | `Err(permanent)` | `AFM_GRANT_INVALID` | `AfmError::GrantInvalid { folder_grant_id }` |
| `session_access` for a run with no applied grants and no instance | `Err(permanent)` | `AFM_SESSION_UNKNOWN` | `AfmError::SessionUnknown { run_id }` |
| File-system operation fails | `Err(retryable)` | `AFM_IO_FAILED` | `AfmError::IoFailed { retryable: true }` |
| Datastore busy or locked | `Err(retryable)` | `AFM_DATASTORE_FAILED` | `AfmError::DatastoreFailed { retryable: true }` |
| Other datastore failure | `Err(permanent)` | `AFM_DATASTORE_FAILED` | `AfmError::DatastoreFailed { retryable: false }` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Agent Folder Manager

  Scenario: Happy path — create, lock for a session, remove
    Given a writable application data root
    When create is called for agent instance "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"
    Then the directories "agents/0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff/folder" and ".../meta" exist with mode 0700
    And the journal holds an entry of kind "agent_folder_created"
    When a Session lease is taken and released, and remove is called
    Then both directories are gone and the row state is "removed"

  Scenario: Edge — a granted root that is a link into ordin's data is not applied
    Given workspace W stores a read grant for instance A on "/Users/asha/shortcut", a link to "<data root>/agents"
    When apply_grants is called for a run of A in W
    Then the returned SessionAccess holds no grant on that place
    And "agent_folders.grant_skipped" is logged with its place id only

  Scenario: Error — session access for an unknown run
    When session_access is called for a run never applied and with no instance
    Then it fails with AFM_SESSION_UNKNOWN

  Scenario: Edge — a pack-choice update during a session
    Given a Session lease held on the instance
    When a SessionUpdate lease is requested
    Then it is granted
    And a Write lease requested meanwhile waits

  Scenario: Edge — a withdrawn grant reaches a live session
    Given workspace W's stored grants give instance A read access to place P
    And a session of A in W whose grants were applied
    When set_workspace_grants is called for W without the grant on P
    Then session_access for the session lists no grant on P

  Scenario: Happy path — session grants are derived from the workspace set
    Given set_workspace_grants stored for W a read grant on P for instance A and a read-and-change grant on Q for crew charter C
    When apply_grants is called for a run of A in W with crew charter C
    Then the returned SessionAccess holds P with read and Q with read_and_change

  Scenario: Edge — a grant on another workspace is refused
    When set_workspace_grants is called for W with a grant whose workspace is V
    Then it fails with AFM_GRANT_INVALID and the stored set of W is unchanged

  Scenario: Error — install while a session runs
    Given a Session lease held on the instance
    When a Write lease is requested
    Then after 30 seconds it fails with AFM_FOLDER_BUSY

  Scenario: Edge — grant pointing into ordin's data is never applied
    When set_workspace_grants is called for W with a grant on "<data root>/agents"
    Then the call succeeds, the grant is not stored and "agent_folders.grant_skipped" is logged

  Scenario: Error — interrupted removal is finished on the next start
    Given an instance whose row is in state "removing" and whose folder still exists
    When recover is called
    Then the folder is deleted and the row state is "removed"

  Scenario: Error — unwritable data root
    Given a data root without write permission
    When the manager is constructed
    Then it fails with AFM_ROOT_UNWRITABLE
```

## Performance, Security, Observability

- **Performance targets:** `set_workspace_grants` with 200 grants and 20 live sessions ≤ 50 ms p95; `apply_grants` with 50 grants ≤ 20 ms p95; `create` ≤ 30 ms p95; `remove` proportional to folder size, ≤ 2 s p95 for 10,000 files. Memory: per live session ≤ 64 KB.
- **Security:** a grant path comes only from the Workspace Service's records through the caller, never from a tool argument; granted roots are canonicalised (symbolic links resolved) and compared by path component; ordin's data root and every agent folder other than the session's own are never grantable; `meta/` is outside the agent folder, so the Rules Engine's data-root rule (H.2) denies it to sessions; directories are created `0700`; removal never follows links. The component reads and stores no secret. On Windows the OS lock uses the same `std::fs::File` locking API; directory modes do not apply there (*assumed here*: the per-user profile directory's ACL protects the data root).
- **Observability:** `tracing` events (SA-OBS-01); never a full path of the person's files (only place and instance identifiers):
  - `agent_folders.created`, `agent_folders.removed` (info): `agent_instance_id`, `duration_ms`.
  - `agent_folders.lock_busy` (warn): `agent_instance_id`, `requested_mode`, `held`.
  - `agent_folders.grants_applied` (info): `run_id`, `agent_instance_id`, `workspace_id`, `grants`.
  - `agent_folders.unknown_directory` (warn): `directory_name`.
  - `agent_folders.grant_skipped`, `agent_folders.grant_unreachable` (warn): `folder_grant_id` or `place_id`, never the path.
  - `agent_folders.workspace_grants_set` (info): `workspace_id`, `grants`, `skipped`, `sessions_updated`.
  No metrics or spans on the desktop and personal server (SA-XC-04, SA-XC-05); none added in the agent profile (no remote call).

## Gaps

None.
