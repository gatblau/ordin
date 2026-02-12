# SPEC: Confluence Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-026, ADR-029: approved content bound to what is sent, a failed first push, the audience read without the restriction scope; awaiting the user's review) · amended 2026-01-10 (automatic publishing off where an agent that can run commands may change the folder, and changed files through **Publish checked items…**, SA-CONF-AUTO-01 to 04, and a failed copy back on **Ready to publish**, SA-CONF-AUTO-05; approved by the user on 2026-01-10) · amended 2026-01-29 (the connection settings a test may override, CONF-SEAM-1 to CONF-SEAM-3; Approved by the user 2026-01-29)
> **Mode:** greenfield
> **derivedFromHld:** 0.66.0
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
- [Amendment 2026-01-10 — Automatic publishing off where an agent can run commands](#amendment-2026-01-10--automatic-publishing-off-where-an-agent-can-run-commands)
- [Amendment 2026-01-29 — The connection settings a test may override](#amendment-2026-01-29--the-connection-settings-a-test-may-override)

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
pub enum ExternalActionState { Sent, Confirmed, UnknownOutcome, Refused }
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
- **C-42** — Confluence states who can see a space or page in one line. *Fallback:* the card names the space and says "everyone who can open it", and a grown audience cannot be detected (10 C-42). Atlassian refusing the restriction-read scope does not trigger it: the page's restrictions go unread and the space's permissions answer (step 8.1).

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
    #[error("SRV_BAD_CURSOR")] BadCursor, // the shared code of Internal Logic 1.4, as the SharePoint Adapter's enum has it
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
//   where_line "Procurement team (42 people) — Confluence — Procurement › Supplier pages",
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
4. Paging follows `_links.next` inside the adapter; the `Cursor` handed to the caller is base64 of the next relative link (at most `CURSOR_MAX_CHARS`), validated to start with `/wiki/` when decoded (else `SRV_BAD_CURSOR`, returned as `ConfluenceError::BadCursor`; the same variant answers a cursor `list_children` is given that does not decode, and a `loc` that has no `page_id` where the cursor names a page).
5. Log `confluence_request {operation, status, elapsed_ms, attempt}` only.

### 2. Reading

1. `find_locations`: `query` trimmed 1–200 (else `CONF_QUERY_INVALID`); CQL text is built with the query as a quoted string, `"` and `\` escaped, so no CQL operator from the person reaches the search; results map to `ConfluenceLocation` (spaces with `page_id = None`).
2. `list_children`: for a space, its root pages; for a page, its child pages then its attachments. Paths: a page `P` under the link root is `<ancestor titles>/<P title>.confluence.xhtml`; its attachments are `<ancestor titles>/<P title>/<file name>`. A `/` in a title is replaced by `∕` (U+2215) in paths only (*chosen here*).
3. `fetch(Page)`: body in storage format; body larger than `MAX_PAGE_BODY_BYTES` → `CONF_TOO_LARGE`; bytes are the UTF-8 storage XHTML. `fetch(Attachment)`: size above `MAX_ATTACHMENT_BYTES` → `CONF_TOO_LARGE`; bytes are the file. Both re-read the version after the transfer and repeat once if it moved, then `CONF_ORIGIN_CHANGED`.
4. The Workspace Service writes fetched bytes to the working copy (B7 contract D-5); this adapter never writes to a place.
5. `resolve_origin`: the page id is read from `confluence:page/{id}` (a malformed form → `CONF_LOCATION_INVALID`). The surface table has no read of a space by id, and a page answers only its `spaceId`; the search row (`GET /wiki/rest/api/search`) with the clause `id={id}` and `limit=1` answers the space key and name together with the page, and maps to the `ConfluenceLocation`. No hit → `CONF_NOT_FOUND`.

### 3. Change tracking (A-25; B7 contract D-8)

1. `track_link(link)`: resolve its origin; walk the tree (≤ `MAX_TREE_PAGES`, 20 levels), store each page and attachment with its version number in `confluence_items`; store `root_page_id` or `space_key` and `last_checked_at = now` in `confluence_link_state`.
2. `check_changes(link)`: run the change CQL with `lastmodified >= last_checked_at − 2 minutes` (overlap for clock skew, *chosen here*); for each page in the result, read its version and its attachments' versions; a version number above the stored one → `OriginChange` (author resolved by account id) and update the row; an item stored but not found when its parent page is re-listed → `removed: true`. Insert the changes into `confluence_origin_changes` and update `last_checked_at` in one transaction.
3. `changes_since`: as the SharePoint Adapter: rows with `found_at > since`, newest first, ≤ 1,000. Rows older than 90 days are deleted at each check (*assumed here*).
4. `item_for_path(link, origin_path)`: no `confluence_link_state` row for `link` → `CONF_NOT_TRACKED`; select the `confluence_items` row with that `origin_path` and `removed = 0`; none → `CONF_NOT_FOUND`; `item_key` `page:<id>` → `RemoteItemRef::Page`, `attachment:<page id>:<id>` → `RemoteItemRef::Attachment`. The caller reads the live version with `item_version`.

### 4. Linking a folder (`request_link_approval`)

1. `link.state` must be `proposed` (else `CONF_LINK_NOT_ACTIVE`).
2. Resolve the space and parent page (`CONF_NOT_FOUND` if gone); read `audience` for the parent page (step 8).
3. List the folder's files through `files.list_folder` (all pages, folders recursed, at most 50 files named in `content`, the count in `affects`).
4. Build the `ApprovalRequest` (words from 11 §5.14): `question` = "Publish {folder friendly name} to Confluence?"; `verb` = "Link and publish"; `reject_style` = `NotNow`; `where_line` = "{audience.line} — Confluence — {space name} › {parent page title}" cut to 200 chars (the audience leads, so the cut removes the end of the location and never the audience); `affects` = one `File` item "{N} documents in {folder name}"; `changes` = "Text documents become pages. Other files are attached to the folder's page as they are."; `content` = `Files { label: "See the files", files }`; `show_details` = true; `asked_by` = `Person`; `proposal` = `ActionProposal { action: "Link a folder to Confluence and publish it", destination: where_line, affects, consequence: changes }`; `asker` = None; `leaves_company` = false.
5. `approvals.request(..)` → `card.approval_id`, returned to the Workspace Service, which stores it on its `confluence_links` row and activates the link when that approval is decided (it owns the row; SA-RECON-05). Failure → `CONF_APPROVAL_FAILED` (retryable). `journal.append_entry(NewJournalEntry { kind: "confluence_link_approval_requested", component: "confluence", workspace_id: Some(link.workspace_id), summary: "Asked to link a folder to Confluence.", data: {"confluence_link_id": link.id}, .. })`.

### 5. Linked-folder sync (`sync_linked_folder`)

Called by the Workspace Service when a link becomes active, when its workspace opens, at start, every 15 minutes, and on a `PlaceChange` inside the linked folder (debounced by the caller). `link.state` must be `active` (paused or proposed → `CONF_LINK_NOT_ACTIVE`).

1. Load the link's copies (`files.published_copies_via_link`) keyed by original path, and the folder → page map (`confluence_folder_pages`).
2. Walk the folder's files (`list_folder`, every page, recursing) and, for each file `f` with hash `h` (`read_file` hash, or the index hash when the File Service returns it): a `read_file` that fails with `FILE_TOO_LARGE` (a file above the File Service's `READ_LIMIT_BYTES`) counts one in `failed` and the walk goes on with the next file, so one such file does not stop the others on every run; any other `read_file` failure → `CONF_FILES_FAILED`.
   1. **No copy** → create a `PublishedCopy` with `state = ready_to_publish`, `published_via = ConfluenceLink`, `destination` = `ConfluencePage` for `TEXT_KINDS` (and `docx` when `WordToMarkdown` is wired) or `ConfluenceAttachment` otherwise, `format` = `page` or `exact_file`, `maker` = `ordin` or `none`, `audience` = `link.audience`, `keep_up_to_date = true`; add a `ReadyItem::New`. A removed copy whose hash equals `h` at another path is a rename instead: `ReadyItem::Renamed { from_path }`.
   2. **Copy `current`, `h` differs from `original_version_published`** → `files.agent_write_open(f)` true means a run may be writing it now (FileService.md Internal Logic 6a): count `waiting_for_approval`, leave the copy as it is, and decide at the next sync. Otherwise read `files.versions(f)`. A write reaches the disk before its version row, and a file too large to index has none, so a latest version whose hash is not `h`, or no version at all, leaves the bytes with no known author. If that is so, or any version newer than the one whose hash is `original_version_published` has origin `Run` or `ArtCraftEdit` (Ordin or an agent changed it, and a person's later edit would carry that text out with it), set the copy `out_of_date` and count `waiting_for_approval`: the Push Service sends it through the request's approval card (ADR-029: "A change Ordin made is never pushed automatically, even in a folder linked under ADR-026"). Otherwise it is the person's own change: push it automatically (step 7) after checking the destination (step 5.3); count `pushed`.
   3. **Before any automatic push** read the destination's current version; above `destination_version_pushed` → set `changed_at_destination` and count `changed_in_confluence` ("Changed in Confluence", with **Bring a copy back** and **Keep mine**, 11 §5.14); the file is not pushed until the person chooses (ADR-026).
   4. Copies in `changed_at_destination`, `not_kept_up_to_date`, `awaiting_approval`, `updating` or `removed` are not pushed by the sync.
3. A copy whose file no longer exists → `ReadyItem::Removed` (nothing is deleted in Confluence without the person's check, 04 *Data Lifecycle*).
4. Upsert `confluence_ready_items` with the current Ready list (new, renamed, removed), replacing the link's previous list; `journal.append_entry` kind `confluence_link_synced`, component `confluence`, with the report counts in `data`.
5. Text pages: a folder's page is created on the first push that needs it (title = the folder's name, parent = the parent folder's page or `link.parent_page_id`), recorded in `confluence_folder_pages`; attachments go to the folder's page (ADR-026).

### 6. Publishing checked items (`request_publish_checked`)

1. Build one card (11 §5.14 "Publish checked items…"): question "Publish {N} checked items to Confluence?", verb "Publish", where as step 4.4, affects one `File` per item (at most 50 per card; a larger selection is refused with `CONF_QUERY_INVALID` and the screens split it; `version` = the number of the file's latest `FileVersion` when that version's hash equals the file as read, otherwise absent), changes listing new, renamed and removed counts, `asked_by` = `Person`, `proposal` = `ActionProposal { action: "Publish checked items to Confluence", destination: where_line, affects, consequence: changes }`, `show_details` = false, `asker` = None, `leaves_company` = false; `approvals.request` → `card.approval_id`; store the request with that `approval_id` in `confluence_publish_requests`. Before the card is raised, read the file of every `New` and `Renamed` item (the link's folder plus `path`) and keep its SHA-256 as the item's `content_hash` on the stored request; a `Removed` item sends no content and has none. An item whose bytes step 5.2.2 gives no person as author, or that has an open write claim, is marked `agent_written` on the stored request: `changes` ends "An agent may have written {n} of them." ("1 of them" for one), `content` is `Files { label: "See the files", files }` naming those items, and `show_details` is true. Each is `ContentRef::FileVersion` of the kept version with that hash, so the card shows the bytes the approval binds even if the file moves on; only when no version holds them is it `PlaceFile`, and the file then differing from the hash holds the item at send time. A file with no versions and no claim is the person's new file and is not marked. A file that cannot be read → `CONF_FILES_FAILED` and no card. The hash is this adapter's own record: `ReadyItem` does not carry it, so a caller cannot supply it.
2. The sink from `event_sink()` receives every `JournalEvent`; it acts only on `kind = "approval_decided"` whose `data.approval_id` is in `confluence_publish_requests` (other events and other approval ids are ignored; `lagged(next_seq)` re-reads the missed events through the Journal's `events_since`). `state = approved` → for each item: read the file once; a hash that differs from the item's `content_hash`, or no `content_hash`, refuses that item without sending anything: its copy keeps its state, it stays in Ready to publish for a fresh approval, and `confluence_item_held` is logged with reason `changed_since_card`. What is sent is the bytes so checked. An item marked `agent_written` goes: the card named the agent and the hash proves these are the bytes it covered (ADR-029). An unmarked `New` also applies step 6.1's test at send time: if an agent is now the author of the same bytes, the item is refused the same way with reason `changed_by_agent`. Each refusal also journals `confluence_item_held`, component `confluence`, `data` `{confluence_link_id, path, reason}`, with the summary "An approved file was not published: it changed after the card. Publish it again for a new card." or "An approved file was not published: an agent changed it and the card did not say so. Publish it again for a new card.". Then `New` → set the copy `awaiting_approval` (`updating` when it is `failed`, the fresh approval being its **Try again**) then push (step 7) and set `current`; if the push fails, set the copy `failed` with the reason from step 7.6 and keep the item in Ready to publish. `Renamed` → update the page title or re-upload the attachment under the new name, update the copy's original path; `Removed` → set the copy `removed` and leave the Confluence page in place (deleting it at the destination is the Push Service's **Remove copy…** with its own check, ADR-069). `rejected` or `expired` → items stay in Ready to publish. Delete the request row.

A renamed attachment sends the file's bytes, so it is marked and held under the same rule as `New` (ADR-029), and the copy records their hash as `original_version_published`. When Confluence then holds the bytes of the file, `keep_published_version` starts the new path's history. A renamed page whose file differs from what was published keeps its `original_version_published`, and the next sync finds a change with no known author. A renamed page sends only its title.

### 7. Pushing a page or attachment

1. External action first: `journal.begin_external_action(NewExternalAction { action_key: key, run_id, kind, destination: "Confluence › {space} › {page title}" })` (`{space}` is the space key when the call is `publish_page`, which has `dest.space`; for every other write only the page is at hand and the page's `spaceId` stands in, because the surface table has no read of a space by id; the destination is cut to the Journal's 2,048 characters) with `kind` = `OriginPush` for `update_page_storage` (a working-copy page sent back to its origin) and `CopyPublish` for every other write, including an attachment the Push Service sends back to its origin (the Push Service's recovery reads both kinds); a `JournalError` whose `code()` is `EXT_KEY_DUPLICATE` → `CONF_ACTION_KEY_USED`, nothing sent; any other `JournalError` → `CONF_DATASTORE_UNAVAILABLE`, nothing sent. This is the only record of the write: the caller mints `key` (UUIDv7) and opens no external action of its own (single bracket, reconciliation row 711). Automatic linked-folder pushes create the key here (UUIDv7) and pass `run_id: None` (B7 contract D-12).
2. `publish_page`: `markdown_to_storage(markdown)`; with `banner`, prepend `<ac:structured-macro ac:name="info"><ac:rich-text-body><p>{banner escaped}</p></ac:rich-text-body></ac:structured-macro>` (ADR-069 "one line at the top of every copy"); resolve the space id from `dest.space`; create the page under `dest.parent_page_id`. `CONF_TITLE_EXISTS` → returned; the Push Service offers **Use another name**.
3. `update_page`: read the page's version; `≠ expected_version` → `CONF_ORIGIN_CHANGED { current }` before sending; send with `version.number = expected + 1`; a 409 → `CONF_ORIGIN_CHANGED`. This step is the only place in ordin that adds 1 to a Confluence version (ruling R19): callers pass the version they read, so a caller that passed `read + 1` would be refused with `CONF_ORIGIN_CHANGED` on every update.
4. `update_page_storage`: as step 3 with the given storage body, after checking it parses as XML (else `CONF_CONVERSION_FAILED`).
5. `attach`: with `expected_version = None` add a new attachment; with `Some(v)` read the attachment's version first (`≠ v` → `CONF_ORIGIN_CHANGED`) and post a new version of its data. Size above `MAX_ATTACHMENT_BYTES` → `CONF_TOO_LARGE`.
6. On success `journal.confirm_external_action(key)`, update `confluence_items` with the new version (so `check_changes` does not report ordin's own push), and for linked-folder pushes `files.keep_published_version` (a file with no versions keeps the bytes sent as version 1, so that its next change is recorded with an author; a failure is logged and leaves the file with no known author, which step 5.2.2 sends to a card) and `record_published_copy` with `state = current`, `original_version_published = h`, `destination_version_pushed = new version`, `destination_item_id`. On a timeout after sending: read the item; version still the expected one → retry once with the same key; otherwise `CONF_OUTCOME_UNKNOWN`, the copy is set `failed` with reason "ordin isn't sure whether Confluence received this change." and nothing is resent automatically (02:747). On a first push (step 6.2) the reason is instead "ordin isn't sure whether Confluence received this. The page or attachment may already be live, so check Confluence before trying again."; any other failure of a first push gives "ordin couldn't publish this to Confluence ({code}).".
7. `delete(item, expected_version, key, run_id)`: read the item's version (404 → `CONF_NOT_FOUND`); `expected_version` present and different → `CONF_ORIGIN_CHANGED { current }` before sending; step 7.1 with kind `CopyPublish`; `DELETE` the page or attachment (Confluence moves it to the space's trash; nothing is purged); 204 → `journal.confirm_external_action(key)` and delete the item's `confluence_items` row so `check_changes` does not report ordin's own removal. Timeout after sending → read the item: 404 → confirm and return `Ok`; same version → retry once with the same key; otherwise `CONF_OUTCOME_UNKNOWN`.
8. After a crash, the Push Service (and this adapter's sync for linked folders) finds external actions in `sent` state; this adapter compares the destination version with `destination_version_pushed` before any new push of that copy and never resends on its own.

### 8. Audience (C-42)

1. Page: without the restriction-read scope the restrictions are not read and the space permissions below answer, since a restriction only narrows what the space allows. Otherwise read restrictions on `read`; with restrictions, principals are the listed users and groups; line = up to two group names joined with " and ", plus "and N people" for individual users. Without page restrictions, read the space permissions: principals with `read` on `space`; a grant to anonymous → "Anyone on the internet"; to all licensed users (`confluence-users` group) → "Everyone in the company"; a group principal with no name in the reply is shown by its id in the line and tested against `confluence-users` by id as well as by name. Only a principal of type `user` counts as a person; a principal of any type other than `anonymous`, `group` or `user` is not known, is counted as nobody, and `people` is then absent: the line is "Audience not known", or "{line} and others not known" when others are named.
2. A 403 on either read, or an empty result → fallback `Audience { line: "everyone who can open it", everyone_who_can_open: true, principals: [] }`.

### 9. Conversions

1. `markdown_to_storage` (pulldown-cmark, CommonMark plus tables and strikethrough): headings → `<h1>`…`<h6>`; paragraphs → `<p>`; emphasis → `<em>`, `<strong>`, `<del>`; inline code → `<code>`; fenced code → `<ac:structured-macro ac:name="code"><ac:parameter ac:name="language">{lang}</ac:parameter><ac:plain-text-body><![CDATA[{code}]]></ac:plain-text-body></ac:structured-macro>` (a `]]>` inside code is split across two CDATA sections); lists → `<ul>`/`<ol>`/`<li>`; a bulleted list whose every item has a tick box (`- [ ]`, `- [x]`) → `<ac:task-list>` of `<ac:task><ac:task-id>{n}</ac:task-id><ac:task-status>incomplete|complete</ac:task-status><ac:task-body>…</ac:task-body></ac:task>`, `n` counting the page's tasks from 1 (a tick box in any other list stays as the text `[ ]` / `[x]`; amended 2026-02-11 after the 0.2.1-test.18 smoke test showed task lists coming back as unreadable lines); tables → `<table><tbody><tr><th>/<td>`; links → `<a href>` for `https`/`http`/`mailto` only (others become text); images with a relative path → `<ac:image><ri:attachment ri:filename="{name}"/></ac:image>` (the file is attached to the same page by the caller); block quotes → `<blockquote>`; rules → `<hr/>`; raw HTML in the Markdown is escaped and shown as text (ADR-028: no markup passes through). All text is XML-escaped.
2. `storage_to_markdown` (`quick-xml`, the same element set in reverse, a task list as `- [x]` for a `complete` task and `- [ ]` otherwise; a table cell's paragraphs and lists are read as blocks and joined with spaces onto the cell's one line, and a `<thead>` row is read as a `<tbody>` row is) for **Bring it in** (ADR-048); any other element or macro becomes the line `[Confluence content ordin can't bring back: {macro or element name}]`; malformed XML → `CONF_CONVERSION_FAILED`.
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
    items               TEXT NOT NULL,                  -- JSON Vec<ApprovedItem>: each ReadyItem with its content hash
    created_at          TEXT NOT NULL
);
```

`ApprovedItem` (private to the crate) is `{ item: ReadyItem, content_hash: Option<String> }`, the hash being SHA-256 hex and absent for `Removed`. Rows of `confluence_folder_pages`, `confluence_ready_items` and `confluence_publish_requests` for a link are deleted when the Workspace Service removes the link (`forget_link(link_id)`, an internal call it makes; no Confluence page is deleted, 04:120). Migration rollback is the SQLite backup (SA-XC-07).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No `confluence.site` in the profile, or Identity answers `CRED_PROVIDER_NOT_CONFIGURED` | `Err(permanent)` | `CONF_NOT_CONFIGURED` | `{"error":"Confluence isn't set up for your company in ordin.","code":"CONF_NOT_CONFIGURED"}` |
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
| Response body does not parse | `Err(permanent)` | `CONF_RESPONSE_INVALID` | `{"error":"Confluence sent an answer ordin couldn't read.","code":"CONF_RESPONSE_INVALID"}` |
| Storage XHTML malformed (bring back, storage push) | `Err(permanent)` | `CONF_CONVERSION_FAILED` | `{"error":"ordin couldn't read this page's layout.","code":"CONF_CONVERSION_FAILED","action":null}` |
| Link not in the state the call needs | `Err(permanent)` | `CONF_LINK_NOT_ACTIVE` | `{"error":"This folder isn't published to Confluence right now.","code":"CONF_LINK_NOT_ACTIVE","action":null}` |
| Link has no tracking state (`check_changes`, `item_for_path`) | `Err(permanent)` (caller defect) | `CONF_NOT_TRACKED` | — (internal) |
| External-action key already used | `Err(permanent)` (caller defect) | `CONF_ACTION_KEY_USED` | — (internal; nothing sent) |
| Approval Service refused the request | `Err(retryable)` | `CONF_APPROVAL_FAILED` | `{"error":"ordin couldn't prepare the approval.","code":"CONF_APPROVAL_FAILED","action":"try_again"}` |
| File Service call failed during sync or while a card is built | `Err(retryable)` | `CONF_FILES_FAILED` | `{"error":"ordin couldn't read the folder's documents.","code":"CONF_FILES_FAILED","action":"try_again"}` |
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

  Scenario: Error path — a harness write is the run's and is not pushed
    Given an active link and a current page copy of "rates.md"
    And a run's Write to "rates.md" has opened its claim and reached the disk
    When sync_linked_folder is called
    Then no request is sent, the copy stays current and the report counts it as waiting_for_approval
    When the call settles and sync_linked_folder is called
    Then the latest version has origin Run with that run and tool
    And no request is sent and the copy is out_of_date

  Scenario: Error path — a path with .. is not pushed
    Given an active link and a current page copy of "rates.md"
    And a run's Write names "nope/../rates.md" and reaches "rates.md"
    When sync_linked_folder is called
    Then no request is sent, the copy stays current and the report counts it as waiting_for_approval

  Scenario: Error path — a rename that sent changed bytes records them
    Given a renamed attachment whose file changed before its card
    When the card is approved
    Then the copy's original_version_published is the hash sent and the new path has a version

  Scenario: Error path — a person's edit does not carry an agent's text out
    Given a current page copy of "rates.md" with a version of origin Run since it was published
    And the latest version has origin Person
    When sync_linked_folder is called
    Then no request is sent and the copy is out_of_date

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
    When publish_page is called with markdown "# Rates" and banner "Published copy of rates.md, from supplier-docs, branch main. Last updated 16 Jan 2026, 10:15. Make changes in the original."
    Then the created page body starts with an info macro holding that line, followed by "<h1>Rates</h1>"

  Scenario: Edge case — raw HTML in Markdown is shown as text
    When markdown_to_storage is called with "<script>alert(1)</script>"
    Then the result holds "&lt;script&gt;" and no script element

  Scenario: Edge case — audience cannot be read (C-42 fallback)
    Given the restriction request answers 403
    When audience is called for a page
    Then the line is "everyone who can open it"

  Scenario: Edge case — the restriction scope is refused and the space still answers
    Given Atlassian refused the restriction-read scope and the space grants read to anonymous
    When audience is called for a page
    Then no restriction request is sent and the line is "Anyone on the internet"

  Scenario: Edge case — a reader of an unknown kind is not a person
    Given the space grants read to a principal of type "role"
    When audience is called for the space
    Then the line is "Audience not known" and people is absent

  Scenario: Error path — a file changed after its card is not sent
    Given "notes.md" is on an approval card and then changes
    When the card is approved
    Then nothing is sent, the copy stays ready_to_publish and the item stays in Ready to publish

  Scenario: Happy path — an agent's new file goes once the card names it
    Given the latest version of "notes.md" has origin Run and it is on an approval card
    Then the card says an agent may have written it and shows the file
    When the card is approved
    Then the page is created and the item leaves Ready to publish

  Scenario: Error path — a card that did not name the agent holds the item and says why
    Given "notes.md" is on a card that does not name an agent
    And a run then writes the same bytes as a new version
    When the card is approved
    Then nothing is sent, the item stays in Ready to publish and confluence_item_held is journalled with the reason in plain words

  Scenario: Error path — a failed first push fails the copy
    Given a card for "notes.md" is approved and Confluence answers 403
    When the push runs
    Then the copy is failed with the reason "ordin couldn't publish this to Confluence (CONF_ACCESS_DENIED)."

  Scenario: Error path — a first push with an unknown outcome says the page may be live
    Given a card for "notes.md" is approved and the create answers 500
    When the push runs
    Then the copy is failed, its reason says the page or attachment may already be live, and one POST was sent

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
- **Security.** The person's own token only (ADR-026); the token is never logged, stored or sent to another host. What is sent from an approval card is the bytes the card was made from, proved by hash at send time. A push checks the destination version first and sends the version number Confluence requires, so no page is overwritten without detection; a page changed in Confluence is never overwritten without the person's choice; nothing in Confluence is deleted by this adapter. Changes by Ordin or an agent are never pushed automatically (ADR-029). Every write is an external action begun before sending. Markdown raw HTML is escaped; CQL input is quoted; Confluence content is data. The paging cursor is accepted only when it decodes to a `/wiki/` path.
- **Never logged.** Tokens, page bodies, titles, file names, search text, author names.
- **Observability (logs only; SA-XC-04/05).** `tracing` events with `component = "confluence"`: `confluence_request {operation, status, elapsed_ms, attempt}`, `confluence_link_approval_requested {confluence_link_id}`, `confluence_link_synced {confluence_link_id, pushed, waiting_for_approval, ready_to_publish, changed_in_confluence, failed}`, `confluence_pushed {kind: "page"|"attachment", created: bool}`, `confluence_outcome_unknown {action_key}`, `confluence_changes_found {source_link_id, count}`, `confluence_item_held {confluence_link_id, reason: "changed_since_card"|"changed_by_agent"}`.

## Gaps

None.

---

## Amendment 2026-01-10 — Automatic publishing off where an agent can run commands

> Status: approved by the user on 2026-01-10 (user decision of 2026-01-10: automatic Confluence publishing is off for a linked folder that an agent with shell access can write). Counterparts: `components/WorkspaceService.md`, amendment 2026-01-10 (SA-WS-AUTO-01 to 03: which folders, and the value the folder reports); `components/PushService.md`, amendment 2026-01-10 (SA-PUSH-LINKED-01: the Push Service makes no automatic update of a linked folder's copies; SA-PUSH-LINKED-02 and 03: no **Try again** and no **Update copies** for them).
> Every rule below adds to the sections above; steps 5.2.2, 6.1 and 6.2 gain the text below.

### Purpose (addition)

A process that an agent's shell started can outlive its run, and the File Service then records its edits as the person's (FileService.md, *Limits of the claims*). In a linked folder such an edit would be pushed automatically with no card. Where an agent that can run commands may change the folder, this adapter pushes nothing automatically: every change to a published file waits in **Ready to publish** as a changed item and goes to Confluence only through the **Publish checked items…** card. The same path now carries the changes step 5.2.2 already held back as an agent's, which until now had no card that could send them.

### Approach (addition)

The Workspace Service, which owns grants, decides per sync whether automatic publishing is on for the link and passes the answer to this adapter; this adapter does not read grants or rule sets. The card path is the existing **Publish checked items…** card, so a changed file is bound by its content hash and named as an agent's in the same way as a new file (step 6.1). Rejected: *a card raised by the sync for each changed file* — a card would be raised at every sync while one waits, and again after the person rejects it, unless a new record kept what was rejected; *turning the link to `paused`* — new files would no longer reach Ready to publish, and the person's **Resume** would turn automatic publishing back on.

Assumptions (*chosen here*):

- **SA-CONF-AUTO-01** `AutomaticPublishing` is `On` or `Off`. `sync_linked_folder_with(link, place, publishing)` runs step 5 with it; `sync_linked_folder(link, place)` is `sync_linked_folder_with(link, place, AutomaticPublishing::On)`. The Workspace Service, the one program caller, calls `sync_linked_folder_with` with the value it derives (WorkspaceService.md SA-WS-AUTO-01).
- **SA-CONF-AUTO-02** With `Off`, step 5.2.2 pushes nothing automatically. A `current` copy whose file's hash differs from `original_version_published` and has no open write claim is set `out_of_date` and counted in `waiting_for_approval`, whatever its versions say. An open write claim keeps its existing rule: the copy stays as it is and is counted in `waiting_for_approval`. Steps 5.2.1 (new and renamed files) and 5.3 (removed files) are unchanged.
- **SA-CONF-AUTO-03** `ReadyKind` gains `Changed`. Each sync lists, as `Changed` with its `path` and `published_copy_id`, every copy of the link in state `out_of_date` whose file exists, with `On` or `Off`; this takes in the copies step 5.2.2 sets `out_of_date` for an agent's change. Step 6.1 treats a `Changed` item as it treats `New`: the file's SHA-256 is its `content_hash`, the item is marked `agent_written` by the same test, and its `affects` entry carries the version the same way. The card's `changes` line is "{n} new, {c} changed, {r} renamed, {d} removed." when `c` is above 0, and "{n} new, {r} renamed, {d} removed." otherwise.
- **SA-CONF-AUTO-04** Step 6.2, `Changed`, on approval: read the file once; a hash that differs from `content_hash` holds the item with reason `changed_since_card`, and the agent tests of `New` hold it with reason `changed_by_agent`, as for `New`. Then read the destination's version (step 5.3): above `destination_version_pushed` → the copy is set `changed_at_destination` (the person chooses **Bring a copy back** or **Keep mine**, 11 §5.14), nothing is sent, and the item is held with reason `changed_in_confluence`. Otherwise the copy is set `updating`, the checked bytes are pushed (step 7, the version read as `expected_version`) and the copy becomes `current`; a push that fails sets it `failed` with the reason of step 7.6.
- **SA-CONF-AUTO-05** A copy of the link in state `failed` whose file exists goes back on the Ready list at each sync, with `On` or `Off`: as `New` when it has no `destination_item_id` (its first push failed), and as `Changed` when it has one and the file's hash differs from `original_version_published`. A failed copy whose file's hash equals `original_version_published` is not listed. The sync sends nothing for it and its state stays `failed`. The Push Service's **Try again** refuses such a copy (PushService.md SA-PUSH-LINKED-02), so this card is the one way it is sent again. Step 6.1 binds the item as any `New` or `Changed`; on approval step 6.2 sets the copy `updating` from `failed` (the fresh approval being its **Try again**) and pushes the checked bytes. For a `Changed` item whose page's version is above `destination_version_pushed`, nothing is sent, the item is held with reason `changed_in_confluence` and the copy stays `failed`, as the state table has no move from `failed` to `changed_at_destination`.

### Public Interface (additions)

```rust
/// Whether the person's own changes in a linked folder are pushed without a card (SA-CONF-AUTO-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticPublishing { On, Off }

