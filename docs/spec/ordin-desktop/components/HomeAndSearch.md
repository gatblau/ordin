# SPEC: Home and Search

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (the SharePoint item types are named `SharePointItem`, `SharePointItemRef` and `SharePointVersion`, apart from the Confluence Adapter's `RemoteItem` family, so each has one name in the generated screen types)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-home-search` · **Build phase:** 8 · **Programs:** D P
> **Dependencies:** Journal, Knowledge Service, File Service, Workspace Service, SharePoint Adapter, Confluence Adapter

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
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)

Line references: `02:N` is `docs/design/ordin-desktop/02-architecture.md` line N; `04:N`, `10:N`, `11:N` likewise; `09 ADR-0NN` is an ADR in `09-decisions.md`; `2C` is `docs/spec/ordin-desktop/phase-2c-shared-types-*.md`; `B7 contract` is the batch's shared contract sheet whose decisions are restated here.

## Purpose

Home and Search answers **Home**, the tree's **changed marks** and the **Search or go to…** box for the person, in the application's own code, with no model call (02:371, 09 ADR-058, ADR-059, ADR-079). Home lists, across every workspace, what waits for the person, every running task and crew run, and what finished or changed since the person last opened Home. Changed marks name, per workspace, the documents someone changed where they live since the person last opened each one; opening a document clears its mark. Search answers from workspaces, agents, workflows, dashboards, earlier conversations, knowledge pages and each workspace's file index, within 1 s for first results (A-21), for the person only. **Run again** reads a finished run's inputs from the journal and names each input that has a newer version (ADR-060, A-29). It decides nothing: it opens, switches workspace and fills forms only (02:376). It owns each workspace's *last seen* mark and the person's Home marks (02:377).

## Approach

