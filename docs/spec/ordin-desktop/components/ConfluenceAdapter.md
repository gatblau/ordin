# SPEC: Confluence Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-confluence` · **Build phase:** 5 · **Programs:** D
> **Dependencies:** Identity, Approval Service, File Service, Journal

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

Line references: `02:N` is `docs/design/ordin-desktop/02-architecture.md` line N; `03:N`, `04:N`, `08:N`, `10:N`, `11:N` likewise; `09 ADR-0NN` is an ADR in `09-decisions.md`; `2C` is `docs/spec/ordin-desktop/phase-2c-shared-types-*.md`; `B7 contract` is the batch's shared contract sheet whose decisions are restated here.

## Purpose

The Confluence Adapter reads from and publishes to the company's Confluence Cloud site with the person's own Atlassian OAuth token (02:743, 09 ADR-026). It lists spaces and page trees, fetches pages and attachments with their versions for the locations a workspace links, and finds what changed there since the last look. For a **linked folder** it compares the folder's files with their published copies: it pushes the person's own changes to already-published files automatically, leaves changes Ordin or an agent made to the Push Service's approval card (09 ADR-029), puts new, renamed and removed files in *Ready to publish*, turns text documents into pages and attaches every other kind as the exact file, and marks a page *Changed in Confluence* without ever overwriting the local file. It also makes and updates the Confluence page copies of Markdown originals for the Push Service (09 ADR-048, ADR-069), and states who can see a space or page (C-42). Linking a folder goes through the Approval Service; every push is an external action in the journal with a unique key, never resent automatically after a crash (02:747).

## Approach

A typed REST client over Confluence Cloud's v2 API through Atlassian's API gateway (`https://api.atlassian.com/ex/confluence/{cloudId}`), using v1 only where v2 has no equivalent (CQL search, attachment upload, content restrictions), with Markdown turned into Confluence storage format in this crate by `pulldown-cmark` (B7 contract D-11) and a bounded reverse conversion for **Bring it in**. Linked-folder state is derived each time from the File Service's published-copy records and file versions, so there is one source of truth for copies (02:748). Rejected: converting through an ArtCraft app (ADR-048 names "a Confluence page, by ordin's Confluence Adapter"); attaching every file (ADR-026 *Alternatives*: readers would have to download text documents).

## Shared Context

### Identifiers, time and paging (2C part 1 *Conventions*; SA-PAGE-01)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);       // RFC 3339, UTC, ms, `Z`
// UUIDv7 newtypes, 36-character lowercase strings:
pub struct SourceLinkId(pub uuid::Uuid);   pub struct WorkspaceId(pub uuid::Uuid);
pub struct PlaceId(pub uuid::Uuid);        pub struct RunId(pub uuid::Uuid);
pub struct ConfluenceLinkId(pub uuid::Uuid); pub struct ApprovalId(pub uuid::Uuid);
pub struct PublishedCopyId(pub uuid::Uuid);  pub struct FileVersionId(pub uuid::Uuid);
pub struct KnowledgePageId(pub uuid::Uuid);  pub struct ExternalActionKey(pub uuid::Uuid);
pub struct Sha256Hex(pub String);            // 64 lowercase hex

// `ordin_types::Page<T>` and `Cursor` (2C domain, SA-PAGE-01; amended C-15), copied:
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Page<T> { pub items: Vec<T>, pub next_cursor: Option<Cursor> }   // `null` on the last page
pub const PAGE_SIZE: usize = 100;
pub const CURSOR_MAX_CHARS: usize = 1024;
```

### Domain types read or written (2C part 1, copied unchanged)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfluenceLink {
    pub id: ConfluenceLinkId,
    pub workspace_id: WorkspaceId,
    pub place_id: PlaceId,
    /// Folder relative to the place; empty = the place's root.
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
pub struct SourceLink {
    pub id: SourceLinkId, pub workspace_id: WorkspaceId, pub kind: SourceLinkKind,
    pub origin_location: String, pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub place_id: Option<PlaceId>,
    pub state: SourceLinkState, pub added_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")] pub last_read_at: Option<Timestamp>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkKind { LocalFolder, GitRepositoryFolder, SharepointLocation, ConfluenceLocation }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkState { Proposed, Active }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Place { pub id: PlaceId, pub friendly_name: String, pub path: String, pub kind: PlaceKind, pub added_at: Timestamp }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind { PersonsFolder, WorkspaceFolder, AgentFolder, WorkingCopy, CodingTaskWorktree } // last two: B7 D-5/D-6

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Workspace {
    pub id: WorkspaceId, pub name: String, pub own_folder_place_id: PlaceId, pub state: WorkspaceState,
    #[serde(skip_serializing_if = "Option::is_none")] pub last_seen_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")] pub moved_to_bin_at: Option<Timestamp>,
    pub created_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState { Active, Archived, InBin }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileVersion { pub id: FileVersionId, pub place_id: PlaceId, pub path: String, pub version: u32,
                         pub hash: Sha256Hex, pub origin: FileVersionOrigin, pub created_at: Timestamp }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileVersionOrigin { Person, Run { run_id: RunId, tool: String }, ArtCraftEdit { app: String } }

// PublishedCopy and its enums: exactly as 2C (field list and state table in the File Service spec):
// id, original: CopyOriginal, destination: CopyDestination, destination_item_id: Option<String>, format: CopyFormat,
// maker: CopyMaker, published_via: PublishedVia, followed_branch: Option<String>, keep_up_to_date: bool,
// audience: String, original_version_published: Option<String>, destination_version_pushed: Option<String>,
// last_updated_at: Option<Timestamp>, state: PublishedCopyState, failure_reason: Option<String>.
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
#[serde(rename_all = "snake_case")] pub enum CopyFormat { Page, Word, Pdf, Slides, ExactFile }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")] pub enum CopyMaker { Ordin, WordCraft, PdfCraft, DeckCraft, None }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublishedVia { ConfluenceLink { confluence_link_id: ConfluenceLinkId }, PublishACopy }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishedCopyState { ReadyToPublish, AwaitingApproval, Current, OutOfDate, Updating,
                              ChangedAtDestination, Failed, NotKeptUpToDate, Removed }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind { SlackPost, ConnectionWrite, WrightrunRunStart, EmailSend, OriginPush, CopyPublish, TelegramPost,
                              PullRequestOpened, HandoverWritten }   // 2C C-7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionState { Sent, Confirmed, UnknownOutcome }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind { Github, Gitlab, Oidc, Slack, Confluence, Microsoft, Google,   // Gitlab: 2C C-10, D9
    MailServerSingleSignOn, MailServerPassword, MailServerAppPassword, Jira }   // Jira: 2C, amended January 2026, ADR-106
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState { Pending, Approved, Rejected, Expired }
```

Approval card types (2C wire §5, copied): `ApprovalAsker { Run { run_id }, CrewRound { crew_run_id, round }, Person }`; `AffectedItem { File { label, version: Option<String>, place_id: Option<PlaceId> }, Record { label }, Recipient { name, address, outside_company } }`; `ApprovalContent::Files { label: String, files: Vec<ContentRef> }` with `ContentRef::PlaceFile { place_id, relative_path }`; `RejectStyle { Reject, NotNow }`; `ActionProposal { action: String, destination: String, affects: Vec<AffectedItem>, consequence: String }` (2C wire). Limits: question 1–120 chars ending "?", verb 1–32, where 1–200, affects 1–50 items, changes 1–1,000, `Files.files` 1–100 (`APPR_*`); `ActionProposal.action` ≤ 500, `destination` ≤ 2,048, `consequence` ≤ 2,000.

### Boundary interfaces of other components (copied from the providers' current specs; rulings R3, R6)

