# SPEC: Workspace Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (the SharePoint item types are named `SharePointItem`, `SharePointItemRef` and `SharePointVersion`, apart from the Confluence Adapter's `RemoteItem` family, so each has one name in the generated screen types)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-workspaces` · **Build phase:** 6 · **Programs:** D P
> **Dependencies:** File Service, Confluence Adapter, SharePoint Adapter, Jira Adapter (amended January 2026, ADR-106), Rules Engine, Journal

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
- [Amendment January 2026 — Team knowledge (ADR-103)](#amendment-january-2026--team-knowledge-adr-103)
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)

Line references: `02:N` is `docs/design/ordin-desktop/02-architecture.md` line N; `03:N`, `04:N`, `10:N`, `11:N` likewise; `09 ADR-0NN` is an ADR in `09-decisions.md`; `2C` is `docs/spec/ordin-desktop/phase-2c-shared-types-*.md`; `1D` is `phase-1-analysis.md` §1D; `B7 contract` is the batch's shared contract sheet whose decisions are restated here.

## Purpose

The Workspace Service keeps the person's workspaces (02:730, 09 ADR-025): each is a named piece of work holding a set of **source links** — local folders, git repository folders, SharePoint locations and Confluence locations — with a local working copy of what has been read from the remote ones (09 ADR-029). It creates, renames, archives, moves to ordin's Bin, restores and deletes workspaces (UX-A68); adds and removes places and links; reads SharePoint and Confluence links into the working copy through their adapters; records each **folder grant** of an agent instance or crew in a workspace, so a grant in one workspace gives nothing in another; keeps Confluence links to linked folders and the email links and email grants the Mail Service enforces; tracks the state of each working-copy item that the Push Service sends back; and, when a workspace opens, at start and every 15 minutes, asks the adapters what changed at the origins (A-25). It is the authority for whether a workspace is active when a task, crew run or conversation starts in it (02:732).

## Approach

The service owns the workspace tables and drives its dependencies in one direction: it calls the File Service (places, working-copy writes, grant hand-off) and the two adapters, and they never call it (2B-2 D2). Approval decisions on Confluence links come back as Journal events, delivered to this service's `EventSink`, which the program registers with `Journal::subscribe` (2B-1 rule 4). Working-copy items follow 2C's state machine with the two transitions HLD 04:85 adds in revision 0.49.0 (Internal Logic 6.3). Rejected: storing grants in the File Service (2C and 1D 2C-pass item 4 give grants to this service; one owner per table, 2B-1 rule 5); fetching remote content only when first opened (search and previews need content on the computer, A-22, so content up to a budget is read in the background, *assumed here*).

## Shared Context

### Identifiers, time and paging (2C part 1 *Conventions*; SA-PAGE-01)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);    // RFC 3339, UTC, ms, `Z`
// UUIDv7 newtypes (36-character lowercase strings; ID_NOT_UUIDV7):
pub struct WorkspaceId(pub uuid::Uuid);     pub struct PlaceId(pub uuid::Uuid);
pub struct FolderGrantId(pub uuid::Uuid);   pub struct SourceLinkId(pub uuid::Uuid);
pub struct WorkingCopyItemId(pub uuid::Uuid); pub struct ConfluenceLinkId(pub uuid::Uuid);
pub struct EmailLinkId(pub uuid::Uuid);     pub struct EmailGrantId(pub uuid::Uuid);
pub struct AgentInstanceId(pub uuid::Uuid); pub struct CrewCharterId(pub uuid::Uuid);
pub struct ApprovalId(pub uuid::Uuid);      pub struct RunId(pub uuid::Uuid);
pub struct TelegramGrantId(pub uuid::Uuid);
pub struct SlackGrantId(pub uuid::Uuid);
```

### Domain types owned here (2C part 1, copied unchanged)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub own_folder_place_id: PlaceId,
    pub state: WorkspaceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_to_bin_at: Option<Timestamp>,
    pub created_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState { Active, Archived, InBin }

/// Membership of a place in a workspace (a place may belong to several, 04:65).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkspacePlace { pub workspace_id: WorkspaceId, pub place_id: PlaceId, pub added_at: Timestamp }

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
/// "can't use" is the absence of a grant (04:61).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess { Read, ReadAndChange }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SourceLink {
    pub id: SourceLinkId,
    pub workspace_id: WorkspaceId,
    pub kind: SourceLinkKind,
    pub origin_location: String,
    pub display_name: String,
    /// Local and repository folders; SharePoint and Confluence links once their working copy exists (2C C-9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<PlaceId>,
    pub state: SourceLinkState,
    pub added_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<Timestamp>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// `GitRemote` is the 2C addition of closing decision K21 (SA-CLOSE-03).
/// `JiraIssue`: one Jira Cloud issue, read only (amended January 2026, ADR-106; amendment §J).
pub enum SourceLinkKind { LocalFolder, GitRepositoryFolder, SharepointLocation, ConfluenceLocation, GitRemote, JiraIssue }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkState { Proposed, Active }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkingCopyItem {
    pub id: WorkingCopyItemId,
    pub source_link_id: SourceLinkId,
    pub origin_path: String,
    /// Version or change time at the origin when read (A-25).
    pub origin_version_read: String,
    pub local_version: u32,
    pub state: WorkingCopyState,
    pub read_at: Timestamp,
    /// Jira issue items only: when Jira reported a newer version that has not been read yet (amendment §J.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_newer_at: Option<Timestamp>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingCopyState { Read, Changed, AwaitingPush, Pushed, OriginChanged }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfluenceLink {
    pub id: ConfluenceLinkId,
    pub workspace_id: WorkspaceId,
    pub place_id: PlaceId,
    pub folder: String,
    pub confluence_space: String,
    pub parent_page_id: String,
    pub audience: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<Timestamp>,
    pub state: ConfluenceLinkState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfluenceLinkState { Proposed, Active, Paused }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailLink {
    pub id: EmailLinkId,
    pub workspace_id: WorkspaceId,
    pub kind: EmailLinkKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_or_label: Option<String>,
    pub addresses: Vec<String>,
    pub domains: Vec<String>,
    pub display_name: String,
    pub state: EmailLinkState,
    pub added_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<Timestamp>,
    pub new_since_last_looked: u32,
    pub imap_folders: Vec<ImapFolderState>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailLinkKind { FolderOrLabel, PeopleAndDomains }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailLinkState { Proposed, Active }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ImapFolderState { pub folder: String, pub uid_validity: u32, pub highest_uid: u32 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailGrant {
    pub id: EmailGrantId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: EmailAccess,
    pub granted_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailAccess { ReadEmail, ReadAndDraftEmail }

/// 2C TelegramGrant (amended January 2026, decision 80: C-25).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TelegramGrant {
    pub id: TelegramGrantId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: TelegramAccess,
    pub granted_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramAccess { ReadTelegram, ReadAndDraftTelegram }
/// 2C SlackGrant (ADR-102).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SlackGrant {
    pub id: SlackGrantId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: SlackAccess,
    pub granted_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackAccess { ReadSlack, ReadAndDraftSlack }

/// 2C ShareHost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareHost { Github, Gitlab }
```

Validation and state tables carried from 2C (codes are 2C's):

| Entity | Rule | Error code |
|---|---|---|
| Workspace.name | 1–80 chars after trimming; unique among workspaces not in the Bin (LD-24) | `WS_NAME_EMPTY`, `WS_NAME_TOO_LONG`, `WS_NAME_DUPLICATE` |
| Workspace.moved_to_bin_at | present iff `state = in_bin` | `WS_BIN_MISMATCH` |
| FolderGrant (workspace_id, grantee, place_id) | unique | `GRANT_DUPLICATE` |
| FolderGrant.place_id | a member of the workspace | `GRANT_PLACE_NOT_IN_WORKSPACE` |
| SourceLink.origin_location | 1–2048 bytes | `SRC_LOCATION_INVALID` |
| SourceLink.display_name | 1–120 chars | `SRC_NAME_INVALID` |
| SourceLink.place_id | required when kind ∈ {local_folder, git_repository_folder}; allowed when kind ∈ {sharepoint_location, confluence_location}, naming the link's working copy, a place of kind `working_copy` (2C C-9) | `SRC_PLACE_MISMATCH` |
| SourceLink (workspace_id, kind, origin_location) | unique | `SRC_DUPLICATE` |
| WorkingCopyItem.origin_path | 1–2048 bytes | `WCI_PATH_INVALID` |
| WorkingCopyItem (source_link_id, origin_path) | unique | `WCI_DUPLICATE` |
| WorkingCopyItem.origin_version_read | 1–256 chars | `WCI_VERSION_INVALID` |
| ConfluenceLink.confluence_space | 1–255 chars | `CFL_SPACE_INVALID` |
| ConfluenceLink.parent_page_id | 1–64 digits | `CFL_PARENT_INVALID` |
| ConfluenceLink.audience | 1–500 chars | `CFL_AUDIENCE_INVALID` |
| ConfluenceLink approval_id, approved_at | present for `active` and `paused` | `CFL_NOT_APPROVED` |
| EmailLink kind = folder_or_label | `folder_or_label` 1–255 chars, lists empty | `MAIL_LINK_FIELDS_MISMATCH` |
| EmailLink kind = people_and_domains | ≥ 1 address or domain, no folder | `MAIL_LINK_FIELDS_MISMATCH` |
| EmailLink.addresses | RFC 5321, ≤ 254 chars each, ≤ 200 | `MAIL_ADDRESS_INVALID` |
| EmailLink.domains | DNS name ≤ 253 chars, ≤ 100 | `MAIL_DOMAIN_INVALID` |
| EmailLink.imap_folders | 32-bit unsigned values | `MAIL_IMAP_STATE_INVALID` |
| EmailGrant (workspace_id, agent_instance_id) | unique | `MAIL_GRANT_DUPLICATE` |

State machines (2C): Workspace — `active ⇄ archived`; `active, archived → in_bin` (removed); `in_bin → active` (restored); `in_bin →` row deleted (deleted in the Bin or Bin emptied). SourceLink and EmailLink — `→ proposed → active`; `proposed →` deleted (declined); `active →` deleted (removed after confirmation). ConfluenceLink — `→ proposed → active` (approved); `active ⇄ paused`; `active, paused →` deleted (removed; no Confluence page deleted). WorkingCopyItem — `→ read`; `read → changed` (agent edits); `changed → awaiting_push`; `awaiting_push → pushed`; `awaiting_push → origin_changed` (origin newer); `origin_changed, pushed → read` (re-read); and, from HLD 04:85 (revision 0.49.0), `awaiting_push → changed` (push rejected, changes kept) and `pushed → read → changed` (a change after a push).

### Types of the adapters and the File Service used here (provider text is authoritative; R6, R7)

```rust
// ordin-files
pub struct Place { pub id: PlaceId, pub friendly_name: String, pub path: String, pub kind: PlaceKind, pub added_at: Timestamp }
/// 2C as amended by decision 80 (C-9).
pub enum PlaceKind { PersonsFolder, WorkspaceFolder, AgentFolder, WorkingCopy, CodingTaskWorktree }
pub struct AddPlace { pub path: std::path::PathBuf, pub friendly_name: Option<String>, pub kind: PlaceKind }
pub struct PlaceChange { pub place_id: PlaceId, pub path: String, pub change: PlaceChangeKind,
                         pub observed_at: Timestamp, pub by_ordin: bool,
                         /// The version recorded for this change; absent when none was written.
                         pub version: Option<u32>,
                         /// The origin of that version; absent exactly when `version` is absent.
                         pub origin: Option<FileVersionOrigin> }
pub enum PlaceChangeKind { Created, Modified, Removed, Renamed { from: String } }
impl FileService {
    pub async fn add_place(&self, req: AddPlace) -> Result<Place, FileError>;        // FileError::PlaceExists { existing } = reuse
    pub async fn remove_place(&self, place_id: PlaceId) -> Result<(), FileError>;
    pub async fn place(&self, place_id: PlaceId) -> Result<Place, FileError>;
    pub async fn places(&self, ids: &[PlaceId], include_hidden: bool) -> Result<Vec<Place>, FileError>;
    pub async fn publish_grants(&self, workspace_id: WorkspaceId, grants: Vec<FolderGrant>) -> Result<(), FileError>;
    pub async fn store_fetched(&self, place_id: PlaceId, path: &str, bytes: Vec<u8>,
                               changed_by: Option<String>, changed_at: Timestamp) -> Result<Sha256Hex, FileError>;
    pub async fn held_edits(&self, place_ids: &[PlaceId]) -> Result<Vec<HeldEdit>, FileError>;
    pub async fn watch(&self, place_ids: &[PlaceId]) -> Result<(), FileError>;
    pub async fn unwatch(&self, place_ids: &[PlaceId]);
    pub fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<PlaceChange>;
    /// Used by `save_to_inbox` and `save_generated` (SA-CLOSE-02).
    pub async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError>;
}
pub struct WriteVersion { pub place_id: PlaceId, pub path: String, pub bytes: Vec<u8>,
                          pub origin: FileVersionOrigin, pub expected_hash: Option<Sha256Hex> }
// FileVersionOrigin { Person, Run { run_id, tool }, ArtCraftEdit { app } } (2C).
// `FileError` is the File Service's enum (`FILE_*` codes); this service reads `code()` and `retryable()`.

// ordin-sharepoint and ordin-confluence (each crate defines its own types of these names)
impl SharePointAdapter {
    pub fn origin_location(loc: &SharePointLocation) -> String;
    pub async fn resolve_origin(&self, origin_location: &str) -> Result<SharePointLocation, SharePointError>;
    /// `Cursor` and `Page` are `ordin_types`' (2C C-15).
    pub async fn list_children(&self, loc: &SharePointLocation, cursor: Option<Cursor>) -> Result<Page<SharePointItem>, SharePointError>;
    pub async fn fetch(&self, item: &SharePointItemRef) -> Result<FetchedItem, SharePointError>;
    pub async fn track_link(&self, link: &SourceLink) -> Result<(), SharePointError>;
    pub async fn untrack_link(&self, link: SourceLinkId) -> Result<(), SharePointError>;
    pub async fn check_changes(&self, link: &SourceLink) -> Result<Vec<OriginChange>, SharePointError>;
}
impl ConfluenceAdapter {
    // the same seven reading methods over ConfluenceLocation (`list_children` takes `Option<Cursor>`), plus:
    pub async fn audience(&self, loc: &ConfluenceLocation) -> Result<Audience, ConfluenceError>;
    pub async fn request_link_approval(&self, link: &ConfluenceLink, place: &Place, workspace: &Workspace) -> Result<ApprovalId, ConfluenceError>;
    pub async fn sync_linked_folder(&self, link: &ConfluenceLink, place: &Place) -> Result<LinkSyncReport, ConfluenceError>;
    pub async fn forget_link(&self, link: ConfluenceLinkId) -> Result<(), ConfluenceError>;
}
// SharePointItem | RemoteItem { item, name|title, path, is_folder|has_children, size_bytes, version }; FetchedItem { item, bytes, version };
// SharePointVersion | RemoteVersion { tag, changed_at, changed_by, .. }; OriginChange { source_link_id, origin_path, new_version, changed_by, changed_at, removed }.
// Error mapping used here, on each adapter error's `code()`: SP_/CONF_ SIGN_IN_NEEDED, NOT_CONFIGURED, ACCESS_DENIED,
// NOT_FOUND, TOO_LARGE, UNAVAILABLE, THROTTLED (Internal Logic 3.5).
```

### Rules Engine and Journal interfaces used here (provider text is authoritative; R3, R5)

```rust
// ordin-rules (RulesEngine.md *The engine*): a concrete type, held as `Arc<ordin_rules::RulesEngine>`.
impl RulesEngine {
    /// Whether a folder may never become a place; called for each own folder and each dropped or picked folder.
    pub fn is_protected_location(&self, path: &std::path::Path) -> bool;
}

// ordin-journal (JournalAndArtefactStore.md): a concrete type, cheap to clone; no trait object.
pub type MigrationStep = (i64, &'static str, &'static str);      // (version, description, SQL)
pub struct ModuleMigrations { pub module: &'static str,
                              pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
pub struct NewJournalEntry { pub kind: String, pub component: String, pub run_id: Option<RunId>,
                             pub crew_run_id: Option<CrewRunId>, pub workspace_id: Option<WorkspaceId>,
                             pub summary: String, pub data: serde_json::Value }
pub struct JournalEvent { pub backend: uuid::Uuid, pub seq: u64, pub at: Timestamp,
                          pub kind: String, pub data: serde_json::Value }
impl Journal {
    pub fn datastore(&self) -> &Datastore;            // D4: this crate's tables live in the Journal's datastore
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    pub async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>;
    pub async fn events_since(&self, after_seq: u64, limit: u32) -> Result<Vec<JournalEvent>, JournalError>;
    pub fn last_seq(&self) -> u64;
    /// Removes reports and dashboards of a removed workspace.
    pub async fn forget_workspace(&self, workspace_id: WorkspaceId) -> Result<u64, JournalError>;
}
impl Datastore { pub fn sqlite(&self) -> Option<&sqlx::SqlitePool>; }
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    fn name(&self) -> &'static str;
    async fn deliver(&self, event: &JournalEvent);
    async fn lagged(&self, next_seq: u64);
}
// 2C wire: `EventKind::{ApprovalDecided, WorkspaceChanged}` with `as_str()`/`parse()`;
// `ServiceEvent::ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String }`;
// `ApprovalState { Pending, Approved, Rejected, Expired }`. `JournalError` implements `CodedError`.
```