A read-side module: it keeps only its own marks and builds every view by querying its dependencies' read methods and a small set of **search sources** registered at start by the program (ADR-100 pattern: Home and Search owns the `SearchSource` trait; the program wraps the Package Catalogue, Ordin Host and other listers that are not in this crate's dependency list, so no crate edge is added). Changed marks are materialised in its own table from File Service change events and the adapters' `changes_since`, so the tree reads one indexed table. Rejected: asking Ordin (a model turn on the person's plan, ADR-058 *Alternatives*); computing marks on each tree render from the adapters (network and journal reads per render would miss A-03's 150 ms).

## Shared Context

### Identifiers and time (2C part 1 *Conventions*)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);   // RFC 3339, UTC, ms, `Z`
pub struct WorkspaceId(pub uuid::Uuid); pub struct PlaceId(pub uuid::Uuid); pub struct ChangedMarkId(pub uuid::Uuid);
pub struct RunId(pub uuid::Uuid); pub struct TaskId(pub uuid::Uuid); pub struct CrewRunId(pub uuid::Uuid);
pub struct ApprovalId(pub uuid::Uuid); pub struct SourceLinkId(pub uuid::Uuid); pub struct KnowledgePageId(pub uuid::Uuid);
pub struct BlockId(pub uuid::Uuid); pub struct HandoverId(pub uuid::Uuid); pub struct ArtefactId(pub uuid::Uuid);
pub struct RemoteAgentId(pub uuid::Uuid);   // a hosted agent of the Organisation Profile's remote_agents[] (SA-CLOSE-05)
// 2C wire `ContentRef` (copied unchanged)
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentRef { Artefact { artefact_id: ArtefactId }, PlaceFile { place_id: PlaceId, relative_path: String } }
```

### Domain types owned here (2C part 1, copied unchanged)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HomeMarks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_last_opened_at: Option<Timestamp>,
    pub changed_marks: Vec<ChangedMark>,
}

/// A document changed elsewhere that the person has not opened since (ADR-079).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChangedMark {
    pub id: ChangedMarkId,
    pub workspace_id: WorkspaceId,
    pub place_id: PlaceId,
    pub path: String,
    pub changed_at: Timestamp,
}
```

| Field | Rule | Error code |
|---|---|---|
| ChangedMark (workspace_id, place_id, path) | unique | `HOME_MARK_DUPLICATE` |
| ChangedMark.path | relative to the place, ≤ 4096 bytes | `HOME_PATH_INVALID` |
| workspace removed | its marks deleted (04:71) | — |

Other 2C types read: `Workspace` (with `last_seen_at`, filled here — B7 contract D-9), `WorkspaceState { Active, Archived, InBin }`, `SourceLink` (kind, `place_id` = the working-copy place for remote links, B7 contract D-5), `InputUsed { name, location, version_read }` (`Run.inputs_used`), `RunState`, `CrewRunState`, `KnowledgePageState { Current, MayBeOutOfDate, Removed }`.

`InputUsed.location` forms read here (2C's forms, closing decision K21; the Session Supervisor writes them): `place:<place_id>/<relative path>`, `graph:drives/<drive-id>/items/<item-id>` (SharePoint), `confluence:page/<page-id>` or `confluence:attachment/<page-id>/<attachment-id>`, `knowledge:<knowledge_page_id>`; anything else is compared as "cannot check". `version_read` is the file hash for places, the cTag for SharePoint, the version number for Confluence and Knowledge pages.

### Boundary interfaces (each provider's current Public Interface; R3, R6)

```rust
// ordin-journal (JournalAndArtefactStore.md); concrete type, held as `Arc<ordin_journal::Journal>`
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
impl Journal {
    pub fn datastore(&self) -> &Datastore;
    // read side used by the program's `JournalReader` composite below:
    pub async fn running_runs(&self) -> Result<Vec<RunIndexEntry>, JournalError>;
    pub async fn runs_ended_since(&self, since: Timestamp, limit: u32) -> Result<Vec<RunIndexEntry>, JournalError>;
    pub async fn list_external_actions(&self, state: Option<ExternalActionState>, cursor: Option<Cursor>)
        -> Result<Page<ExternalAction>, JournalError>;
}
pub struct RunIndexEntry { pub run_id: RunId, pub state: RunState, pub first_seen_at: Timestamp, pub ended_at: Option<Timestamp> }

/// Mine, implemented by the program (decision D3; the Journal names it "the program's `JournalReader`
/// composite"). It joins reads of modules this crate has no edge to; the sources the program uses are
/// listed per method. A source the program cannot reach leaves its rows out.
#[async_trait::async_trait]
pub trait JournalReader: Send + Sync {
    /// Approval Service `list_pending(None, …)`, Journal `list_external_actions(Some(UnknownOutcome), …)`,
    /// Push Service `list_pending(workspace)` (held pushes), Handover Service `list(filter)` (waiting handovers),
    /// and Ordin Host `waiting_blocks()` (question, form, crew-plan and offer blocks, as `OpenTarget::Block`;
    /// SA-CLOSE-01), and Hosted Agent Adapter `approver_cards(agent)` for each hosted agent (kind
    /// `hosted_approval`, `workspace_id: None`, `OpenTarget::HostedApproval`; SA-CLOSE-05).
    async fn waiting_items(&self) -> Result<Vec<WaitingItem>, HomeSourceError>;
    /// Journal `running_runs`, titled from Session Supervisor `get(run_id)` and Crew Orchestrator `get(crew_run_id)`.
    async fn running(&self) -> Result<Vec<RunSummary>, HomeSourceError>;
    /// Journal `runs_ended_since(since, limit)`, titled as `running`. A run of a Slack remote agent that ended
    /// `completed` is the agent's reply in Slack and carries `state = "replied_in_slack"` (SA-CLOSE-02).
    async fn finished_since(&self, since: Timestamp, limit: u32) -> Result<Vec<RunSummary>, HomeSourceError>;
    /// `Run.inputs_used` from Session Supervisor `get(run_id)`; a crew run's inputs from Crew Orchestrator `get`.
    async fn run_inputs(&self, run: RunAgainSubject) -> Result<Vec<InputUsed>, HomeSourceError>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WaitingItem { pub workspace_id: Option<WorkspaceId> /* None: a hosted agent's approver card, SA-CLOSE-05 */, pub kind: String, pub title: String,
                         pub since: Timestamp, pub opens: OpenTarget }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSummary { pub workspace_id: WorkspaceId, pub run_id: Option<RunId>, pub crew_run_id: Option<CrewRunId>,
                        pub title: String, pub state: String, pub at: Timestamp, pub can_run_again: bool }
/// Built here from `WorkspaceService::origin_changes_since` (SA-CLOSE-03): one per (workspace, link).
pub struct CopyUpdateSummary { pub workspace_id: WorkspaceId, pub original_name: String, pub count: u32, pub source: String, pub at: Timestamp }
/// Mine. `NotFound` → `HOME_RUN_NOT_FOUND` (run_inputs only); `Unavailable` → `HOME_SOURCE_UNAVAILABLE`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum HomeSourceError { #[error("not found")] NotFound, #[error("unavailable")] Unavailable }

// ordin-knowledge (KnowledgeService.md; obtained with `KnowledgeService::reader()`)
#[async_trait::async_trait]
pub trait KnowledgeReader: Send + Sync {
    async fn search_pages(&self, workspaces: &[WorkspaceId], query: &str, limit: u32) -> Result<Vec<PageHit>, KnowledgeError>;
    async fn out_of_date_since(&self, since: Timestamp) -> Result<Vec<PageSummary>, KnowledgeError>;
    async fn page_version(&self, id: KnowledgePageId) -> Result<Option<String>, KnowledgeError>;
}
pub struct PageHit { pub workspace_id: WorkspaceId, pub id: KnowledgePageId, pub title: String,
                     pub state: KnowledgePageState, pub snippet: String, pub rank: f64 }
pub struct PageSummary { pub workspace_id: WorkspaceId, pub id: KnowledgePageId, pub title: String, pub state: KnowledgePageState,
                         pub written_by: KnowledgeAuthor, pub last_changed_at: Timestamp }
// `search_pages` takes 1–500 workspaces and `limit` 1–50 (else KNOW_QUERY_INVALID).

// ordin-files (FileService.md)
impl FileService {
    pub fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<PlaceChange>;
    pub async fn search(&self, place_ids: &[PlaceId], query: &str, limit: u32) -> Result<SearchResults, FileError>;
    pub async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError>;   // FileBytes.hash
}
// PlaceChange { place_id, path, change: PlaceChangeKind, observed_at, by_ordin, version?, origin? };
// SearchResults { hits: Vec<SearchHit { place_id, path, name_match, snippet?, changed_at }>, coverage: IndexCoverage { indexed, total } }

// ordin-workspaces (WorkspaceService.md)
impl WorkspaceService {
    pub async fn list(&self, filter: WorkspaceListFilter) -> Result<Vec<Workspace>, WorkspaceError>;  // Active | Archived | InBin | NotInBin
    pub async fn places(&self, id: WorkspaceId) -> Result<Vec<Place>, WorkspaceError>;
    pub async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError>;    // SourceLink.place_id = working copy
    /// The latest origin change of each working-copy item since `since`, newest first, at most 1,000 (SA-CLOSE-03).
    pub async fn origin_changes_since(&self, since: Timestamp) -> Result<Vec<OriginChanged>, WorkspaceError>;
}
// OriginChanged { workspace_id, source_link_id, link_kind, origin_path, version: OriginVersion { version, changed_by?, changed_at? }, removed }

// ordin-sharepoint (SharePointAdapter.md)
impl SharePointAdapter {
    pub async fn item_version(&self, item: &SharePointItemRef) -> Result<SharePointVersion, SharePointError>;   // SharePointItemRef { drive_id, item_id }
    pub async fn changes_since(&self, links: &[SourceLinkId], since: Timestamp) -> Result<Vec<OriginChange>, SharePointError>;
}
// ordin-confluence (ConfluenceAdapter.md); absent in the personal server (2B-3 *Absent handles*)
impl ConfluenceAdapter {
    pub async fn item_version(&self, item: &RemoteItemRef) -> Result<RemoteVersion, ConfluenceError>;   // Page { page_id } | Attachment { page_id, attachment_id }
    pub async fn changes_since(&self, links: &[SourceLinkId], since: Timestamp) -> Result<Vec<OriginChange>, ConfluenceError>;
}
// Both adapters: SharePointVersion.tag and RemoteVersion.tag are the "version read"; OriginChange { source_link_id, origin_path, new_version, changed_by?, changed_at, removed }.

/// Mine (owned by this crate): other listers the program registers (agents, workflows, dashboards, conversations).
#[async_trait::async_trait]
pub trait SearchSource: Send + Sync {
    fn group(&self) -> GoToKind;
    async fn find(&self, query: &str, limit: u32) -> Vec<GoToResult>;
}
```

### Reconciliation assumptions

- **SA-CLOSE-01** (*chosen here*; closing decision K19) Waiting question, form, crew-plan and offer blocks come from the Ordin Host's `waiting_blocks()`, which the program's `JournalReader::waiting_items` maps to `WaitingItem { kind: <block kind>, title, since, opens: OpenTarget::Block { workspace_id, block_id } }`.
- **SA-CLOSE-02** (*chosen here*) A Slack reply is the end of a Slack remote agent's run (the Slack Thread Adapter ends the run with the agent's reply), so *Since you last looked* shows it as that finished run, worded "replied in Slack"; no separate read of Slack threads exists, and Home makes no Slack call.
- **SA-CLOSE-03** (*chosen here*; closing decision K18) Copy updates are the Workspace Service's `origin_changes_since(since)`: changes grouped by workspace and link become one row "N documents in <link display name> updated from <SharePoint | Confluence>", `at` = the newest `changed_at`; link names come from `workspaces.source_links`.
- **SA-CLOSE-05** (*chosen here*) A hosted agent's approver card belongs to no workspace, so `WaitingItem.workspace_id` is `Option<WorkspaceId>`; the program's `JournalReader::waiting_items` maps each `HostedAgentAdapter::approver_cards` entry to `WaitingItem { workspace_id: None, kind: "hosted_approval", title: <card.question>, since: <card.asked_at>, opens: OpenTarget::HostedApproval { remote_agent_id, approval_id } }`. *Needs you* shows such rows under one heading, "Hosted agents", after the workspace groups; pressing approve or reject is the Service API command `hosted.decide_card`, which this module does not serve.
- **SA-CLOSE-04** (*chosen here*; closing decisions K21, K22) The `InputUsed.location` forms below are 2C's; migrations are the Journal's R3 tuple form, `ModuleMigrations { module: "home_search", sqlite: &[(1, "home_search tables", <the Data Model SQL>)], postgres: &[] }`.
- **SA-RECON-01** Home and Search reaches its tables through the Journal's `Datastore` (`journal.datastore()`) and exports `pub fn migrations() -> ModuleMigrations` (decision D4, rulings R3, R20).
- **SA-RECON-02** `JournalReader` and `SearchSource` are owned here and implemented by the program over modules this crate has no edge to (decision D3); the Journal itself offers only its run index and external-action reads (R3).
- **SA-RECON-03** The Knowledge Service's `KnowledgeReader`, `PageHit` and `PageSummary` are used as that service defines them; a Knowledge row's `at` is `PageSummary.last_changed_at` (KnowledgeService SA-RECON-03, R6).
- **SA-RECON-04** Remote changed marks map an `OriginChange` to its link's working-copy place through `SourceLink.place_id` (2C C-9); a link with no working copy yet gets no mark.
- **SA-RECON-05** `needs_you` is read on each `needs_you_changed` event, which the Approval, Push and Handover Services append through the Journal; this module appends no event (R1, R6).
- **SA-RECON-06** Service API command names are this module's own list, adopted by the catalogue (R1).

### Constants (*chosen here* unless cited)

| Constant | Value |
|---|---|
| `FIRST_RESULTS_DEADLINE` | 1 s (A-21); sources not answered by then are dropped from this answer |
| `GROUP_LIMIT` | 8 results per group |
| `SINCE_LIMIT` | 200 rows in *Since you last looked* |
| `REMOTE_MARK_POLL` | 60 s for the adapters' `changes_since` |
| `QUERY_MAX` | 200 characters |
| Actions group | Home, New workspace, Add documents, Summarise, Clear, Earlier conversations, Settings (11 §5.17) |

### Confirmation item bound here

- **C-36** — Home, changed marks and search answer within A-03 and A-21 from local data with no model and only read operations. The design is built to it; *if the spike misses 1 s*, the fallback (*assumed here*: the HLD gives none) is to answer *Go to* first and stream *Found in pages and documents* as a second update, which this spec already does (Internal Logic 4.3).

## Public Interface

Error prefix: `HOME_`.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenTarget {
    Workspace { workspace_id: WorkspaceId },
    Document { workspace_id: WorkspaceId, place_id: PlaceId, path: String },
    Run { workspace_id: WorkspaceId, run_id: RunId },
    CrewRun { workspace_id: WorkspaceId, crew_run_id: CrewRunId },
    Approval { workspace_id: WorkspaceId, approval_id: ApprovalId },
    /// A question, form, crew plan or offer block waiting for the person (reconciliation row 182).
    Block { workspace_id: WorkspaceId, block_id: BlockId },
    /// A hosted agent's approver card (SA-CLOSE-05); the screen offers approve and reject through `hosted.decide_card`.
    HostedApproval { remote_agent_id: RemoteAgentId, approval_id: ApprovalId },
    /// A handover handed or returned to the person; the desktop's notifier reads this variant.
    Handover { workspace_id: Option<WorkspaceId>, handover_id: HandoverId },
    KnowledgePage { workspace_id: WorkspaceId, page_id: KnowledgePageId },
    Screen { name: String },                         // "home", "settings", "add_documents" …
    Item { kind: String, id: String, workspace_id: Option<WorkspaceId> },   // from a SearchSource
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HomeRow { pub workspace_id: Option<WorkspaceId>, pub workspace_name: Option<String>, pub line: String,
                     pub at: Timestamp, pub is_new: bool, pub opens: OpenTarget, pub run_again: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HomeView { pub waiting: Vec<HomeRow>, pub running: Vec<HomeRow>, pub since_last_looked: Vec<HomeRow>, pub new_count: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoToKind { Workspace, Agent, Workflow, Dashboard, KnowledgePage, Conversation, Document }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GoToResult { pub kind: GoToKind, pub label: String, pub detail: Option<String>,
                        pub workspace_id: Option<WorkspaceId>, pub opens: OpenTarget }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FoundResult { pub label: String, pub snippet: Option<String>, pub workspace_id: WorkspaceId, pub opens: OpenTarget }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchAnswer {
    pub query: String,
    pub go_to: Vec<GoToResult>,
    pub found: Vec<FoundResult>,
    pub actions: Vec<GoToResult>,
    /// "Searched 41,200 of 58,000 documents" while indexes are still building (11 §5.6).
    pub coverage: Option<(u64, u64)>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunAgainSubject { Run { run_id: RunId }, CrewRun { crew_run_id: CrewRunId } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunAgainInput { pub input: InputUsed, pub newer_version: Option<String>, pub check: VersionCheck }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionCheck { Same, Newer, Gone, CannotCheck }

pub struct HomeSearch { /* private */ }

impl HomeSearch {
    /// `ModuleMigrations { module: "home_search", sqlite: &[(1, "home_search tables", <the Data Model SQL>)], postgres: &[] }`;
    /// the program registers it with the Journal (R3, SA-CLOSE-04).
    pub fn migrations() -> ModuleMigrations;
    pub async fn new(deps: HomeSearchDeps) -> Result<std::sync::Arc<HomeSearch>, HomeError>;
    pub fn register_source(&self, source: std::sync::Arc<dyn SearchSource>);
    pub async fn start(self: &std::sync::Arc<Self>);
    pub async fn home(&self, filter: Option<WorkspaceId>) -> Result<HomeView, HomeError>;
    /// The *Needs you* rows alone (Home's `waiting` section), for the Needs you tab and the desktop notifier.
    pub async fn needs_you(&self, filter: Option<WorkspaceId>) -> Result<Vec<HomeRow>, HomeError>;
    pub async fn home_opened(&self) -> Result<(), HomeError>;
    pub async fn workspace_opened(&self, id: WorkspaceId) -> Result<(), HomeError>;
    pub async fn with_last_seen(&self, ws: Vec<Workspace>) -> Result<Vec<Workspace>, HomeError>;
    pub async fn changed_marks(&self, workspace: WorkspaceId) -> Result<Vec<ChangedMark>, HomeError>;
    /// Clears the changed mark of the item; `ContentRef::Artefact` has no mark and returns `Ok` (reconciliation row 184).
    pub async fn document_opened(&self, workspace: WorkspaceId, item: ContentRef) -> Result<(), HomeError>;
    pub async fn search(&self, query: &str, current: Option<WorkspaceId>) -> Result<SearchAnswer, HomeError>;
    pub async fn run_again_inputs(&self, subject: RunAgainSubject) -> Result<Vec<RunAgainInput>, HomeError>;
}
pub struct HomeSearchDeps {
    /// The datastore is `journal.datastore()` (SA-RECON-01).
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    /// The program's composite (SA-RECON-02).
    pub reader: std::sync::Arc<dyn JournalReader>,
    /// `KnowledgeService::reader()`.
    pub knowledge: std::sync::Arc<dyn KnowledgeReader>,
    pub files: std::sync::Arc<FileService>,
    pub workspaces: std::sync::Arc<WorkspaceService>,
    pub sharepoint: std::sync::Arc<SharePointAdapter>,
    /// `None` in the personal server (2B-3 *Absent handles*).
    pub confluence: Option<std::sync::Arc<ConfluenceAdapter>>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum HomeError {
    #[error("HOME_QUERY_INVALID")] QueryInvalid,
    #[error("HOME_WORKSPACE_NOT_FOUND")] WorkspaceNotFound,
    #[error("HOME_RUN_NOT_FOUND")] RunNotFound,
    #[error("HOME_SOURCE_UNAVAILABLE")] SourceUnavailable,
    #[error("HOME_DATASTORE_UNAVAILABLE")] DatastoreUnavailable,
    #[error("{0}")] Invalid(&'static str),   // HOME_MARK_DUPLICATE, HOME_PATH_INVALID
}
```

### Service API commands (*chosen here*)

| Command | Input | Result |
|---|---|---|
| `home.view` | `{ "workspace_id": WorkspaceId? }` | `HomeView` |
| `home.needs_you` | `{ "workspace_id": WorkspaceId? }` | `Vec<HomeRow>` |
| `home.opened` | `{}` | `{}` |
| `home.workspace_opened` | `{ "workspace_id" }` | `{}` |
| `home.changed_marks` | `{ "workspace_id" }` | `Vec<ChangedMark>` |
| `home.document_opened` | `{ "workspace_id", "item": ContentRef }` | `{}` |
| `home.run_again_inputs` | `{ "subject": RunAgainSubject }` | `Vec<RunAgainInput>` |
| `search.query` | `{ "query", "current_workspace_id": WorkspaceId? }` | `SearchAnswer` |

These are the catalogue's names (R1); the Service API and the screens take this module's inputs and results.

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| Service API commands above | Service API catalogue (screens: Home, Needs you tab, tree, search dialog, Run again) |
| `needs_you` | desktop program's notifier (handover rows, `OpenTarget::Handover`) on each `needs_you_changed` event |
| `register_source` | desktop and server programs, once per lister (Package Catalogue agents, Ordin Host workflows and conversations, dashboards) |
| `with_last_seen` | the program's `workspaces.list` path, after `WorkspaceService::list` |
| `workspace_opened` | the program, on the Workspace Service's `opened()` notice and on `home.workspace_opened` |

### Example

```json
{"command":"search.query","input":{"query":"Kestrel","current_workspace_id":"0192a0f1-1111-7222-8333-444455556666"}}
```
→ `{"query":"Kestrel","go_to":[{"kind":"document","label":"Kestrel quote.pdf","detail":"· Q4 forecast","workspace_id":"0192b…","opens":{"kind":"document",…}}],"found":[{"label":"Supplier rates Q3","snippet":"…Kestrel's day rate of £640…","workspace_id":"0192a0f1-…","opens":{"kind":"knowledge_page",…}}],"actions":[],"coverage":[41200,58000],"complete":true}`

## Internal Logic

### 1. Start

1. Subscribe to `files.subscribe_changes()` (step 3.1); start the `REMOTE_MARK_POLL` task (step 3.2). On `RecvError::Lagged`, the local changes of the lagged window are not marked (the File Service offers no replay); this is logged `marks_lagged {skipped}`, and remote marks are unaffected because they come from the polls of step 3.2.

### 2. Home (ADR-058, 11 §5.17)

1. `home(filter)`: read in parallel `reader.waiting_items()`, `reader.running()`, `reader.finished_since(home_last_opened_at or now−7 days, SINCE_LIMIT)`, `workspaces.origin_changes_since(same)` grouped as SA-CLOSE-03 (at most `SINCE_LIMIT` groups), `knowledge.out_of_date_since(same)` (row time = `last_changed_at`), changed marks newer than that time (step 3), and workspace names from `workspaces.list(WorkspaceListFilter::NotInBin)`.
2. Rows: waiting → "Waiting for you", grouped by workspace, oldest first, then the items with `workspace_id = None` as one group under the heading "Hosted agents" (SA-CLOSE-05; row `workspace_id` and `workspace_name` are `None`, the heading text is the screens' message catalogue), oldest first; running → "Running now" with its state label; finished runs, changed documents ("3 documents changed in SharePoint" when ≥ 3 in one workspace and origin), out-of-date knowledge pages, copy updates ("3 documents in Contracts updated from SharePoint"), Slack replies (finished runs with `state = "replied_in_slack"`) → "Since you last looked", newest first, at most `SINCE_LIMIT`; `is_new` when `at > home_last_opened_at`; finished runs carry `run_again = can_run_again`. Filter keeps one workspace's rows. Workspaces in the Bin are left out.
3. Empty section lines ("Nothing running now") are the screens' message catalogue.
4. `home_opened`: set `home_last_opened_at = now`. `workspace_opened(id)`: upsert `workspace_last_seen(id, now)`; writes nothing into the conversation (ADR-079). `with_last_seen` fills `Workspace.last_seen_at` (B7 contract D-9).
5. `needs_you(filter)`: `reader.waiting_items()` mapped to rows as step 2's "Waiting for you" section, same filter and Bin rule (a workspace filter leaves out the "Hosted agents" group, whose items belong to no workspace; with no filter they are kept); a `HomeSourceError` → `HOME_SOURCE_UNAVAILABLE` returned (here the rows are the whole answer).
6. A dependency that fails leaves its section's rows out, the view still returns, and `HOME_SOURCE_UNAVAILABLE` is logged (never shown as an error on Home; *assumed here*).

### 3. Changed marks (ADR-079)

1. Local: a `PlaceChange` (Created, Modified, Renamed) with `by_ordin = false` on a place held by any non-bin workspace upserts a mark for each such workspace (*assumed here*: a change made outside ordin, possibly by a colleague on a synced folder, is "changed where it lives"); a change to a path the person has open (the last `document_opened` within 10 s) is ignored. Removed → the mark is deleted.
2. Remote: every `REMOTE_MARK_POLL`, read `workspaces.source_links(id)` of each `WorkspaceListFilter::Active` workspace; pass the SharePoint links' ids to `sharepoint.changes_since(&ids, last_poll)` and the Confluence links' ids to `confluence.changes_since(&ids, last_poll)` (skipped when `confluence` is `None`); map each `OriginChange` to `(SourceLink.place_id, origin_path)` (SA-RECON-04); upsert a mark with `changed_at = OriginChange.changed_at`; `removed = true` → delete the mark. An adapter error leaves `remote_mark_poll.last_poll` unchanged for that adapter and logs `home_source_unavailable {source}`.
3. `changed_marks(ws)`: rows of that workspace. Folder counts are the screens' sum by path prefix.
4. `document_opened(ws, item)`: `PlaceFile { place_id, relative_path }` → validate the path (`HOME_PATH_INVALID`) and delete the mark; `Artefact` → nothing to clear, `Ok`.
5. Workspace removed from the Bin → its marks and last-seen row are deleted (read from Workspace Service `list(InBin)` at each poll: marks of workspaces no longer listed at all are deleted).

### 4. Search (ADR-059)

1. `query` trimmed, 1–`QUERY_MAX` chars, else `HOME_QUERY_INVALID`.
2. In parallel, each with `FIRST_RESULTS_DEADLINE`: workspaces by name (`workspaces.list(WorkspaceListFilter::NotInBin)`), each registered `SearchSource`, `knowledge.search_pages(non-bin workspace ids, query, GROUP_LIMIT)` (in chunks of 500 ids), `files.search(places of every non-bin workspace from workspaces.places, query, 2 × GROUP_LIMIT)` (name hits → Go to *document*; text hits → Found). Actions: the fixed list filtered by case-insensitive prefix or word match.
3. Results from the deadline are returned with `complete = false` when any source missed it; the screens call again after 1 s for the late groups (C-36 fallback).
4. Each result from a workspace other than `current` carries `detail = "· <workspace name>"`; choosing it switches workspace first (screens). No result is offered to an agent session or the MCP Front; no model call and no origin request is made (ADR-059).
5. Order within groups: exact name match, prefix match, then source order; at most `GROUP_LIMIT` per group.

### 5. Run again (ADR-060, A-29)

1. `run_again_inputs(subject)`: `reader.run_inputs(subject)` (`HomeSourceError::NotFound` → `HOME_RUN_NOT_FOUND`).
2. For each `InputUsed` by location form: `place:` → `files.read_file(place_id, path)` and compare `FileBytes.hash` with `version_read` (`FILE_NOT_FOUND` → `Gone`); `graph:drives/<d>/items/<i>` → `sharepoint.item_version(&SharePointItemRef { drive_id, item_id }).tag`; `confluence:page/<p>` and `confluence:attachment/<p>/<a>` → `confluence.item_version(&SharePointItemRef::Page { page_id } | ::Attachment { page_id, attachment_id }).tag` (`CannotCheck` when `confluence` is `None`); an adapter's not-found error → `Gone`, any other adapter error → `CannotCheck`; `knowledge:` → `knowledge.page_version` (`None` → `Gone`); other forms (git commits, email, Telegram) → `CannotCheck` *(git originals are compared through places, since a repository folder is a place; a commit-addressed input would need the Project Service, which is not a dependency)*.
3. `Newer` carries the current version; the task form names it ("Quote — Kestrel.pdf: newer version since last time", 11 §5.17). Nothing starts here.

## Data Model

SQLite (desktop, personal server).

```sql
CREATE TABLE home_marks (
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    home_last_opened_at  TEXT
);
CREATE TABLE changed_marks (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL,
    place_id      TEXT NOT NULL,
    path          TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    changed_at    TEXT NOT NULL,
    UNIQUE (workspace_id, place_id, path)
);
CREATE INDEX changed_marks_ws ON changed_marks (workspace_id, changed_at);
CREATE TABLE workspace_last_seen (
    workspace_id  TEXT PRIMARY KEY,
    last_seen_at  TEXT NOT NULL
);
CREATE TABLE remote_mark_poll (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    last_poll_at  TEXT NOT NULL
);
```

Rollback: SA-XC-07 backup. Marks are ordin's own notes; deleting them changes no document.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Search text empty or over 200 characters | `Err(permanent)` | `HOME_QUERY_INVALID` | `{"error":"Type what to look for.","code":"HOME_QUERY_INVALID","action":null}` |
| Workspace filter unknown | `Err(permanent)` | `HOME_WORKSPACE_NOT_FOUND` | `{"error":"That workspace is no longer in ordin.","code":"HOME_WORKSPACE_NOT_FOUND","action":null}` |
| Run again on an unknown run | `Err(permanent)` | `HOME_RUN_NOT_FOUND` | `{"error":"That earlier work is no longer kept.","code":"HOME_RUN_NOT_FOUND","action":null}` |
| A dependency failed while building a view | logged; section left out | `HOME_SOURCE_UNAVAILABLE` | — (Home still shows) |
| The Journal failed while listing *Needs you* (`needs_you`) | `Err(retryable)` | `HOME_SOURCE_UNAVAILABLE` | `{"error":"ordin couldn't load what needs you.","code":"HOME_SOURCE_UNAVAILABLE","action":"try_again"}` |
| The program could not read one hosted agent's approver cards (unreachable, or token refused) | the program's reader leaves that agent's cards out and returns the other items (a source it cannot reach leaves its rows out) | — (the adapter's `AdapterError`, logged by the program) | — (Needs you still shows) |
| Own tables unreachable | `Err(retryable)` | `HOME_DATASTORE_UNAVAILABLE` | `{"error":"ordin couldn't reach its own records.","code":"HOME_DATASTORE_UNAVAILABLE","action":"try_again"}` |
| Mark path malformed | `Err(permanent)` | `HOME_PATH_INVALID` | — (internal) |
| Duplicate mark | absorbed by upsert | `HOME_MARK_DUPLICATE` | — (internal) |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Home and Search

  Scenario: Happy path — Home lists what changed since last opened
    Given Home was last opened at 09:00 and a run finished at 10:00 in "Q3 supplier review"
    When home(None) is called
    Then "Since you last looked" holds the finished run marked new with run_again true

  Scenario: Edge case — a filtered Home shows one workspace only
    When home(Some("Q3 supplier review")) is called
    Then no row from another workspace is returned

  Scenario: Happy path — a SharePoint change marks the document until opened
    Given the SharePoint Adapter reports "Kestrel MSA.docx" changed by Sam Lee
    When the remote mark poll runs
    Then changed_marks for the workspace holds the working-copy path
    And after document_opened for it the mark is gone

  Scenario: Edge case — ordin's own writes do not mark documents
    When the File Service reports a change with by_ordin true
    Then no mark is created

  Scenario: Happy path — search across workspaces names the other workspace
    Given "Kestrel quote.pdf" is in "Q4 forecast" and the current workspace is "Q3 supplier review"
    When search("Kestrel") is called
    Then a Go to document result has detail "· Q4 forecast"
    And the answer arrives within 1 second

  Scenario: Edge case — a slow source does not delay first results
    Given a registered SearchSource takes 3 seconds
    When search("Kestrel") is called
    Then the answer returns within 1 second with complete false

  Scenario: Error path — empty search
    When search("  ") is called
    Then it fails with HOME_QUERY_INVALID

  Scenario: Happy path — Run again names a newer input
    Given a finished run read "place:P/Q3/Kestrel quote.pdf" at hash h1 and the file now has hash h2
    When run_again_inputs is called
    Then that input's check is newer with version h2

  Scenario: Happy path — Needs you lists a handover for the notifier
    Given a handover was handed to the person in "Q3 supplier review"
    When needs_you(None) is called
    Then one row opens OpenTarget handover with that handover id

  Scenario: Edge case — opening an artefact clears nothing and succeeds
    When document_opened is called with ContentRef artefact A
    Then it returns Ok and no mark changes

  Scenario: Error path — Run again on a pruned run
    When run_again_inputs is called for a run the journal no longer keeps
    Then it fails with HOME_RUN_NOT_FOUND

  Scenario: A waiting question block appears in Needs you
    Given Ordin showed a question block in "Q3 supplier review" that the person has not answered
    When needs_you(None) is called
    Then one row opens OpenTarget block with that block id

  Scenario: A hosted agent's approver card appears under "Hosted agents" (SA-CLOSE-05)
    Given the reader returns a waiting item with workspace_id None, kind "hosted_approval" and opens HostedApproval for agent "support-agent" and approval A
    And a waiting handover in "Q3 supplier review"
    When needs_you(None) is called
    Then the handover row comes first under its workspace
    And the card row has workspace_id None and opens OpenTarget hosted_approval with approval A, under the "Hosted agents" group
    When needs_you(Some(<"Q3 supplier review" id>)) is called
    Then the card row is left out

  Scenario: Error path — one hosted agent cannot be read
    Given the program cannot reach hosted agent "support-agent" while building waiting items
    When needs_you(None) is called
    Then the reader returns the other items and needs_you returns their rows with no error

  Scenario: Working copies refreshed from SharePoint are summarised
    Given three items of the "Contracts" SharePoint link changed at their origin after Home was last opened
    When home(None) is called
    Then "Since you last looked" holds one row "3 documents in Contracts updated from SharePoint"
```

## Performance / Security / Observability

- **Performance targets.** `home` ≤ 150 ms p95 and `changed_marks` ≤ 150 ms p95 (A-03, C-36); `search` first results ≤ 1 s p95 across 20 workspaces at A-21's limits (A-21, C-36); `document_opened` ≤ 20 ms p95.
- **Security.** Read-only towards every dependency (C-36: no approve, send or post); results go to the person's WebView only, never to an agent or the MCP Front (ADR-059); search text is passed as a value to each source, never as query syntax; no model call.
- **Never logged.** Search text, document names, snippets, workspace names.
- **Observability (logs only; SA-XC-04/05).** `component = "home"`: `home_built {rows, elapsed_ms}`, `search_answered {groups, results, complete, elapsed_ms}`, `marks_updated {added, removed}`, `marks_lagged {skipped}`, `home_source_unavailable {source}`.

## Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86; HLD revision 0.54.0, ADR-106, 11 §5.13 as amended ("the changed rows carry the changed mark (ADR-079)"). Counterpart: `components/WorkspaceService.md` (amendment January 2026 — Jira issues and handovers, §J.4), which checks a linked Jira issue at its origin and sends `OriginChanged` with `link_kind = JiraIssue`. No new dependency: this service reads the Workspace Service, never the Jira Adapter.

### Purpose (addition)

A changed item of a linked Jira issue carries the changed mark in the tree until the person opens it, as a changed SharePoint or Confluence document does.

### Shared Context (additions)

```rust
// 2C SourceLinkKind gains JiraIssue (amended January 2026, ADR-106); OriginChanged is unchanged.
// OriginChanged { workspace_id, source_link_id, link_kind: SourceLinkKind::JiraIssue, origin_path, version, removed }
```

### Internal Logic (changes)

1. **§3 Changed marks, step 2a (Jira).** At each `REMOTE_MARK_POLL`, `workspaces.origin_changes_since(last_poll)` filtered to `link_kind = JiraIssue`: each change maps to `(SourceLink.place_id, origin_path)` through `workspaces.source_links(workspace_id)` and upserts a mark with `changed_at = version.changed_at` (or the poll time when absent); `removed = true` deletes the mark. A link with no `place_id` yet (not confirmed) is skipped. No Jira call is made here.
2. **SA-CLOSE-03, copy updates.** Changes with `link_kind = JiraIssue` are left out of the "N documents in <link> updated from …" rows: nothing local is updated until the person chooses **Bring in the new version** (decision 86), so the mark is the only Home signal for them.
3. `document_opened` clears a Jira item's mark as any other (§3 step 4).

### Error Table (rows added)

N/A — no new code; a failed `origin_changes_since` is `HOME_SOURCE_UNAVAILABLE`, logged, as for every source (§2 step 6).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a Jira change marks the item until opened
    Given a workspace links Jira issue ABC-12 with a confirmed working copy
    And the Workspace Service reports an OriginChanged for "summary-and-description.md" of that link
    When the remote mark poll runs
    Then changed_marks for the workspace holds that working-copy path
    And after document_opened for it the mark is gone

  Scenario: Edge case — a Jira change is not a copy update
    Given the same OriginChanged since Home was last opened
    When home(None) is called
    Then no "updated from" row names ABC-12

  Scenario: Edge case — a removed Jira comment clears its mark
    Given a mark on "comments/20261021T090000Z-10001.md" of ABC-12
    When the Workspace Service reports an OriginChanged with removed true for it
    And the remote mark poll runs
    Then the mark is gone
```

### Performance, Security, Observability (additions)

- **Performance.** One `origin_changes_since` read per poll (at most 1,000 changes, its own limit); `changed_marks` keeps its 150 ms p95.
- **Security.** Issue text never reaches this service; a mark holds a place id and a path only.
- **Observability.** `marks_updated {added, removed}` counts Jira marks with the others.

### Gaps

None.