```rust
// ordin-identity — IdentityService (IdentityAndCredentials.md, "Tokens and state")
pub struct AccessToken { pub token: secrecy::SecretString, pub expires_at: Option<Timestamp>, pub credential_id: CredentialId }
impl IdentityService {
    /// As `access_token(kind, None)`, and the credential's granted scopes must contain every entry of
    /// `scopes`; otherwise `CRED_SCOPE_MISSING`.
    pub async fn access_token_for(&self, kind: CredentialKind, scopes: &[&str]) -> Result<AccessToken, CredError>;
}
pub enum CredError { Retryable { code: &'static str }, Permanent { code: &'static str } }   // CodedError: code(), retryable()

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
}
// ApprovalError: CodedError (`code()`, `retryable()`). There is no owner lookup: this adapter keeps the
// `approval_id` on its own row (`confluence_publish_requests`), and the Workspace Service keeps the link card's.

// ordin-journal — Journal (JournalAndArtefactStore.md; ruling R3)
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
pub struct NewJournalEntry { pub kind: String, pub component: String, pub run_id: Option<RunId>,
                             pub crew_run_id: Option<CrewRunId>, pub workspace_id: Option<WorkspaceId>,
                             pub summary: String, pub data: serde_json::Value }
pub struct NewExternalAction { pub action_key: ExternalActionKey, pub run_id: Option<RunId>,
                               pub kind: ExternalActionKind, pub destination: String }
impl Journal {
    pub fn datastore(&self) -> &Datastore;
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    pub async fn events_since(&self, after_seq: u64, limit: u32) -> Result<Vec<JournalEvent>, JournalError>;
    /// A used key → `JournalError` with `code() == "EXT_KEY_DUPLICATE"`.
    pub async fn begin_external_action(&self, new: NewExternalAction) -> Result<ExternalAction, JournalError>;
    pub async fn confirm_external_action(&self, key: ExternalActionKey) -> Result<ExternalAction, JournalError>;
    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId;   // called by the program
}
/// Implemented by this adapter (`event_sink()`), registered by the program with `Journal::subscribe`.
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    fn name(&self) -> &'static str;
    async fn deliver(&self, event: &JournalEvent);   // `kind` "approval_decided": data { approval_id, state, decided_on }
    async fn lagged(&self, next_seq: u64);
}
pub struct JournalEvent { pub backend: uuid::Uuid, pub seq: u64, pub at: Timestamp, pub kind: String, pub data: serde_json::Value }

// ordin-files — FileService (FileService.md, Public Interface)
impl FileService {
    pub async fn list_folder(&self, place_id: PlaceId, folder: &str, sort: FolderSort, cursor: Option<Cursor>) -> Result<Page<FileEntry>, FileError>;
    pub async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError>;
    pub async fn versions(&self, place_id: PlaceId, path: &str) -> Result<Vec<FileVersion>, FileError>;
    pub async fn record_published_copy(&self, c: PublishedCopy) -> Result<PublishedCopy, FileError>;
    pub async fn set_published_copy_state(&self, id: PublishedCopyId, state: PublishedCopyState, failure_reason: Option<String>) -> Result<PublishedCopy, FileError>;
    pub async fn published_copies_via_link(&self, link: ConfluenceLinkId) -> Result<Vec<PublishedCopy>, FileError>;
}
// FileEntry { place_id, path, name, is_folder, kind, size_bytes, changed_at, changed_by, has_versions }
// FileBytes { place_id, path, bytes, hash, media_type }

/// Owned by this crate (decision D3, ADR-100 item 1 as amended); implemented by the desktop program over the
/// Craft Toolkit's `to_markdown(CopyFormat::Word, bytes)` once C-13 confirms WordCraft's headless Word → Markdown
/// export. The program maps a `CraftError` to its `code()` text.
#[async_trait::async_trait]
pub trait WordToMarkdown: Send + Sync {
    async fn convert(&self, docx: Vec<u8>) -> Result<String, String>;
}
```

### Confluence Cloud surface used (08:103)

Base for every call: `https://api.atlassian.com/ex/confluence/{cloudId}`. `cloudId` is resolved once per token from `GET https://api.atlassian.com/oauth/token/accessible-resources`, choosing the resource whose `url` equals the injected `confluence.site` (B7 contract D-4).