The Approval Service is not in this crate's 2B-3 dependency list, so decisions are matched by the `approval_id` the service stored on the Confluence link row when the Confluence Adapter returned it; nothing else is read from the Approval Service.

### Constants (*chosen here* unless cited)

| Constant | Value |
|---|---|
| `CHECK_INTERVAL` | 15 minutes while ordin runs (A-36, A-37 analogue; B7 contract D-8) |
| `CHECK_CONCURRENCY` | 2 links at a time across workspaces |
| `FETCH_CONCURRENCY` | 4 items at a time per link |
| `LINK_MAX_ITEMS` | 100,000 files and 20 folder levels per link (A-21) |
| `LINK_READ_BUDGET` | 5 GiB of content per remote link; items beyond are listed in the link's progress as not read and read on first use *(assumed here; bounds disk use)* |
| `OWN_FOLDER_ROOT` | `<Documents>/ordin/` where `<Documents>` is `$HOME/Documents` on macOS and Linux and `%USERPROFILE%\Documents` on Windows *(assumed here; A-23 names "the person's Documents")* |
| `WORKING_COPY_ROOT` | `<app data root>/working-copies/<source_link_id>/` (B7 contract D-5) |
| `LINKED_FOLDER_DEBOUNCE` | 5 s after the last change in a linked folder before `sync_linked_folder` |
| `INBOX_FOLDER` | `Inbox/` in the workspace's own folder: files the person sends in a chat (SA-CLOSE-02) |
| `RESULTS_FOLDER` | `Results/` in the workspace's own folder: files made for a chat reply (SA-CLOSE-02) |
| `SAVED_FILE_MAX_BYTES` | 100 MiB per saved file *(chosen here; above the chat channels' receive limits)* |

### Reconciliation assumptions

- **SA-CLOSE-01** (*chosen here*; closing decisions K21, K22) — 2C's WorkingCopyItem transition table carries `awaiting_push → changed` and `pushed → read` (HLD 04:85, 0.49.0), which Internal Logic 6.3 applies; the Journal's caller table lists this service for `append_event`, `events_since` and `last_seq`.
- **SA-CLOSE-02** (*chosen here*; closing decision K18) — `save_to_inbox` and `save_generated` write through the File Service into the workspace's own folder (`Inbox/` and `Results/`), so the File Service keeps the versions and the tree shows the files; `origin_changes_since` reads the origin changes already stored on working-copy items.
- **SA-CLOSE-03** (*chosen here*; closing decision K21) — `LinkLocation::GitRemote { url, branch, path }` links a repository folder without a local clone, for a handover's Git place and Git result links. It is a link only: no place, no working copy, no grant, no watch, no origin check; the tree shows it with the repository's web address, which the screens open in the browser.
- **SA-RECON-01** — The Workspace Service owns `working_copy_items` and their states, including items of local and git folder links (R12, Internal Logic 6.7); the Push Service reaches them through its `WorkingCopyStore`, which the program implements over `working_copy_entry`, `set_working_copy_state` and `subscribe_working_copy_changes` (D3), so this crate has no Push Service edge.
- **SA-RECON-02** — Rules Engine, File Service, Journal and the adapters are used through their own published names and concrete types (R3, R5, R6): `ordin_rules::RulesEngine::is_protected_location`, the concrete `Journal` with `append_entry`, `append_event`, `forget_workspace` and `MigrationStep` tuples, and adapter errors read through `code()`.
- **SA-RECON-03** — This crate's tables live in the Journal's `Datastore` (D4, R20); the program hands it no separate pool.
- **SA-RECON-04** — Journal events reach this service through its own `EventSink` implementation registered by the program; there is no `EventEnvelope` broadcast receiver (Journal reconciliation row 15).
- **SA-RECON-05** — Service API command names are this module's own, adopted by the catalogue (R1); `workspace_changed` is emitted with `EventKind::WorkspaceChanged` (2C wire).
- **SA-RECON-06** — `PlaceKind::WorkingCopy`, `PlaceKind::CodingTaskWorktree` and `SourceLink.place_id` on SharePoint and Confluence links are 2C's as amended (C-9); `Page` and `Cursor` are `ordin_types`' (C-15).
- **SA-RECON-07** — SharePoint and Confluence writes are bracketed as external actions by the adapters alone (single bracket); this service writes nothing to an origin.

## Public Interface

Error prefix: `WS_` for this component's own codes; the 2C validation codes above.

```rust
pub struct WorkspaceDeps {
    /// Also the source of this crate's `Datastore` (`journal.datastore()`, D4).
    pub journal: ordin_journal::Journal,
    pub rules: std::sync::Arc<ordin_rules::RulesEngine>,
    pub files: std::sync::Arc<FileService>,
    pub sharepoint: std::sync::Arc<SharePointAdapter>,
    pub confluence: Option<std::sync::Arc<ConfluenceAdapter>>,   // None on the personal server (2B-3: Confluence is D only)
    /// None on the personal server: Jira is D only (amended January 2026, ADR-106).
    pub jira: Option<std::sync::Arc<ordin_jira::JiraAdapter>>,
    pub data_root: std::path::PathBuf,
    pub documents_dir: std::path::PathBuf,
    /// True in `ordin-server` (personal profile): folders on the person's computer cannot be linked there.
    pub on_server: bool,
}

pub struct WorkspaceService { /* private */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceListFilter { Active, Archived, InBin, NotInBin }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkLocation {
    LocalFolder { path: String },
    GitRepositoryFolder { path: String },
    Sharepoint { location: SharePointLocation },
    Confluence { location: ConfluenceLocation },
    /// A folder of a GitHub or GitLab repository, linked without a local clone (2C addition, K21;
    /// SA-CLOSE-03). `url` `https://<host>/<owner>/<name>(.git)?` or `git@<host>:<owner>/<name>(.git)?`;
    /// `branch` 1–255 characters; `path` relative to the repository root, "" for the root, ≤ 1,024 bytes, no `..`.
    GitRemote { url: String, branch: String, path: String },
    /// A Jira Cloud issue address as the person pasted or picked it (amended January 2026, ADR-106; amendment §J.1).
    JiraIssue { address: String },
}

/// A file this service saved in a workspace's own folder (SA-CLOSE-02).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlaceFileRef {
    pub place_id: PlaceId,
    pub relative_path: String,
    /// "<workspace name> › Inbox › <file name>", for the reply that names where the file went.
    pub display_path: String,
}

/// Where a file saved with `save_to_inbox` came from (SA-CLOSE-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InboxOrigin { Chat { channel: ChannelKind } }   // 2C `ChannelKind { Telegram, Slack }`

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkProgress {
    pub source_link_id: SourceLinkId,
    pub listed: u64,
    pub read: u64,
    pub not_read: u64,         // beyond LINK_READ_BUDGET or failed
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OpenReport {
    pub workspace: Workspace,
    pub origin_changes: u32,       // changes found by the adapters at this open
    pub checks_failed: Vec<SourceLinkId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BinImpact {
    /// Documents changed here and not yet sent back (working-copy items not `read`/`pushed`, and held edits).
    pub unsent_changes: u32,
    pub documents: Vec<String>,   // display names, at most 10
}

/// What the Push Service needs about one working-copy item (through the program's `WorkingCopyStore`, R12).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkingCopyEntry {
    pub item: WorkingCopyItem,
    pub workspace_id: WorkspaceId,
    pub link_kind: SourceLinkKind,
    /// The link's `origin_location` (SharePoint `graph:drives/<drive>/items/<item>` form, Confluence origin form).
    pub origin_location: String,
    pub display_name: String,
    /// The working-copy place and the item's path in it; the bytes and versions are the File Service's.
    /// For a local or git folder link this is the folder's own place: the folder is its own working copy.
    pub place_id: PlaceId,
    pub relative_path: String,
    /// Git folder links only: the branch checked out when the item was recorded, and the repository
    /// (`None` when its `origin` remote is not on GitHub or GitLab, Internal Logic 10.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<LinkedRepository>,
}

/// The origin version this service last recorded for an item (Internal Logic 6.4, 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginVersion {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_at: Option<Timestamp>,
}

/// Broadcast when an origin check records a new origin version for an item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginChanged {
    pub workspace_id: WorkspaceId,
    pub source_link_id: SourceLinkId,
    pub link_kind: SourceLinkKind,
    pub origin_path: String,
    pub version: OriginVersion,
    /// True when the item was removed at its origin.
    pub removed: bool,
}

/// A linked git repository folder whose `origin` remote is on GitHub or GitLab (Internal Logic 10.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkedRepository {
    pub source_link_id: SourceLinkId,
    pub host: ShareHost,               // 2C `ShareHost { Github, Gitlab }`
    /// `owner/name` (2C repository rule).
    pub repository: String,
    /// The remote's host name, e.g. "github.com" or "gitlab.example.com".
    pub host_name: String,
    pub place_id: PlaceId,
}

/// The facts of one workspace that Ordin's seed needs (OrdinHost X-7).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceSummary {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub state: WorkspaceState,
    /// Display names of the workspace's places and active links, in the tree's order.
    pub places: Vec<String>,
    pub repositories: Vec<LinkedRepository>,
}

pub struct NewEmailLink {
    pub kind: EmailLinkKind,
    pub folder_or_label: Option<String>,
    pub addresses: Vec<String>,
    pub domains: Vec<String>,
    pub display_name: String,
}

/// Registered by the program with the Journal's `MigrationRegistry` (R3): module `workspaces`,
/// `sqlite: &[(1, "workspace tables", <the Data Model SQL>)]`, `postgres: &[]`.
pub fn migrations() -> ModuleMigrations;

/// The program registers it with `Journal::subscribe` (name `"workspaces"`); Internal Logic 8.3.
#[async_trait::async_trait]
impl ordin_journal::EventSink for WorkspaceService { /* name, deliver, lagged */ }

impl WorkspaceService {
    /// Fails with `WS_DATASTORE_UNAVAILABLE` when the Journal's datastore is not SQLite (this module does not
    /// run in the agent profile).
    pub async fn new(deps: WorkspaceDeps) -> Result<std::sync::Arc<WorkspaceService>, WorkspaceError>;
    /// Publishes every non-bin workspace's grants, watches their places, tracks links without state,
    /// runs the start-up origin check, subscribes to File Service changes, starts the 15-minute timer.
    pub async fn start(self: &std::sync::Arc<Self>) -> Result<(), WorkspaceError>;
    pub async fn shutdown(&self);