impl ConfluenceAdapter {
    /// Step 5 with automatic publishing as the Workspace Service derived it (SA-CONF-AUTO-02).
    pub async fn sync_linked_folder_with(&self, link: &ConfluenceLink, place: &Place,
                                         publishing: AutomaticPublishing) -> Result<LinkSyncReport, ConfluenceError>;
}

// ReadyKind gains one variant (SA-CONF-AUTO-03):
pub enum ReadyKind { New, Renamed, Removed, Changed }
```

The Service API's copy of `ReadyKind` (`ordin-service-api`, `confluence.ready_to_publish` and `confluence.request_publish_checked`) gains `changed` with it.

### Internal Logic (additions)

- **Step 5.2.2 (addition).** SA-CONF-AUTO-02 applies when the sync runs with `Off`.
- **Step 5.2.4 (addition).** A `failed` copy is not pushed by the sync; SA-CONF-AUTO-05 lists it.
- **Step 5.4 (addition).** The Ready list written to `confluence_ready_items` includes the `Changed` items of SA-CONF-AUTO-03 and the `New` and `Changed` items of SA-CONF-AUTO-05.
- **Step 6.1 (addition).** SA-CONF-AUTO-03.
- **Step 6.2 (addition).** SA-CONF-AUTO-04. The words of the held item with reason `changed_in_confluence` are "An approved file was not published: its page changed in Confluence. Choose Bring a copy back or Keep mine, then publish it again."

### Data Model (migration step 2)

`ordin-confluence` migration step 2, `confluence ready items changed`: `confluence_ready_items.kind` gains `changed`. SQLite cannot widen a `CHECK`, so the step rebuilds the table in the migration's transaction: create `confluence_ready_items_new` with `kind TEXT NOT NULL CHECK (kind IN ('new','renamed','removed','changed'))` and every other column as step 1 has it; copy every row; drop `confluence_ready_items`; rename the new table to `confluence_ready_items`. No other table changes. Rollback is the SQLite backup (SA-XC-07).

### Error Table (rows added)

None. A held `Changed` item is journalled as `confluence_item_held` (step 6.2), not returned as an error.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Confluence Adapter — automatic publishing off, and changed files through the card

  Scenario: Error path — with automatic publishing off, the person's own edit is not pushed (SA-CONF-AUTO-02)
    Given an active link and a current page copy of "rates.md"
    And the person saved a new version of "rates.md" in another app (latest version origin person)
    When sync_linked_folder_with is called with AutomaticPublishing Off
    Then no request is sent to Confluence for "rates.md"
    And the copy is out_of_date and the report counts it as waiting_for_approval
    And ready_to_publish lists "rates.md" as changed

  Scenario: Happy path — with automatic publishing on, the person's own edit is still pushed
    Given an active link and a current page copy of "rates.md"
    And the person saved a new version of "rates.md" in another app (latest version origin person)
    When sync_linked_folder_with is called with AutomaticPublishing On
    Then the copy is current with the new original hash and the new page version

  Scenario: Happy path — a changed file goes to Confluence through the card (SA-CONF-AUTO-04)
    Given "rates.md" is listed as changed after a sync with AutomaticPublishing Off
    When request_publish_checked is called with that item
    Then the card's changes line is "0 new, 1 changed, 0 renamed, 0 removed."
    When the card is approved
    Then the page is updated with the checked bytes and the copy is current with their hash

  Scenario: Error path — a changed file edited after the card is held
    Given "rates.md" is listed as changed and its card is raised
    When "rates.md" changes again and the card is approved
    Then nothing is sent and confluence_item_held is journalled with reason changed_since_card

  Scenario: Error path — a page changed in Confluence is not overwritten from the card
    Given "rates.md" is listed as changed and its card is raised
    And the page's version in Confluence is above the version pushed
    When the card is approved
    Then nothing is sent, the copy is changed_at_destination and confluence_item_held is journalled with reason changed_in_confluence

  Scenario: Edge case — an agent's change is listed as changed
    Given a current page copy of "rates.md" whose latest version has origin run
    When sync_linked_folder is called
    Then the copy is out_of_date and ready_to_publish lists "rates.md" as changed

  Scenario: Edge case — migration step 2 keeps every Ready item
    Given confluence_ready_items holds one new, one renamed and one removed item
    When migration step 2 runs
    Then the three items read back unchanged and a changed item can be stored

  Scenario: Error path — a failed copy of a changed file goes back on Ready to publish (SA-CONF-AUTO-05)
    Given a page copy of "rates.md" that failed after the file changed
    When sync_linked_folder_with is called with AutomaticPublishing On
    Then nothing is sent, the copy is still failed and ready_to_publish lists "rates.md" as changed
    When its card is raised and approved
    Then the page is updated with the checked bytes and the copy is current with their hash

  Scenario: Error path — a copy whose first push failed goes back on Ready to publish (SA-CONF-AUTO-05)
    Given a copy of "rates.md" whose first push failed
    When sync_linked_folder is called
    Then nothing is sent, the copy is still failed and ready_to_publish lists "rates.md" as new

  Scenario: Error path — a failed copy whose page changed in Confluence is not overwritten from the card (SA-CONF-AUTO-05)
    Given a page copy of "rates.md" that failed after the file changed, listed as changed and its card raised
    And the page's version in Confluence is above the version pushed
    When the card is approved
    Then nothing is sent, the copy is still failed and confluence_item_held is journalled with reason changed_in_confluence
```