| Use | Request |
|---|---|
| Find spaces and pages | `GET /wiki/rest/api/search?cql=(type=space OR type=page) AND title ~ "{q}"&limit=25` (v1: v2 has no search) |
| Space by key | `GET /wiki/api/v2/spaces?keys={key}` |
| Top pages of a space | `GET /wiki/api/v2/spaces/{spaceId}/pages?depth=root&limit=250` |
| Page children | `GET /wiki/api/v2/pages/{id}/children?limit=250` (cursor in `_links.next`) |
| Page with body | `GET /wiki/api/v2/pages/{id}?body-format=storage` (`version.number`, `version.authorId`, `version.createdAt`) |
| Attachments of a page | `GET /wiki/api/v2/pages/{id}/attachments?limit=250` (`downloadLink`, `version.number`, `fileSize`, `mediaType`) |
| Attachment content | `GET /wiki{downloadLink}` |
| Changes in a tree or space | `GET /wiki/rest/api/search?cql=(ancestor={rootId} OR id={rootId}) AND lastmodified >= "{yyyy/MM/dd HH:mm}"&limit=100` (space links: `space="{key}"`) |
| Create page | `POST /wiki/api/v2/pages` `{"spaceId","status":"current","title","parentId","body":{"representation":"storage","value"}}` |
| Update page | `PUT /wiki/api/v2/pages/{id}` `{"id","status":"current","title","body":{…},"version":{"number":n+1,"message":"Updated from ordin"}}` |
| Add attachment | `POST /wiki/rest/api/content/{pageId}/child/attachment` (multipart `file`, header `X-Atlassian-Token: no-check`) |
| New attachment version | `POST /wiki/rest/api/content/{pageId}/child/attachment/{attachmentId}/data` (same headers) |
| Delete page (to the space's trash) | `DELETE /wiki/api/v2/pages/{id}` |
| Delete attachment (to the trash) | `DELETE /wiki/api/v2/attachments/{id}` |
| Who can see a page | `GET /wiki/rest/api/content/{id}/restriction/byOperation/read` |
| Who can see a space | `GET /wiki/api/v2/spaces/{spaceId}/permissions` |
| Author name | `GET /wiki/rest/api/user?accountId={id}` (cached per account for the session) |

OAuth scopes requested from Identity (*chosen here*, least set that covers the table): `read:space:confluence`, `read:page:confluence`, `write:page:confluence`, `read:attachment:confluence`, `write:attachment:confluence`, `delete:page:confluence`, `delete:attachment:confluence`, `read:space.permission:confluence`, `read:content.restriction:confluence` *(scope name to confirm with C-11; if Atlassian refuses it, the audience read takes the C-42 fallback below)*, `search:confluence`, `read:confluence-user`, `offline_access`.

### Injected settings (B7 contract D-4)

```rust
#[derive(Debug, Clone)]
pub struct ConfluenceSettings {
    /// `confluence.site` from the organisation profile (2D-2), an `https` URL; None = Confluence off.
    pub site: Option<url::Url>,
}
```

The crate receives a `tokio::sync::watch::Receiver<ConfluenceSettings>` at construction; a site change clears the cached `cloudId`. `url` is already a dependency of `reqwest`.

### Constants (*chosen here* unless cited)

| Constant | Value |
|---|---|
| `REQUEST_TIMEOUT` | 30 s metadata; 120 s for a body or attachment transfer |
| `MAX_PAGE_BODY_BYTES` | 5 MiB of storage XHTML per page *(assumed here)* |
| `MAX_ATTACHMENT_BYTES` | 100 MiB *(assumed here: Confluence Cloud's default attachment limit)* |
| `RETRIES`, `BACKOFF` | 3; 1 s, 2 s, 4 s ±20 % jitter, `Retry-After` wins |
| `MAX_TREE_PAGES` | 10,000 pages and 20 levels per linked tree (A-21 scaled to pages) |
| `TEXT_KINDS` | `md`, `markdown`, `txt` become pages; `docx` becomes a page only when `WordToMarkdown` is wired (C-13) |
| Working-copy file names | page → `<title>.confluence.xhtml` (storage format); attachment → its file name inside a folder named after its page (*chosen here*, B7 contract D-5) |
| Origin form | `SourceLink.origin_location` = `confluence:space/{key}` or `confluence:page/{pageId}` (*chosen here*) |
| Item forms read by others | `confluence:page/{pageId}` and `confluence:attachment/{pageId}/{attachmentId}`, one per `RemoteItemRef` variant; Home and Search writes them in `InputUsed` (reconciliation row 714) |

### Confirmation items bound here (Phase 1, *Confirmation items*)

- **C-12** — Atlassian OAuth 3LO serves a desktop without a stored client secret (PKCE) and issues refresh tokens. Identity owns the sign-in; this adapter only asks for a token. *If not confirmed*, the HLD's fallback is a small company-hosted token exchange (10 C-12); this crate is unchanged because it receives the token through Identity.
- **C-11** — the token can read trees, create and update pages with versions, add attachments, and reads a page's current version. *Fallback:* none in the HLD beyond not offering Confluence; every write method returns `CONF_ACCESS_DENIED` when Confluence answers 403, and the person sees the sentence for it.
- **C-13 / OQ-24** — WordCraft converts Word to Markdown headless. *Until confirmed* Word documents are attached as files (02:743); afterwards the program wires `WordToMarkdown` and `docx` becomes a page.
- **C-42** — Confluence states who can see a space or page in one line. *Fallback:* the card names the space and says "everyone who can open it", and a grown audience cannot be detected (10 C-42). The same fallback applies when Atlassian refuses the restriction-read scope.

### Reconciliation assumptions

- **SA-RECON-01** Single external-action bracket (reconciliation row 711): the caller mints the `ExternalActionKey` (UUIDv7) and passes `key` and `run_id`; this adapter alone calls `begin_external_action` before sending and `confirm_external_action` after. The Push Service and the Workspace Service open no external action for these writes.
- **SA-RECON-02** Ruling R19: callers pass the Confluence version they read; this adapter is the one place in ordin that adds 1 when it sends (Internal Logic 7.3).
- **SA-RECON-03** Ruling R1: the Service API catalogue carries this crate's five `confluence.*` command names unchanged; there is no `confluence.publish`.
- **SA-RECON-04** Decision D4 and ruling R20: this crate's tables live in the Journal's `Datastore` (`journal.datastore()`), and its migrations are `migrations() -> ModuleMigrations` built from `MigrationStep` tuples (ruling R3).
- **SA-RECON-05** Reconciliation row 224: the Approval Service has no owner lookup; this adapter keeps the `approval_id` of each **Publish checked items…** card on its own row, and the Workspace Service keeps the link card's `approval_id` on its `confluence_links` row.
- **SA-RECON-06** Reconciliation row 15: approval decisions arrive through the Journal's push-style `EventSink` (`deliver`, `lagged`), which this adapter implements; there is no broadcast receiver and no `EventEnvelope`.
- **SA-RECON-07** Ruling R6: Identity's own `access_token_for(kind, scopes)` and `CredError` codes replace the `Identity` trait this spec once assumed.
- **SA-RECON-08** Decision D3: `WordToMarkdown` is owned here and implemented by the desktop program over the Craft Toolkit's `to_markdown`, so this crate takes no Craft Toolkit edge.
- **SA-RECON-09** `item_for_path` is added for the Push Service (reconciliation rows 703–711), which holds a working-copy item's `origin_path` and needs the page or attachment behind it.

## Public Interface

Error prefix: `CONF_`.

```rust
pub struct ConfluenceDeps {
    /// The adapter's tables live in `journal.datastore()` (decision D4; no separate pool).
    pub journal: std::sync::Arc<Journal>,
    pub identity: std::sync::Arc<IdentityService>,
    pub approvals: std::sync::Arc<ApprovalService>,
    pub files: std::sync::Arc<FileService>,
    pub http: reqwest::Client,                                     // SA-HTTP-02
    pub settings: tokio::sync::watch::Receiver<ConfluenceSettings>,
    pub word_to_markdown: Option<std::sync::Arc<dyn WordToMarkdown>>,
}

pub struct ConfluenceAdapter { /* private */ }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfluenceLocation {
    pub space_key: String,
    pub space_name: String,
    /// None = the whole space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_id: Option<String>,
    pub title: String,
    pub web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteItemRef { Page { page_id: String }, Attachment { page_id: String, attachment_id: String } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteVersion {
    /// The Confluence version number as text ("7"); the "version read" of A-25.
    pub tag: String,
    pub changed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteItem {
    pub item: RemoteItemRef,
    pub title: String,
    /// Path in the working copy relative to the link root (Constants).
    pub path: String,
    pub has_children: bool,
    pub size_bytes: Option<u64>,
    pub version: RemoteVersion,
}

#[derive(Debug, Clone)]
pub struct FetchedItem { pub item: RemoteItemRef, pub path: String, pub bytes: Vec<u8>, pub version: RemoteVersion }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginChange {
    pub source_link_id: SourceLinkId,
    pub origin_path: String,
    pub new_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    pub changed_at: Timestamp,
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Audience {
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub people: Option<u32>,
    pub everyone_who_can_open: bool,
    /// Sorted account and group ids with read access; empty under the C-42 fallback.
    pub principals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReadyItem {
    pub confluence_link_id: ConfluenceLinkId,
    pub kind: ReadyKind,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_copy_id: Option<PublishedCopyId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadyKind { New, Renamed, Removed }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkSyncReport {
    pub pushed: u32,                 // the person's own changes, pushed automatically
    pub waiting_for_approval: u32,   // changes by Ordin or an agent, left to the Push Service
    pub ready_to_publish: u32,       // new, renamed, removed
    pub changed_in_confluence: u32,
    pub failed: u32,
}

impl ConfluenceAdapter {
    /// Module `confluence`; the program passes it to `MigrationRegistry::register` (R3).
    pub fn migrations() -> ModuleMigrations;   // module "confluence" (ModuleMigrations.module)
    pub fn new(deps: ConfluenceDeps) -> std::sync::Arc<ConfluenceAdapter>;
    /// The sink the program registers with `Journal::subscribe`; it acts on `approval_decided` for the
    /// approval ids stored in `confluence_publish_requests` (Internal Logic 6.2).
    pub fn event_sink(self: &std::sync::Arc<Self>) -> std::sync::Arc<dyn EventSink>;

    // Reading (same surface as the SharePoint Adapter)
    pub async fn find_locations(&self, query: &str, limit: u32) -> Result<Vec<ConfluenceLocation>, ConfluenceError>;
    pub fn origin_location(loc: &ConfluenceLocation) -> String;
    pub async fn resolve_origin(&self, origin_location: &str) -> Result<ConfluenceLocation, ConfluenceError>;
    pub async fn list_children(&self, loc: &ConfluenceLocation, cursor: Option<Cursor>) -> Result<Page<RemoteItem>, ConfluenceError>;
    pub async fn fetch(&self, item: &RemoteItemRef) -> Result<FetchedItem, ConfluenceError>;
    pub async fn item_version(&self, item: &RemoteItemRef) -> Result<RemoteVersion, ConfluenceError>;
    /// The page or attachment a tracked link holds at `origin_path` (the `RemoteItem.path` the Workspace Service
    /// stored as `WorkingCopyItem.origin_path`), from `confluence_items`; no request to Confluence (Internal Logic 3.4).
    pub async fn item_for_path(&self, link: SourceLinkId, origin_path: &str) -> Result<RemoteItemRef, ConfluenceError>;
    pub async fn track_link(&self, link: &SourceLink) -> Result<(), ConfluenceError>;
    pub async fn untrack_link(&self, link: SourceLinkId) -> Result<(), ConfluenceError>;
    pub async fn check_changes(&self, link: &SourceLink) -> Result<Vec<OriginChange>, ConfluenceError>;
    pub async fn changes_since(&self, links: &[SourceLinkId], since: Timestamp) -> Result<Vec<OriginChange>, ConfluenceError>;
    pub async fn audience(&self, loc: &ConfluenceLocation) -> Result<Audience, ConfluenceError>;

    // Linked folders (ADR-026, ADR-069)
    pub async fn request_link_approval(&self, link: &ConfluenceLink, place: &Place, workspace: &Workspace) -> Result<ApprovalId, ConfluenceError>;
    pub async fn sync_linked_folder(&self, link: &ConfluenceLink, place: &Place) -> Result<LinkSyncReport, ConfluenceError>;
    pub async fn ready_to_publish(&self, link: ConfluenceLinkId) -> Result<Vec<ReadyItem>, ConfluenceError>;
    /// **Publish checked items…**: one approval card for the chosen items.
    pub async fn request_publish_checked(&self, link: &ConfluenceLink, place: &Place, items: Vec<ReadyItem>) -> Result<ApprovalId, ConfluenceError>;
    /// **Keep mine** on a page changed in Confluence: the next push replaces it.
    pub async fn keep_local(&self, copy: PublishedCopyId) -> Result<(), ConfluenceError>;
    /// The Workspace Service removed the link: deletes this crate's rows for it; no Confluence page is deleted.
    pub async fn forget_link(&self, link: ConfluenceLinkId) -> Result<(), ConfluenceError>;

    // Copies and pushes (Push Service; B7 contract §2)
    pub async fn publish_page(&self, dest: &CopyDestination, title: &str, markdown: &str, banner: Option<&str>,
                              key: ExternalActionKey, run_id: Option<RunId>) -> Result<(String, RemoteVersion), ConfluenceError>;
    /// `expected_version` is the page version the caller **read** (`RemoteVersion.tag`, e.g. "7"), never that
    /// number plus one: this adapter is the one place that adds 1 when it sends (ruling R19, Internal Logic 7.3).
    pub async fn update_page(&self, page_id: &str, title: &str, markdown: &str, banner: Option<&str>,
                             expected_version: &str, key: ExternalActionKey, run_id: Option<RunId>) -> Result<RemoteVersion, ConfluenceError>;
    /// As `update_page`: `expected_version` is the version read; the adapter adds 1 (R19).
    pub async fn update_page_storage(&self, page_id: &str, storage_xhtml: &str, expected_version: &str,
                                     key: ExternalActionKey, run_id: Option<RunId>) -> Result<RemoteVersion, ConfluenceError>;
    pub async fn attach(&self, page_id: &str, file_name: &str, bytes: Vec<u8>, expected_version: Option<&str>,
                        key: ExternalActionKey, run_id: Option<RunId>) -> Result<(String, RemoteVersion), ConfluenceError>;
    /// Moves a page or an attachment to the space's trash (the Push Service's **Remove copy…**, ADR-069).
    /// With `expected_version`, refused with `CONF_ORIGIN_CHANGED` when the item's version differs.
    pub async fn delete(&self, item: &RemoteItemRef, expected_version: Option<&str>,
                        key: ExternalActionKey, run_id: Option<RunId>) -> Result<(), ConfluenceError>;
    /// Pure functions.
    pub fn markdown_to_storage(markdown: &str) -> Result<String, ConfluenceError>;
    pub fn storage_to_markdown(storage_xhtml: &str) -> Result<String, ConfluenceError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfluenceError {
    #[error("CONF_NOT_CONFIGURED")] NotConfigured,
    #[error("CONF_SIGN_IN_NEEDED")] SignInNeeded,
    #[error("CONF_SITE_NOT_GRANTED")] SiteNotGranted,
    #[error("CONF_ACCESS_DENIED")] AccessDenied,
    #[error("CONF_NOT_FOUND")] NotFound,
    #[error("CONF_LOCATION_INVALID")] LocationInvalid,
    #[error("CONF_QUERY_INVALID")] QueryInvalid,
    #[error("CONF_ORIGIN_CHANGED")] OriginChanged { current: RemoteVersion },
    #[error("CONF_TITLE_EXISTS")] TitleExists,
    #[error("CONF_TOO_LARGE")] TooLarge,
    #[error("CONF_THROTTLED")] Throttled,
    #[error("CONF_UNAVAILABLE")] Unavailable,
    #[error("CONF_OUTCOME_UNKNOWN")] OutcomeUnknown,
    #[error("CONF_RESPONSE_INVALID")] ResponseInvalid,
    #[error("CONF_CONVERSION_FAILED")] ConversionFailed,
    #[error("CONF_LINK_NOT_ACTIVE")] LinkNotActive,
    #[error("CONF_NOT_TRACKED")] NotTracked,
    #[error("CONF_ACTION_KEY_USED")] ActionKeyUsed,
    #[error("CONF_APPROVAL_FAILED")] ApprovalFailed,
    #[error("CONF_FILES_FAILED")] FilesFailed,
    #[error("CONF_DATASTORE_UNAVAILABLE")] DatastoreUnavailable,
}
impl ConfluenceError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

### Service API commands (*chosen here*)

| Command | Input | Result |
|---|---|---|
| `confluence.find` | `{ "query": string (1–200), "limit": u32 (1–25) }` | `Vec<ConfluenceLocation>` |
| `confluence.list` | `{ "location": ConfluenceLocation, "cursor": Cursor? }` | `Page<RemoteItem>` |
| `confluence.ready_to_publish` | `{ "confluence_link_id" }` | `Vec<ReadyItem>` |
| `confluence.publish_checked` | `{ "confluence_link_id", "items": [ReadyItem] }` | `{ "approval_id" }` |
| `confluence.keep_mine` | `{ "published_copy_id" }` | `{}` |

These are the catalogue's names (R1). This crate has no `confluence.publish`: publishing one document as a copy is the Push Service's `publish_copy`, and linking a folder is the Workspace Service's `workspaces.propose_confluence_link`.

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| reading methods (`find_locations` … `check_changes`), `audience`, `request_link_approval`, `sync_linked_folder`, `forget_link` | Workspace Service |
| `changes_since`, `item_version` | Home and Search |
| `resolve_origin`, `item_for_path`, `item_version`, `fetch` + `storage_to_markdown` (the Push Service's "page as Markdown"), `publish_page`, `update_page`, `update_page_storage`, `attach`, `audience`, `delete` | Push Service (absent on the ordin server, which has no Confluence Adapter) |
| `migrations`, `new`, `event_sink` | desktop program (`MigrationRegistry::register`, `Journal::subscribe`) |
| `find_locations`, `list_children`, `ready_to_publish`, `request_publish_checked`, `keep_local` | Service API catalogue (`confluence.*`) |

Ownership of copies: copies made by a linked folder (ADR-026; `published_via = ConfluenceLink`) are synced, checked and approved here (the approval id kept in `confluence_publish_requests`); copies published one by one (ADR-069) are the Push Service's. The Knowledge Service reads Confluence versions only through the Workspace Service's `origin_version`.

### Example

```rust
let card = conf.request_link_approval(&link, &place, &workspace).await?;
// Approval card (built by the Approval Service from this request):
//   question "Publish Supplier quotes to Confluence?", verb "Link and publish", reject_style NotNow,
//   where_line "Confluence — Procurement › Supplier pages — Procurement team (42 people)",
//   affects [File { label: "18 documents in Supplier quotes", .. }],
//   changes "Text documents become pages. Other files are attached to the folder's page as they are.",
//   content Files { label: "Review files", files: [PlaceFile { place_id, relative_path: "rates.md" }, …] }

let report = conf.sync_linked_folder(&link, &place).await?;
// LinkSyncReport { pushed: 2, waiting_for_approval: 1, ready_to_publish: 3, changed_in_confluence: 0, failed: 0 }

let xhtml = ConfluenceAdapter::markdown_to_storage("# Rates\n\nDay rate: **£640**\n")?;
// "<h1>Rates</h1><p>Day rate: <strong>£640</strong></p>"
```

## Internal Logic

### 1. Calling Confluence (every request)

1. `settings.site` is `None` → `CONF_NOT_CONFIGURED`. Token: `identity.access_token_for(CredentialKind::Confluence, SCOPES)`, sent as `Bearer {token.token.expose_secret()}`. `CredError` by `code()`: `CRED_SIGN_IN_REQUIRED`, `CRED_SCOPE_MISSING`, `CRED_PROVIDER_REFUSED`, `CRED_KEYCHAIN_MISSING` → `CONF_SIGN_IN_NEEDED` ("Sign in to Confluence to see the pages linked to this workspace.", 11 §5.14); `CRED_PROVIDER_NOT_CONFIGURED` → `CONF_NOT_CONFIGURED`; any other retryable code → `CONF_UNAVAILABLE`; any other permanent code → `CONF_SIGN_IN_NEEDED`.
2. Resolve `cloudId` (cached per token): no accessible resource matches the site → `CONF_SITE_NOT_GRANTED` (the person signed in with an Atlassian account that cannot reach the company site).
3. Status mapping: 401 → refresh once, then `CONF_SIGN_IN_NEEDED`; 403 → `CONF_ACCESS_DENIED`; 404 → `CONF_NOT_FOUND`; 409 on a page update → `CONF_ORIGIN_CHANGED`; 400 whose body says a page with this title exists → `CONF_TITLE_EXISTS`; 413 → `CONF_TOO_LARGE`; 429/503 → backoff and retry for reads (and for writes proven not to have landed, step 7.6), then `CONF_THROTTLED` / `CONF_UNAVAILABLE`; an unparseable body → `CONF_RESPONSE_INVALID`.
4. Paging follows `_links.next` inside the adapter; the `Cursor` handed to the caller is base64 of the next relative link (at most `CURSOR_MAX_CHARS`), validated to start with `/wiki/` when decoded (else `SRV_BAD_CURSOR`).
5. Log `confluence_request {operation, status, elapsed_ms, attempt}` only.

### 2. Reading

1. `find_locations`: `query` trimmed 1–200 (else `CONF_QUERY_INVALID`); CQL text is built with the query as a quoted string, `"` and `\` escaped, so no CQL operator from the person reaches the search; results map to `ConfluenceLocation` (spaces with `page_id = None`).
2. `list_children`: for a space, its root pages; for a page, its child pages then its attachments. Paths: a page `P` under the link root is `<ancestor titles>/<P title>.confluence.xhtml`; its attachments are `<ancestor titles>/<P title>/<file name>`. A `/` in a title is replaced by `∕` (U+2215) in paths only (*chosen here*).
3. `fetch(Page)`: body in storage format; body larger than `MAX_PAGE_BODY_BYTES` → `CONF_TOO_LARGE`; bytes are the UTF-8 storage XHTML. `fetch(Attachment)`: size above `MAX_ATTACHMENT_BYTES` → `CONF_TOO_LARGE`; bytes are the file. Both re-read the version after the transfer and repeat once if it moved, then `CONF_ORIGIN_CHANGED`.
4. The Workspace Service writes fetched bytes to the working copy (B7 contract D-5); this adapter never writes to a place.

### 3. Change tracking (A-25; B7 contract D-8)

1. `track_link(link)`: resolve its origin; walk the tree (≤ `MAX_TREE_PAGES`, 20 levels), store each page and attachment with its version number in `confluence_items`; store `root_page_id` or `space_key` and `last_checked_at = now` in `confluence_link_state`.
2. `check_changes(link)`: run the change CQL with `lastmodified >= last_checked_at − 2 minutes` (overlap for clock skew, *chosen here*); for each page in the result, read its version and its attachments' versions; a version number above the stored one → `OriginChange` (author resolved by account id) and update the row; an item stored but not found when its parent page is re-listed → `removed: true`. Insert the changes into `confluence_origin_changes` and update `last_checked_at` in one transaction.
3. `changes_since`: as the SharePoint Adapter: rows with `found_at > since`, newest first, ≤ 1,000. Rows older than 90 days are deleted at each check (*assumed here*).
4. `item_for_path(link, origin_path)`: no `confluence_link_state` row for `link` → `CONF_NOT_TRACKED`; select the `confluence_items` row with that `origin_path` and `removed = 0`; none → `CONF_NOT_FOUND`; `item_key` `page:<id>` → `RemoteItemRef::Page`, `attachment:<page id>:<id>` → `RemoteItemRef::Attachment`. The caller reads the live version with `item_version`.

### 4. Linking a folder (`request_link_approval`)

1. `link.state` must be `proposed` (else `CONF_LINK_NOT_ACTIVE`).
2. Resolve the space and parent page (`CONF_NOT_FOUND` if gone); read `audience` for the parent page (step 8).
3. List the folder's files through `files.list_folder` (all pages, folders recursed, at most 50 files named in `content`, the count in `affects`).
4. Build the `ApprovalRequest` (words from 11 §5.14): `question` = "Publish {folder friendly name} to Confluence?"; `verb` = "Link and publish"; `reject_style` = `NotNow`; `where_line` = "Confluence — {space name} › {parent page title} — {audience.line}" cut to 200 chars; `affects` = one `File` item "{N} documents in {folder name}"; `changes` = "Text documents become pages. Other files are attached to the folder's page as they are."; `content` = `Files { label: "See the files", files }`; `show_details` = true; `asked_by` = `Person`; `proposal` = `ActionProposal { action: "Link a folder to Confluence and publish it", destination: where_line, affects, consequence: changes }`; `asker` = None; `leaves_company` = false.
5. `approvals.request(..)` → `card.approval_id`, returned to the Workspace Service, which stores it on its `confluence_links` row and activates the link when that approval is decided (it owns the row; SA-RECON-05). Failure → `CONF_APPROVAL_FAILED` (retryable). `journal.append_entry(NewJournalEntry { kind: "confluence_link_approval_requested", component: "confluence", workspace_id: Some(link.workspace_id), summary: "Asked to link a folder to Confluence.", data: {"confluence_link_id": link.id}, .. })`.

### 5. Linked-folder sync (`sync_linked_folder`)

Called by the Workspace Service when a link becomes active, when its workspace opens, at start, every 15 minutes, and on a `PlaceChange` inside the linked folder (debounced by the caller). `link.state` must be `active` (paused or proposed → `CONF_LINK_NOT_ACTIVE`).

1. Load the link's copies (`files.published_copies_via_link`) keyed by original path, and the folder → page map (`confluence_folder_pages`).
2. Walk the folder's files (`list_folder`, every page, recursing) and, for each file `f` with hash `h` (`read_file` hash, or the index hash when the File Service returns it):
   1. **No copy** → create a `PublishedCopy` with `state = ready_to_publish`, `published_via = ConfluenceLink`, `destination` = `ConfluencePage` for `TEXT_KINDS` (and `docx` when `WordToMarkdown` is wired) or `ConfluenceAttachment` otherwise, `format` = `page` or `exact_file`, `maker` = `ordin` or `none`, `audience` = `link.audience`, `keep_up_to_date = true`; add a `ReadyItem::New`. A removed copy whose hash equals `h` at another path is a rename instead: `ReadyItem::Renamed { from_path }`.
   2. **Copy `current`, `h` differs from `original_version_published`** → read `files.versions(f)`. If the latest version's origin is `Run` or `ArtCraftEdit` (Ordin or an agent changed it), set the copy `out_of_date` and count `waiting_for_approval`: the Push Service sends it through the request's approval card (ADR-029: "A change Ordin made is never pushed automatically, even in a folder linked under ADR-026"). Otherwise (origin `Person`, or no version history) it is the person's own change: push it automatically (step 7) after checking the destination (step 5.3); count `pushed`.
   3. **Before any automatic push** read the destination's current version; above `destination_version_pushed` → set `changed_at_destination` and count `changed_in_confluence` ("Changed in Confluence", with **Bring a copy back** and **Keep mine**, 11 §5.14); the file is not pushed until the person chooses (ADR-026).
   4. Copies in `changed_at_destination`, `not_kept_up_to_date`, `awaiting_approval`, `updating` or `removed` are not pushed by the sync.
3. A copy whose file no longer exists → `ReadyItem::Removed` (nothing is deleted in Confluence without the person's check, 04 *Data Lifecycle*).
4. Upsert `confluence_ready_items` with the current Ready list (new, renamed, removed), replacing the link's previous list; `journal.append_entry` kind `confluence_link_synced`, component `confluence`, with the report counts in `data`.
5. Text pages: a folder's page is created on the first push that needs it (title = the folder's name, parent = the parent folder's page or `link.parent_page_id`), recorded in `confluence_folder_pages`; attachments go to the folder's page (ADR-026).

### 6. Publishing checked items (`request_publish_checked`)

1. Build one card (11 §5.14 "Publish checked items…"): question "Publish {N} checked items to Confluence?", verb "Publish", where as step 4.4, affects one `File` per item (at most 50 per card; a larger selection is refused with `CONF_QUERY_INVALID` and the screens split it), changes listing new, renamed and removed counts, `asked_by` = `Person`, `proposal` = `ActionProposal { action: "Publish checked items to Confluence", destination: where_line, affects, consequence: changes }`, `show_details` = false, `asker` = None, `leaves_company` = false; `approvals.request` → `card.approval_id`; store the request with that `approval_id` in `confluence_publish_requests`.
2. The sink from `event_sink()` receives every `JournalEvent`; it acts only on `kind = "approval_decided"` whose `data.approval_id` is in `confluence_publish_requests` (other events and other approval ids are ignored; `lagged(next_seq)` re-reads the missed events through the Journal's `events_since`). `state = approved` → for each item: `New` → set the copy `awaiting_approval` then push (step 7) and set `current`; `Renamed` → update the page title or re-upload the attachment under the new name, update the copy's original path; `Removed` → set the copy `removed` and leave the Confluence page in place (deleting it at the destination is the Push Service's **Remove copy…** with its own check, ADR-069). `rejected` or `expired` → items stay in Ready to publish. Delete the request row.

### 7. Pushing a page or attachment

1. External action first: `journal.begin_external_action(NewExternalAction { action_key: key, run_id, kind, destination: "Confluence › {space} › {page title}" })` with `kind` = `OriginPush` for `update_page_storage` (a working-copy page sent back to its origin) and `CopyPublish` for every other write, including an attachment the Push Service sends back to its origin (the Push Service's recovery reads both kinds); a `JournalError` whose `code()` is `EXT_KEY_DUPLICATE` → `CONF_ACTION_KEY_USED`, nothing sent; any other `JournalError` → `CONF_DATASTORE_UNAVAILABLE`, nothing sent. This is the only record of the write: the caller mints `key` (UUIDv7) and opens no external action of its own (single bracket, reconciliation row 711). Automatic linked-folder pushes create the key here (UUIDv7) and pass `run_id: None` (B7 contract D-12).
2. `publish_page`: `markdown_to_storage(markdown)`; with `banner`, prepend `<ac:structured-macro ac:name="info"><ac:rich-text-body><p>{banner escaped}</p></ac:rich-text-body></ac:structured-macro>` (ADR-069 "one line at the top of every copy"); resolve the space id from `dest.space`; create the page under `dest.parent_page_id`. `CONF_TITLE_EXISTS` → returned; the Push Service offers **Use another name**.
3. `update_page`: read the page's version; `≠ expected_version` → `CONF_ORIGIN_CHANGED { current }` before sending; send with `version.number = expected + 1`; a 409 → `CONF_ORIGIN_CHANGED`. This step is the only place in ordin that adds 1 to a Confluence version (ruling R19): callers pass the version they read, so a caller that passed `read + 1` would be refused with `CONF_ORIGIN_CHANGED` on every update.
4. `update_page_storage`: as step 3 with the given storage body, after checking it parses as XML (else `CONF_CONVERSION_FAILED`).
5. `attach`: with `expected_version = None` add a new attachment; with `Some(v)` read the attachment's version first (`≠ v` → `CONF_ORIGIN_CHANGED`) and post a new version of its data. Size above `MAX_ATTACHMENT_BYTES` → `CONF_TOO_LARGE`.
6. On success `journal.confirm_external_action(key)`, update `confluence_items` with the new version (so `check_changes` does not report ordin's own push), and for linked-folder pushes `record_published_copy` with `state = current`, `original_version_published = h`, `destination_version_pushed = new version`, `destination_item_id`. On a timeout after sending: read the item; version still the expected one → retry once with the same key; otherwise `CONF_OUTCOME_UNKNOWN`, the copy is set `failed` with reason "ordin isn't sure whether Confluence received this change." and nothing is resent automatically (02:747).
7. `delete(item, expected_version, key, run_id)`: read the item's version (404 → `CONF_NOT_FOUND`); `expected_version` present and different → `CONF_ORIGIN_CHANGED { current }` before sending; step 7.1 with kind `CopyPublish`; `DELETE` the page or attachment (Confluence moves it to the space's trash; nothing is purged); 204 → `journal.confirm_external_action(key)` and delete the item's `confluence_items` row so `check_changes` does not report ordin's own removal. Timeout after sending → read the item: 404 → confirm and return `Ok`; same version → retry once with the same key; otherwise `CONF_OUTCOME_UNKNOWN`.
8. After a crash, the Push Service (and this adapter's sync for linked folders) finds external actions in `sent` state; this adapter compares the destination version with `destination_version_pushed` before any new push of that copy and never resends on its own.

### 8. Audience (C-42)

1. Page: read restrictions on `read`; with restrictions, principals are the listed users and groups; line = up to two group names joined with " and ", plus "and N people" for individual users. Without page restrictions, read the space permissions: principals with `read` on `space`; a grant to anonymous → "Anyone on the internet"; to all licensed users (`confluence-users` group) → "Everyone in the company".
2. A 403 on either read, or an empty result → fallback `Audience { line: "everyone who can open it", everyone_who_can_open: true, principals: [] }`.

### 9. Conversions

1. `markdown_to_storage` (pulldown-cmark, CommonMark plus tables and strikethrough): headings → `<h1>`…`<h6>`; paragraphs → `<p>`; emphasis → `<em>`, `<strong>`, `<del>`; inline code → `<code>`; fenced code → `<ac:structured-macro ac:name="code"><ac:parameter ac:name="language">{lang}</ac:parameter><ac:plain-text-body><![CDATA[{code}]]></ac:plain-text-body></ac:structured-macro>` (a `]]>` inside code is split across two CDATA sections); lists → `<ul>`/`<ol>`/`<li>`; tables → `<table><tbody><tr><th>/<td>`; links → `<a href>` for `https`/`http`/`mailto` only (others become text); images with a relative path → `<ac:image><ri:attachment ri:filename="{name}"/></ac:image>` (the file is attached to the same page by the caller); block quotes → `<blockquote>`; rules → `<hr/>`; raw HTML in the Markdown is escaped and shown as text (ADR-028: no markup passes through). All text is XML-escaped.
2. `storage_to_markdown` (`quick-xml`, the same element set in reverse) for **Bring it in** (ADR-048); any other element or macro becomes the line `[Confluence content ordin can't bring back: {macro or element name}]`; malformed XML → `CONF_CONVERSION_FAILED`.
3. Word to page (C-13 confirmed and `word_to_markdown` wired): convert, then as step 9.1; a conversion error → the file is attached instead, and the copy's `format` is `exact_file` (02:743 "until then they are attached").

## Data Model

SQLite (desktop only).

```sql
CREATE TABLE confluence_link_state (
    source_link_id   TEXT PRIMARY KEY,
    space_key        TEXT NOT NULL,
    root_page_id     TEXT,                              -- NULL for a whole-space link
    tracked_at       TEXT NOT NULL,
    last_checked_at  TEXT NOT NULL,
    last_error       TEXT
);

CREATE TABLE confluence_items (
    source_link_id   TEXT NOT NULL REFERENCES confluence_link_state(source_link_id) ON DELETE CASCADE,
    item_key         TEXT NOT NULL,                     -- 'page:<id>' or 'attachment:<page id>:<id>'
    origin_path      TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    version_number   INTEGER NOT NULL,
    changed_at       TEXT NOT NULL,
    changed_by       TEXT,
    removed          INTEGER NOT NULL DEFAULT 0 CHECK (removed IN (0,1)),
    PRIMARY KEY (source_link_id, item_key)
);

CREATE TABLE confluence_origin_changes (
    id               TEXT PRIMARY KEY,
    source_link_id   TEXT NOT NULL REFERENCES confluence_link_state(source_link_id) ON DELETE CASCADE,
    origin_path      TEXT NOT NULL,
    new_version      TEXT NOT NULL,
    changed_by       TEXT,
    changed_at       TEXT NOT NULL,
    removed          INTEGER NOT NULL CHECK (removed IN (0,1)),
    found_at         TEXT NOT NULL
);
CREATE INDEX confluence_origin_changes_found ON confluence_origin_changes (source_link_id, found_at);

CREATE TABLE confluence_folder_pages (
    confluence_link_id  TEXT NOT NULL,
    folder_path         TEXT NOT NULL,                  -- relative to the link's folder; '' = the link root
    page_id             TEXT NOT NULL,
    PRIMARY KEY (confluence_link_id, folder_path)
);

CREATE TABLE confluence_ready_items (
    confluence_link_id  TEXT NOT NULL,
    kind                TEXT NOT NULL CHECK (kind IN ('new','renamed','removed')),
    path                TEXT NOT NULL,
    from_path           TEXT,
    published_copy_id   TEXT,
    found_at            TEXT NOT NULL,
    PRIMARY KEY (confluence_link_id, path)
);

CREATE TABLE confluence_publish_requests (
    id                  TEXT PRIMARY KEY,
    confluence_link_id  TEXT NOT NULL,
    approval_id         TEXT NOT NULL UNIQUE,
    items               TEXT NOT NULL,                  -- JSON Vec<ReadyItem>
    created_at          TEXT NOT NULL
);
```

Rows of `confluence_folder_pages`, `confluence_ready_items` and `confluence_publish_requests` for a link are deleted when the Workspace Service removes the link (`forget_link(link_id)`, an internal call it makes; no Confluence page is deleted, 04:120). Migration rollback is the SQLite backup (SA-XC-07).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No `confluence.site` in the profile, or Identity answers `CRED_PROVIDER_NOT_CONFIGURED` | `Err(permanent)` | `CONF_NOT_CONFIGURED` | `{"error":"Confluence isn't set up for your company in ordin.","code":"CONF_NOT_CONFIGURED","action":"send_to_it"}` |
| No sign-in, refused, or a second 401 | `Err(permanent)` until sign-in | `CONF_SIGN_IN_NEEDED` | `{"error":"Sign in to Confluence to see the pages linked to this workspace.","code":"CONF_SIGN_IN_NEEDED","action":"sign_in_again"}` |
| Signed-in account cannot reach the company site | `Err(permanent)` | `CONF_SITE_NOT_GRANTED` | `{"error":"This Atlassian account can't open your company's Confluence.","code":"CONF_SITE_NOT_GRANTED","action":"sign_in_again"}` |
| 403 | `Err(permanent)` | `CONF_ACCESS_DENIED` | `{"error":"You don't have access to this Confluence location.","code":"CONF_ACCESS_DENIED","action":null}` |
| 404 | `Err(permanent)` | `CONF_NOT_FOUND` | `{"error":"This Confluence page or space no longer exists.","code":"CONF_NOT_FOUND","action":null}` |
| Stored origin form malformed | `Err(permanent)` | `CONF_LOCATION_INVALID` | `{"error":"ordin can't use that Confluence location.","code":"CONF_LOCATION_INVALID","action":null}` |
| Search text empty or too long; more than 50 checked items | `Err(permanent)` | `CONF_QUERY_INVALID` | `{"error":"Type what to look for in Confluence.","code":"CONF_QUERY_INVALID","action":null}` |
| Page or attachment version differs from the one read, or 409 | `Err(permanent)` for this push | `CONF_ORIGIN_CHANGED` | `{"error":"This page was changed in Confluence after ordin read it.","code":"CONF_ORIGIN_CHANGED","action":null}` |
| A page with this title already exists in the space | `Err(permanent)` | `CONF_TITLE_EXISTS` | `{"error":"A page with that title is already in this space.","code":"CONF_TITLE_EXISTS","action":null}` |
| Body above 5 MiB, attachment above 100 MiB, or 413 | `Err(permanent)` | `CONF_TOO_LARGE` | `{"error":"This document is too large for Confluence.","code":"CONF_TOO_LARGE","action":null}` |
| 429 after retries | `Err(retryable)` | `CONF_THROTTLED` | `{"error":"Confluence asked ordin to slow down. ordin will try again shortly.","code":"CONF_THROTTLED","action":"try_again"}` |
| 5xx or network failure after retries | `Err(retryable)` | `CONF_UNAVAILABLE` | `{"error":"Can't reach Confluence right now.","code":"CONF_UNAVAILABLE","action":"try_again"}` |
| Timeout after sending and the destination moved | `Err(permanent)`, never retried automatically | `CONF_OUTCOME_UNKNOWN` | `{"error":"ordin isn't sure whether Confluence received this change.","code":"CONF_OUTCOME_UNKNOWN","action":null}` |
| Response body does not parse | `Err(permanent)` | `CONF_RESPONSE_INVALID` | `{"error":"Confluence sent an answer ordin couldn't read.","code":"CONF_RESPONSE_INVALID","action":"send_to_it"}` |
| Storage XHTML malformed (bring back, storage push) | `Err(permanent)` | `CONF_CONVERSION_FAILED` | `{"error":"ordin couldn't read this page's layout.","code":"CONF_CONVERSION_FAILED","action":null}` |
| Link not in the state the call needs | `Err(permanent)` | `CONF_LINK_NOT_ACTIVE` | `{"error":"This folder isn't published to Confluence right now.","code":"CONF_LINK_NOT_ACTIVE","action":null}` |
| Link has no tracking state (`check_changes`, `item_for_path`) | `Err(permanent)` (caller defect) | `CONF_NOT_TRACKED` | — (internal) |
| External-action key already used | `Err(permanent)` (caller defect) | `CONF_ACTION_KEY_USED` | — (internal; nothing sent) |
| Approval Service refused the request | `Err(retryable)` | `CONF_APPROVAL_FAILED` | `{"error":"ordin couldn't prepare the approval.","code":"CONF_APPROVAL_FAILED","action":"try_again"}` |
| File Service call failed during sync | `Err(retryable)` | `CONF_FILES_FAILED` | `{"error":"ordin couldn't read the folder's documents.","code":"CONF_FILES_FAILED","action":"try_again"}` |
| Own tables unreachable, or the Journal refused to record the external action for a reason other than a used key | `Err(retryable)` | `CONF_DATASTORE_UNAVAILABLE` | `{"error":"ordin couldn't reach its own records.","code":"CONF_DATASTORE_UNAVAILABLE","action":"try_again"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Confluence Adapter

  Background:
    Given a Confluence test double behind the Atlassian gateway, the site "https://acme.atlassian.net" in settings
    And Identity returning a Confluence token whose accessible resources include that site

  Scenario: Happy path — linking a folder asks one approval
    Given a proposed link of "Supplier quotes" to space "PROC" under page "Supplier pages"
    When request_link_approval is called
    Then the Approval Service receives question "Publish Supplier quotes to Confluence?", verb "Link and publish" and reject style not_now
    And its changes line is "Text documents become pages. Other files are attached to the folder's page as they are."

  Scenario: Happy path — the person's own edit is pushed automatically
    Given an active link and a current page copy of "rates.md"
    And the person saved a new version of "rates.md" in another app (latest version origin person)
    When sync_linked_folder is called
    Then an external action is begun before the page update and confirmed after it
    And the copy is current with the new original hash and the new page version

  Scenario: Error path — an agent's change is never pushed automatically
    Given an active link and a current page copy of "rates.md"
    And the latest version of "rates.md" has origin run
    When sync_linked_folder is called
    Then no request is sent to Confluence for "rates.md"
    And the copy is out_of_date and the report counts it as waiting_for_approval

  Scenario: Edge case — a new file waits in Ready to publish
    Given "notes.md" was added to the linked folder
    When sync_linked_folder is called
    Then a published copy in state ready_to_publish exists for "notes.md" and ready_to_publish lists it as new
    And nothing was sent to Confluence

  Scenario: Edge case — a renamed file is recognised by its content
    Given "draft.md" was renamed "final.md" with the same content
    When sync_linked_folder is called
    Then ready_to_publish lists "final.md" as renamed from "draft.md"

  Scenario: Error path — a page changed in Confluence is held
    Given the copy of "rates.md" was pushed as version 7 and Confluence now has version 8
    And the person changed "rates.md" locally
    When sync_linked_folder is called
    Then the copy becomes changed_at_destination and nothing is pushed
    And the local file is unchanged

  Scenario: Error path — update refused when the page moved on
    When update_page is called with expected_version "7" and the page is at version 8
    Then it fails with CONF_ORIGIN_CHANGED before any PUT is sent

  Scenario: Error path — timeout after sending is not resent
    Given the PUT times out and the page afterwards is at version 8, not 7
    When update_page is called with expected_version "7"
    Then it fails with CONF_OUTCOME_UNKNOWN and exactly one PUT was sent

  Scenario: Edge case — Word is attached until C-13
    Given no WordToMarkdown is wired
    When "terms.docx" is first seen in a linked folder
    Then its copy destination is confluence_attachment with format exact_file and maker none

  Scenario: Happy path — Markdown becomes storage format with the copy's line on top
    When publish_page is called with markdown "# Rates" and banner "Published copy of rates.md, from supplier-docs, branch main. Last updated 20 Oct 2026, 10:15. Make changes in the original."
    Then the created page body starts with an info macro holding that line, followed by "<h1>Rates</h1>"

  Scenario: Edge case — raw HTML in Markdown is shown as text
    When markdown_to_storage is called with "<script>alert(1)</script>"
    Then the result holds "&lt;script&gt;" and no script element

  Scenario: Edge case — audience cannot be read (C-42 fallback)
    Given the restriction request answers 403
    When audience is called for a page
    Then the line is "everyone who can open it"

  Scenario: Error path — signed in with an account that cannot reach the site
    Given the accessible resources do not include the site
    When find_locations is called
    Then it fails with CONF_SITE_NOT_GRANTED

  Scenario: Happy path — the caller passes the version it read and the adapter adds one (R19)
    Given page 4411 is at version 7
    When update_page is called with expected_version "7"
    Then the PUT carries version.number 8 and the returned version tag is "8"

  Scenario: Error path — a caller that adds one itself is refused
    Given page 4411 is at version 7
    When update_page is called with expected_version "8"
    Then it fails with CONF_ORIGIN_CHANGED carrying version 7 and no PUT is sent

  Scenario: Happy path — a published copy is moved to the trash
    Given the copy page 5120 is at version 3
    When delete is called for Page 5120 with expected_version "3" and key K
    Then a DELETE is sent and the external action K is begun and confirmed

  Scenario: Error path — a copy changed in Confluence is not removed
    Given the copy page 5120 is now at version 4
    When delete is called with expected_version "3"
    Then it fails with CONF_ORIGIN_CHANGED and no DELETE is sent

  Scenario: Happy path — the Push Service finds the page behind a working-copy path
    Given link L is tracked and holds page 4411 at path "Supplier pages/Rates.confluence.xhtml"
    When item_for_path is called with L and "Supplier pages/Rates.confluence.xhtml"
    Then it returns Page 4411 and no request is sent to Confluence

  Scenario: Error path — a decision for an approval this adapter did not ask is ignored
    Given confluence_publish_requests holds approval A1 only
    When the Journal delivers approval_decided for approval A2 with state approved
    Then nothing is sent to Confluence and no row changes
```

## Performance / Security / Observability

- **Performance targets.** Excluding Confluence's own time: conversion of a 1 MiB Markdown document ≤ 50 ms p95; `sync_linked_folder` over 1,000 unchanged files ≤ 2 s p95 (one File Service listing plus no network calls, because unchanged files need no request); `check_changes` with no change ≤ 1 s p95 (one CQL query). At most 4 concurrent Confluence requests (*chosen here*).
- **Security.** The person's own token only (ADR-026); the token is never logged, stored or sent to another host. A push checks the destination version first and sends the version number Confluence requires, so no page is overwritten without detection; a page changed in Confluence is never overwritten without the person's choice; nothing in Confluence is deleted by this adapter. Changes by Ordin or an agent are never pushed automatically (ADR-029). Every write is an external action begun before sending. Markdown raw HTML is escaped; CQL input is quoted; Confluence content is data. The paging cursor is accepted only when it decodes to a `/wiki/` path.
- **Never logged.** Tokens, page bodies, titles, file names, search text, author names.
- **Observability (logs only; SA-XC-04/05).** `tracing` events with `component = "confluence"`: `confluence_request {operation, status, elapsed_ms, attempt}`, `confluence_link_approval_requested {confluence_link_id}`, `confluence_link_synced {confluence_link_id, pushed, waiting_for_approval, ready_to_publish, changed_in_confluence, failed}`, `confluence_pushed {kind: "page"|"attachment", created: bool}`, `confluence_outcome_unknown {action_key}`, `confluence_changes_found {source_link_id, count}`.

## Gaps

None.