    // Workspaces
    pub async fn create(&self, name: String) -> Result<Workspace, WorkspaceError>;
    pub async fn rename(&self, id: WorkspaceId, name: String) -> Result<Workspace, WorkspaceError>;
    pub async fn archive(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    pub async fn restore(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    pub async fn bin_impact(&self, id: WorkspaceId) -> Result<BinImpact, WorkspaceError>;
    pub async fn move_to_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    pub async fn restore_from_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    pub async fn delete_from_bin(&self, id: WorkspaceId) -> Result<(), WorkspaceError>;
    pub async fn empty_bin(&self) -> Result<u32, WorkspaceError>;
    pub async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    pub async fn list(&self, filter: WorkspaceListFilter) -> Result<Vec<Workspace>, WorkspaceError>;
    /// A task, crew run or conversation may start only in an active workspace.
    pub async fn ensure_active(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    pub async fn open(&self, id: WorkspaceId) -> Result<OpenReport, WorkspaceError>;
    pub async fn close(&self, id: WorkspaceId);

    // Places and links
    pub async fn places(&self, id: WorkspaceId) -> Result<Vec<Place>, WorkspaceError>;
    pub async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError>;
    pub async fn propose_link(&self, id: WorkspaceId, location: LinkLocation, display_name: Option<String>) -> Result<SourceLink, WorkspaceError>;
    pub async fn confirm_link(&self, link: SourceLinkId) -> Result<SourceLink, WorkspaceError>;
    pub async fn decline_link(&self, link: SourceLinkId) -> Result<(), WorkspaceError>;
    /// **Add** on the Add to this workspace screen: propose and confirm in one step.
    pub async fn add_link(&self, id: WorkspaceId, location: LinkLocation, display_name: Option<String>) -> Result<SourceLink, WorkspaceError>;
    pub async fn remove_link(&self, link: SourceLinkId) -> Result<(), WorkspaceError>;
    pub async fn link_progress(&self, link: SourceLinkId) -> Result<LinkProgress, WorkspaceError>;
    /// Reads one item that was listed but not read (budget), on first use.
    pub async fn read_item(&self, link: SourceLinkId, origin_path: &str) -> Result<WorkingCopyItem, WorkspaceError>;

    // Working copy (this service owns `working_copy_items`, R12; the Push Service sets push states
    // through the program's implementation of its `WorkingCopyStore`, because 2B-3 gives it no edge here)
    pub async fn working_copy_items(&self, link: SourceLinkId) -> Result<Vec<WorkingCopyItem>, WorkspaceError>;
    pub async fn working_copy_item(&self, id: WorkingCopyItemId) -> Result<WorkingCopyItem, WorkspaceError>;
    pub async fn working_copy_entry(&self, id: WorkingCopyItemId) -> Result<WorkingCopyEntry, WorkspaceError>;
    pub async fn set_working_copy_state(&self, id: WorkingCopyItemId, state: WorkingCopyState,
                                        origin_version: Option<String>) -> Result<WorkingCopyItem, WorkspaceError>;
    /// The id of each item that moves to `changed` (Internal Logic 6.2). Capacity 1,024.
    pub fn subscribe_working_copy_changes(&self) -> tokio::sync::broadcast::Receiver<WorkingCopyItemId>;

    // Origin versions (Knowledge Service freshness, reconciliation rows 675–676)
    /// The origin version last recorded for the item at `origin_path` of the workspace's active link of `link_kind`
    /// (`sharepoint_location` or `confluence_location`); `None` when no such item exists.
    pub async fn origin_version(&self, workspace_id: WorkspaceId, link_kind: SourceLinkKind,
                                origin_path: &str) -> Result<Option<OriginVersion>, WorkspaceError>;
    /// Capacity 1,024; a lagging receiver re-reads through `origin_version`.
    pub fn subscribe_origin_changes(&self) -> tokio::sync::broadcast::Receiver<OriginChanged>;
    /// The latest origin change recorded for each working-copy item since `since`, newest first, at most
    /// 1,000 (SA-CLOSE-02). `removed` is always false here (a removal is reported on the broadcast only).
    pub async fn origin_changes_since(&self, since: Timestamp) -> Result<Vec<OriginChanged>, WorkspaceError>;

    // Files saved by the Channel Gateway (SA-CLOSE-02)
    /// Saves a file the person sent in a chat into `Inbox/` of the workspace's own folder.
    pub async fn save_to_inbox(&self, ws: WorkspaceId, name: &str, bytes: bytes::Bytes,
                               origin: InboxOrigin) -> Result<PlaceFileRef, WorkspaceError>;
    /// Saves a file made for a chat reply by run `by` into `Results/` of the workspace's own folder.
    pub async fn save_generated(&self, ws: WorkspaceId, name: &str, bytes: bytes::Bytes,
                                by: RunId) -> Result<PlaceFileRef, WorkspaceError>;

    // Workspace notices for later modules (Handover Service, program → Ordin Host)
    /// The id of each workspace the person opens (`open`). Capacity 256.
    pub fn opened(&self) -> tokio::sync::broadcast::Receiver<WorkspaceId>;
    /// The id of each workspace deleted from the Bin (`delete_from_bin`, `empty_bin`). Capacity 256.
    pub fn deleted(&self) -> tokio::sync::broadcast::Receiver<WorkspaceId>;
    /// Each link `remove_link` removed, as (workspace, link), sent after the row is deleted. Capacity 256.
    /// (amended January 2026, ADR-103)
    pub fn links_removed(&self) -> tokio::sync::broadcast::Receiver<(WorkspaceId, SourceLinkId)>;
    /// Name, places, links and linked repositories (OrdinHost X-7).
    pub async fn summary(&self, id: WorkspaceId) -> Result<WorkspaceSummary, WorkspaceError>;
    pub async fn linked_repositories(&self, id: WorkspaceId) -> Result<Vec<LinkedRepository>, WorkspaceError>;

    // Grants
    /// `access = None` is "can't use": the grant is removed.
    pub async fn set_grant(&self, id: WorkspaceId, grantee: Grantee, place_id: PlaceId,
                           access: Option<FolderAccess>) -> Result<Option<FolderGrant>, WorkspaceError>;
    pub async fn grants(&self, id: WorkspaceId) -> Result<Vec<FolderGrant>, WorkspaceError>;
    /// Removes a grantee's grants in every workspace (agent instance removed, 04:61).
    pub async fn revoke_grantee(&self, grantee: Grantee) -> Result<u32, WorkspaceError>;

    // Confluence links (ADR-026)
    pub async fn propose_confluence_link(&self, id: WorkspaceId, place_id: PlaceId, folder: String,
                                         location: ConfluenceLocation) -> Result<ConfluenceLink, WorkspaceError>;
    pub async fn confluence_links(&self, id: WorkspaceId) -> Result<Vec<ConfluenceLink>, WorkspaceError>;
    pub async fn pause_confluence_link(&self, link: ConfluenceLinkId) -> Result<ConfluenceLink, WorkspaceError>;
    pub async fn resume_confluence_link(&self, link: ConfluenceLinkId) -> Result<ConfluenceLink, WorkspaceError>;
    pub async fn remove_confluence_link(&self, link: ConfluenceLinkId) -> Result<(), WorkspaceError>;

    // Email links and grants (ADR-070; the Mail Service enforces them)
    pub async fn propose_email_link(&self, id: WorkspaceId, link: NewEmailLink) -> Result<EmailLink, WorkspaceError>;
    pub async fn confirm_email_link(&self, link: EmailLinkId) -> Result<EmailLink, WorkspaceError>;
    pub async fn remove_email_link(&self, link: EmailLinkId) -> Result<(), WorkspaceError>;
    /// Every email link of the workspace, proposed and active; callers that need active links filter on `state`.
    /// The check fields come back empty (`last_checked_at: None`, `new_since_last_looked: 0`, `imap_folders: []`):
    /// the Mail Service owns them in `mail_link_check_state` (2C C-26) and fills them where it returns links.
    pub async fn email_links(&self, id: WorkspaceId) -> Result<Vec<EmailLink>, WorkspaceError>;
    pub async fn email_link(&self, link: EmailLinkId) -> Result<Option<EmailLink>, WorkspaceError>;
    /// Active workspaces holding at least one active email link.
    pub async fn active_workspaces_with_email(&self) -> Result<Vec<WorkspaceId>, WorkspaceError>;
    pub async fn set_email_grant(&self, id: WorkspaceId, agent: AgentInstanceId,
                                 access: Option<EmailAccess>) -> Result<Option<EmailGrant>, WorkspaceError>;
    pub async fn email_grants(&self, id: WorkspaceId) -> Result<Vec<EmailGrant>, WorkspaceError>;
    pub async fn email_grant(&self, id: WorkspaceId, agent: AgentInstanceId) -> Result<Option<EmailGrant>, WorkspaceError>;

    // Telegram grants (2C TelegramGrant, C-25; the Telegram Service enforces them)
    pub async fn set_telegram_grant(&self, id: WorkspaceId, agent: AgentInstanceId,
                                    access: Option<TelegramAccess>) -> Result<Option<TelegramGrant>, WorkspaceError>;
    pub async fn telegram_grants(&self, id: WorkspaceId) -> Result<Vec<TelegramGrant>, WorkspaceError>;
    pub async fn telegram_grant(&self, id: WorkspaceId, agent: AgentInstanceId) -> Result<Option<TelegramGrant>, WorkspaceError>;
    // Slack grants (2C SlackGrant; the Slack Service enforces them)
    pub async fn set_slack_grant(&self, id: WorkspaceId, agent: AgentInstanceId,
                                 access: Option<SlackAccess>) -> Result<Option<SlackGrant>, WorkspaceError>;
    pub async fn slack_grants(&self, id: WorkspaceId) -> Result<Vec<SlackGrant>, WorkspaceError>;
    pub async fn slack_grant(&self, id: WorkspaceId, agent: AgentInstanceId) -> Result<Option<SlackGrant>, WorkspaceError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum WorkspaceError {
    #[error("WS_NOT_FOUND")] NotFound,
    #[error("WS_NOT_ACTIVE")] NotActive { state: WorkspaceState },
    #[error("WS_STATE_INVALID")] StateInvalid,
    #[error("WS_OWN_FOLDER_FAILED")] OwnFolderFailed,
    #[error("WS_LINK_NOT_FOUND")] LinkNotFound,
    #[error("WS_LINK_STATE_INVALID")] LinkStateInvalid,
    #[error("WS_NOT_A_REPOSITORY")] NotARepository,
    #[error("WS_PLACE_REFUSED")] PlaceRefused { cause: &'static str },
    #[error("WS_SOURCE_SIGN_IN_NEEDED")] SourceSignInNeeded,
    #[error("WS_SOURCE_NOT_CONFIGURED")] SourceNotConfigured,
    #[error("WS_SOURCE_ACCESS_DENIED")] SourceAccessDenied,
    #[error("WS_SOURCE_NOT_FOUND")] SourceNotFound,
    #[error("WS_SOURCE_UNAVAILABLE")] SourceUnavailable,
    #[error("WS_CONFLUENCE_UNAVAILABLE")] ConfluenceUnavailable,
    #[error("WS_ITEM_TOO_LARGE")] ItemTooLarge,
    #[error("WS_WORKING_COPY_TRANSITION_INVALID")] WorkingCopyTransitionInvalid { from: WorkingCopyState, to: WorkingCopyState },
    #[error("WS_GRANT_PUBLISH_FAILED")] GrantPublishFailed,
    #[error("WS_FILES_FAILED")] FilesFailed,
    #[error("WS_DATASTORE_UNAVAILABLE")] DatastoreUnavailable,
    #[error("WS_LOCAL_FOLDER_ON_SERVER")] LocalFolderOnServer,
    #[error("WS_WORKING_COPY_ITEM_NOT_FOUND")] WorkingCopyItemNotFound,
    #[error("WS_FILE_NAME_INVALID")] FileNameInvalid,
    #[error("WS_FILE_TOO_LARGE")] FileTooLarge,
    #[error("WS_GIT_REMOTE_INVALID")] GitRemoteInvalid,
    /// A 2C validation code (WS_NAME_*, SRC_*, WCI_*, GRANT_*, CFL_*, MAIL_*).
    #[error("{0}")] Invalid(&'static str),
}
impl WorkspaceError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

### Service API commands (*chosen here*; SA-API-01)

| Command | Input | Result |
|---|---|---|
| `workspaces.create` | `{ "name" }` | `Workspace` |
| `workspaces.rename` | `{ "workspace_id", "name" }` | `Workspace` |
| `workspaces.archive` / `workspaces.restore` | `{ "workspace_id" }` | `Workspace` |
| `workspaces.bin_impact` | `{ "workspace_id" }` | `BinImpact` |
| `workspaces.move_to_bin` / `workspaces.restore_from_bin` | `{ "workspace_id" }` | `Workspace` |
| `workspaces.delete_from_bin` | `{ "workspace_id" }` | `{}` |
| `workspaces.empty_bin` | `{}` | `{ "deleted": u32 }` |
| `workspaces.list` | `{ "filter": WorkspaceListFilter }` | `Vec<Workspace>` |
| `workspaces.open` | `{ "workspace_id" }` | `OpenReport` |
| `workspaces.links` | `{ "workspace_id" }` | `Vec<SourceLink>` |
| `workspaces.add_link` | `{ "workspace_id", "location": LinkLocation, "display_name": string? }` | `SourceLink` |
| `workspaces.confirm_link` / `workspaces.decline_link` / `workspaces.remove_link` | `{ "source_link_id" }` | `SourceLink` / `{}` / `{}` |
| `workspaces.link_progress` | `{ "source_link_id" }` | `LinkProgress` |
| `workspaces.set_grant` | `{ "workspace_id", "grantee": Grantee, "place_id", "access": FolderAccess? }` | `FolderGrant?` |
| `workspaces.grants` | `{ "workspace_id" }` | `Vec<FolderGrant>` |
| `workspaces.propose_confluence_link` | `{ "workspace_id", "place_id", "folder", "location": ConfluenceLocation }` | `ConfluenceLink` |
| `workspaces.pause_confluence_link` / `workspaces.resume_confluence_link` / `workspaces.remove_confluence_link` | `{ "confluence_link_id" }` | `ConfluenceLink` / `ConfluenceLink` / `{}` |
| `workspaces.email_links` | `{ "workspace_id" }` | `Vec<EmailLink>` |
| `workspaces.add_email_link` | `{ "workspace_id", "kind", "folder_or_label"?, "addresses", "domains", "display_name" }` | `EmailLink` (proposed then confirmed) |
| `workspaces.remove_email_link` | `{ "email_link_id" }` | `{}` |
| `workspaces.set_email_grant` | `{ "workspace_id", "agent_instance_id", "access": EmailAccess? }` | `EmailGrant?` |
| `workspaces.set_telegram_grant` | `{ "workspace_id", "agent_instance_id", "access": TelegramAccess? }` | `TelegramGrant?` |
| `workspaces.telegram_grants` | `{ "workspace_id" }` | `Vec<TelegramGrant>` |
| `workspaces.set_slack_grant` | `{ "workspace_id", "agent_instance_id", "access": SlackAccess? }` | `SlackGrant?` |
| `workspaces.slack_grants` | `{ "workspace_id" }` | `Vec<SlackGrant>` |
| `workspaces.tree` | `{ "workspace_id" }` | `WorkspaceTree` (ServiceApi.md's type) — implemented by the program, which composes `get` and `places` here with the Package Catalogue's agents, the Ordin Host's workflows, dashboards and running items, the Knowledge Service's and Memory Service's counts and Home and Search's `needs_you` count (no dependency edge from this crate reaches them) |

These are the catalogue's names (R1; ServiceApi.md lists `workspaces.tree` with input `WorkspaceRef { workspace_id }`). Events: this service appends `workspace_changed { workspace_id }` through the Journal on each change the tree shows (Internal Logic 11).

`propose_link` and `confirm_link` are reached only through the Service API commands; the Ordin Host makes no link call (OrdinHost.md X-7), and the person's confirmation is the `confirm_link` command.

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| `create`, `rename`, `archive`, `restore`, bin methods, `list`, `open`, link and grant methods, Service API commands | Service API catalogue (screens); `create` also Handover Service (`.id` of the result) |
| `get` | Knowledge Service, Team Knowledge Service (existence through `WS_NOT_FOUND`), Telegram Service (own folder through `own_folder_place_id`; existence through `WS_NOT_FOUND`), Mail Service, Handover Service (workspace names in `following`), Slack Service (existence through `WS_NOT_FOUND`, the workspace name on the link screen) |
| `list(filter)` | Home and Search (the program then fills `last_seen_at` with Home's `with_last_seen`) |
| `places` | Home and Search |
| `source_links` | Home and Search, Handover Service (linked SharePoint places), Team Knowledge Service (its `TeamWorkspaces`: links offered for Team Knowledge mode) |
| `ensure_active` | Ordin Host (before a task, crew run or conversation starts) |
| `add_link` | Handover Service (`accept_and_start`, `start_workspace_for_stage`: result and place links), Team Knowledge Service (a new `GitRemote` link chosen in **Share knowledge with a team…**, SA-TEAM-01) |
| `revoke_grantee` | Package Catalogue (agent instance removed) |
| `working_copy_entry`, `set_working_copy_state`, `subscribe_working_copy_changes` | program, implementing the Push Service's `WorkingCopyStore` (R12) |
| `origin_version`, `subscribe_origin_changes` | Knowledge Service |
| `email_links`, `email_link`, `email_grant`, `email_grants`, `set_email_grant`, `active_workspaces_with_email` | Mail Service |
| `telegram_grant`, `telegram_grants` | Telegram Service |
| `slack_grant`, `slack_grants` | Slack Service |
| `opened` | Handover Service, Team Knowledge Service (check of a bound folder) |
| `deleted` | program (calls `OrdinHost::workspace_removed`), Team Knowledge Service (drops the binding and its clone) |
| `links_removed` | Team Knowledge Service (turns Team Knowledge mode off when its link is removed) |
| `summary` | Ordin Host (X-7) |
| `linked_repositories` | Ordin Host (X-7), Handover Service (Git places of a draft) |
| `save_to_inbox`, `save_generated` | Channel Gateway (chat files, step E; blocks without a text form, step H) |
| `origin_changes_since` | Home and Search (copy updates in *Since you last looked*) |

### Example

```rust
let ws = workspaces.create("Q3 supplier review".into()).await?;
// Workspace { id: 0192a0f1-1111-7222-8333-444455556666, name: "Q3 supplier review",
//   own_folder_place_id: 0192a0f1-…, state: Active, last_seen_at: None, moved_to_bin_at: None,
//   created_at: 2026-01-10T09:00:12.004Z }   (folder ~/Documents/ordin/Q3 supplier review created)

let link = workspaces.add_link(ws.id, LinkLocation::Sharepoint { location: contracts }, None).await?;
// SourceLink { kind: SharepointLocation, origin_location: "graph:drives/b!x9Fq…/items/01ABCDEF2GHIJ",
//   display_name: "Contracts", place_id: Some(<working-copy place>), state: Active, last_read_at: None, .. }
// background: LinkProgress { listed: 212, read: 212, not_read: 0, done: true }

workspaces.set_grant(ws.id, Grantee::AgentInstance { agent_instance_id: analyst }, link.place_id.unwrap(),
                     Some(FolderAccess::Read)).await?;
// FolderGrant { access: Read, .. }; the File Service hands it to the Agent Folder Manager and the Rules Engine.
```

## Internal Logic

### 1. Start

1. Load workspaces not in the Bin. For each: `files.publish_grants(id, grants(id))`; failures are retried with backoff 1 s, 2 s, 4 s, then logged `grants_publish_failed {workspace_id}` and retried at the next timer tick; until it succeeds the Rules Engine has no grants for that workspace, which denies (fails closed).
2. Watch every place of active and archived workspaces (`files.watch`). Places only of workspaces in the Bin are not watched.
3. For each active remote link without adapter tracking state, call `track_link`. Then run the origin check (step 7) for every active workspace, `CHECK_CONCURRENCY` links at a time, in the background.
4. Subscribe to `files.subscribe_changes()` (step 6.2, step 8.4). Journal events reach this service through its `EventSink` (step 8.3), registered by the program. Start the `CHECK_INTERVAL` timer.

Throughout, "Journal `k {fields}`" means `journal.append_entry(NewJournalEntry { kind: "k", component: "workspaces", run_id: None, crew_run_id: None, workspace_id: Some(<the workspace>), summary: <one plain sentence naming the action, no names or paths>, data: {fields} })`. A failed append is logged `journal_entry_failed {kind}` and does not undo the change.

### 2. Creating and changing workspaces

1. `create(name)`: trim; validate 1–80 (`WS_NAME_EMPTY`, `WS_NAME_TOO_LONG`); unique among `state != in_bin` (`WS_NAME_DUPLICATE`), compared case-insensitively *(assumed here)*.
2. Own folder (1D 2C-pass item 8 reading: the own folder stays, for loose files saved by the application — email attachments 02:399, Telegram files — not for dropped files, which are linked where they are, ADR-029): path `OWN_FOLDER_ROOT/<name>`, with characters `/ \ : * ? " < > |` replaced by `-`; if the folder exists and is not empty, append " (2)", " (3)" … ; create it; `rules.is_protected_location` must be false; `files.add_place(kind = workspace_folder)`. Failure → `WS_OWN_FOLDER_FAILED` (retryable) and no workspace row is written.
3. Insert the workspace (`state = active`) and its own-folder membership in `workspace_places` in one transaction. Journal `workspace_created {workspace_id}`.
4. `rename`: validate as step 2.1 excluding itself. The own folder is not renamed on disk (*assumed here*: the person may have opened it elsewhere). Journal `workspace_renamed`.
5. `archive` (active → archived), `restore` (archived → active): any other source state → `WS_STATE_INVALID`. Grants stay published while archived (*assumed here*: archived work can still be read by its agents when the person runs something there after restoring; no run starts while archived because `ensure_active` refuses).
6. `move_to_bin` (active or archived → in_bin): set `moved_to_bin_at`; `files.publish_grants(id, [])` so no agent reaches its places; unwatch places used by no other non-bin workspace; stop its timers. Journal `workspace_moved_to_bin`. Undo is `restore_from_bin`.
7. `restore_from_bin` (in_bin → active): name must still be unique among non-bin workspaces (else `WS_NAME_DUPLICATE`; the person renames first); clear `moved_to_bin_at`; republish grants; watch places; run the origin check.
8. `delete_from_bin(id)` (state must be `in_bin`): for each remote link, the adapter's `untrack_link(link.id)` and remove its working-copy place (`files.remove_place`) and delete `WORKING_COPY_ROOT/<link>` (ordin's own copy, not the person's documents; its unsent changes were named in `bin_impact` before the person confirmed); for each Confluence link `forget_link`; for each place membership, delete it and call `files.remove_place` when no other workspace holds the place (the folder on disk is untouched, 04:57); delete the workspace's grants, links, items, email links and email grants, then the workspace row, in one transaction after the external calls; then `journal.forget_workspace(id)` (its reports and dashboards; a failure is logged `workspace_forget_failed {workspace_id}` and retried at the next start for workspace ids journalled `workspace_deleted` without a `workspace_forgotten` entry). Journal `workspace_deleted`, and `workspace_forgotten` once `forget_workspace` succeeds; send the id on `deleted()`. `empty_bin` repeats this for every workspace in the Bin and returns the count.
9. `bin_impact`: count working-copy items in `changed`, `awaiting_push` or `origin_changed`, plus `files.held_edits` over its places; up to 10 display names.
10. `ensure_active`: not found → `WS_NOT_FOUND`; state not `active` → `WS_NOT_ACTIVE { state }`.
11. `Workspace.last_seen_at` is always returned `None` by this service (B7 contract D-9; Home and Search fills it).

### 3. Adding a link

1. `propose_link(id, location, display_name)`: workspace must be active. By kind:
   - `LocalFolder` / `GitRepositoryFolder`: with `on_server = true` → `WS_LOCAL_FOLDER_ON_SERVER` (the server's file system is not the person's computer; ServiceApi.md:564). Otherwise canonicalise the path; `rules.is_protected_location(&path)` true → `WS_PLACE_REFUSED` before any place is added; `GitRepositoryFolder` requires `<path>/.git` to exist (folder or file), else `WS_NOT_A_REPOSITORY`; `files.add_place(kind = persons_folder)` (an existing place is reused); a `FileError` for a protected or unreadable folder → `WS_PLACE_REFUSED { cause }` (words: "This folder belongs to the system and can't be added.", 11 §5.6); `origin_location` = the canonical path; `place_id` = the place.
   - `Sharepoint`: `origin_location = SharePointAdapter::origin_location(loc)`; `place_id` is set at confirmation (step 3.2).
   - `Confluence`: as SharePoint with the Confluence form; with `confluence = None` (personal server) → `WS_CONFLUENCE_UNAVAILABLE`.
   - `GitRemote` (SA-CLOSE-03; allowed on the server too): `url` must parse as `https://<host>/<owner>/<name>(.git)?` or `git@<host>:<owner>/<name>(.git)?`, `branch` 1–255 characters without control characters, `path` relative without `..` and ≤ 1,024 bytes, else `WS_GIT_REMOTE_INVALID`; `origin_location` = `git:<url with .git removed>#<branch>:<path>`; `place_id` stays `None`; `display_name` defaults to `<name>/<path>` or `<name>`.
   `display_name` defaults to the folder name, the location's path tail or the page title, 1–120 chars. Insert `state = proposed`; a duplicate `(workspace_id, kind, origin_location)` → `SRC_DUPLICATE`. Journal `source_link_proposed`.
2. `confirm_link(link)`: state must be `proposed` (else `WS_LINK_STATE_INVALID`). Local and git: insert `workspace_places` for the place (if absent), `files.watch([place])`. Remote: create `WORKING_COPY_ROOT/<link id>/`, `files.add_place(kind = working_copy, friendly_name = display_name)`, set `place_id`, insert `workspace_places`, `track_link`, then spawn the background read (step 4). Set `state = active`. Journal `source_link_added {kind}` (the Project Service opens a git repository folder as a project when it sees this entry's kind — Interaction).
   A `GitRemote` link at confirmation: `state = active` only (no place, `files.watch`, `track_link` or read). It is never checked by step 7, never granted (step 8.1 finds no place) and `remove_link` deletes only its row.
3. `add_link` = `propose_link` then `confirm_link`.
4. `decline_link`: `proposed` only; delete the row (and, for local folders, nothing else: the place row is kept for other uses or removed by `remove_place` when no workspace holds it).
5. Adapter errors map, on `code()`, to: `*_SIGN_IN_NEEDED` → `WS_SOURCE_SIGN_IN_NEEDED` ("Sign in to SharePoint so Ordin can read the supplier folder.", 11 §5.13); `*_NOT_CONFIGURED` → `WS_SOURCE_NOT_CONFIGURED`; `*_ACCESS_DENIED` → `WS_SOURCE_ACCESS_DENIED` (the conversation offers "Ask the owner for access", 03:442 error posture); `*_NOT_FOUND` → `WS_SOURCE_NOT_FOUND`; `*_UNAVAILABLE`, `*_THROTTLED` → `WS_SOURCE_UNAVAILABLE` (retryable). On a failure during `confirm_link` the link stays `proposed` and nothing is half-added (the working-copy place is removed again).

### 4. Reading a remote link into the working copy (ADR-029; 03:442)

1. Walk the location with `list_children` breadth-first, stopping at `LINK_MAX_ITEMS` and 20 levels; progress `listed` counts files.
2. For each file in walk order, while the link's read bytes stay within `LINK_READ_BUDGET`: `fetch(item)` (`FETCH_CONCURRENCY` at a time) → `files.store_fetched(place, path, bytes, version.changed_by, version.changed_at)` → upsert `working_copy_items { origin_path, origin_version_read: version.tag, local_version: 1, state: read, read_at: now }` (insert only when absent; an existing item follows step 6). A file above the adapter's size limit, or beyond the budget, counts `not_read` and is kept in `working_copy_unread(link, origin_path, size)`; `read_item` reads one on first use.
3. Per-item failures are logged `link_item_read_failed {source_link_id, code}` and the item is retried at the next origin check; the walk continues.
4. When done: `last_read_at = now`; journal `source_link_read {source_link_id, read, not_read}`. The walk runs once per link at confirmation and again after a 410/resync reported by the adapter; later changes arrive through step 7.

### 5. Removing a link

1. `remove_link` (the confirmation is the screens', 11 §5.6): state `active` (proposed → use `decline_link`).
2. Remove the grants on its place in this workspace and republish the workspace's grants.
3. Local and git: delete the `workspace_places` row; when no other workspace holds the place, `files.unwatch` and `files.remove_place`. The folder is untouched.
4. Remote: `untrack_link`, `files.remove_place(working-copy place)`, delete `WORKING_COPY_ROOT/<link>`, delete its items. Unsent changes in that working copy are lost with it; the screens' confirmation names them using `bin_impact`-style counts per link (*assumed here*: the HLD's removal confirmation for links says "Your documents stay where they are", which holds for the origin).
5. Delete the link row. Journal `source_link_removed {kind}`.
6. After the transaction, send `(workspace_id, link_id)` on `links_removed()` (amended January 2026, ADR-103); a send with no receiver is not an error.

### 6. Working-copy item states

1. `set_working_copy_state(id, state, origin_version)` validates the 2C transition table plus the two additions of step 6.3; else `WS_WORKING_COPY_TRANSITION_INVALID`. `origin_version` is required for `pushed` (the new origin version, stored in `origin_version_read`) and ignored otherwise. Callers: the Push Service (`changed → awaiting_push`, `awaiting_push → pushed`, `awaiting_push → origin_changed`, and back to `changed` when a push is rejected).
2. File Service changes on a working-copy place (`PlaceChange` Modified or Created for a path that has an item): `read → changed` whether the change was an agent's (`by_ordin = true`) or the person's held edit (ADR-048: "A saved edit is held … like a change by Ordin"); `local_version += 1`; send the item id on `subscribe_working_copy_changes` (the Push Service holds a waiting push on it, ADR-035). Removed → the item stays and the Push Service treats it as a local deletion it does not send (*assumed here*: deleting a remote document is outside ADR-029's pushes).
3. Transitions HLD 04:85 adds in revision 0.49.0, carried by 2C's table as amended (SA-CLOSE-01): `rejected` push → the Push Service sets `awaiting_push → changed` (changes kept locally, 04 PendingPush *rejected*; PushService.md does the same); a local change to a `pushed` item first applies `pushed → read` (the working copy equals the origin after a push) and then `read → changed`.
4. Origin changes (step 7): every `OriginChange` stores `origin_changed_by` and `origin_changed_at` on the item and is sent on `subscribe_origin_changes` as `OriginChanged`. Item `read` → re-fetch, `store_fetched`, new `origin_version_read`, stays `read`; `pushed` → re-fetch, `pushed → read`; `awaiting_push` → `origin_changed` (the Push Service stops the push, ADR-029); `changed` → unchanged (the Push Service checks the origin version before it proposes a push, 03:523); `origin_changed` → unchanged; removed at the origin → the item is kept with state unchanged and the origin change is reported to Home through the adapter.
5. `working_copy_entry(id)`: unknown id → `WS_WORKING_COPY_ITEM_NOT_FOUND`; join the item to its link (`kind`, `origin_location`, `display_name`, `place_id`, `workspace_id`); `relative_path` is the item's `origin_path` relative to the link root (the path `store_fetched` wrote, or the path in the local or git folder); for a git link, `git_branch` from the item row and `repository` from step 10.2.
6. `origin_version(workspace_id, link_kind, origin_path)`: `link_kind` other than `sharepoint_location` or `confluence_location` → `Invalid("SRC_KIND_MISMATCH")`; find the item of an active link of that kind in the workspace; return `OriginVersion { version: origin_version_read, changed_by: origin_changed_by, changed_at: origin_changed_at }`, or `None`. No adapter call is made: the value is at most `CHECK_INTERVAL` old while ordin runs (step 7).
7. Local and git folder items (ruling R12; the folder is its own working copy, nothing is fetched). A `PlaceChange` Created or Modified with `by_ordin = true` on the place of an active `local_folder` or `git_repository_folder` link, for a path with no item, inserts an item: `origin_path` = the path; `state = changed`, `local_version = 2`, `read_at = now`; `origin_version_read` = for a local folder `fver:<n>` where `n` is the version before the change (`PlaceChange.version − 1`, or `fver:0` when the file is new), for a git folder the commit of `refs/remotes/origin/<branch>` read as text from `<path>/.git` (`refs/remotes/origin/<branch>`, else the line of `packed-refs` naming it), with `git_branch` = the branch named by `.git/HEAD` (*chosen here*: no git library in this crate; a detached `HEAD` or an unreadable ref records no item and logs `working_copy_git_unreadable {source_link_id}`). The id is sent on `subscribe_working_copy_changes`. A change with `by_ordin = false` records nothing (the person's own edit of their own folder is not Ordin's change to send back); a later change to a path with an item follows step 6.2. These items take no part in origin checks (step 7): the Push Service checks the remote branch itself (git) or needs no check (local folder).

### 7. Origin checks (A-25; B7 contract D-8)

1. Triggers: `open(id)` (that workspace's links, awaited up to 10 s, then continued in the background; `open` also sends the id on `opened()`), start (every active workspace), and every `CHECK_INTERVAL` (every active workspace). A link already being checked is skipped.
2. For each active remote link: `check_changes(link)`; apply each `OriginChange` to its item (step 6.4); count it for `OpenReport.origin_changes`. A failing link is added to `checks_failed`, logged `origin_check_failed {source_link_id, code}`, and retried at the next trigger; a sign-in error stops further checks of that adapter until the next trigger.
3. For each active Confluence link of the workspace (folder publishing), call `confluence.sync_linked_folder(link, place)`; its report is journalled by the adapter.
4. Journal `origin_check_done {workspace_id, changes, failed}` per workspace per trigger.

### 8. Grants, Confluence links, approvals and linked-folder changes

1. `set_grant`: workspace not in the Bin; the place must be a member (`GRANT_PLACE_NOT_IN_WORKSPACE`); `access = None` deletes the grant; `Some(a)` inserts or updates (the unique key makes a repeat an update, never `GRANT_DUPLICATE` towards the caller); `kind = agent_folder` or `coding_task_worktree` places cannot be granted here (`WS_PLACE_REFUSED { cause: "hidden_place" }`; a worktree is granted to its session by the caller that starts the coding task, the Session Supervisor or the Crew Orchestrator, as the Project Service states — reconciliation row 662). Then `files.publish_grants(id, grants(id))`; failure → `WS_GRANT_PUBLISH_FAILED` (retryable) and the stored grant is rolled back, so stored and enforced grants never differ. Journal `grant_set {workspace_id, place_id, grantee_kind, access}`.
2. `propose_confluence_link`: workspace active; place a member; `folder` relative to the place (no `..`); resolve `audience` from `confluence.audience(location)` (`line`); insert `ConfluenceLink { state: proposed }`; `confluence.request_link_approval(link, place, workspace)` → store `approval_id`. On an approval error the row is deleted and the error returned (`WS_CONFLUENCE_UNAVAILABLE`).
3. Approval decisions (`EventSink::deliver`): an event whose `EventKind::parse(&event.kind)` is `Some(EventKind::ApprovalDecided)` has `event.data` decoded as `{ approval_id, state, decided_on }` (a body that does not decode is logged `approval_event_invalid {seq}` and skipped); other kinds are ignored. `lagged(next_seq)` re-reads with `journal.events_since(next_seq - 1, 1000)` until it reaches `Journal::last_seq` and applies each event as `deliver` does. On a decision matching a proposed link's `approval_id`: `approved` → `state = active`, `approved_at = now` (`CFL_NOT_APPROVED` guards the fields), then `sync_linked_folder`; `rejected` or `expired` → delete the proposed row (*assumed here*: 2C's table has no rejected state; **Not now** leaves nothing linked). Journal `confluence_link_decided {confluence_link_id, state}`.
4. Linked-folder changes: a `PlaceChange` under an active Confluence link's folder schedules `sync_linked_folder` after `LINKED_FOLDER_DEBOUNCE`; at most one sync per link runs at a time.
5. `pause` (active → paused), `resume` (paused → active, then sync), `remove` (active or paused → row deleted; `confluence.forget_link`; no Confluence page is deleted, 04:120).
6. `revoke_grantee(grantee)`: delete its folder grants and, for an agent instance, its email, Telegram and Slack grants in every workspace; republish each affected workspace's grants.

### 9. Email links and grants (ADR-070)

1. `propose_email_link`: validate the 2C rules; addresses lower-cased and deduplicated; insert `state = proposed`; `confirm_email_link` → `active`; `remove_email_link` deletes (the confirmation is the screens'). Journal `email_link_added` / `email_link_removed` with the kind only (no addresses in the journal entry).
2. Check state (`last_checked_at`, `new_since_last_looked`, `imap_folders`) is the Mail Service's, kept in its own table (2C C-26); this service neither stores nor writes it and returns the empty values on every `EmailLink` it reads.
3. `set_email_grant`: `None` deletes; `Some(access)` upserts. Journal `email_grant_set {workspace_id, access}`. `email_grant(id, agent)` reads the one row by its unique key.
4. `email_link(link)`: the row or `None`. `active_workspaces_with_email()`: distinct `workspace_id` of `email_links` with `state = 'active'` joined to workspaces with `state = 'active'`.

### 10. Telegram and Slack grants, and linked repositories

1. `set_telegram_grant(id, agent, access)`: workspace not in the Bin (`WS_NOT_ACTIVE`); `None` deletes; `Some(access)` upserts on `(workspace_id, agent_instance_id)`. Journal `telegram_grant_set {workspace_id, access}`. `telegram_grant` reads one row by that key; `telegram_grants` lists the workspace's rows.
1a. `set_slack_grant(id, agent, access)`: workspace not in the Bin (`WS_NOT_ACTIVE`); unknown workspace (`WS_NOT_FOUND`); `None` deletes; `Some(access)` upserts on `(workspace_id, agent_instance_id)`, keeping `id` and setting `granted_at = now` on each change. Journal `slack_grant_set {workspace_id, access}` (`access` is `null` for a removal; no agent name). `slack_grant` reads one row by that key; `slack_grants` lists the workspace's rows ordered by `granted_at`.
2. `linked_repositories(id)`: for each active `git_repository_folder` link, read `<place path>/.git/config` as text (a `.git` file pointing to a git directory is followed once) and take `url` of `[remote "origin"]`. Parse `https://<host>/<owner>/<name>(.git)?` or `git@<host>:<owner>/<name>(.git)?`; host `github.com` → `ShareHost::Github`; host `gitlab.com` or a host name starting with `gitlab.` → `ShareHost::Gitlab` *(assumed here)*; any other host, no origin remote, or an unreadable file → the link is left out and logged `repository_remote_unknown {source_link_id}`.
3. `summary(id)`: the workspace row, the friendly names of its places and the display names of its active links (own folder first, then links by `added_at`), and `linked_repositories(id)`.

### 10a. Saved chat files and origin changes since a time (SA-CLOSE-02)

1. `save_to_inbox(ws, name, bytes, origin)`: workspace not in the Bin (`WS_NOT_ACTIVE`); `name` trimmed, 1–255 characters, no `/`, `\`, `:`, control characters, and not `.` or `..` (`WS_FILE_NAME_INVALID`); `bytes.len()` ≤ `SAVED_FILE_MAX_BYTES` (`WS_FILE_TOO_LARGE`). Path `Inbox/<name>`; when it exists, `Inbox/<stem> (2).<ext>`, `(3)` … up to `(99)`, then `WS_FILE_NAME_INVALID`. `files.write_version(WriteVersion { place_id: own_folder_place_id, path, bytes, origin: FileVersionOrigin::Person, expected_hash: None })` (a chat file is the person's); a `FileError` → `WS_FILES_FAILED` (retryable as the File Service says). Return `PlaceFileRef { place_id, relative_path: path, display_path: "<workspace name> › Inbox › <saved name>" }`. Journal `inbox_file_saved {workspace_id, channel}` (no file name); `workspace_changed` (step 11).
2. `save_generated(ws, name, bytes, by)`: as step 1 with folder `Results/` and `origin: FileVersionOrigin::Run { run_id: by, tool: "channel_gateway" }`; display path "<workspace name> › Results › <saved name>". Journal `generated_file_saved {workspace_id, run_id}`.
3. `origin_changes_since(since)`: `working_copy_items` with `origin_changed_at > since`, joined to active links of non-bin workspaces, ordered by `origin_changed_at` descending, at most 1,000; each row becomes `OriginChanged { workspace_id, source_link_id, link_kind, origin_path, version: OriginVersion { version: origin_version_read, changed_by: origin_changed_by, changed_at: origin_changed_at }, removed: false }`. A datastore error → `WS_DATASTORE_UNAVAILABLE`.

### 11. `workspace_changed` events

1. After each committed change the tree shows — `create`, `rename`, `archive`, `restore`, `move_to_bin`, `restore_from_bin`, `delete_from_bin`, `confirm_link`, `add_link`, `remove_link`, `set_grant`, `revoke_grantee`, a Confluence link's proposal, decision, pause, resume or removal, an email link's confirmation or removal, `set_email_grant`, `set_telegram_grant`, `set_slack_grant`, and the end of a background link read — append `journal.append_event(EventKind::WorkspaceChanged.as_str(), json!({"workspace_id": id}))` once per affected workspace. A failed append is logged `workspace_event_failed {workspace_id}` and does not undo the change (the screens reload on their next open).

## Data Model

SQLite (desktop, personal server), in the Journal's `Datastore` (D4). This module does not run in the agent profile, so `migrations()` has an empty `postgres` slice; the SQL below is migration step 1 of module `workspaces`.

```sql
CREATE TABLE workspaces (
    id                   TEXT PRIMARY KEY,
    name                 TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    name_key             TEXT NOT NULL,                  -- lower(trim(name)) for the uniqueness rule
    own_folder_place_id  TEXT NOT NULL,
    state                TEXT NOT NULL CHECK (state IN ('active','archived','in_bin')),
    moved_to_bin_at      TEXT,
    created_at           TEXT NOT NULL,
    CHECK ((state = 'in_bin') = (moved_to_bin_at IS NOT NULL))
);
CREATE UNIQUE INDEX workspaces_name_not_in_bin ON workspaces (name_key) WHERE state != 'in_bin';

CREATE TABLE workspace_places (
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    place_id      TEXT NOT NULL,
    added_at      TEXT NOT NULL,
    PRIMARY KEY (workspace_id, place_id)
);
CREATE INDEX workspace_places_place ON workspace_places (place_id);

CREATE TABLE folder_grants (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    grantee       TEXT NOT NULL,                         -- JSON Grantee
    grantee_key   TEXT NOT NULL,                         -- 'agent:<id>' or 'crew:<id>'
    place_id      TEXT NOT NULL,
    access        TEXT NOT NULL CHECK (access IN ('read','read_and_change')),
    granted_at    TEXT NOT NULL,
    UNIQUE (workspace_id, grantee_key, place_id),
    FOREIGN KEY (workspace_id, place_id) REFERENCES workspace_places(workspace_id, place_id) ON DELETE CASCADE
);
CREATE INDEX folder_grants_grantee ON folder_grants (grantee_key);

CREATE TABLE source_links (
    id               TEXT PRIMARY KEY,
    workspace_id     TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind IN ('local_folder','git_repository_folder','sharepoint_location','confluence_location','git_remote')),
    origin_location  TEXT NOT NULL CHECK (length(origin_location) BETWEEN 1 AND 2048),
    display_name     TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    place_id         TEXT,
    state            TEXT NOT NULL CHECK (state IN ('proposed','active')),
    added_at         TEXT NOT NULL,
    last_read_at     TEXT,
    UNIQUE (workspace_id, kind, origin_location)
);

CREATE TABLE working_copy_items (
    id                   TEXT PRIMARY KEY,
    source_link_id       TEXT NOT NULL REFERENCES source_links(id) ON DELETE CASCADE,
    origin_path          TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    origin_version_read  TEXT NOT NULL CHECK (length(origin_version_read) BETWEEN 1 AND 256),
    local_version        INTEGER NOT NULL CHECK (local_version >= 1),
    state                TEXT NOT NULL CHECK (state IN ('read','changed','awaiting_push','pushed','origin_changed')),
    read_at              TEXT NOT NULL,
    origin_changed_by    TEXT,                            -- from the adapter's OriginChange
    git_branch           TEXT,                            -- git folder links only (Internal Logic 6.7)
    origin_changed_at    TEXT,
    UNIQUE (source_link_id, origin_path)
);
CREATE INDEX working_copy_items_origin_changed ON working_copy_items (origin_changed_at);

CREATE TABLE working_copy_unread (
    source_link_id  TEXT NOT NULL REFERENCES source_links(id) ON DELETE CASCADE,
    origin_path     TEXT NOT NULL,
    item_ref        TEXT NOT NULL,                        -- JSON SharePointItemRef or RemoteItemRef of the adapter
    size_bytes      INTEGER NOT NULL,
    PRIMARY KEY (source_link_id, origin_path)
);

CREATE TABLE confluence_links (
    id                TEXT PRIMARY KEY,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    place_id          TEXT NOT NULL,
    folder            TEXT NOT NULL,
    confluence_space  TEXT NOT NULL CHECK (length(confluence_space) BETWEEN 1 AND 255),
    parent_page_id    TEXT NOT NULL CHECK (length(parent_page_id) BETWEEN 1 AND 64 AND parent_page_id GLOB '[0-9]*'),
    audience          TEXT NOT NULL CHECK (length(audience) BETWEEN 1 AND 500),
    approval_id       TEXT,
    approved_at       TEXT,
    state             TEXT NOT NULL CHECK (state IN ('proposed','active','paused')),
    CHECK (state = 'proposed' OR (approval_id IS NOT NULL AND approved_at IS NOT NULL))
);
CREATE INDEX confluence_links_approval ON confluence_links (approval_id);

CREATE TABLE email_links (
    id                     TEXT PRIMARY KEY,
    workspace_id           TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind                   TEXT NOT NULL CHECK (kind IN ('folder_or_label','people_and_domains')),
    folder_or_label        TEXT,
    addresses              TEXT NOT NULL,                -- JSON array
    domains                TEXT NOT NULL,                -- JSON array
    display_name           TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    state                  TEXT NOT NULL CHECK (state IN ('proposed','active')),
    added_at               TEXT NOT NULL
    -- no check columns: the Mail Service keeps them in mail_link_check_state (2C C-26)
);

CREATE TABLE email_grants (
    id                 TEXT PRIMARY KEY,
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_instance_id  TEXT NOT NULL,
    access             TEXT NOT NULL CHECK (access IN ('read_email','read_and_draft_email')),
    granted_at         TEXT NOT NULL,
    UNIQUE (workspace_id, agent_instance_id)
);

CREATE TABLE telegram_grants (
    id                 TEXT PRIMARY KEY,
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_instance_id  TEXT NOT NULL,
    access             TEXT NOT NULL CHECK (access IN ('read_telegram','read_and_draft_telegram')),
    granted_at         TEXT NOT NULL,
    UNIQUE (workspace_id, agent_instance_id)
);
-- step (2, "slack grants", …)
CREATE TABLE slack_grants (
    id                 TEXT PRIMARY KEY,
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_instance_id  TEXT NOT NULL,
    access             TEXT NOT NULL CHECK (access IN ('read_slack','read_and_draft_slack')),
    granted_at         TEXT NOT NULL,
    UNIQUE (workspace_id, agent_instance_id)
);
```

Rollback: the migration only creates tables (SA-XC-07 backup); step 2 (`slack_grants`, amended January 2026, ADR-102) only creates a table that no other row refers to. Deleting a workspace deletes only rows of this module and ordin's own working-copy folders under the application data root; no statement or step deletes a folder or file of the person's (04:57, 11 §5.13 "Removing a workspace never deletes documents").

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Saved file name empty, too long, with a path character, or no free numbered name | `Err(permanent)` | `WS_FILE_NAME_INVALID` | `{"error":"ordin couldn't save a file with that name.","code":"WS_FILE_NAME_INVALID","action":null}` |
| Saved file over 100 MiB | `Err(permanent)` | `WS_FILE_TOO_LARGE` | `{"error":"That file is too large to keep in the workspace.","code":"WS_FILE_TOO_LARGE","action":null}` |
| Git repository address, branch or path malformed | `Err(permanent)` | `WS_GIT_REMOTE_INVALID` | `{"error":"That repository address isn't one ordin can link.","code":"WS_GIT_REMOTE_INVALID","action":null}` |
| Workspace id unknown | `Err(permanent)` | `WS_NOT_FOUND` | `{"error":"That workspace is no longer in ordin.","code":"WS_NOT_FOUND","action":null}` |
| Work started in an archived or binned workspace | `Err(permanent)` | `WS_NOT_ACTIVE` | `{"error":"Restore this workspace before starting work in it.","code":"WS_NOT_ACTIVE","action":null}` |
| Name empty / longer than 80 / already used | `Err(permanent)` | `WS_NAME_EMPTY`, `WS_NAME_TOO_LONG`, `WS_NAME_DUPLICATE` | `{"error":"Another workspace already has that name.","code":"WS_NAME_DUPLICATE","action":null}` (one sentence per code) |
| Lifecycle change from a state that does not allow it | `Err(permanent)` | `WS_STATE_INVALID` | `{"error":"That can't be done to this workspace right now.","code":"WS_STATE_INVALID","action":null}` |
| Own folder cannot be created | `Err(retryable)` | `WS_OWN_FOLDER_FAILED` | `{"error":"ordin couldn't create the workspace's folder in Documents.","code":"WS_OWN_FOLDER_FAILED","action":"try_again"}` |
| Link id unknown | `Err(permanent)` | `WS_LINK_NOT_FOUND` | `{"error":"That location is no longer in this workspace.","code":"WS_LINK_NOT_FOUND","action":null}` |
| Link not in the state the call needs | `Err(permanent)` | `WS_LINK_STATE_INVALID` | — (internal; screens offer only valid actions) |
| Chosen folder is not a git repository | `Err(permanent)` | `WS_NOT_A_REPOSITORY` | `{"error":"This folder isn't a code repository.","code":"WS_NOT_A_REPOSITORY","action":null}` |
| Folder protected, unreadable, or a hidden place | `Err(permanent)` | `WS_PLACE_REFUSED` | `{"error":"This folder belongs to the system and can't be added.","code":"WS_PLACE_REFUSED","action":null}` |
| Location already linked | `Err(permanent)` | `SRC_DUPLICATE` | `{"error":"That location is already in this workspace.","code":"SRC_DUPLICATE","action":null}` |
| Location or name malformed | `Err(permanent)` | `SRC_LOCATION_INVALID`, `SRC_NAME_INVALID`, `SRC_PLACE_MISMATCH` | — (internal) |
| Origin needs a sign-in | `Err(permanent)` until sign-in | `WS_SOURCE_SIGN_IN_NEEDED` | `{"error":"Sign in to SharePoint so Ordin can read this location.","code":"WS_SOURCE_SIGN_IN_NEEDED","action":"sign_in_again"}` (Confluence: "Sign in to Confluence to see the pages linked to this workspace.") |
| Origin not set up by the company | `Err(permanent)` | `WS_SOURCE_NOT_CONFIGURED` | `{"error":"This place isn't set up for your company in ordin.","code":"WS_SOURCE_NOT_CONFIGURED","action":"send_to_it"}` |
| No access at the origin | `Err(permanent)` | `WS_SOURCE_ACCESS_DENIED` | `{"error":"You don't have access to this location.","code":"WS_SOURCE_ACCESS_DENIED","action":null}` |
| Origin location gone | `Err(permanent)` | `WS_SOURCE_NOT_FOUND` | `{"error":"This location no longer exists where it lived.","code":"WS_SOURCE_NOT_FOUND","action":null}` |
| Origin unreachable or throttled | `Err(retryable)` | `WS_SOURCE_UNAVAILABLE` | `{"error":"Can't reach this location right now.","code":"WS_SOURCE_UNAVAILABLE","action":"try_again"}` |
| Confluence asked for on the personal server, or its approval could not be requested | `Err(permanent)` / `Err(retryable)` | `WS_CONFLUENCE_UNAVAILABLE` | `{"error":"Confluence isn't available here.","code":"WS_CONFLUENCE_UNAVAILABLE","action":null}` |
| `read_item` of a file above the adapter's limit | `Err(permanent)` | `WS_ITEM_TOO_LARGE` | `{"error":"This document is too large for ordin to read.","code":"WS_ITEM_TOO_LARGE","action":null}` |
| Working-copy state change not allowed | `Err(permanent)` (caller defect) | `WS_WORKING_COPY_TRANSITION_INVALID` | — (internal) |
| Grant on a place not in the workspace | `Err(permanent)` | `GRANT_PLACE_NOT_IN_WORKSPACE` | — (internal) |
| Grants could not be handed to enforcement | `Err(retryable)` | `WS_GRANT_PUBLISH_FAILED` | `{"error":"ordin couldn't update who can use this folder.","code":"WS_GRANT_PUBLISH_FAILED","action":"try_again"}` |
| Confluence link fields invalid | `Err(permanent)` | `CFL_SPACE_INVALID`, `CFL_PARENT_INVALID`, `CFL_AUDIENCE_INVALID`, `CFL_NOT_APPROVED` | — (internal) |
| Email link or grant invalid | `Err(permanent)` | `MAIL_LINK_FIELDS_MISMATCH`, `MAIL_ADDRESS_INVALID`, `MAIL_DOMAIN_INVALID`, `MAIL_IMAP_STATE_INVALID`, `MAIL_GRANT_DUPLICATE` | `{"error":"Check the addresses and try again.","code":"MAIL_ADDRESS_INVALID","action":null}` |
| File Service call failed | `Err(retryable)` | `WS_FILES_FAILED` | `{"error":"ordin couldn't update this workspace's documents.","code":"WS_FILES_FAILED","action":"try_again"}` |
| Local or git folder linked on the ordin server | `Err(permanent)` | `WS_LOCAL_FOLDER_ON_SERVER` | `{"error":"Folders on your computer can't be added to a workspace on the server.","code":"WS_LOCAL_FOLDER_ON_SERVER","action":null}` |
| Working-copy item id unknown | `Err(permanent)` | `WS_WORKING_COPY_ITEM_NOT_FOUND` | — (internal; caller defect) |
| `origin_version` asked for a local or git link kind | `Err(permanent)` | `SRC_KIND_MISMATCH` | — (internal; caller defect) |
| Own tables unreachable | `Err(retryable)` | `WS_DATASTORE_UNAVAILABLE` | `{"error":"ordin couldn't reach its own records.","code":"WS_DATASTORE_UNAVAILABLE","action":"try_again"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Workspace Service

  Scenario: Happy path — create a workspace with its own folder
    When create("Q3 supplier review") is called
    Then a workspace in state active is returned
    And the folder "~/Documents/ordin/Q3 supplier review" exists and is a workspace_folder place in it

  Scenario: Error path — duplicate name among workspaces not in the Bin
    Given a workspace "Q3 supplier review" exists
    When create("q3 supplier review") is called
    Then it fails with WS_NAME_DUPLICATE

  Scenario: Edge case — a binned workspace's name can be reused
    Given "Q3 supplier review" is in the Bin
    When create("Q3 supplier review") is called
    Then it succeeds
    And restore_from_bin of the binned one fails with WS_NAME_DUPLICATE

  Scenario: Happy path — add a SharePoint location and read it
    Given the SharePoint double lists 212 files under "Suppliers/Contracts"
    When add_link is called with that location
    Then the link is active with a working_copy place
    And after the background read link_progress shows read 212 and done true
    And 212 working-copy items are in state read with the cTag read as origin_version_read

  Scenario: Error path — SharePoint needs a sign-in
    Given the SharePoint Adapter returns SP_SIGN_IN_NEEDED
    When add_link is called with a SharePoint location
    Then it fails with WS_SOURCE_SIGN_IN_NEEDED and no link or working-copy place remains

  Scenario: Error path — a protected folder is refused
    When add_link is called with LocalFolder "/System"
    Then it fails with WS_PLACE_REFUSED

  Scenario: Error path — a folder without .git is not a repository
    When add_link is called with GitRepositoryFolder "/Users/asha/notes"
    Then it fails with WS_NOT_A_REPOSITORY

  Scenario: Happy path — a grant in one workspace gives nothing in another
    Given place P belongs to workspaces A and B
    When set_grant(A, analyst, P, Some(read)) is called
    Then the File Service receives grants for A containing the grant
    And grants(B) is empty

  Scenario: Edge case — "can't use" removes a grant
    Given analyst can read P in A
    When set_grant(A, analyst, P, None) is called
    Then grants(A) no longer holds it and the File Service receives A's grants without it

  Scenario: Error path — grant hand-off failure leaves the stored grant unchanged
    Given the File Service fails publish_grants
    When set_grant(A, analyst, P, Some(read_and_change)) is called
    Then it fails with WS_GRANT_PUBLISH_FAILED and grants(A) is as before

  Scenario: Happy path — an origin change refreshes an unchanged working copy
    Given item "Kestrel MSA.docx" is read at cTag c4
    And check_changes reports it at cTag c5 changed by "Sam Lee"
    When the workspace is opened
    Then the item is re-read, stays read, and its origin_version_read is c5

  Scenario: Error path — an origin change during a pending push holds it
    Given item "Kestrel MSA.docx" is awaiting_push
    When check_changes reports a new cTag for it
    Then its state becomes origin_changed

  Scenario: Edge case — an agent's edit marks the item changed
    Given item "Pricing.pptx" is read
    When the File Service reports Modified by_ordin true for its path
    Then its state is changed and local_version is 2

  Scenario: Happy path — moving to the Bin withdraws agents' access and keeps documents
    Given workspace A has grants and a local folder link
    When move_to_bin(A) is called
    Then the File Service receives an empty grant list for A
    And the local folder still exists on disk

  Scenario: Happy path — deleting from the Bin removes ordin's records only
    Given workspace A is in the Bin with a SharePoint link
    When delete_from_bin(A) is called
    Then A's rows are gone, its working-copy folder under the application data root is deleted
    And the person's own folder "~/Documents/ordin/A" still exists

  Scenario: Happy path — a Confluence link becomes active on approval
    Given a proposed Confluence link with approval_id X
    When the event approval_decided with approval_id X and state approved arrives
    Then the link is active with approved_at set and sync_linked_folder is called

  Scenario: Edge case — Not now on the link card leaves nothing linked
    When the event approval_decided with approval_id X and state rejected arrives
    Then the proposed Confluence link row is deleted

  Scenario: Error path — work cannot start in an archived workspace
    Given workspace A is archived
    When ensure_active(A) is called
    Then it fails with WS_NOT_ACTIVE

  Scenario: Error path — a local folder cannot be linked on the server
    Given the service runs with on_server true
    When add_link is called with LocalFolder "/srv/data"
    Then it fails with WS_LOCAL_FOLDER_ON_SERVER and no place is added

  Scenario: Happy path — an origin change is broadcast and its version is readable
    Given item "Kestrel MSA.docx" of A's SharePoint link is read at cTag c4
    When check_changes reports it at cTag c5 changed by "Sam Lee"
    Then subscribe_origin_changes delivers OriginChanged for A with version c5 and changed_by "Sam Lee"
    And origin_version(A, sharepoint_location, "Kestrel MSA.docx") returns version c5

  Scenario: Happy path — an agent's edit to a working copy is announced to the Push Service's store
    Given item "Pricing.pptx" is read
    When the File Service reports Modified by_ordin true for its path
    Then subscribe_working_copy_changes delivers the item's id
    And working_copy_entry for it names the working-copy place and the path "Pricing.pptx"

  Scenario: Happy path — an agent's change in a git folder becomes a working-copy item
    Given A links the git folder "supplier-portal" on branch main whose refs/remotes/origin/main is c0ffee
    When the File Service reports Modified by_ordin true for "docs/rates.md"
    Then a working-copy item in state changed is recorded with origin_version_read c0ffee and git_branch main
    And subscribe_working_copy_changes delivers its id

  Scenario: Edge case — the person's own edit of a local folder records no item
    When the File Service reports Modified by_ordin false for a file in a local folder link
    Then no working-copy item is recorded

  Scenario: Happy path — a Telegram grant is recorded and then removed
    When set_telegram_grant(A, analyst, Some(read_telegram)) is called
    Then telegram_grant(A, analyst) returns access read_telegram
    And after set_telegram_grant(A, analyst, None) telegram_grant(A, analyst) returns None

  Scenario: Happy path — opening a workspace is announced
    When open(A) is called
    Then opened() delivers A

  Scenario: Happy path — a linked GitHub repository is summarised
    Given A links the git folder whose origin remote is "git@github.com:acme/pricing.git"
    When summary(A) is called
    Then repositories holds host github and repository "acme/pricing"

  Scenario: Edge case — a repository on another host is left out
    Given A links a git folder whose origin remote is on "git.example.net"
    When linked_repositories(A) is called
    Then the list is empty and repository_remote_unknown is logged

  Scenario: Happy path — a change the tree shows emits workspace_changed
    When rename(A, "Q3 suppliers") is called
    Then the Journal receives the event workspace_changed with workspace_id A

  Scenario: A chat file is saved in the workspace's Inbox
    Given workspace A "Q3 supplier review" already holds "Inbox/quote.pdf"
    When save_to_inbox(A, "quote.pdf", bytes, chat telegram) is called
    Then the File Service writes "Inbox/quote (2).pdf" in A's own folder with origin person
    And the result's display_path is "Q3 supplier review › Inbox › quote (2).pdf"

  Scenario: A file name with a path character is refused
    When save_generated(A, "../x.json", bytes, run R) is called
    Then it fails with WS_FILE_NAME_INVALID and nothing is written

  Scenario: Origin changes since a time
    Given a SharePoint item of A changed at its origin at 10:00 and another at 08:00
    When origin_changes_since(09:00) is called
    Then one OriginChanged is returned for the 10:00 item

  Scenario: A remote Git folder is linked without a clone
    When add_link(A, GitRemote { url: "https://github.com/acme/contracts", branch: "main", path: "handover" }) is called
    Then the link is active with kind git_remote and no place
    And no folder is created and no origin check runs for it
```

## Performance / Security / Observability

- **Performance targets.** `list`, `get`, `ensure_active`, `grants`, `source_links` ≤ 20 ms p95 at 200 workspaces (*chosen here*; within A-03's 150 ms); `create` ≤ 150 ms p95 (A-03); `open` returns within 150 ms p95 with checks continuing in the background, and its awaited origin checks are capped at 10 s; `set_grant` ≤ 100 ms p95 including the hand-off; a background read of 1,000 SharePoint files of 200 KiB median ≤ 5 minutes on the company network (*chosen here*, network-bound). Memory: at most `FETCH_CONCURRENCY` fetched items in memory per link.
- **Security.** Grants are scoped per workspace and handed to enforcement before the change is reported (stored and enforced never differ); a workspace in the Bin has no grants; protected locations are refused; dropped and picked paths come from the system picker or a drop event; Service API inputs are typed with unknown fields refused; email addresses and Confluence fields are validated as 2C states; approval decisions are matched by approval id the service stored, never by data from the event payload alone.
- **Never logged.** Workspace names, folder paths, display names, email addresses and domains, origin locations, file names. Logs carry ids and counts.
- **Observability (logs only; SA-XC-04/05).** `tracing` events with `component = "workspaces"`: `workspace_created|renamed|archived|restored|moved_to_bin|deleted {workspace_id}`, `source_link_proposed|added|removed {source_link_id, kind}`, `source_link_read {source_link_id, read, not_read, elapsed_ms}`, `link_item_read_failed {source_link_id, code}`, `origin_check_done {workspace_id, changes, failed}`, `origin_check_failed {source_link_id, code}`, `grant_set {workspace_id, place_id, grantee_kind, access}`, `grants_publish_failed {workspace_id}`, `confluence_link_decided {confluence_link_id, state}`, `workspace_forget_failed {workspace_id}`, `approval_event_invalid {seq}`, `journal_entry_failed {kind}`.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

Slack grants are recorded here exactly as Telegram grants are (2C `TelegramGrant`, C-25): this service records them; the Slack Service enforces them.

The sections above carry the change in place: `SlackGrantId`, `SlackGrant`, `SlackAccess` (Shared Context); `set_slack_grant`, `slack_grants`, `slack_grant` and the Service API rows `workspaces.set_slack_grant`, `workspaces.slack_grants` (Public Interface); the Slack Service among the callers of `get`, `slack_grant` and `slack_grants`; §8 step 6 (removing a grantee deletes its Slack grants), §10 step 1a (`set_slack_grant`, journal `slack_grant_set`) and §11 step 1 (`workspace_changed`); and migration step 2, `(2, "slack grants", MIGRATION_2_SQL)` in the SQLite slice (the `postgres` slice stays empty: no agent profile), which creates `slack_grants`. Deleting a workspace cascades to its Slack grants as to its Telegram grants. No new error code: `set_slack_grant` refuses with the existing `WS_NOT_FOUND` and `WS_NOT_ACTIVE` rows.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a Slack grant is recorded and then removed
    When set_slack_grant(A, analyst, Some(read_and_draft_slack)) is called
    Then slack_grant(A, analyst) returns access read_and_draft_slack
    And "slack_grant_set" is journalled with access read_and_draft_slack and workspace_changed is appended for A
    And after set_slack_grant(A, analyst, None) slack_grant(A, analyst) returns None

  Scenario: Edge case — a grant in one workspace gives nothing in another
    Given analyst has read_slack in workspace A
    Then slack_grant(B, analyst) returns None

  Scenario: Error path — a Slack grant in a binned workspace
    Given workspace A is in the Bin
    When set_slack_grant(A, analyst, Some(read_slack)) is called
    Then it fails with WS_NOT_ACTIVE and no row is written

  Scenario: Happy path — removing an agent removes its Slack grants
    Given analyst has Slack grants in workspaces A and B
    When revoke_grantee(analyst) is called
    Then slack_grants(A) and slack_grants(B) hold no row for analyst
```

### Gaps

None.

## Amendment January 2026 — Team knowledge (ADR-103)

> Source: decision 83 (the user's decisions D1–D10 of January 2026); HLD revision 0.51.0, ADR-103. Counterpart: `components/TeamKnowledgeService.md` (§A, §B and its `TeamWorkspaces` trait).

### Purpose (addition)

A workspace's `GitRemote` link may be the folder a workspace shares team knowledge through. This service still only records the link; the Team Knowledge Service owns the binding, its clone and every read and write of the folder.

### Approach (addition)

The Team Knowledge Service learns about link removal from a new broadcast, `links_removed()`, in the same shape as `opened()` and `deleted()`, so this service gains no edge to a later module and no knowledge of Team Knowledge mode. Rejected: *refusing `remove_link` while a workspace shares through the link* — the person removes links on the Add to this workspace screen and would meet a refusal with no way to act on it there; removing the link turns the mode off instead.

### Shared Context (additions)

N/A — no type is added; `SourceLinkId`, `WorkspaceId` and `LinkLocation::GitRemote` are those above.

### Public Interface (additions)

The sections above carry the change in place: `links_removed()` (Public Interface, *Workspace notices for later modules*) and the Team Knowledge Service among the callers of `get`, `source_links`, `add_link`, `opened`, `deleted` and `links_removed`. No Service API command is added.

**A second emitter of `workspace_changed`.** The Team Knowledge Service appends `workspace_changed { workspace_id }` through the Journal (`EventKind::WorkspaceChanged`, 2C wire) when a workspace's team knowledge status changes (turned on or off, something new to get, a get finished). The payload is the one of §11; the screens re-read `team_knowledge.status` on it as they re-read the tree. This service's own §11 rule is unchanged.

### Internal Logic (changes)

§5 step 6 (added in place): after a link row is deleted, send `(workspace_id, link_id)` on `links_removed()`.

### Data Model (additive migration)

N/A — no table or column changes.

### Error Table (rows added)

N/A — no new code; `remove_link` refuses exactly as above.

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: Happy path — removing a link is announced to later modules
    Given workspace A with an active GitRemote link L
    And a receiver subscribed to links_removed()
    When remove_link(L) is called
    Then the receiver gets (A, L) after the link row is gone
    And the Journal receives workspace_changed with workspace_id A
```

### Performance, Security, Observability (additions)

N/A — one broadcast send per removed link; nothing logged beyond `source_link_removed`.

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86 (the user's decisions of January 2026: Jira as a handover place and Jira issues as workspace links); HLD revision 0.54.0, ADR-106. Counterparts: `components/JiraAdapter.md` (every call to Jira), `components/HandoverService.md` (amendment January 2026 — Jira issues and handovers: linking the issue on acceptance).

### Purpose (addition)

A workspace can link one or more **Jira issues**. A linked issue becomes read-only working-copy items: its summary and description, each comment, each attachment, and one list of its sub-tasks and linked issues. Ordin and the agents read them like any other linked item. When the issue changes in Jira, the item is shown as changed at its origin within one check interval and nothing local is overwritten until the person chooses **Bring in the new version** (11 §5.13). Removing the link removes the local copy only. This service never writes to Jira.

### Approach (addition)

The Jira Adapter owns every call to Jira and keeps no link state, so this service maps one issue to its items itself, from one `JiraAdapter::issue` read, and compares versions on each origin check. Rejected: *giving the Jira Adapter the seven reading methods the SharePoint and Confluence Adapters have* — an issue is one record with parts, not a folder tree, and a walk would cost a request per part where one read returns them all; *re-reading a changed item at once, as for SharePoint and Confluence* — the person must see that the issue changed before the copy Ordin quotes moves (decision 86, "nothing local is overwritten").

### Shared Context (additions)

The sections above carry these in place, each marked: `SourceLinkKind::JiraIssue` (stored `jira_issue`), `LinkLocation::JiraIssue { address }`, `WorkingCopyItem.origin_newer_at`, `WorkspaceDeps.jira`.

Jira Adapter items used here (provider text in `components/JiraAdapter.md` is authoritative):

```rust
// ordin-jira
impl JiraAdapter {
    pub fn parse_issue_address(address: &str) -> Result<JiraIssueRef, JiraError>;
    pub fn issue_address(issue: &JiraIssueRef) -> String;         // canonical form
    pub async fn issue(&self, issue: &JiraIssueRef) -> Result<JiraIssueContent, JiraError>;
    pub async fn download_attachment(&self, issue: &JiraIssueRef, attachment_id: &str,
                                     dest_path: &std::path::Path) -> Result<JiraDownload, JiraError>;
}
// JiraDownload { bytes_written: u64, sha256: Sha256Hex }; MAX_ATTACHMENT_BYTES 100 MiB (the adapter refuses above it with JIRA_TOO_LARGE).
// JiraIssueRef { site, key }; JiraIssueContent { summary: JiraIssueSummary, description_markdown, comments: Vec<JiraComment>,
// attachments: Vec<JiraAttachment>, subtasks: Vec<JiraIssueSummary>, linked: Vec<JiraLinkedIssue> };
// JiraIssueSummary { issue, id, summary, assignee: Option<JiraUser>, status_category, labels, updated };
// JiraComment { id, author: JiraUser, created, updated, body_markdown, code_blocks: Vec<JiraCodeBlock { language, text }> };
// JiraAttachment { id, filename, mime_type, size, created }; JiraLinkedIssue { relation, issue: JiraIssueSummary }.
// `JiraError` implements `CodedError`; this service reads `code()` (`JIRA_*`).
```

Constants (*chosen here* unless cited):

| Constant | Value |
|---|---|
| `JIRA_ATTACHMENT_MAX_BYTES` | 100 MiB per attachment, the Jira Adapter's `MAX_ATTACHMENT_BYTES`; a larger one is listed as not read, as step 4.2 does for any remote item above its adapter's limit |
| `JIRA_DOWNLOAD_DIR` | `<data_root>/tmp/jira/` — one file per download, named by a new UUIDv7, deleted after `store_fetched` whether it succeeded or not |
| Record-comment markers | a comment is a **handover record comment** when one of its `code_blocks` has a first line equal to `# ordin.handover/v1 event` or `# ordin.handover/v1 record` (HandoverService.md amendment January 2026 — Jira issues and handovers) |

### Public Interface (additions)

- `propose_link`, `add_link` and `confirm_link` accept `LinkLocation::JiraIssue { address }`; no new method. The Handover Service links the issue of an accepted Jira handover with `add_link(ws, LinkLocation::JiraIssue { address }, Some(display_name))`, the method it already calls for SharePoint and Git places.
- `read_item(link, origin_path)` also brings one Jira item up to date when its `origin_newer_at` is set (§J.4).
- `origin_version(workspace_id, link_kind, origin_path)` accepts `jira_issue` as `link_kind` (§J.4).
- Service API: no new command. `workspaces.add_link` carries the new `LinkLocation` variant; `workspaces.read_item` brings a Jira item up to date.

### Internal Logic (additions) — §J. Jira issue links

1. **Proposing.** `propose_link(id, LinkLocation::JiraIssue { address })`: with `jira = None` (personal server) → `WS_JIRA_UNAVAILABLE`. `JiraAdapter::parse_issue_address(address)`: `JIRA_ADDRESS_INVALID` → `WS_JIRA_ADDRESS_INVALID`. `origin_location` = `JiraAdapter::issue_address(&issue_ref)`, the canonical address `https://<site>/browse/<KEY>-<N>`. `display_name` defaults to `<KEY>-<N>`, and after the first read (§J.2) becomes `<KEY>-<N> <summary>` cut to 120 characters. A duplicate `(workspace_id, jira_issue, origin_location)` → `SRC_DUPLICATE`, as for every kind. `place_id` is set at confirmation.
2. **Confirming and the first read.** `confirm_link` creates the working-copy place exactly as step 3.2 does for a remote link (no `track_link`: Jira keeps no tracking state), then reads the issue once with `jira.issue(&issue_ref)` and writes these items through `files.store_fetched(place, origin_path, bytes, changed_by, changed_at)`, each upserted into `working_copy_items` with `state = read`, `local_version = 1`:

   | `origin_path` | Bytes (UTF-8 Markdown unless stated) | `origin_version_read` | `changed_by`, `changed_at` |
   |---|---|---|---|
   | `summary-and-description.md` | `# <KEY>-<N> <summary>`, a line `Status: <status category> · Assignee: <display name or "nobody">`, a blank line, then `description_markdown` | `issue:<summary.updated>` | assignee's display name or none; `summary.updated` |
   | `comments/<created as YYYYMMDDTHHMMSSZ>-<comment id>.md` — one per comment that is not a handover record comment, plus the handover note comment with its record code block removed | `**<author display name>** · <created>`, a blank line, then `body_markdown` | `comment:<updated>` | author's display name; `updated` |
   | `attachments/<attachment id>-<filename>` (the filename with `/ \ : * ? " < > |` replaced by `-`) | the file, downloaded by `jira.download_attachment(&issue_ref, id, JIRA_DOWNLOAD_DIR/<uuid>)` and read whole (`bytes_written` bytes; the item's bytes are those `store_fetched` receives) | `attachment:<id>` | none; `created` |
   | `related.md` | `## Sub-tasks` then one line per sub-task `<KEY>-<N> <summary> — <status category>`; `## Linked issues` then one line per linked issue `<relation>: <KEY>-<N> <summary> — <status category>`; "None" under an empty heading | `issue:<summary.updated>` | none; `summary.updated` |

   A handover record comment is not written as an item (its YAML is ordin's record, not discussion); the comment whose record block is the opening `# ordin.handover/v1 record` block keeps its note text as an item, without that block (*chosen here*: the note is what the person and the agents read). An attachment above `JIRA_ATTACHMENT_MAX_BYTES`, or beyond `LINK_READ_BUDGET`, is kept in `working_copy_unread` (`item_ref` = `{"attachment_id": <id>}`) and counted `not_read`; `read_item` reads it on first use. Sub-tasks and linked issues are listed, never read (decision 86 item 8). Content read from Jira is data: nothing in a summary, description, comment or attachment is followed as an instruction (R-81). Journal `source_link_read {source_link_id, read, not_read}` as step 4.4.
3. **Read only.** A Jira item never moves to `changed`: a `PlaceChange` on a Jira link's working-copy place records no state change and sends nothing on `subscribe_working_copy_changes`, so the Push Service never proposes a push to Jira; it is logged `jira_item_local_change {source_link_id}`. `set_grant` on a Jira link's place with `ReadAndChange` → `WS_JIRA_READ_ONLY`; `Read` is granted as for any place.
4. **Origin checks.** At each step 7 trigger, for each active `jira_issue` link: one `jira.issue(&issue_ref)`. For each item the read yields, compare its version with the stored `origin_version_read`:
   - equal → nothing;
   - different → set `origin_newer_at = now`, `origin_changed_by`, `origin_changed_at` from the table of §J.2, send `OriginChanged` on `subscribe_origin_changes`, count it for `OpenReport.origin_changes`; the stored bytes and `origin_version_read` are unchanged (decision 86: nothing local is overwritten);
   - a new comment or attachment → a new item, read as §J.2;
   - a comment or attachment gone from the issue → the item is kept, its state unchanged, and an `OriginChanged` with `removed = true` is sent (step 6.4's rule for an item removed at the origin).
   `read_item(link, origin_path)` on an item with `origin_newer_at` set re-reads it (one `jira.issue`, plus one `download_attachment` for an attachment), writes it with `store_fetched`, sets the new `origin_version_read`, clears `origin_newer_at`, and journals `jira_item_brought_up_to_date {source_link_id}`. `origin_version` for `jira_issue` returns the stored version, as for the other remote kinds. Error mapping for the read follows step 3.5 with the rows below; a failing link is retried at the next trigger, as step 7.2 states.
5. **Removing.** `remove_link` on a `jira_issue` link: steps 5.2–5.6 without `untrack_link`. No call is made to Jira. `delete_from_bin` does the same for each Jira link.
6. **Error mapping (step 3.5 additions).** `JIRA_NOT_CONNECTED`, `JIRA_SIGN_IN_NEEDED`, `JIRA_TOKEN_REFUSED` → `WS_SOURCE_SIGN_IN_NEEDED` with the sentence "Connect Jira in Settings so Ordin can read this issue."; `JIRA_SITE_MISMATCH` → `WS_JIRA_SITE_MISMATCH`; `JIRA_ACCESS_DENIED` → `WS_SOURCE_ACCESS_DENIED`; `JIRA_NOT_FOUND` → `WS_SOURCE_NOT_FOUND`; `JIRA_THROTTLED`, `JIRA_UNAVAILABLE`, `JIRA_RESPONSE_INVALID` → `WS_SOURCE_UNAVAILABLE` (retryable); `JIRA_TOO_LARGE` on a download → the attachment is listed as not read.

### Data Model (additive migration)

Migration step 3, `(3, "jira issue links", MIGRATION_3_SQL)` in the SQLite slice (the `postgres` slice stays empty: no agent profile). SQLite cannot change a `CHECK` in place, so the step builds new tables, copies every row, and replaces the old ones inside the Journal's single migration transaction (DatabaseMigrations step 8). Child tables are copied and dropped before their parent, so no `ON DELETE CASCADE` fires; the renames then carry the child tables' references to the new `source_links`.

```sql
-- step (3, "jira issue links", …)
CREATE TABLE source_links_v3 (
    id               TEXT PRIMARY KEY,
    workspace_id     TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind IN ('local_folder','git_repository_folder','sharepoint_location','confluence_location','git_remote','jira_issue')),
    origin_location  TEXT NOT NULL CHECK (length(origin_location) BETWEEN 1 AND 2048),
    display_name     TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    place_id         TEXT,
    state            TEXT NOT NULL CHECK (state IN ('proposed','active')),
    added_at         TEXT NOT NULL,
    last_read_at     TEXT,
    UNIQUE (workspace_id, kind, origin_location)
);
CREATE TABLE working_copy_items_v3 (
    id                   TEXT PRIMARY KEY,
    source_link_id       TEXT NOT NULL REFERENCES source_links_v3(id) ON DELETE CASCADE,
    origin_path          TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    origin_version_read  TEXT NOT NULL CHECK (length(origin_version_read) BETWEEN 1 AND 256),
    local_version        INTEGER NOT NULL CHECK (local_version >= 1),
    state                TEXT NOT NULL CHECK (state IN ('read','changed','awaiting_push','pushed','origin_changed')),
    read_at              TEXT NOT NULL,
    origin_changed_by    TEXT,
    git_branch           TEXT,
    origin_changed_at    TEXT,
    origin_newer_at      TEXT,                            -- Jira issue items only (§J.4)
    UNIQUE (source_link_id, origin_path)
);
CREATE TABLE working_copy_unread_v3 (
    source_link_id  TEXT NOT NULL REFERENCES source_links_v3(id) ON DELETE CASCADE,
    origin_path     TEXT NOT NULL,
    item_ref        TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL,
    PRIMARY KEY (source_link_id, origin_path)
);
INSERT INTO source_links_v3 SELECT id, workspace_id, kind, origin_location, display_name, place_id, state, added_at, last_read_at FROM source_links;
INSERT INTO working_copy_items_v3 (id, source_link_id, origin_path, origin_version_read, local_version, state, read_at,
                                   origin_changed_by, git_branch, origin_changed_at)
    SELECT id, source_link_id, origin_path, origin_version_read, local_version, state, read_at,
           origin_changed_by, git_branch, origin_changed_at FROM working_copy_items;
INSERT INTO working_copy_unread_v3 SELECT source_link_id, origin_path, item_ref, size_bytes FROM working_copy_unread;
DROP TABLE working_copy_unread;
DROP TABLE working_copy_items;
DROP TABLE source_links;
ALTER TABLE source_links_v3 RENAME TO source_links;
ALTER TABLE working_copy_items_v3 RENAME TO working_copy_items;
ALTER TABLE working_copy_unread_v3 RENAME TO working_copy_unread;
CREATE INDEX working_copy_items_origin_changed ON working_copy_items (origin_changed_at);
```

Rollback: the step widens a `CHECK` list and adds one nullable column; every row is kept. It drops and recreates three tables to do so, as a widening rebuild under DatabaseMigrations rule 12a: the cascading children are copied and dropped before their parent, and the scenario *the migration keeps every link and item* is the rule's migration test (row counts unchanged, `PRAGMA foreign_key_check` empty). Rollback is the backup copy taken by DatabaseMigrations step 7, restored with the previous program; the previous program refuses the migrated file with `JRNL_SCHEMA_NEWER`, so it never reads a `jira_issue` row.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Jira issue link asked for on the personal server | `Err(permanent)` | `WS_JIRA_UNAVAILABLE` | `{"error":"Jira isn't available here.","code":"WS_JIRA_UNAVAILABLE","action":null}` |
| The pasted text is not a Jira Cloud issue address | `Err(permanent)` | `WS_JIRA_ADDRESS_INVALID` | `{"error":"Paste the address of a Jira issue, as Jira shows it.","code":"WS_JIRA_ADDRESS_INVALID","action":null}` |
| The issue is on another Jira site than the one connected | `Err(permanent)` | `WS_JIRA_SITE_MISMATCH` | `{"error":"This issue is on a different Jira site from the one you connected.","code":"WS_JIRA_SITE_MISMATCH","action":null}` |
| Change access asked for on a Jira issue's items | `Err(permanent)` | `WS_JIRA_READ_ONLY` | `{"error":"Items from Jira can be read but not changed.","code":"WS_JIRA_READ_ONLY","action":null}` |
| Jira not connected or its sign-in expired | `Err(permanent)` until connected | `WS_SOURCE_SIGN_IN_NEEDED` (existing) | `{"error":"Connect Jira in Settings so Ordin can read this issue.","code":"WS_SOURCE_SIGN_IN_NEEDED","action":"sign_in_again"}` |

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: Happy path — linking a Jira issue makes read-only items
    Given Jira is connected for site "acme.atlassian.net"
    And the test double holds ABC-12 with a description, two comments, one attachment "rates.xlsx" and one sub-task ABC-13
    When add_link(A, JiraIssue { address: "https://acme.atlassian.net/browse/ABC-12" }, None) is called
    Then working_copy_items holds "summary-and-description.md", two "comments/…" items, "attachments/<id>-rates.xlsx" and "related.md", each with state read and the version read
    And related.md lists ABC-13 and the test double received no request for ABC-13
    And the test double received no write

  Scenario: Edge case — handover record comments are not items
    Given ABC-12 carries a handover note comment with a "# ordin.handover/v1 record" block and two "# ordin.handover/v1 event" comments
    When the link is read
    Then the note comment is an item without its record block
    And no item holds an event comment

  Scenario: Happy path — an origin change is shown and nothing local is overwritten
    Given ABC-12 is linked and its description item was read at version "issue:2026-01-10T09:00:00.000Z"
    When the test double changes the description and the next origin check runs
    Then the description item has origin_newer_at set and OriginChanged is sent for it
    And its stored bytes and origin_version_read are unchanged
    When read_item is called for that item
    Then it holds the new description and origin_newer_at is cleared

  Scenario: Happy path — removing the link removes the local copy only
    Given ABC-12 is linked in workspace A
    When remove_link is called for that link
    Then its items, its working-copy place and WORKING_COPY_ROOT/<link> are gone
    And the test double received no request at all

  Scenario: Edge case — an agent's edit of a Jira item is not sent back
    Given an agent writes to "summary-and-description.md" of a linked Jira issue
    Then the item stays read and no id is sent on subscribe_working_copy_changes

  Scenario: Error path — a Jira link on the personal server
    Given the Workspace Service runs with jira None
    When propose_link is called with a JiraIssue location
    Then it fails with WS_JIRA_UNAVAILABLE and no row is written

  Scenario: Error path — a pasted address that is not an issue
    When propose_link is called with JiraIssue { address: "https://acme.atlassian.net/projects/ABC" }
    Then it fails with WS_JIRA_ADDRESS_INVALID

  Scenario: Error path — Jira not connected
    Given no Jira credential exists
    When add_link is called with a JiraIssue location
    Then it fails with WS_SOURCE_SIGN_IN_NEEDED and the link stays proposed

  Scenario: Edge case — the migration keeps every link and item
    Given a journal database at workspaces step 2 with 3 source links, 40 working-copy items and 2 working_copy_unread rows
    When step 3 runs
    Then source_links holds the same 3 rows, working_copy_items the same 40 rows with origin_newer_at NULL and working_copy_unread the same 2 rows
    And PRAGMA foreign_key_check returns no row
    And a row of kind jira_issue can be inserted
```

### Performance, Security, Observability (additions)

- **Performance.** One `jira.issue` request per Jira link per origin check, plus one download per new or brought-up-to-date attachment; with `CHECK_CONCURRENCY` 2 links at a time. The first read of an issue with 100 comments and no attachments ≤ 3 s p95 excluding Jira's own time.
- **Security.** This service sends nothing to Jira; Jira text is stored as data and never followed as an instruction; downloaded files go to `JIRA_DOWNLOAD_DIR` under ordin's data root and are deleted after storing.
- **Observability.** `tracing` events with `component = "workspaces"`: `jira_item_local_change {source_link_id}`, `jira_item_brought_up_to_date {source_link_id}`; the existing `source_link_*`, `origin_check_*` and `link_item_read_failed` events carry kind `jira_issue`.

### Gaps

None.