### Performance / Security / Observability (additions)

- **Performance:** no extra request to Confluence during a sync; a `Changed` item costs one file read when the card is raised and one at send time, as `New`.
- **Security:** with `Off`, no edit leaves the computer without the person's card; the card binds the bytes it covers (step 6.1). A failed copy is sent again only through that card (SA-CONF-AUTO-05).
- **Observability:** `confluence_link_synced` is unchanged; `confluence_item_held` gains the reason `changed_in_confluence`.

### Gaps

None.

---

## Amendment 2026-01-29 — The connection settings a test may override

> **Status:** Approved by the user 2026-01-29 ("proceed with recommendations"). Source: the user's decision of 29 January 2026 that the end-to-end job drives a real link against a stand-in through a specified test seam, in the style the code already uses for test-only overrides, with the production values and the `https`-only rule kept for every program build. Every section above stays in force; CONF-SEAM-1 restates Internal Logic 1.1 and 1.2 as settings, and CONF-SEAM-2 adds a seam that no program build contains. Counterpart: `components/ProjectService.md`, amendment 2026-01-29 (PROJ-SEAM-1 to PROJ-SEAM-3).
>
> Evidence read: Internal Logic 1 above; `crates/ordin-confluence/src/client.rs` (`GATEWAY`, `Transport::site`, `Transport::token`, `Transport::use_stand_in`); `crates/ordin-confluence/src/lib.rs` (`ConfluenceAdapter::with_stand_in`, under `test-support`); `crates/ordin-setup/src/updates.rs` (`ALLOW_LOOPBACK_HTTP`, loopback `http` accepted only with `test-support`); `apps/desktop/src/wiring.rs` (row 3.30 builds the adapter with `ConfluenceAdapter::new`); `apps/desktop/tests/end_to_end.rs`.

