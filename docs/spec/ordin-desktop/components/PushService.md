# SPEC: Push Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-103) · amended January 2026 (the SharePoint item types are named `SharePointItem`, `SharePointItemRef` and `SharePointVersion`, apart from the Confluence Adapter's `RemoteItem` family, so each has one name in the generated screen types) · amended 2026-01-10 (an approved item is sent only at the version the person approved: §C2, *Error Table*, *Acceptance Criteria*) · amended 2026-01-10 (no automatic update of a linked folder's copies, SA-PUSH-LINKED-01; no **Try again** and no **Update copies** for them, SA-PUSH-LINKED-02 and 03, `PUSH_COPY_LINKED`; approved by the user on 2026-01-10)
> **Mode:** greenfield
> **derivedFromHld:** 0.66.0
> **Crate:** `ordin-push` · **Build phase:** 6 · **Programs:** D P
> **Dependencies:** Approval Service, SharePoint Adapter, Confluence Adapter, Project Service, Craft Toolkit, File Service, Journal

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
- [Amendment January 2026 — Team knowledge (ADR-103)](#amendment-january-2026--team-knowledge-adr-103)
- [Amendment 2026-01-10 — A linked folder's copies are not updated automatically](#amendment-2026-01-10--a-linked-folders-copies-are-not-updated-automatically)

## Purpose

The Push Service is the one way a change made in ordin leaves the computer for a shared place (02:408–415; ADR-029, ADR-035, ADR-069, ADR-084, ADR-092). It groups the changed documents of one request into one approval card, checks before each push that the origin has not moved since the working copy was read (A-25), and after approval sends each file to SharePoint, Confluence or a Git remote, recording every send as an external action with a unique key so that a restart never sends twice. It also opens and updates the pull requests that publish agent and part drafts and that carry handovers in Git, and it makes, pushes and keeps up to date the published copies of Markdown originals. It never pushes a change an agent made without the person's approval.

## Approach

A table-driven state machine per pending push (2C `PendingPushState`) and per pull-request submission, persisted in the Journal's datastore and resumed after a restart. Every external send is an external action begun before it leaves and confirmed after (G8): this service mints each key; the SharePoint and Confluence Adapters begin and confirm the action for their own writes, and this service begins and confirms it for Git pushes and pull requests (SA-RECON-03). Git commits and pushes to local repositories use **`git2` 0.21.0 with its bundled libgit2 1.9.x, accepted as a named licence exception** (SA-GIT-01; settled by OQ-67 / OQ-GIT-LICENCE option (a), decision 79); the person's token comes from the Project Service's `GithubAccess::git_credentials`, and pull requests are opened through `GithubAccess::open_pull_request`. Approval decisions arrive as `approval_decided` events through the Journal's `EventSink`, which this service implements (2B-2 D1). Error codes use the 2C prefix `PUSH_` (*chosen here* for codes beyond 2C's). Rejected: *shell out to an installed `git`* — business laptops need not have git (05 — Git; OQ-67 option (b)); *one approval card per file* — ADR-029 groups one request's changes into one card.

## Shared Context

### Shared types used (copied from Phase 2C, unchanged)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);   // RFC 3339, UTC, ms, `Z`
pub struct Sha256Hex(pub String);
// UUIDv7 newtypes: PendingPushId, WorkingCopyItemId, SourceLinkId, ApprovalId, ExternalActionKey,
// WorkspaceId, RunId, PublishedCopyId, PlaceId, KnowledgePageId, ConfluenceLinkId, ArtefactId,
// AgentRepositoryId, AgentDraftId, PartDraftId, HandoverId, CrewRunId, FileVersionId

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingPush {
    pub id: PendingPushId,
    pub workspace_id: WorkspaceId,
    pub request: String,                      // 1–4000 chars (LD-30)
    pub items: Vec<PendingPushItem>,          // 1–200
    pub origins: Vec<SourceLinkId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    pub state: PendingPushState,
    pub created_at: Timestamp,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingPushItem {
    pub working_copy_item_id: WorkingCopyItemId,
    pub summary: String,                      // 1–120 chars, one line
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_action_key: Option<ExternalActionKey>,   // set only once pushed
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingPushState { Proposed, Held, Approved, Rejected, Closed }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkingCopyItem {
    pub id: WorkingCopyItemId,
    pub source_link_id: SourceLinkId,
    pub origin_path: String,
    pub origin_version_read: String,          // A-25
    pub local_version: u32,
    pub state: WorkingCopyState,
    pub read_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingCopyState { Read, Changed, AwaitingPush, Pushed, OriginChanged }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkKind { LocalFolder, GitRepositoryFolder, SharepointLocation, ConfluenceLocation }

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
pub enum CopyOriginal { PlaceFile { place_id: PlaceId, path: String }, KnowledgePage { knowledge_page_id: KnowledgePageId } }
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
pub enum PublishedVia { ConfluenceLink { confluence_link_id: ConfluenceLinkId }, PublishACopy }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishedCopyState {
    ReadyToPublish, AwaitingApproval, Current, OutOfDate, Updating,
    ChangedAtDestination, Failed, NotKeptUpToDate, Removed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExternalAction {
    pub action_key: ExternalActionKey,
    /// Absent for an action started outside any run (2C C-7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,
    pub destination: String,                  // 1–2048 chars
    pub state: ExternalActionState,
    pub sent_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<Timestamp>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind { SlackPost, ConnectionWrite, WrightrunRunStart, EmailSend, OriginPush, CopyPublish, TelegramPost,
                              PullRequestOpened, HandoverWritten }   // 2C C-7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionState { Sent, Confirmed, UnknownOutcome, Refused }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState { Pending, Approved, Rejected, Expired }

/// 2C domain `PullRequestRef` (Authoring Service and Handover Service read it in `PushNotice`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PullRequestRef { pub number: u64, pub url: String, pub state: PullRequestState }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState { Open, Closed, Merged }

// 2C wire §5 — the approval card's parts this component supplies to the Approval Service
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalAsker { Run { run_id: RunId }, CrewRound { crew_run_id: CrewRunId, round: u32 }, Person }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AffectedItem {
    File { label: String, version: Option<String>, place_id: Option<PlaceId> },
    Record { label: String },
    Recipient { name: String, address: String, outside_company: bool },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalContent {
    Message { label: String, text: String, destination_note: Option<String> },
    Command { label: String, command: String, runs_in: String },
    Changes { label: String, change_set: ContentRef },
    Page { label: String, page: ContentRef },
    Files { label: String, files: Vec<ContentRef> },
    Email { label: String, email: EmailApproval },   // EmailApproval as 2C wire §5; never built by this component
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentRef { Artefact { artefact_id: ArtefactId }, PlaceFile { place_id: PlaceId, relative_path: String } }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectStyle { Reject, NotNow }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain }

/// 2C wire, inside `ApprovalRequest` (Approval Service).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActionProposal { pub action: String, pub destination: String, pub affects: Vec<AffectedItem>, pub consequence: String }
```

2C card limits this component must respect when it builds a request: `question` 1–120 chars ending "?" (`APPR_QUESTION_INVALID`); `verb` 1–32, never "OK", "Yes", "Continue" (`APPR_VERB_INVALID`); `where_line` 1–200 (`APPR_WHERE_INVALID`); `affects` 1–50 items, each label 1–200 (`APPR_AFFECTS_INVALID`); `changes` 1–1,000 (`APPR_CHANGES_INVALID`); `Files.files` 1–100. 2C `PendingPush` rules: `request` 1–4000 (`PUSH_REQUEST_INVALID`); items 1–200 (`PUSH_ITEMS_OUT_OF_RANGE`); summary 1–120, one line (`PUSH_SUMMARY_INVALID`); `external_action_key` only once pushed (`PUSH_ACTION_KEY_PREMATURE`); `approval_id` required to enter `approved` (`PUSH_APPROVAL_MISSING`). `ExternalAction`: key unique and written before the action leaves (`EXT_KEY_DUPLICATE`); never resent automatically (`EXT_AUTO_RESEND`). `ActionProposal`: `action` ≤ 500, `destination` ≤ 2,048, `consequence` ≤ 2,000. `PublishedCopy`: `exact_file` only with `confluence_attachment` and maker `none` (`PUB_FORMAT_MAKER_MISMATCH`); `page` only with `confluence_page` (`PUB_FORMAT_DESTINATION_MISMATCH`); an automatic update to a grown audience is held (`PUSH_AUDIENCE_GREW`); deletion at the destination only after the person's check (`PUSH_DELETE_UNCHECKED`).

### Reconciliation assumptions

- **SA-RECON-01** Ruling R12 and decision D3: the Workspace Service owns `working_copy_items`, including items for local and git folder links (WorkspaceService.md 6.7); this crate owns `WorkingCopyStore`, which the programs implement over the Workspace Service and the File Service, so the Push Service has no Workspace Service edge.
- **SA-RECON-02** Ruling R12: the Push Service commits and pushes changes to Git originals itself with `git2`, with credentials from the Project Service's `GithubAccess::git_credentials`; the Project Service has no `commit_and_push` and commits only inside coding-task worktrees.
- **SA-RECON-03** Single external-action bracket (reconciliation rows 711, 723): this service mints every key; the SharePoint Adapter (`ExternalWrite { key, run_id, kind }`) and the Confluence Adapter (`key, run_id`) begin and confirm the external action for their writes, and this service never calls `begin_external_action` for those writes. It begins and confirms only its own Git pushes (`OriginPush`) and pull requests (`PullRequestOpened`).
- **SA-RECON-04** Ruling R19: the Push Service passes the Confluence version it read; only the Confluence Adapter adds 1.
- **SA-RECON-05** Ruling R1: the Service API catalogue carries this crate's six `push.*` command names; there is no `push.send_back` or `push.pending`.
- **SA-RECON-06** Ruling R6: the Project Service's one `GithubAccess` trait with `GithubError` (`GH_*`), `subscribe_original_changes`/`RepositoryOriginChange`, `follow_original`/`unfollow_original`, `read_at_commit` and `remote_state` replace this spec's own `GithubAccess` and `BranchWatch`.
- **SA-RECON-07** Decision D4: this crate's tables live in the Journal's `Datastore`; migrations are `migrations() -> ModuleMigrations` (ruling R3).
- **SA-RECON-08** Reconciliation rows 218–227: the Approval Service's own `ApprovalRequest` (with `show_details`, `proposal`, `asker`, `leaves_company`), `request` → `ApprovalCard`, `withdraw` and `get(id)?.state` replace the `Approvals` trait this spec once assumed.
- **SA-RECON-09** Reconciliation rows 15 and 19: approval decisions arrive through the Journal's push-style `EventSink`, which this service implements; external actions use `begin_external_action`, `confirm_external_action` and `unsettled_external_actions`; after a restart only the person settles an `unknown_outcome` row.
- **SA-RECON-10** 2C C-7: `ExternalAction.run_id` is optional (pushes the person asked for, copies and pull requests carry none), and pull requests are recorded with kind `PullRequestOpened`.
- **SA-RECON-11** Reconciliation rows 355–356, 366: the Push Service calls the Craft Toolkit (`conversion_available`, `make_copy`, `to_markdown`) and the File Service (published-copy rows, versions, changes) directly through its 2B-3 edges; the `CopyMaking` and `PublishedCopies` traits are dropped.
- **SA-CLOSE-01** (*chosen here*; closes G-PUSH-5) A knowledge-page copy is read through `PublishCopyRequest.page_file`, which the Knowledge Service fills from its page row (KnowledgeService.md SA-CLOSE-01); 2C `CopyOriginal::KnowledgePage` stays as it is.
- **SA-RECON-12** Workspace Service reading of 2C's working-copy table (WorkspaceService.md 6.3): a rejected push sets `awaiting_push → changed`.

### Configuration

| Item | Value | Source |
|---|---|---|
| Fetch of followed branches for originals | at start, on workspace open, every 15 minutes (done by the Project Service for paths this service follows with `follow_original`; it broadcasts `RepositoryOriginChange`) | 2D-4, A-36 |
| SharePoint and Confluence originals with copies | compared at start and every 15 minutes *(assumed here: 02:384 says "when a workspace opens"; this component has no workspace-open signal, so it uses the 2D-4 interval)* | this spec |
| Header line on every copy | "Published copy of [file], from [repository or place], branch [name]. Last updated [date, time]. Make changes in the original." (ADR-069), composed in the person's locale through the Fluent message `push-copy-header` (*chosen here*) | ADR-069 |
| Until C-28 is confirmed | **Publish a copy…** offers only a Confluence page (ADR-069 *Before the apps are confirmed*) | ADR-069, C-28 |
| Recovery | on every start, after the program's `Journal::recover_external_actions` and before accepting new work | G8 |
| SharePoint copy destination | `CopyDestination::SharepointFile { site, library, folder }` holds the site id, the drive id in `library` and the folder's item id in `folder`; the folder is resolved with `SharePointAdapter::resolve_origin("graph:drives/{library}/items/{folder}")` (*chosen here*; the adapter's origin form) | this spec |
| Confluence copy destination | `CopyDestination::Confluence*{ space, parent_page_id }`; the location for the audience is `ConfluenceAdapter::resolve_origin("confluence:page/{parent_page_id}")` (the adapter's origin form) | ConfluenceAdapter.md *Constants* |

### Boundary interfaces this spec requires (copied from the providers' current specs; rulings R3, R6)

```rust
// ordin-approval — ApprovalService (ApprovalService.md, Public Interface)
pub struct ApprovalRequest {                 // serde deny_unknown_fields
    pub workspace_id: WorkspaceId, pub asked_by: ApprovalAsker,
    pub question: String, pub verb: String, pub where_line: String,
    pub affects: Vec<AffectedItem>, pub changes: String,
    pub content: Option<ApprovalContent>, pub reject_style: RejectStyle,
    pub show_details: bool,                  // true only when `content` is present
    pub proposal: ActionProposal,
    pub asker: Option<String>,               // agent profile only; None here
    pub leaves_company: bool,                // agent profile only; false here
}
impl ApprovalService {
    /// The caller takes `card.approval_id` from the returned 2C `ApprovalCard`.
    pub async fn request(&self, req: ApprovalRequest) -> Result<ApprovalCard, ApprovalError>;
    /// Pending → `expired`, buttons closed, `approval_decided` emitted (ADR-035: a local edit holds the push).
    pub async fn withdraw(&self, approval_id: ApprovalId) -> Result<(), ApprovalError>;
    /// `get(id)?.state` is the one way to read an approval's state; unknown id → `APPR_NOT_FOUND`.
    pub async fn get(&self, approval_id: ApprovalId) -> Result<ApprovalRecord, ApprovalError>;
}
// ApprovalError: CodedError (`code()`, `retryable()`); ApprovalRecord { card, proposal, state, decided_by, decided_on, decided_at }.

// ordin-journal — Journal (JournalAndArtefactStore.md; ruling R3)
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
pub struct NewExternalAction { pub action_key: ExternalActionKey, pub run_id: Option<RunId>,
                               pub kind: ExternalActionKind, pub destination: String }
impl Journal {
    pub fn datastore(&self) -> &Datastore;
    /// `kind` is `EventKind::as_str()`; `data` is the variant's fields.
    pub async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>;
    pub async fn events_since(&self, after_seq: u64, limit: u32) -> Result<Vec<JournalEvent>, JournalError>;
    /// A used key → `JournalError` with `code() == "EXT_KEY_DUPLICATE"`.
    pub async fn begin_external_action(&self, new: NewExternalAction) -> Result<ExternalAction, JournalError>;
    /// `sent` or `confirmed` → `confirmed`; `unknown_outcome` → `JRNL_EXT_STATE_INVALID` (only the person settles it).
    pub async fn confirm_external_action(&self, key: ExternalActionKey) -> Result<ExternalAction, JournalError>;
    pub async fn external_action(&self, key: ExternalActionKey) -> Result<Option<ExternalAction>, JournalError>;
    /// Every action of `kinds` still `sent` or `unknown_outcome`, oldest first, at most 1,000.
    pub async fn unsettled_external_actions(&self, kinds: &[ExternalActionKind]) -> Result<Vec<ExternalAction>, JournalError>;
    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId;   // called by the program
}
/// Implemented by this service (`event_sink()`), registered by the program with `Journal::subscribe`.
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    fn name(&self) -> &'static str;
    async fn deliver(&self, event: &JournalEvent);   // `kind` "approval_decided": data { approval_id, state, decided_on }
    async fn lagged(&self, next_seq: u64);
}
pub struct JournalEvent { pub backend: uuid::Uuid, pub seq: u64, pub at: Timestamp, pub kind: String, pub data: serde_json::Value }
// JournalError: CodedError (`code()`, `retryable()`).

// ordin-files — FileService (FileService.md, Public Interface)
impl FileService {
    pub async fn place(&self, place_id: PlaceId) -> Result<Place, FileError>;
    pub async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError>;
    pub async fn versions(&self, place_id: PlaceId, path: &str) -> Result<Vec<FileVersion>, FileError>;
    pub async fn read_version(&self, version_id: FileVersionId) -> Result<FileBytes, FileError>;
    pub async fn current_version(&self, place_id: PlaceId, path: &str) -> Result<Option<FileVersion>, FileError>;
    pub fn subscribe_changes(&self) -> tokio::sync::broadcast::Receiver<PlaceChange>;
    pub async fn record_published_copy(&self, c: PublishedCopy) -> Result<PublishedCopy, FileError>;   // checks the 2C PUB_* rules
    pub async fn set_published_copy_state(&self, id: PublishedCopyId, state: PublishedCopyState,
                                          failure_reason: Option<String>) -> Result<PublishedCopy, FileError>;
    pub async fn published_copy(&self, id: PublishedCopyId) -> Result<PublishedCopy, FileError>;
    pub async fn published_copies_of(&self, original: &CopyOriginal) -> Result<Vec<PublishedCopy>, FileError>;
    /// The person's Save on a Markdown document (ADR-048): a new version with origin Person, held until it goes back.
    pub async fn save_person_edit(&self, place_id: PlaceId, path: &str, bytes: Vec<u8>) -> Result<FileVersion, FileError>;
    pub async fn mark_edit_sent(&self, place_id: PlaceId, path: &str) -> Result<(), FileError>;
}
// FileBytes { place_id, path, bytes, hash, media_type }; FileVersion { id, place_id, path, version: u32, hash, origin, created_at };
// PlaceChange { place_id, path, change: PlaceChangeKind, observed_at, by_ordin, version: Option<u32>, origin: Option<FileVersionOrigin> }.

// ordin-projects — ProjectService (ProjectService.md, "originals in repositories" and "Repository hosts in process")
pub struct RepositoryOriginChange { pub place_id: PlaceId, pub path: String, pub branch: String,
    pub commit: String, pub author: String, pub committed_at: Timestamp }
pub struct RemoteBranchState { pub branch: String, pub commit: String, pub checked_at: Timestamp }
impl ProjectService {
    pub async fn follow_original(&self, place_id: PlaceId, path: &str, branch: &str) -> Result<(), ProjectError>;
    pub async fn unfollow_original(&self, place_id: PlaceId, path: &str, branch: &str) -> Result<(), ProjectError>;
    pub fn subscribe_original_changes(&self) -> tokio::sync::broadcast::Receiver<RepositoryOriginChange>;
    pub async fn read_at_commit(&self, repo_place: PlaceId, commit: &str, path: &str) -> Result<Vec<u8>, ProjectError>;
    /// Fetches, then returns the remote tip of `branch`.
    pub async fn remote_state(&self, place_id: PlaceId, branch: &str) -> Result<RemoteBranchState, ProjectError>;
}
// ProjectError codes used: PROJ_SIGN_IN_NEEDED, PROJ_REMOTE_UNREACHABLE, PROJ_RATE_LIMITED, PROJ_NOT_FOUND.
#[async_trait::async_trait]
pub trait GithubAccess: Send + Sync {        // implemented by ProjectService; the methods this spec calls
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError>;
    async fn open_pull_request(&self, req: OpenPullRequest) -> Result<PullRequestRef, GithubError>;   // 2C PullRequestRef
    async fn pull_request_for_branch(&self, host: &str, repository: &str, head: &str) -> Result<Option<PullRequestRef>, GithubError>;
    async fn create_branch_with_files(&self, req: BranchWithFiles) -> Result<String /* commit */, GithubError>;
}
pub struct GitCredential { pub username: String, pub token: GitToken }   // GitToken::expose(); Debug "GitToken(***)"
pub struct OpenPullRequest { pub host: String, pub repository: String, pub head: String, pub base: String,
                             pub title: String, pub body: String, pub labels: Vec<String>, pub reviewers: Vec<String> }
pub struct BranchWithFiles { pub host: String, pub repository: String, pub base: String, pub branch: String,
                             pub message: String, pub files: Vec<(String /* repository path */, Vec<u8>)> }
// GithubAccess returns the 2C PullRequestRef directly.
pub enum GithubError { SignInNeeded, NotFound, RateLimited { retry_after_s: u32 }, Unreachable,
                       Conflict { current_sha: String }, HostNotSupported, Other(String) }   // GH_* codes

// ordin-sharepoint — SharePointAdapter (SharePointAdapter.md, Public Interface)
pub struct SharePointItemRef { pub drive_id: String, pub item_id: String }            // sharepoint::SharePointItemRef (amended January 2026)
pub struct SharePointVersion { pub tag: String, pub etag: String, pub label: Option<String>,
                           pub changed_at: Timestamp, pub changed_by: Option<String> }
/// The caller mints `key`; the adapter begins and confirms the external action itself.
pub struct ExternalWrite { pub key: ExternalActionKey, pub run_id: Option<RunId>, pub kind: ExternalActionKind }
impl SharePointAdapter {
    pub async fn resolve_origin(&self, origin_location: &str) -> Result<SharePointLocation, SharePointError>;
    pub async fn item_at(&self, folder: &SharePointLocation, path: &str) -> Result<Option<SharePointItem>, SharePointError>;
    pub async fn item_version(&self, item: &SharePointItemRef) -> Result<SharePointVersion, SharePointError>;
    pub async fn fetch(&self, item: &SharePointItemRef) -> Result<FetchedItem, SharePointError>;
    pub async fn upload_version(&self, item: &SharePointItemRef, bytes: Vec<u8>, expected_version: &str,
                                action: ExternalWrite) -> Result<SharePointVersion, SharePointError>;
    pub async fn create_file(&self, folder: &SharePointLocation, name: &str, bytes: Vec<u8>,
                             action: ExternalWrite) -> Result<(SharePointItemRef, SharePointVersion), SharePointError>;
    pub async fn delete(&self, item: &SharePointItemRef, expected_version: Option<&str>, action: ExternalWrite) -> Result<(), SharePointError>;
    pub async fn exists(&self, folder: &SharePointLocation, name: &str) -> Result<bool, SharePointError>;
    pub async fn audience(&self, target: &SharePointLocation) -> Result<Audience, SharePointError>;
}
// sharepoint::Audience { line, people: Option<u32>, everyone_who_can_open, principals: Vec<String>, link_scope: Option<LinkScope> }
// SharePointError codes: SP_ORIGIN_CHANGED { current }, SP_SIGN_IN_NEEDED, SP_ACCESS_DENIED, SP_NOT_FOUND, SP_NAME_EXISTS,
// SP_TOO_LARGE, SP_THROTTLED, SP_UNAVAILABLE, SP_OUTCOME_UNKNOWN, SP_ACTION_KEY_USED, and others (SP_NOT_CONFIGURED, …).

// ordin-confluence — ConfluenceAdapter (ConfluenceAdapter.md, Public Interface); absent on the personal server
pub enum RemoteItemRef { Page { page_id: String }, Attachment { page_id: String, attachment_id: String } }   // confluence::RemoteItemRef
pub struct RemoteVersion { pub tag: String, pub changed_at: Timestamp, pub changed_by: Option<String> }  // tag = version number as text
impl ConfluenceAdapter {
    pub async fn resolve_origin(&self, origin_location: &str) -> Result<ConfluenceLocation, ConfluenceError>;
    pub async fn item_for_path(&self, link: SourceLinkId, origin_path: &str) -> Result<RemoteItemRef, ConfluenceError>;
    pub async fn item_version(&self, item: &RemoteItemRef) -> Result<RemoteVersion, ConfluenceError>;
    pub async fn fetch(&self, item: &RemoteItemRef) -> Result<FetchedItem, ConfluenceError>;
    pub async fn publish_page(&self, dest: &CopyDestination, title: &str, markdown: &str, banner: Option<&str>,
                              key: ExternalActionKey, run_id: Option<RunId>) -> Result<(String, RemoteVersion), ConfluenceError>;
    /// `expected_version` is the version READ; the adapter adds 1 (R19).
    pub async fn update_page(&self, page_id: &str, title: &str, markdown: &str, banner: Option<&str>,
                             expected_version: &str, key: ExternalActionKey, run_id: Option<RunId>) -> Result<RemoteVersion, ConfluenceError>;
    pub async fn update_page_storage(&self, page_id: &str, storage_xhtml: &str, expected_version: &str,
                                     key: ExternalActionKey, run_id: Option<RunId>) -> Result<RemoteVersion, ConfluenceError>;
    pub async fn attach(&self, page_id: &str, file_name: &str, bytes: Vec<u8>, expected_version: Option<&str>,
                        key: ExternalActionKey, run_id: Option<RunId>) -> Result<(String, RemoteVersion), ConfluenceError>;
    pub async fn delete(&self, item: &RemoteItemRef, expected_version: Option<&str>,
                        key: ExternalActionKey, run_id: Option<RunId>) -> Result<(), ConfluenceError>;
    pub async fn audience(&self, loc: &ConfluenceLocation) -> Result<Audience, ConfluenceError>;
    pub fn storage_to_markdown(storage_xhtml: &str) -> Result<String, ConfluenceError>;
}
// confluence::Audience { line, people: Option<u32>, everyone_who_can_open, principals: Vec<String> }
// ConfluenceError codes: CONF_ORIGIN_CHANGED { current }, CONF_SIGN_IN_NEEDED, CONF_SITE_NOT_GRANTED, CONF_ACCESS_DENIED,
// CONF_NOT_FOUND, CONF_TITLE_EXISTS, CONF_TOO_LARGE, CONF_THROTTLED, CONF_UNAVAILABLE, CONF_OUTCOME_UNKNOWN,
// CONF_ACTION_KEY_USED, CONF_NOT_TRACKED, and others (CONF_NOT_CONFIGURED, CONF_CONVERSION_FAILED, …).

// ordin-craft-toolkit — CraftToolkit (CraftToolkit.md, Public Interface)
pub struct MakeCopy { pub original_name: String, pub original_from: String, pub branch: Option<String>,
                      pub markdown: String, pub format: CopyFormat, pub updated_at: Timestamp }
pub struct MadeCopy { pub bytes: Vec<u8>, pub file_name: String, pub maker: CopyMaker }
impl CraftToolkit {
    /// C-28: false until the app's Markdown conversion tool is confirmed and installed.
    pub fn conversion_available(&self, format: CopyFormat) -> bool;
    /// Writes the ADR-069 header line into the copy from `original_name`, `original_from`, `branch`, `updated_at`.
    pub async fn make_copy(&self, req: MakeCopy) -> Result<MadeCopy, CraftError>;
    /// Conditional on C-13; `format` must be `Word`; until then `CRAFT_CONVERSION_UNAVAILABLE`.
    pub async fn to_markdown(&self, format: CopyFormat, bytes: Vec<u8>) -> Result<String, CraftError>;
}
// CraftError: code() (`CRAFT_*`), retryable().

/// Mine (decision D3, ADR-100 item 1 as amended). The Workspace Service owns `working_copy_items` (ruling R12) and
/// 2B-3 gives this crate no edge to it, so the desktop and server programs implement this trait:
/// `entry` ← `WorkspaceService::working_copy_entry` (field for field; `git_host` and `git_repository` from its
/// `repository: Option<LinkedRepository>` `host_name` and `repository`); `set_state` ← `set_working_copy_state`;
/// `local_changes` ← `subscribe_working_copy_changes`; `content` and `restore_version` ← the File Service's
/// `read_file`, `versions`, `read_version` and `restore_version` on the entry's `place_id` and `relative_path`.
#[async_trait::async_trait]
pub trait WorkingCopyStore: Send + Sync {
    async fn entry(&self, id: WorkingCopyItemId) -> Result<WorkingCopyEntry, StoreFailure>;
    /// `origin_version` is required for `pushed` (the new origin version) and ignored otherwise.
    async fn set_state(&self, id: WorkingCopyItemId, state: WorkingCopyState, origin_version: Option<String>) -> Result<(), StoreFailure>;
    /// The current bytes, or those of `local_version` (File Service version number) when given.
    async fn content(&self, id: WorkingCopyItemId, local_version: Option<u32>) -> Result<Vec<u8>, StoreFailure>;
    async fn restore_version(&self, id: WorkingCopyItemId, local_version: u32) -> Result<(), StoreFailure>;
    /// Ids of items changed on this computer (an item moving to `changed`, or a further change to it).
    fn local_changes(&self) -> tokio::sync::broadcast::Receiver<WorkingCopyItemId>;
}
#[derive(Debug, Clone)]
pub struct WorkingCopyEntry {
    pub item: WorkingCopyItem,
    pub workspace_id: WorkspaceId,
    pub link_kind: SourceLinkKind,
    /// SharePoint `graph:drives/<drive>/items/<item>`; Confluence `confluence:space/{key}` or `confluence:page/{id}`;
    /// the canonical folder path for local and git folder links.
    pub origin_location: String,
    pub display_name: String,
    pub place_id: PlaceId,
    pub relative_path: String,
    pub git_branch: Option<String>,
    /// Git folder links whose `origin` remote is on GitHub or GitLab: the host name and `owner/name`.
    pub git_host: Option<String>,
    pub git_repository: Option<String>,
}
/// The provider's error code (`WS_*` or `FILE_*`) as the program received it.
#[derive(Debug, Clone)]
pub struct StoreFailure { pub code: &'static str, pub retryable: bool }
```

`Datastore`, `ApprovalCard`, `ApprovalRecord`, `Place`, `FetchedItem`, `SharePointItem`, `SharePointLocation`, `ConfluenceLocation`, `LinkScope`, `PlaceChangeKind` and the error enums are the owning components' types, used as they define them.

## Public Interface

```rust
pub struct PushService { /* private */ }

pub struct PushDeps {
    /// This service's tables live in `journal.datastore()` (decision D4; no separate pool).
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    pub approvals: std::sync::Arc<ordin_approval::ApprovalService>,
    /// Implemented by the program (D3; see *Boundary interfaces*).
    pub working_copy: std::sync::Arc<dyn WorkingCopyStore>,
    pub files: std::sync::Arc<ordin_files::FileService>,
    pub projects: std::sync::Arc<ordin_projects::ProjectService>,
    /// The Project Service as `GithubAccess` (the same object as `projects`).
    pub github: std::sync::Arc<dyn ordin_projects::GithubAccess>,
    pub sharepoint: std::sync::Arc<ordin_sharepoint::SharePointAdapter>,
    /// `None` on the ordin server, which has no Confluence Adapter (2B-3 absent handle); a Confluence item
    /// then fails with `PUSH_CONFLUENCE_UNAVAILABLE`.
    pub confluence: Option<std::sync::Arc<ordin_confluence::ConfluenceAdapter>>,
    pub craft: std::sync::Arc<ordin_craft_toolkit::CraftToolkit>,
}

impl PushService {
    /// Module `push`; the program passes it to `MigrationRegistry::register` (R3).
    pub fn migrations() -> ordin_journal::ModuleMigrations;
    pub fn new(deps: PushDeps) -> std::sync::Arc<Self>;
    /// The sink the program registers with `Journal::subscribe`; it forwards `approval_decided` events to §B.
    pub fn event_sink(self: &std::sync::Arc<Self>) -> std::sync::Arc<dyn ordin_journal::EventSink>;
    /// Recovery (§G), then the event loops (§B, §D, §E). Returns when `shutdown` fires.
    pub async fn run(&self, shutdown: tokio_util::sync::CancellationToken) -> Result<(), PushError>;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PushNotice>;

    // Documents back to their origins (ADR-029, ADR-035)
    pub async fn propose(&self, proposal: PushProposal) -> Result<PendingPush, PushError>;
    pub async fn list_pending(&self, workspace_id: WorkspaceId) -> Result<Vec<PendingPush>, PushError>;
    pub async fn choose_version(&self, input: ChooseVersionInput) -> Result<PendingPush, PushError>;
    pub async fn try_again(&self, input: TryAgainInput) -> Result<PendingPush, PushError>;

    // Pull requests (ADR-084, ADR-091, ADR-092)
    pub async fn propose_pull_request(&self, req: PullRequestProposal) -> Result<SubmissionId, PushError>;
    pub async fn send_approved_pull_request(&self, req: ApprovedPullRequest) -> Result<PullRequestRef, PushError>;

    // Published copies (ADR-069)
    pub async fn publish_copy(&self, req: PublishCopyRequest) -> Result<PublishedCopyId, PushError>;
    pub async fn update_copies(&self, original: CopyOriginal, workspace_id: WorkspaceId) -> Result<Option<ApprovalId>, PushError>;
    pub async fn copy_action(&self, input: CopyActionInput) -> Result<PublishedCopy, PushError>;
}
```

Inputs and results (*chosen here*; Service API inputs carry `deny_unknown_fields`):

```rust
pub struct PushProposal {
    pub workspace_id: WorkspaceId,
    pub run_id: Option<RunId>,                // Ordin's run when proposed through `propose_push`
    pub request: String,
    pub items: Vec<ProposedItem>,
}
pub struct ProposedItem { pub working_copy_item_id: WorkingCopyItemId, pub summary: String, pub change_view: Option<ArtefactId> }

pub struct ChooseVersionInput { pub pending_push_id: PendingPushId, pub working_copy_item_id: WorkingCopyItemId, pub choice: HeldChoice }
#[serde(rename_all = "snake_case")]
pub enum HeldChoice { KeepMine, KeepOrdins, ApplyOrdinsToMine }
pub struct TryAgainInput { pub pending_push_id: PendingPushId, pub working_copy_item_id: WorkingCopyItemId }

pub struct SubmissionId(pub uuid::Uuid);      // UUIDv7, chosen here
pub struct PullRequestProposal {
    pub workspace_id: WorkspaceId,
    pub origin: SubmissionOrigin,
    pub host: String, pub repository: String,
    pub content: PullRequestContent,
    pub branch: String, pub base: String,
    pub title: String,                        // 1–256 chars
    pub body: String,                         // ≤ 65,536 chars (GitHub's body limit)
    pub labels: Vec<String>, pub reviewers: Vec<String>,
    pub existing: Option<PullRequestRef>,     // Send the change…: push to the same pull request
    pub card: CardText,
}
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubmissionOrigin {
    AgentDraft { id: AgentDraftId }, PartDraft { id: PartDraftId }, Handover { id: HandoverId },
    /// A saved workflow shared to a repository folder by the Ordin Host (ADR-066; reconciliation row 497):
    /// `content` is `Files` with the one `.workflow.yaml` file.
    WorkflowShare { workspace_id: WorkspaceId, file_name: String },
}
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PullRequestContent {
    /// A worktree whose changed files are committed with git2 and pushed (agent and part drafts).
    Worktree { path: std::path::PathBuf, paths: Vec<String> },
    /// Files written through GitHub's API on a new branch, no clone (handovers).
    Files { files: Vec<(String, Vec<u8>)> },
}
pub struct CardText { pub question: String, pub verb: String, pub where_line: String, pub affects: Vec<AffectedItem>, pub changes: String, pub content: Option<ApprovalContent> }
pub struct ApprovedPullRequest { pub approval_id: ApprovalId, pub proposal: PullRequestProposal }

pub struct PublishCopyRequest {
    pub workspace_id: WorkspaceId,
    /// `original_version`: the commit for an original in a repository place; otherwise the File Service
    /// version number as text ("4").
    pub original: CopyOriginal, pub original_version: String, pub original_name: String,
    /// `CopyOriginal::KnowledgePage` only: the page's file (place and path), supplied by the Knowledge Service
    /// from its page row (SA-CLOSE-01); `None` for `PlaceFile`.
    pub page_file: Option<(PlaceId, String)>,
    pub destination: CopyDestination, pub format: CopyFormat,
    pub followed_branch: Option<String>, pub keep_up_to_date: bool,
    pub file_name: String,                    // "Use another name" sets it
}
pub struct CopyActionInput { pub published_copy_id: PublishedCopyId, pub action: CopyAction }
#[serde(rename_all = "snake_case")]
pub enum CopyAction { TryAgain, BringItIn, LeaveIt, ReplaceAtNextUpdate, StopKeepingUpToDate, KeepUpToDate, Remove }

#[derive(Debug, Clone)]
pub enum PushNotice {
    PushFinished { pending_push_id: PendingPushId, workspace_id: WorkspaceId, pushed: Vec<WorkingCopyItemId>, failed: Vec<(WorkingCopyItemId, &'static str)>, origin_changed: Vec<WorkingCopyItemId> },
    PushHeld { pending_push_id: PendingPushId, item: WorkingCopyItemId },
    PullRequestOpened { submission: SubmissionId, origin: SubmissionOrigin, pull_request: PullRequestRef },
    PullRequestFailed { submission: SubmissionId, origin: SubmissionOrigin, code: &'static str },
    PullRequestRejected { submission: SubmissionId, origin: SubmissionOrigin },   // card rejected
    CopiesUpdated { original: CopyOriginal, updated: u32, held: u32, failed: u32, from_version: String },
}

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct PushError { pub code: &'static str, pub retryable: bool, pub action: Option<RepairAction>, pub detail: String }
```

Service API commands (*chosen here*): `push.list_pending`, `push.choose_version`, `push.try_again`, `push.publish_copy`, `push.update_copies`, `push.copy_action`. These are the catalogue's names (R1); there is no `push.send_back`: `propose` already opens the approval card. Events: this service appends `needs_you_changed { workspace_id }` through the Journal (Internal Logic H).

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| `propose` (with `PushProposal`, per-item `summary`; reads `PendingPush.id`) | Ordin Host (`propose_push` tool) |
| `propose_pull_request` | Authoring Service, Ordin Host (`SubmissionOrigin::WorkflowShare`, content `Files`) |
| `send_approved_pull_request` | Handover Service |
| `publish_copy` (`CopyOriginal::KnowledgePage { knowledge_page_id }` with `page_file` for knowledge pages) | Knowledge Service; Service API `push.publish_copy` |
| `subscribe` (`PushNotice`) | Ordin Host (result blocks, Home lines, `apply_ordins_to_mine`), Authoring Service, Handover Service |
| `list_pending`, `choose_version`, `try_again`, `update_copies`, `copy_action` | Service API catalogue (screens) |
| `list_pending` | program's `JournalReader` for Home and Search (`waiting_items`: held pushes) |
| `migrations`, `new`, `event_sink`, `run` | desktop and server programs, at start (`MigrationRegistry::register`, `Journal::subscribe`) |
| `propose_team_share`, `resend_team_share`, `withdraw_team_share`; `subscribe` (`TeamShare*` notices) | Team Knowledge Service, through its `TeamPush` trait, which the programs implement by delegation (amendment January 2026, ADR-103) |

### Example

`propose` input:

```json
{"workspace_id":"0192a0f1-1111-7222-8333-444455556666","run_id":"0192a1c2-3344-7556-8677-8899aabbccdd",
 "request":"Update the supplier summary with the new rates and refresh the slides",
 "items":[{"working_copy_item_id":"0192a1d0-0000-7000-8000-000000000011","summary":"New Q3 rates in the summary table","change_view":"0192a1c4-0000-7000-8000-00000000a001"},
          {"working_copy_item_id":"0192a1d0-0000-7000-8000-000000000012","summary":"Slides 3 and 4 use the new rates","change_view":null}]}
```

Result: a `PendingPush` in state `proposed` with `approval_id` set; the card reads question "Send 2 changes back to SharePoint?", verb "Send back", where "Supplier onboarding in SharePoint", affects the two files with their versions.

## Internal Logic

**A. Propose (documents).**
1. Validate the 2C `PendingPush` rules; a working copy item already in an open push (`proposed`, `held`, `approved`) → `PUSH_ITEM_ALREADY_PENDING`.
2. For each item, `working_copy.entry(id)`; `item.state` must be `changed` → else `PUSH_ITEM_NOT_CHANGED`. A `StoreFailure` → `PUSH_STORE_FAILED` (retryable when the failure is).
3. Origin check (A-25) per item, by `link_kind` (§A3a); at most 4 at once. A version different from `item.origin_version_read` → `working_copy.set_state(id, OriginChanged, None)`, the item is left out of the push and reported in the result's `PushNotice::PushFinished.origin_changed` (03 *Ordin has documents changed*: "the rest of the push continues"). An unreachable origin → the item stays in and is re-checked before its send (§C1).
   a. Reading the origin version:
      - `SharepointLocation`: `sharepoint.resolve_origin(origin_location)` → link root; `sharepoint.item_at(&root, &relative_path)` → `SharePointItem`; its `version.tag` (the cTag; `None` → the item is gone, `PUSH_ORIGIN_NOT_FOUND`).
      - `ConfluenceLocation`: `confluence.item_for_path(item.source_link_id, &item.origin_path)` → `RemoteItemRef`; `confluence.item_version(&ref)?.tag` (the version number read). With `confluence = None` → `PUSH_CONFLUENCE_UNAVAILABLE`.
      - `GitRepositoryFolder`: `git_branch` absent → `PUSH_GIT_BRANCH_UNKNOWN`; `projects.remote_state(place_id, git_branch)?.commit`.
      - `LocalFolder`: no check (the folder is the origin).
4. No item left → `PUSH_ALL_ORIGINS_CHANGED`.
5. For items that are Markdown originals with published copies (`files.published_copies_of(&CopyOriginal::PlaceFile { place_id, path: relative_path })`), add each copy to the card's affects ("Also updates: [copy name] in [destination]") — ADR-069: the card updates the copies with the original.
6. Build the `ApprovalRequest`: `asked_by` = `Run { run_id }` when given, else `Person`; question "Send [n] change(s) back to [origin display name]?" (several origins: "Send [n] changes back to where they came from?"), verb "Send back", `where_line` the origins (**PS-CARD-1**, *amended January 2026*: each distinct "SharePoint — <link display name>", "Confluence — <link display name>", or the display name alone for a folder on the computer, joined with ", "), `affects` one `File` per item (label = the document's own name, PS-CARD-1: the last part of its origin path without a Confluence page's `.confluence.xhtml`, or the link's display name when the path is empty; version = local version as text), `changes` the summaries joined by "; " cut at 1,000 chars, `content` = `Changes { label: "See the changes", change_set: Artefact { artefact_id: change_view } }` when one item has a change view, else `Files { label: "See the files", files: [PlaceFile { place_id, relative_path }, …] }`; `show_details` = true; `reject_style` = `Reject`; `proposal` = `ActionProposal { action: "Send changed documents back to where they came from", destination: where_line, affects, consequence: changes }`; `asker` = None; `leaves_company` = false. *(PS-CARD-1 found in the Phase 5 walk on Windows: the card read "Where: Procurement" and "Affects: Procurement · version 3" for the page Rates — the link's name twice, and neither the service nor the document.)* Texts come from Fluent messages `push-card-question-one`, `push-card-question-many`, `push-card-verb` (*chosen here*), formatted by the `ordin_localisation::Localisation` the program passes in `PushDeps.localisation`; the copy-header and audience texts of §E1 use the same localiser, so no card or header text is literal in this crate (the en-GB text is in the `push.ftl` catalogue).
7. In one transaction: insert `pending_pushes` (`proposed`) and `pending_push_items` (with `ordins_local_version` = `item.local_version`); then `working_copy.set_state(id, AwaitingPush, None)` per item. Then `approvals.request(…)` → `card.approval_id`; store it. An `ApprovalError` → set each item back to `Changed`, delete the rows → `PUSH_APPROVAL_FAILED` (retryable).
8. Log `push.proposed` {pending_push_id, workspace_id, items, origins}.

**B. Decision loop.** The sink from `event_sink()` receives every `JournalEvent` in order; it forwards events with `kind = "approval_decided"` to an internal queue (capacity 1,024) and returns, so the Journal's delivery is never held. `lagged(next_seq)` re-reads with `journal.events_since(next_seq - 1, 1000)` until it reaches the head. For each decision whose `data.approval_id` matches a stored push, submission or copy operation:
1. `approved` → push: `state = approved` (requires `approval_id`, `PUSH_APPROVAL_MISSING`); run §C for every item.
2. `rejected` → `state = rejected` then `closed`; each item `working_copy.set_state(id, Changed, None)` ("changes stay in the working copy"; WorkspaceService.md 6.3); notice `PushFinished` with nothing pushed.
3. `expired` (its run ended first, or the card was withdrawn by §D) → as `rejected`, except for a push in `held`, which stays `held` (*assumed here: 04 Approval lifecycle has no other outcome for the push*).
4. A decision for an id this component does not hold is ignored.

**C. Send one item** (after approval).
1. Re-check the origin version (§A3a). Changed → `OriginChanged`, nothing recorded or sent, next item.
2. *(Amended 2026-01-10.)* Confirm the version approved, before step 1's origin read: the item is sent only if `entry.item.local_version == item.ordins_local_version` (the version the card named, §A7). Otherwise it is not sent — nothing is recorded or sent, the outcome is a failed item with `PUSH_APPROVED_VERSION_CHANGED`, the item is set back with `working_copy.set_state(id, Changed, None)` so that it can be proposed again, and the person is told the document changed after approval and a new approval is needed. **Try again** sends through this same step. Then read the content of the approved version with `working_copy.content(id, Some(ordins_local_version))`, so that an edit made between the check and the read is not sent. For Git, the file on disk at `relative_path` is confirmed to be that version before the commit (step 4): its bytes must equal `working_copy.content(id, Some(ordins_local_version))`, else nothing is committed or sent and the item fails with `PUSH_APPROVED_VERSION_CHANGED` as above.
3. Mint `key` (UUIDv7) and store it on `pending_push_items.pending_action_key` before the send.
4. Send (`run_id` = the push's `run_id`):
   - SharePoint: `sharepoint.upload_version(&item_ref, bytes, expected_version = origin_version_read, ExternalWrite { key, run_id, kind: OriginPush })`. The adapter begins and confirms the external action (SA-RECON-03).
   - Confluence page (`RemoteItemRef::Page`, a `.confluence.xhtml` working-copy file): `confluence.update_page_storage(page_id, xhtml, expected_version = origin_version_read, key, run_id)`; attachment: `confluence.attach(page_id, file_name, bytes, Some(origin_version_read), key, run_id)`. The version passed is the one read, unchanged: the Confluence Adapter is the one place that adds 1 (ruling R19). The adapter begins and confirms the external action. With `confluence = None` → `PUSH_CONFLUENCE_UNAVAILABLE` for that item.
   - Git: `journal.begin_external_action(NewExternalAction { action_key: key, run_id, kind: OriginPush, destination: "<git_host>/<git_repository>#<git_branch>" })` (`EXT_KEY_DUPLICATE` → nothing sent, item `PUSH_OUTCOME_UNKNOWN`); then in the folder at `files.place(place_id)?.path`, with `git2`: verify the checked-out branch is `git_branch`; add exactly `relative_path` to the index; commit with the repository's configured `user.name` / `user.email` (missing → `PUSH_GIT_IDENTITY_MISSING`, no action), message "[summary]" plus a blank line and "Sent from ordin after approval"; push `refs/heads/<git_branch>` without force, with credentials from `github.git_credentials(git_host)` (username, `token.expose()`). A non-fast-forward rejection → `OriginChanged` (03: "nothing is forced"); the local commit is kept and the person sees both versions. On success `journal.confirm_external_action(key)`; the new origin version is the pushed commit id.
   - Local folder: no push — the working copy is the origin; set `Pushed` directly with origin version `fver:<local_version>`.
5. Success: `working_copy.set_state(id, Pushed, Some(new_origin_version))` (SharePoint `SharePointVersion.tag`, Confluence `RemoteVersion.tag`, Git commit); set `external_action_key` = `key`; `files.mark_edit_sent(place_id, relative_path)` (clears a held person's edit, ADR-048); if the item is a Markdown original with copies, run §E3 for its copies with the new version.
6. Failure, by the adapter's or provider's `code()`:

   | Code | Item outcome |
   |---|---|
   | `SP_ORIGIN_CHANGED`, `CONF_ORIGIN_CHANGED`, git non-fast-forward | `OriginChanged` (`PUSH_ORIGIN_CHANGED`) |
   | `SP_SIGN_IN_NEEDED`, `CONF_SIGN_IN_NEEDED`, `CONF_SITE_NOT_GRANTED`, `GH_SIGN_IN_NEEDED`, `PROJ_SIGN_IN_NEEDED` | stays `awaiting_push`, `PUSH_SIGN_IN_NEEDED`, action `sign_in_again` |
   | `SP_ACCESS_DENIED`, `CONF_ACCESS_DENIED`, git push 403 | `PUSH_FORBIDDEN` |
   | `SP_NOT_FOUND`, `CONF_NOT_FOUND` | `PUSH_ORIGIN_NOT_FOUND` |
   | `SP_TOO_LARGE`, `CONF_TOO_LARGE` | `PUSH_TOO_LARGE` |
   | `SP_THROTTLED`, `SP_UNAVAILABLE`, `CONF_THROTTLED`, `CONF_UNAVAILABLE`, `GH_UNREACHABLE`, `GH_RATE_LIMITED`, `PROJ_REMOTE_UNREACHABLE`, `PROJ_RATE_LIMITED`, git network error | `PUSH_ORIGIN_UNREACHABLE` (retryable) |
   | `SP_OUTCOME_UNKNOWN`, `CONF_OUTCOME_UNKNOWN`, `SP_ACTION_KEY_USED`, `CONF_ACTION_KEY_USED`, `EXT_KEY_DUPLICATE`, git timeout after the push was sent | stays `awaiting_push`, `PUSH_OUTCOME_UNKNOWN`; nothing is resent automatically (02:747) |
   | any other code | `PUSH_DESTINATION_REFUSED`, no action |

   The other items continue (03 error posture). The code is stored in `pending_push_items.result_code`. **PS-REF-1** *(amended January 2026)*: for every code except `PUSH_OUTCOME_UNKNOWN` the destination's answer is known and nothing arrived, so when an external action was begun under the item's `pending_action_key` it is settled with `journal.refuse_external_action(key)` (JournalAndArtefactStore.md JA-EXT-1; no row → nothing to settle; another failure is logged `push.refuse_failed` and changes nothing else); recovery (§G.1) refuses such a row left `unknown_outcome` from an earlier start. *(Found in the Phase 5 walk on Windows: a Confluence send refused with 503 later showed in Needs you as not known whether it arrived.)* A Git push begun in the Journal and refused by the remote is refused the same way (PS-REF-1); recovery (§G) skips, and refuses, actions whose item carries a `result_code` other than `PUSH_OUTCOME_UNKNOWN`, because the destination's answer is known.
7. When every item is `pushed`, `origin_changed` or failed-and-reported, `state = closed`; notice `PushFinished`. Log `push.item_sent` {pending_push_id, item, origin_kind, result, duration_ms} per item.

**D. Local edit while a push waits (ADR-035).** On an id from `working_copy.local_changes()` for an item in a `proposed` push: `approvals.withdraw(approval_id)`; `state = held`; notice `PushHeld`. `choose_version`:
- `keep_mine`: drop the item from the push and set it `Changed` (the person's version stays; Ordin may redo its change later);
- `keep_ordins`: `working_copy.restore_version(id, ordins_local_version)`;
- `apply_ordins_to_mine`: returns the push in `held` with notice for the Ordin Host to re-run the change; the Ordin Host calls `propose` again, which supersedes this push (`closed`).
When the new card is built, each remaining item's `ordins_local_version` is set to the item's `local_version` now (the version a restore or the person's edit produced), so the card's approval counts for the version now on the card and §C2's `PUSH_APPROVED_VERSION_CHANGED` check does not block it; this applies after `keep_ordins` and after an edit made while the push was held. Then, if items remain, a new approval request (§A6) and `state = proposed`; none remain → `closed`. `choose_version` on a push not `held` → `PUSH_NOT_HELD`.

**E. Published copies (ADR-069).**
1. `publish_copy`: validate the 2C `PUB_*` format rules; `format ∈ {word, pdf, slides}` and `!craft.conversion_available(format)` → `PUSH_FORMAT_NOT_READY` (C-28 fallback: only a Confluence page); `CopyOriginal::KnowledgePage` without `page_file` → `PUSH_REQUEST_INVALID`. Destination checks: SharePoint `sharepoint.exists(&folder_loc, file_name)`; a name in use and `file_name` unchanged → the card's `changes` says it will be replaced (the screen offers **Use another name** before the call). Audience from the adapter (`sharepoint.audience(&folder_loc)` or `confluence.audience(&parent_loc)`, locations per *Configuration*) — **conditional on C-42**: when `everyone_who_can_open` is true, the card names the site or space and says "everyone who can open it" (Fluent `push-copy-audience-unknown`), and later automatic updates cannot detect growth. Store the audience in `copy_audiences` (`people`, `principals_digest` = SHA-256 of the sorted `principals` joined by "\n", plus "\n" and the SharePoint `link_scope` when present). Card: question "Publish a copy of [original]?", verb "Publish", where = destination, affects = the copy, changes = format and maker, followed branch, keep-up-to-date choice, `asked_by` = `Person`, `proposal` built from the same lines, `show_details` = false. `files.record_published_copy(PublishedCopy { state: awaiting_approval, .. })`; the submission in `copy_operations` (*chosen here*).
2. On approval, read the original at `original_version` (§E6) and make the copy: `craft.make_copy(MakeCopy { original_name, original_from, branch: followed_branch, markdown, format, updated_at: now })` for Word, PDF and slides (the Craft Toolkit writes the header line); for a Confluence page the Markdown goes to `confluence.publish_page(&destination, title, markdown, Some(header_line), key, None)`; `exact_file` sends the bytes with `confluence.attach(parent_page_id, file_name, bytes, None, key, None)`. SharePoint: `sharepoint.create_file(&folder_loc, file_name, bytes, ExternalWrite { key, run_id: None, kind: CopyPublish })`. The adapter begins and confirms the external action with the minted `key`. Save `state = current`, `original_version_published`, `destination_version_pushed` (`RemoteVersion.tag`), `destination_item_id` (page id, attachment id or SharePoint item id) with `files.record_published_copy`. A repository original with `keep_up_to_date` → `projects.follow_original(place_id, path, followed_branch)`. A maker failure → `set_published_copy_state(id, Failed, Some(<one sentence>))`, `PUSH_COPY_MAKER_FAILED`; `SP_NAME_EXISTS` or `CONF_TITLE_EXISTS` → `PUSH_NAME_EXISTS` (the screen offers **Use another name**); the other copies continue.
3. Original changed and reached its origin: `projects.subscribe_original_changes()` (a `RepositoryOriginChange` on the `followed_branch` of a followed path; version = its `commit`); `files.subscribe_changes()` for a `PlaceChange` with `by_ordin = false` and a `version` on a place that is not a repository place (version = that number); the 15-minute comparison for SharePoint and Confluence originals. For each copy of that original (`files.published_copies_of`) with `keep_up_to_date` and state `current` or `out_of_date`: read the destination version (`sharepoint.item_version` or `confluence.item_version`) — different from `destination_version_pushed` → `changed_at_destination` (updates stop); read the audience — `people` grew or `principals_digest` changed (never when `everyone_who_can_open`) → hold the copy and raise a new card naming the audience change (`PUSH_AUDIENCE_GREW`, `copy_operations.kind = audience_hold`); else remake from the original at that version (§E6; never from the working copy), mint a key and upload a new version (`sharepoint.upload_version(.., expected_version = destination_version_pushed, ExternalWrite { kind: CopyPublish, .. })`, `confluence.update_page(page_id, title, markdown, Some(header_line), expected_version = destination_version_pushed, key, None)` or `confluence.attach(.., Some(destination_version_pushed), ..)`), save `current`. Copies with `keep_up_to_date = false` → `out_of_date`. Notice `CopiesUpdated` (Home: "3 copies of rates.md updated from GitHub"). A move on a branch other than `followed_branch` changes nothing.
4. `copy_action`: `try_again` (`failed` → §E3 for that copy); `bring_it_in` (`changed_at_destination`, page or Word only: page → `confluence.fetch(&Page { page_id })` then `ConfluenceAdapter::storage_to_markdown`; Word → `sharepoint.fetch(&item_ref)` then `craft.to_markdown(Word, bytes)`; the Markdown is saved as the person's edit of the original with `files.save_person_edit(place_id, path, bytes)` (for a knowledge page, its `page_file`); PDF or slides → `PUSH_COPY_CANNOT_BRING_BACK`); `leave_it` and `replace_at_next_update` → `current` with `destination_version_pushed` set to the destination's version, so the next update replaces it; `stop_keeping_up_to_date` → `not_kept_up_to_date`, and the last kept copy of a repository original → `projects.unfollow_original`; `keep_up_to_date` → `out_of_date` and `projects.follow_original` for a repository original; `remove` → the person's check is the screen's; mint a key and delete at the destination with `sharepoint.delete(&item_ref, Some(destination_version_pushed), ExternalWrite { kind: CopyPublish, .. })` or `confluence.delete(&ref, Some(destination_version_pushed), key, None)` (`*_ORIGIN_CHANGED` → `changed_at_destination`, nothing removed); then `removed`. An action not allowed from the copy's state → `PUSH_COPY_STATE`.
5. `update_copies` (not kept up to date): one card for every out-of-date copy of the original; on approval as §E3 without the audience hold.
6. Reading an original at a version: `PlaceFile` in a repository place (`followed_branch` present) → `projects.read_at_commit(place_id, commit, path)`; other `PlaceFile` → the `FileVersion` with that number from `files.versions(place_id, path)`, then `files.read_version(id)`; `KnowledgePage` → as `PlaceFile` on its `page_file`. No such version → `PUSH_ORIGINAL_UNREADABLE` (the copy becomes `failed`).

**F. Pull requests.**
1. `propose_pull_request`: validate title, body, branch (git ref ≤ 255 bytes), labels ≤ 10; store the submission (`pull_request_submissions`, `proposed`); request the card (`asked_by = Person`; texts from `card`; `proposal` = `ActionProposal { action: "Open a pull request", destination: "<host>/<repository>", affects: card.affects, consequence: card.changes }`; `show_details` = `card.content.is_some()`); store `card.approval_id`; return the `SubmissionId`.
2. On approval (or at once in `send_approved_pull_request`, which first checks `approvals.get(id)?.state == Approved` → else `PUSH_APPROVAL_MISSING`): mint `key`; `journal.begin_external_action(NewExternalAction { action_key: key, run_id: None, kind: PullRequestOpened, destination: "<host>/<repository>#<branch>" })`; store the key.
   - `Worktree`: `git2` add exactly `paths`, commit ("[title]"), push `refs/heads/<branch>` to `origin` (non-force, credentials from `github.git_credentials(host)`; a rejection on an existing pull request's branch → `PUSH_BRANCH_MOVED`).
   - `Files`: `github.create_branch_with_files(BranchWithFiles { host, repository, base, branch, message: title, files })`.
   - No `existing`: `github.open_pull_request(OpenPullRequest { host, repository, head: branch, base, title, body, labels, reviewers })`; the 2C `PullRequestRef` it returns is stored as is. `existing`: no new pull request (ADR-084 *Send the change…*).
   - `journal.confirm_external_action(key)`; submission `sent` with the `PullRequestRef`; notice `PullRequestOpened`.
3. `GH_NOT_FOUND` or a 403 on push → `PUSH_NO_REVIEW_RIGHT` ("the draft stays theirs", ADR-084); `GH_SIGN_IN_NEEDED` → `PUSH_SIGN_IN_NEEDED`; `GH_UNREACHABLE`, `GH_RATE_LIMITED` → `PUSH_ORIGIN_UNREACHABLE`; `GH_HOST_NOT_SUPPORTED` or `GH_FAILED` → `PUSH_DESTINATION_REFUSED`; notice `PullRequestFailed`. Card rejected → notice `PullRequestRejected`.

**G. Recovery on start** (after the program's `Journal::recover_external_actions`, so every action left `sent` is now `unknown_outcome`).
1. `journal.unsettled_external_actions(&[OriginPush, CopyPublish, PullRequestOpened])`. For each key stored on a push item, submission or copy operation: re-read the destination — the version moved to one this item produced (SharePoint or Confluence version later than the one read and the content digest equal to the local content; Git: the remote branch, by `projects.remote_state`, contains the local commit) → mark the item `pushed` (or the copy `current`) locally and log `push.recovered_arrived`; the Journal's row stays `unknown_outcome` for the person's **It arrived — mark as done** (JournalAndArtefactStore.md 9.2, 9.4). Otherwise leave the item `awaiting_push` with **Try again** and log `push.unknown_outcome` — nothing is resent automatically (`EXT_AUTO_RESEND`; 03 *Recovery*). Keys this service does not hold are ignored.
2. Pushes in `proposed` or `held` keep waiting; their approvals survive restarts (04 Approval). **PS-REC-1** *(amended January 2026)*: a push in `proposed` whose card is no longer `pending` (`approvals.get(approval_id).state`: decided while this service's loops were not running, so its `approval_decided` event was never acted on) is decided now as §B does for that decision — `approved` sends it, `rejected` or `expired` sets its items back; log `push.recovered_decision {pending_push_id, state}`. A card that cannot be read is left for the next start. *(Found in the Phase 5 walk on Windows: a Send back pressed before the service's loops were started stayed `proposed` after a restart and was never sent.)*

```gherkin
  Scenario: Recovery carries out a card decided while the service was stopped (PS-REC-1)
    Given a push of Kestrel-MSA.docx was proposed and the service stopped
    When the person approves its card and the service starts again
    Then the item is sent and pushed
```
3. Pull-request submissions in `approved` without a `PullRequestRef`: `github.pull_request_for_branch(host, repository, branch)` → `Some(r)` → submission `sent` with `r`, confirm the action, notice `PullRequestOpened`; `None` → `failed` with **Try again** and notice `PullRequestFailed` with `PUSH_OUTCOME_UNKNOWN` (nothing is resent automatically; for `SubmissionOrigin::Handover` the Handover Service resubmits, HandoverService.md Internal Logic C.6).

**H. Needs-you events (reconciliation row 500).** After each change that adds or removes something waiting for the person in a workspace — a push enters or leaves `held` (§D), an item fails with a code whose action is `try_again` or `sign_in_again` (§C6), a copy enters or leaves `changed_at_destination` or `failed` (§E3, §E4) — `journal.append_event("needs_you_changed", json!({"workspace_id": id}))` once per affected workspace. Approval cards are announced by the Approval Service, not here. A failed append is logged `push.event_failed` and changes nothing else.

## Data Model

Module `push`, tables in the Journal's `Datastore`, migrations as `migrations() -> ModuleMigrations { module: "push", sqlite: &[(1, "push tables", <SQL below>), (2, "team knowledge shares", <the team-knowledge amendment's SQL>), (3, "copy requests, knowledge page files, pull request files", <step 3 SQL below>)], postgres: &[] }` (ruling R3). The component does not run in the agent profile, so the PostgreSQL slice is empty: no PostgreSQL step exists for any of the three steps.

```sql
-- MigrationStep 1
CREATE TABLE pending_pushes (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  run_id TEXT,                                             -- chosen here: the proposing run, if any
  request TEXT NOT NULL CHECK (length(request) BETWEEN 1 AND 4000),
  origins TEXT NOT NULL,                                   -- JSON Vec<SourceLinkId>
  approval_id TEXT,
  state TEXT NOT NULL CHECK (state IN ('proposed','held','approved','rejected','closed')),
  created_at TEXT NOT NULL,
  CHECK (state NOT IN ('approved') OR approval_id IS NOT NULL)
);
CREATE INDEX ix_pending_pushes_workspace ON pending_pushes(workspace_id, state);
CREATE UNIQUE INDEX ux_pending_pushes_approval ON pending_pushes(approval_id) WHERE approval_id IS NOT NULL;

CREATE TABLE pending_push_items (
  pending_push_id TEXT NOT NULL REFERENCES pending_pushes(id) ON DELETE CASCADE,
  working_copy_item_id TEXT NOT NULL,
  position INTEGER NOT NULL,
  summary TEXT NOT NULL CHECK (length(summary) BETWEEN 1 AND 120),
  change_view TEXT,                                        -- ArtefactId
  ordins_local_version INTEGER NOT NULL,                   -- chosen here (ADR-035 keep Ordin's)
  pending_action_key TEXT,                                 -- chosen here: key recorded before sending
  external_action_key TEXT,                                -- 2C: set only once pushed
  result_code TEXT,
  PRIMARY KEY (pending_push_id, working_copy_item_id)
);
CREATE INDEX ix_push_items_item ON pending_push_items(working_copy_item_id);

CREATE TABLE pull_request_submissions (                    -- chosen here
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  origin TEXT NOT NULL,                                    -- JSON SubmissionOrigin
  proposal TEXT NOT NULL,                                  -- JSON PullRequestProposal without file bytes
  approval_id TEXT,
  action_key TEXT,
  pull_request TEXT,                                       -- JSON PullRequestRef
  state TEXT NOT NULL CHECK (state IN ('proposed','approved','sent','rejected','failed')),
  failure_code TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);

CREATE TABLE copy_operations (                             -- chosen here: card and send bracket per copy
  id TEXT PRIMARY KEY,
  published_copy_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('first_publish','update','audience_hold','remove','update_copies')),
  approval_id TEXT,
  action_key TEXT,
  original_version TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('awaiting_approval','sending','done','failed','held')),
  created_at TEXT NOT NULL
);

CREATE TABLE copy_audiences (                              -- chosen here: the audience approved for a copy (§E1, §E3)
  published_copy_id TEXT PRIMARY KEY,
  line TEXT NOT NULL CHECK (length(line) BETWEEN 1 AND 500),
  people INTEGER,                                          -- NULL under the C-42 fallback
  everyone_who_can_open INTEGER NOT NULL CHECK (everyone_who_can_open IN (0,1)),
  principals_digest TEXT NOT NULL,                         -- 64 lowercase hex
  recorded_at TEXT NOT NULL
);
```

```sql
-- MigrationStep 3 (additive: three tables, no existing table changes)
CREATE TABLE copy_requests (                               -- chosen here: what a first publish knew in memory only
  published_copy_id TEXT PRIMARY KEY,
  workspace_id      TEXT NOT NULL,                         -- the copy's workspace
  original_name     TEXT NOT NULL,                         -- the original's display name
  file_name         TEXT NOT NULL                          -- the name chosen for the copy
);

CREATE TABLE knowledge_page_files (                        -- chosen here: the file a knowledge page's copy is made from (SA-CLOSE-01)
  knowledge_page_id TEXT PRIMARY KEY,
  place_id          TEXT NOT NULL,
  path              TEXT NOT NULL
);
CREATE INDEX ix_knowledge_page_files_file ON knowledge_page_files(place_id, path);

CREATE TABLE pull_request_files (                          -- chosen here: file bytes of a pull request until it is sent
  submission_id TEXT NOT NULL,
  position      INTEGER NOT NULL,
  path          TEXT NOT NULL,
  content       BLOB NOT NULL,
  PRIMARY KEY (submission_id, position)
);
```

`copy_requests` is written by `publish_copy` and read when a copy is updated or needs the person (workspace, names); a copy published before step 3 has no row, so its workspace is unknown and its names are derived from the original. `knowledge_page_files` is written by `publish_copy` for a `KnowledgePage` original (`page_file`), read to find the file of an update, and queried by `(place_id, path)` to find the knowledge pages whose copies a changed file affects (§E3). `pull_request_files` holds the bytes of `PullRequestContent::Files`, one row per file in order: `propose_pull_request` writes them (a failed write removes the submission), and they are deleted when the submission is sent, fails or its card is refused. After a restart, an approved submission reloads the bytes from this table and sends; when the table has no rows for it, the submission is `failed` with `PUSH_OUTCOME_UNKNOWN` (notice `PullRequestFailed`), and the Handover Service resubmits through a new approval card when the person presses **Try again** (HandoverService.md Internal Logic C.6). Rollback (sql-safety): additive only, three new tables and one index; no `DROP`, no `ALTER`, no backfill. Forward-only; the pre-migration backup copy of the SQLite file restores.

## Gaps

None.

## Amendment January 2026 — Team knowledge (ADR-103)

> Source: decision 83 (the user's decisions D1–D10 of January 2026); HLD revision 0.51.0, ADR-103 (amends ADR-030 and ADR-057; reuses ADR-066's sharing path). Counterpart: `components/TeamKnowledgeService.md` (§E, §G and its `TeamPush` trait).
> Every rule below adds to the sections above; nothing above changes. A program in which no workspace is in Team Knowledge mode never reaches any of it.

### Purpose (addition)

The Push Service also sends **team knowledge shares**: a commit the Team Knowledge Service built in its own bare clone of a team's repository, after one approval card listing what is shared. It pushes the commit to the linked branch, or — where the branch requires review — to a review branch and opens a pull request (GitHub) or merge request (GitLab) into the linked branch. It stays the one way a change leaves the computer (ADR-029, ADR-066).

### Approach (addition)

The commit arrives ready-made; this service never builds trees and never touches a working tree. It raises the card, brackets the send as an external action (`OriginPush` for a direct push, `PullRequestOpened` for a review), pushes without force from the bare clone with `git2` and the token from `GithubAccess::git_credentials`, and reports through `PushNotice`. When the branch moved after the card, it does not rebuild — it reports `TeamShareMoved` and waits for the Team Knowledge Service to rebuild on the new tip and call `resend_team_share`, sending under the same approval (the person approved the items, not a commit position). Rejected: *a new card after every move* — two people sharing at the same time would each be asked again for the same items; *force-pushing the original commit* — it would drop a colleague's share (03: "nothing is forced").

### Shared Context (additions)

```rust
// ordin-types (2C; added January 2026, ADR-103)
uuid_id!(TeamShareId);

// Owned by the Team Knowledge Service (TeamKnowledgeService.md §Shared Context 2), copied:
pub struct TeamShareProposal {
    pub share_id: TeamShareId, pub workspace_id: WorkspaceId,
    pub clone_path: std::path::PathBuf, pub commit: String,          // 40 lowercase hex
    pub host: String, pub repository: String, pub branch: String,
    /// Some = the branch requires review: push here and open a pull or merge request into `branch`.
    pub review_branch: Option<String>,
    pub title: String /* 1–256 chars */, pub body: String /* ≤ 65,536 chars */,
    pub card: CardText,
}
```

`CardText`, `ApprovalRequest`, `ActionProposal`, `AffectedItem`, `GithubAccess` (`git_credentials`, `open_pull_request`, `pull_request_for_branch`), `OpenPullRequest`, `PullRequestRef`, `NewExternalAction` and `ExternalActionKind::{OriginPush, PullRequestOpened}` are those of *Shared Context* and *Boundary interfaces* above, unchanged.

### Public Interface (additions)

```rust
impl PushService {
    // Team knowledge shares (ADR-103)
    /// Stores the submission and raises the card; returns the card's approval id.
    pub async fn propose_team_share(&self, p: TeamShareProposal) -> Result<ApprovalId, PushError>;
    /// After `TeamShareMoved`: send `commit` (rebuilt on the new tip) under the same approval.
    pub async fn resend_team_share(&self, share_id: TeamShareId, commit: String) -> Result<(), PushError>;
    /// Before the decision only: closes the card as `expired`; the submission becomes `withdrawn`.
    pub async fn withdraw_team_share(&self, share_id: TeamShareId) -> Result<(), PushError>;
}

// PushNotice gains (additive variants):
//   TeamShareSent { share_id: TeamShareId, commit: String, pull_request: Option<PullRequestRef> },
//   TeamShareMoved { share_id: TeamShareId },
//   TeamShareFailed { share_id: TeamShareId, code: &'static str },
//   TeamShareRejected { share_id: TeamShareId },
```

**Called by:** `propose_team_share`, `resend_team_share`, `withdraw_team_share` and the four notices — Team Knowledge Service (its `TeamPush`, implemented by the desktop and personal-server programs over this service). No Service API command is added: the screens reach a share through `team_knowledge.share`.

#### Example

```json
{"share_id":"0192a4c0-0000-7000-8000-00000000b001","workspace_id":"0192a0f1-1111-7222-8333-444455556666",
 "clone_path":"/Users/asha/Library/Application Support/ordin/team-knowledge/0192a0f1-1111-7222-8333-444455556666.git",
 "commit":"3f9a1c0d2e4b5a6978c1d2e3f4a5b6c7d8e9f0a1","host":"github.com","repository":"acme/supplier-team","branch":"main",
 "review_branch":null,"title":"Team knowledge from Asha Patel: 12 new facts, 2 changed pages, 1 page removed","body":"Shared from ordin after approval",
 "card":{"question":"Share what you've learned with acme/supplier-team?","verb":"Share","where_line":"acme/supplier-team › knowledge",
         "affects":[{"kind":"record","label":"12 new facts"},{"kind":"record","label":"2 changed pages: Supplier rates, Board pack"},{"kind":"record","label":"1 page removed"}],
         "changes":"12 new facts, 2 changed pages, 1 page removed. What you share stays in the repository's history, even after it is deleted.","content":null}}
```

Result: an `ApprovalId`; on approval the branch `main` gains commit `3f9a1c0…` and the notice `TeamShareSent { pull_request: None }` follows.

### Internal Logic (additions)

**I. Team knowledge shares.**

1. `propose_team_share(p)`: validate — `title` 1–256 chars, `body` ≤ 65,536 chars, `branch` and `review_branch` valid git refs ≤ 255 bytes, `commit` 40 lowercase hex characters, `card` within the 2C card limits → else `PUSH_REQUEST_INVALID`; a submission with this `share_id` already stored → `PUSH_TEAM_SHARE_STATE`. Open the bare clone at `clone_path` with `git2::Repository::open_bare`; `commit` not found in it → `PUSH_TEAM_COMMIT_MISSING`. Insert `team_share_submissions` (`proposed`). Request the card: `ApprovalRequest { workspace_id, asked_by: Person, question, verb, where_line, affects, changes, content: None, reject_style: Reject, show_details: false, proposal: ActionProposal { action: "Share team knowledge", destination: "<host>/<repository>", affects: card.affects, consequence: card.changes }, asker: None, leaves_company: false }`; store `approval_id`. An `ApprovalError` → delete the row, `PUSH_APPROVAL_FAILED` (retryable). Log `push.team_share { share_id, step: "proposed" }`.
2. Decision (§B): a decision whose `approval_id` matches a team-share submission — `approved` → state `approved`, then §I3; `rejected` or `expired` → state `rejected`, notice `TeamShareRejected`.
3. **Send** (state `approved`, or after `resend_team_share`): state `sending`; mint `key` (UUIDv7) and store it in `action_key` before anything leaves.
   - Direct (`review_branch = None`): `journal.begin_external_action(NewExternalAction { action_key: key, run_id: None, kind: OriginPush, destination: "<host>/<repository>#<branch>" })` (`EXT_KEY_DUPLICATE` → `PUSH_OUTCOME_UNKNOWN`, nothing sent); in the bare clone, push refspec `<commit>:refs/heads/<branch>` to `origin` without force, credentials from `github.git_credentials(host)` (username, `token.expose()` inside the callback only). Accepted → `journal.confirm_external_action(key)`, state `sent`, notice `TeamShareSent { share_id, commit, pull_request: None }`.
   - Review (`review_branch = Some(rb)`): `begin_external_action` with kind `PullRequestOpened`, destination `"<host>/<repository>#<rb>"`; push `<commit>:refs/heads/<rb>` without force; then `github.open_pull_request(OpenPullRequest { host, repository, head: rb, base: branch, title, body, labels: [], reviewers: [] })`; `confirm_external_action(key)`; store the `PullRequestRef`; state `sent`; notice `TeamShareSent { share_id, commit, pull_request: Some(r) }`.
4. **Branch moved.** A direct push refused as non-fast-forward sent nothing: the refused attempt's key is appended to `refused_keys` (JSON), its Journal row stays `sent` and becomes `unknown_outcome` at the next start like every refused Git push of §C6, and recovery (§I7) skips keys listed in `refused_keys` because the answer is known. `moves += 1`; state back to `approved`; notice `TeamShareMoved { share_id }`. The service waits; it never retries by itself.
5. `resend_team_share(share_id, commit)`: unknown `share_id` → `PUSH_TEAM_SHARE_UNKNOWN`; state not `approved` after a move → `PUSH_TEAM_SHARE_STATE`; `moves ≥ 3` → `PUSH_TEAM_SHARE_STATE` (the Team Knowledge Service fails the share with `TEAM_SHARE_RACE` before this happens); `commit` not in the clone → `PUSH_TEAM_COMMIT_MISSING`. Store `commit_id = commit`, then §I3 under the same approval (`approvals.get(approval_id)?.state == Approved`, else `PUSH_APPROVAL_MISSING`).
6. **Failures** of §I3, by code, using §C6 and §F3: `GH_SIGN_IN_NEEDED`, `PROJ_SIGN_IN_NEEDED`, git authentication failure → `PUSH_SIGN_IN_NEEDED`; git push 403 (direct) → `PUSH_FORBIDDEN`; 403 on the review branch push or `GH_NOT_FOUND` from `open_pull_request` → `PUSH_NO_REVIEW_RIGHT`; `GH_UNREACHABLE`, `GH_RATE_LIMITED`, git network error → `PUSH_ORIGIN_UNREACHABLE`; git timeout after the push left, `EXT_KEY_DUPLICATE` → `PUSH_OUTCOME_UNKNOWN` (nothing resent automatically); a review branch that already exists and is not an ancestor of `commit` → `PUSH_BRANCH_MOVED`; any other → `PUSH_DESTINATION_REFUSED`. State `failed`, `result_code` stored, notice `TeamShareFailed { share_id, code }`. A code whose action is `try_again` or `sign_in_again` → `journal.append_event("needs_you_changed", {"workspace_id": id})` (§H).
7. **Recovery** (§G, same pass): `unsettled_external_actions(&[OriginPush, PullRequestOpened])` also returns team-share keys. For a key held in a submission's `action_key` and not in its `refused_keys`: fetch `refs/heads/<branch>` (or `<review_branch>`) into the bare clone, then `git2::Repository::graph_descendant_of(tip, commit)` or `tip == commit` → the push arrived: for a review, `github.pull_request_for_branch(host, repository, review_branch)` → `Some(r)` → `TeamShareSent { pull_request: Some(r) }`, `None` → failed as below; direct → `TeamShareSent { pull_request: None }`; state `sent`; the Journal row stays `unknown_outcome` for the person's **It arrived — mark as done**, as §G1. Not arrived, or the fetch failed → state `failed`, notice `TeamShareFailed { share_id, code: "PUSH_OUTCOME_UNKNOWN" }`; nothing is resent automatically (`EXT_AUTO_RESEND`). Submissions in `proposed` keep waiting for their card.
8. `withdraw_team_share(share_id)`: unknown → `PUSH_TEAM_SHARE_UNKNOWN`; state not `proposed` → `PUSH_TEAM_SHARE_STATE`; `approvals.withdraw(approval_id)`; state `withdrawn`. The `approval_decided` event with `expired` that follows is ignored for a `withdrawn` submission (no `TeamShareRejected`).

### Data Model (additive migration)

```sql
-- migrations(): module `push`, step (2, "team knowledge shares", <this SQL>)
CREATE TABLE team_share_submissions (                       -- chosen here (ADR-103)
  share_id      TEXT PRIMARY KEY,                           -- TeamShareId
  workspace_id  TEXT NOT NULL,
  approval_id   TEXT,
  state         TEXT NOT NULL CHECK (state IN ('proposed','approved','sending','sent','failed','rejected','withdrawn')),
  clone_path    TEXT NOT NULL,
  commit_id     TEXT NOT NULL CHECK (length(commit_id) = 40),
  host          TEXT NOT NULL,
  repository    TEXT NOT NULL,
  branch        TEXT NOT NULL CHECK (length(CAST(branch AS BLOB)) BETWEEN 1 AND 255),
  review_branch TEXT CHECK (review_branch IS NULL OR length(CAST(review_branch AS BLOB)) BETWEEN 1 AND 255),
  title         TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 256),
  body          TEXT NOT NULL,
  pull_request  TEXT,                                       -- JSON PullRequestRef
  action_key    TEXT,
  refused_keys  TEXT NOT NULL DEFAULT '[]',                 -- JSON Vec<ExternalActionKey> of non-fast-forward attempts
  moves         INTEGER NOT NULL DEFAULT 0 CHECK (moves BETWEEN 0 AND 3),
  result_code   TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  CHECK (state NOT IN ('approved','sending','sent') OR approval_id IS NOT NULL)
);
CREATE UNIQUE INDEX ux_team_share_approval ON team_share_submissions(approval_id) WHERE approval_id IS NOT NULL;
CREATE INDEX ix_team_share_open ON team_share_submissions(state) WHERE state IN ('proposed','approved','sending');
```

Rollback (sql-safety): additive only — one new table and two indexes; no `DROP`, no `ALTER`, no backfill. Forward-only per SA-XC-07; the pre-migration backup copy of the SQLite file restores.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `resend_team_share` or `withdraw_team_share` names a share this service does not hold | `Err(permanent)` | `PUSH_TEAM_SHARE_UNKNOWN` | in-process (Team Knowledge Service) |
| A team share is proposed twice, or resent or withdrawn from a state that does not allow it (including a fourth move) | `Err(permanent)` | `PUSH_TEAM_SHARE_STATE` | in-process |
| The commit to send is not in the bare clone | `Err(permanent)` | `PUSH_TEAM_COMMIT_MISSING` | in-process |

Team-share send outcomes reuse `PUSH_SIGN_IN_NEEDED`, `PUSH_FORBIDDEN`, `PUSH_NO_REVIEW_RIGHT`, `PUSH_ORIGIN_UNREACHABLE`, `PUSH_OUTCOME_UNKNOWN`, `PUSH_BRANCH_MOVED`, `PUSH_DESTINATION_REFUSED`, `PUSH_REQUEST_INVALID`, `PUSH_APPROVAL_FAILED` and `PUSH_APPROVAL_MISSING` from the Error Table above, carried to the Team Knowledge Service in `TeamShareFailed.code` or `PushError`.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Push Service — team knowledge shares

  Background:
    Given a bare clone of "acme/supplier-team" holding commit C whose parent is the head of "main"
    And a test Git host where the person has write access

  Scenario: Happy path — a direct push after the card
    When the Team Knowledge Service proposes share S with commit C and no review branch
    And the person approves the card
    Then an OriginPush external action is begun before the push
    And "main" on the host points at C, pushed without force
    And the notice TeamShareSent for S carries commit C and no pull request

  Scenario: Review — the branch requires review
    Given the proposal names review branch "ordin/team-knowledge/1a2b3c4d"
    When the person approves the card
    Then C is pushed to "ordin/team-knowledge/1a2b3c4d"
    And one pull request into "main" is opened and recorded as PullRequestOpened
    And TeamShareSent for S carries that pull request

  Scenario: Edge — the branch moved after the card, then resent
    Given a colleague pushed to "main" after the card was raised
    When the person approves and the push is refused as non-fast-forward
    Then the notice TeamShareMoved for S is sent and moves is 1
    And the refused attempt's key is in refused_keys
    When resend_team_share(S, C2) is called with a commit C2 built on the new head
    Then C2 is pushed under the same approval without a new card and TeamShareSent carries C2

  Scenario: Edge — a share withdrawn before its decision
    Given share S in state proposed
    When withdraw_team_share(S) is called
    Then the card closes as expired, S is withdrawn and no TeamShareRejected notice is sent

  Scenario: Error — resend from a state that does not allow it
    Given share S in state sent
    When resend_team_share(S, C2) is called
    Then it fails with PUSH_TEAM_SHARE_STATE

  Scenario: Error — the commit is not in the clone
    When propose_team_share names a commit absent from the bare clone
    Then it fails with PUSH_TEAM_COMMIT_MISSING and no card is raised

  Scenario: Recovery — a push whose answer was lost
    Given share S in state sending with key K begun and ordin stopped before the answer
    When ordin starts and the fetched "main" contains C
    Then TeamShareSent for S is sent and K stays unknown_outcome for the person
    But when "main" does not contain C, TeamShareFailed carries PUSH_OUTCOME_UNKNOWN and nothing is pushed again
```

### Performance, Security, Observability (additions)

- **Performance:** a team-share push of one commit ≤ 15 s p95 (the Git target above); `propose_team_share` returns ≤ 1 s p95 excluding the Approval Service.
- **Security:** nothing leaves before the person approves the card (ADR-029); a resend after a move carries only items of the approved share (the Team Knowledge Service drops additions, TeamKnowledgeService.md §G3); no force push; tokens only inside the credentials callback; the card text comes from the Team Knowledge Service's fixed messages and contains no Git words.
- **Observability (logs only):** `push.team_share { share_id, step, result, moves }` with `step` ∈ `proposed`, `decided`, `sent`, `moved`, `failed`, `withdrawn`, `recovered`. Never logged: repository names beyond host, titles, tokens.

### Gaps

None.

---

## Amendment 2026-01-10 — A linked folder's copies are not updated automatically

> Status: approved by the user on 2026-01-10. Counterparts: `components/ConfluenceAdapter.md`, amendment 2026-01-10 (SA-CONF-AUTO-01 to 05) and steps 5.2.2 and 6; `components/WorkspaceService.md`, amendment 2026-01-10 (SA-WS-AUTO-01); `cross-cutting/ErrorHandling.md` (`PUSH_COPY_LINKED`).
> Every rule below adds to §E3, §E4 and §E5.

### Purpose (addition)

A copy that a linked folder made (`published_via: ConfluenceLink`) is sent to Confluence by the Confluence Adapter only: automatically by its sync when automatic publishing is on and the change is the person's own (ConfluenceAdapter.md step 5.2.2), and otherwise through **Publish checked items…** (SA-CONF-AUTO-03). Before this amendment Internal Logic 3 of §E also updated such a copy when a `PlaceChange` with `by_ordin = false` reached its original, so a person's later save carried an agent's change out with no card (against ADR-029), and an edit by a process an agent's shell left running was published while automatic publishing was off.

The same holds for the person's actions on such a copy. **Try again** (§E4) sent the file's bytes as they stood when pressed, with no content hash bound to a card and no test for an agent's change, and a linked copy reaches `failed` whenever the Confluence Adapter's push of it fails (ConfluenceAdapter.md step 7.6 and SA-CONF-AUTO-04). The card of **Update copies** (§E5) binds no content either.

### Approach (addition)

The automatic update of §E3 skips linked-folder copies, `copy_action` refuses the one action that sends the original's bytes, and `update_copies` leaves linked-folder copies off its card. A failed linked copy goes back to the folder's **Ready to publish** list, where a new card binds its bytes (ConfluenceAdapter.md SA-CONF-AUTO-05). Rejected: *checking the link's automatic publishing here* — this service would read grants and rule sets the Workspace Service already reads, and the linked-folder state would have two writers; *calling the Confluence Adapter's card from **Try again*** — the Ready list already offers that card, and a second route to it would need its own hash binding.

Assumptions (*chosen here*):

- **SA-PUSH-LINKED-01** Every automatic update of §E3 — a `PlaceChange` with `by_ordin = false` (Internal Logic 3), a `RepositoryOriginChange`, the 15-minute comparison and the refresh after an approved push (§C5) — leaves out each copy whose `published_via` is `ConfluenceLink`: its state, versions and destination are unchanged, it is not counted in `CopiesUpdated`, and no request is sent. `copy_action` follows SA-PUSH-LINKED-02 and `update_copies` follows SA-PUSH-LINKED-03.
- **SA-PUSH-LINKED-02** `copy_action` with `try_again` on a copy whose `published_via` is `ConfluenceLink` returns `PUSH_COPY_LINKED` before the original or the destination is read, whether the copy's first publish or a later update failed: its state, versions, failure reason and destination are unchanged and no request is sent. `try_again` is the one `copy_action` that sends the original's bytes. `bring_it_in`, `leave_it`, `replace_at_next_update`, `stop_keeping_up_to_date`, `keep_up_to_date` and `remove` send none of them and are unchanged for such a copy; `remove` stays the one way to delete its page at the destination (ADR-069).
- **SA-PUSH-LINKED-03** `update_copies` leaves out each copy whose `published_via` is `ConfluenceLink`: it is not named on the card and no copy operation is stored for it. When every out-of-date copy of the original that is not kept up to date is such a copy, the call returns `None` and no card is raised.

### Shared Context, Public Interface, Data Model

N/A — no type, method or table changes.

### Error Table (row added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `copy_action` with `try_again` on a copy whose `published_via` is `ConfluenceLink` (SA-PUSH-LINKED-02) | `Err(permanent)` | `PUSH_COPY_LINKED` | `{"error":"This file is published from its linked folder. Send it again from the folder's Ready to publish list.","code":"PUSH_COPY_LINKED"}` |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Push Service — a linked folder's copies are not updated automatically

  Scenario: Error path — the person's save does not update a linked folder's copy (SA-PUSH-LINKED-01)
    Given a current copy of "rates.md" made by "Publish a copy" and a current copy of "rates.md" made by a linked folder
    When the person saves a new version of "rates.md"
    Then CopiesUpdated reports 1 updated, 0 held and 0 failed
    And the linked folder's copy is unchanged and no request is sent for its page

  Scenario: Error path — Try again on a failed linked folder's copy is refused (SA-PUSH-LINKED-02)
    Given a failed copy of "rates.md" made by a linked folder, published once to page "5300"
    When the person chooses Try again on it
    Then the call returns PUSH_COPY_LINKED
    And the copy is still failed with its versions unchanged and no request is sent for its page

  Scenario: Error path — Try again on a linked folder's copy whose first publish failed is refused (SA-PUSH-LINKED-02)
    Given a failed copy of "rates.md" made by a linked folder that was never published
    When the person chooses Try again on it
    Then the call returns PUSH_COPY_LINKED
    And the copy is still failed and no page is created

  Scenario: Happy path — Try again on a failed copy made by "Publish a copy" still sends it
    Given a failed copy of "rates.md" made by "Publish a copy", published once to page "5200"
    When the person chooses Try again on it
    Then the page is updated from "rates.md" and the copy is current
```

### Performance, Security, Observability (additions)

- **Security:** nothing a linked folder holds leaves the computer through this service's automatic updates, its **Try again** or its **Update copies** card; it leaves only through the Confluence Adapter, under that adapter's agent test and hash-bound card.
- **Performance, Observability:** no change.

### Gaps

None.