### Internal Logic (additions)

**CONF-SEAM-1 Three connection settings and their production values.** Every request is made with three settings: the *gateway* (`https://api.atlassian.com`, the base of `accessible-resources` and of `/ex/confluence/{cloudId}`), the *site* (`ConfluenceSettings.site`, from the verified organisation profile; absent → `CONF_NOT_CONFIGURED`) and the *token* (`identity.access_token_for(CredentialKind::Confluence, SCOPES)`, step 1.1). In every program build these are the only values, and Internal Logic 1 applies unchanged.

**CONF-SEAM-2 A test may override all three, on an adapter already built.** `ConfluenceAdapter::use_test_seam(&self, seam: ConfluenceTestSeam) -> Result<(), ConfluenceError>`, with `ConfluenceTestSeam { gateway: String, site: Url, token: String }`, exists only under the crate's `test-support` feature, as `with_stand_in` does. For every later request:

1. The gateway is `seam.gateway` without a trailing `/`. It must be `https://…`, or `http://` on the host `127.0.0.1` or `localhost`, with no query and no fragment; any other address is `CONF_NOT_CONFIGURED` and nothing changes.
2. The site is `seam.site`; it is matched against `accessible-resources` as the profile's site is (step 1.2) and is never called itself. The held `cloudId` is dropped when the seam is put in.
3. The token is `seam.token`, sent as `Bearer`; Identity is not asked. The restriction scope counts as held (the audience is read from the page's restrictions, not the C-42 fallback). A 401 is `CONF_SIGN_IN_NEEDED` at once, with no refresh through Identity, since no sign-in stands behind the token.
4. Everything else in Internal Logic 1 is unchanged: the status mapping, the retries, the four-request limit and the rule that a paging link stays on the gateway.

**CONF-SEAM-3 No program build contains the seam.** The `test-support` feature is enabled only in `[dev-dependencies]` (this crate's own and the desktop program's, for its end-to-end job), never by a program's normal dependency edge, so the method, the type and the fields behind them are not compiled into a build that ships, and the gateway there is always `https://api.atlassian.com`. *Amended January 2026 (user decision 2026-01-30):* the one exception is the desktop program's `journeys` test build (OrdinDesktop.md DSK-JRN-1, DSK-JRN-4), which enables `test-support` so a journey can reach a stand-in. The release build never names that switch (DSK-JRN-2), so no installer contains the seam.

### Security (addition)

The seam sends a test's token only to the test's gateway, over plain `http` only on loopback. Nothing in a shipped build can change the gateway, the site or the token.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Confluence Adapter — the connection settings a test may override

  Scenario: Happy path — a seam gives the gateway, the site and the token (CONF-SEAM-2)
    Given an adapter whose settings have no site and with nobody signed in to Confluence
    And a stand-in on loopback answering accessible-resources for "https://acme.atlassian.net"
    When a test puts in a seam with the stand-in's http address, that site and the token "seam-token-0001"
    And the audience of a page is read
    Then the stand-in receives the restriction read with "Bearer seam-token-0001"
    And the audience is the restriction's group

  Scenario: Error path — a seam off loopback over http is refused (CONF-SEAM-2)
    When a test puts in a seam with the gateway "http://gateway.example"
    Then the answer is CONF_NOT_CONFIGURED
    And nothing changes: the audience read still answers CONF_SIGN_IN_NEEDED and no request is made

  Scenario: Error path — a refused seam token is not refreshed (CONF-SEAM-2)
    Given a seam is in place
    When the stand-in answers 401
    Then the read answers CONF_SIGN_IN_NEEDED after one request
```

### Gaps

None.

## Amendment January 2026 — Confluence connected with an API token (amended January 2026; user decision; Approved by the user January 2026)

> **Status:** Approved by the user January 2026. Source: the user's decision that Confluence is self-service: a person whose organisation profile names no Atlassian app connects Confluence with an Atlassian API token they make themselves, as Jira already allows. The browser sign-in (Internal Logic 1.1, C-12) is unchanged and stays available whenever the profile names an app.
>
> Evidence read: `crates/ordin-confluence/src/client.rs` (`Transport::send`, `Transport::token`, `Transport::site`), `crates/ordin-jira/src/adapter.rs` (`connect_with_token`), `crates/ordin-identity/src/accounts.rs` (`save_jira_token`, `jira_access`).

### Public Interface (additions)

```rust
/// The person a pasted API token belongs to, as `GET /wiki/rest/api/user/current` names them.
pub struct ConfluenceUser {
    pub account_id: String,   // 1–128 characters
    pub display_name: String, // 1–255 characters
}

impl ConfluenceAdapter {
    pub async fn connect_with_token(&self, site: &str, email: &str, token: SecretString)
        -> Result<ConfluenceUser, ConfluenceError>;
}

pub enum ConfluenceError {
    // … the existing variants, and:
    TokenRefused, // CONF_TOKEN_REFUSED
    SiteInvalid,  // CONF_SITE_INVALID
}
```

### Internal Logic (additions)

**CONF-TOKEN-1 Connecting with a pasted API token.** `connect_with_token(site, email, token)`, in order:

1. The site is trimmed and put in lower case. It must be `{name}.atlassian.net`, `name` being 1–63 lower-case letters, digits or `-` and starting with a letter or a digit (the Jira Adapter's site rule); otherwise `CONF_SITE_INVALID` and no request is made.
2. The email is trimmed; it must be 3–254 characters with exactly one `@`, and the token 1–1,024 bytes; otherwise `CONF_TOKEN_REFUSED` and no request is made.
3. One request: `GET https://{site}/wiki/rest/api/user/current` with `Authorization: Basic base64(email:token)`, `Accept: application/json`, under the 30 s metadata timeout, outside the retry loop. The reply decides:
   - no answer, 429 or 5xx → `CONF_UNAVAILABLE` (retryable);
   - 401 or 403 → `CONF_TOKEN_REFUSED`;
   - any other status outside 2xx (a 404 from a site with no Confluence) → `CONF_SITE_INVALID`;
   - 2xx whose body is not JSON → `CONF_SITE_INVALID`;
   - 2xx naming the anonymous user (`type = "anonymous"`) → `CONF_TOKEN_REFUSED`;
   - 2xx with a non-empty `accountId` (at most 128 characters) and `displayName` (at most 255 characters) → the person; anything else → `CONF_SITE_INVALID`.
4. The person found, Identity keeps the token: `IdentityService::save_confluence_token(site, email, token, account_id, display_name)` (IdentityAndCredentials.md ID-CONF-TOKEN-1). An Identity failure is `CONF_UNAVAILABLE` when retryable, else `CONF_SIGN_IN_NEEDED`.
5. `confluence_token_checked` is logged with `accepted` (`true` or `false`) after a check that reached Confluence; the request itself is logged as every request is (`confluence_request`, `operation = "connect_token"`, `status`). The token, the email and the `Authorization` header are never logged, journalled or returned; the result carries the account id and the display name only.

**CONF-TOKEN-2 Every later request with a pasted API token.** Internal Logic 1.1 gains a first step: before asking Identity for an OAuth token, the adapter asks `IdentityService::confluence_token_access()`. When it answers a token:

1. The request goes straight to the site, `https://{site}{path}` (the `path` being the `/wiki/…` part of every surface row), with `Authorization: Basic base64(email:token)`. The `cloudId` look-up (step 1.2) and the gateway are not used, so `CONF_SITE_NOT_GRANTED` cannot arise.
2. The site of the token is the company site for that request and for the web addresses built from its answer, whatever the profile's `confluence.site` names; with no profile site, Confluence still works.
3. A 401 is not refreshed: Identity is told the credential was refused (`report_refused`, reason `CONF_SIGN_IN_NEEDED`, so its row turns `needs_sign_in`) and the call answers `CONF_SIGN_IN_NEEDED`. Every other status, the retries, the four-request limit and the paging rule are unchanged.
4. The restriction scope counts as held: an API token acts with all the person's own rights, so the audience is read from the page's restrictions (C-42's fallback is not taken).

When Identity answers no token (no Confluence credential, or a browser sign-in), Internal Logic 1 is unchanged. Every read and every write of the adapter — finding and listing, fetching, the linked folder's sync, publishing for the Push Service and the Knowledge Service — goes through this one transport, so all of them use the token.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The pasted email or API token is refused by Confluence (401, 403, the anonymous user) or out of range | `Err(permanent)` | `CONF_TOKEN_REFUSED` | `{"error":"Confluence didn't accept those sign-in details. Check them and try again.","code":"CONF_TOKEN_REFUSED"}` |
| The site given with an API token is not `{name}.atlassian.net`, or does not answer as a Confluence site | `Err(permanent)` | `CONF_SITE_INVALID` | `{"error":"That address isn't a Confluence site. Check the site address and try again.","code":"CONF_SITE_INVALID"}` |

### Security (addition)

The API token is sent only to the site the person typed, over `https`, and only as the Basic `Authorization` header; it is kept by Identity in the same secret store as every other token, never in preferences, and appears in no log line, journal record or reply.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Confluence Adapter — connected with an API token

  Scenario: Happy path — a checked token is kept and signs every later request (CONF-TOKEN-1, CONF-TOKEN-2)
    Given nobody is signed in to Confluence and the profile names no Confluence site
    And the site "acme.atlassian.net" answers /wiki/rest/api/user/current for Basic "sam@acme.com:<token>" with Sam Lee
    When connect_with_token is called with that site, email and token
    Then it answers Sam Lee and Identity keeps the token for "acme.atlassian.net"
    And a later search is sent to the site with the same Basic header, with no accessible-resources look-up
    And no log line contains the token

  Scenario: Error path — Confluence refuses the token (CONF-TOKEN-1)
    When the site answers 401
    Then the call fails with CONF_TOKEN_REFUSED and nothing is kept

  Scenario: Error path — a wrong site (CONF-TOKEN-1)
    When the site answers 404, or a page that is not JSON
    Then the call fails with CONF_SITE_INVALID and nothing is kept

  Scenario: Error path — a kept token later refused (CONF-TOKEN-2)
    Given a kept API token
    When the site answers a read with 401
    Then the read answers CONF_SIGN_IN_NEEDED and the credential needs signing in again
```

### Gaps

None.
