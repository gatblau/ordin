# SPEC: SharePoint Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (the SharePoint item types are named `SharePointItem`, `SharePointItemRef` and `SharePointVersion`, apart from the Confluence Adapter's `RemoteItem` family, so each has one name in the generated screen types)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-sharepoint` · **Build phase:** 2 · **Programs:** D P
> **Dependencies:** Identity, Journal

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

The SharePoint Adapter reads from and pushes to SharePoint through Microsoft Graph with the person's own Microsoft account (02:386, 09 ADR-029). It lists the sites, document libraries and folders the person can see; fetches files with the version read so the core can tell when an origin changed (A-25); finds what changed at a linked location since the last check; uploads a new version of a file on an approved push only when the origin still has the version that was read; creates a file in a chosen folder on a published copy's first publish, after checking whether the name is taken; and reports who can see a destination for the approval card and before each automatic update of a copy (09 ADR-069, C-42). It owns the sync state of SharePoint links; published copies are recorded by the File Service (02:390).

## Approach

A thin typed client over Graph v1.0 REST with `reqwest` (SA-HTTP-02), returning bytes and version tags to its callers: the Workspace Service writes the working copy and the Push Service decides what to push, so this crate needs only Identity and the Journal (2B-3). Change detection uses the drive's **delta query from the library root with `token=latest`**, filtered to the link's folder path, because Graph supports delta only on the root of a SharePoint library and `token=latest` gives a starting point without listing the whole library. Rejected: comparing every item's version at each check (up to 100,000 requests per link, A-21); Graph change notifications (webhooks need a public endpoint, which a desktop does not have, 09 ADR-069 *Alternatives*).

## Shared Context

### Identifiers, time and paging (2C part 1 *Conventions*; SA-PAGE-01)

```rust
/// RFC 3339, UTC, millisecond precision, `Z` suffix.
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
// UUIDv7 newtypes, 36-character lowercase strings:
pub struct SourceLinkId(pub uuid::Uuid);   pub struct WorkspaceId(pub uuid::Uuid);
pub struct PlaceId(pub uuid::Uuid);        pub struct RunId(pub uuid::Uuid);
pub struct ExternalActionKey(pub uuid::Uuid);

// Paging (2C part 1 *Paging*, C-15): imported from `ordin_types`, not redefined here.
pub const PAGE_SIZE: usize = 100;
pub const CURSOR_MAX_CHARS: usize = 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Page<T> { pub items: Vec<T>, /* always serialised; null on the last page */ pub next_cursor: Option<Cursor> }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(String);
impl Cursor {
    pub fn encode<K: Serialize>(key: &K) -> Cursor;
    /// Refuses more than 1,024 characters, bad base64url, or JSON that is not a `K` (`SRV_BAD_CURSOR`).
    pub fn decode<K: serde::de::DeserializeOwned>(&self) -> Result<K, TypesError>;
}
```

### Domain types read (2C part 1, copied unchanged as amended by decision 80)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SourceLink {
    pub id: SourceLinkId,
    pub workspace_id: WorkspaceId,
    pub kind: SourceLinkKind,
    pub origin_location: String,
    pub display_name: String,
    /// Local and repository folders; SharePoint and Confluence links once their working copy exists (C-9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<PlaceId>,
    pub state: SourceLinkState,
    pub added_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<Timestamp>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkKind { LocalFolder, GitRepositoryFolder, SharepointLocation, ConfluenceLocation }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkState { Proposed, Active }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Github, Gitlab, Oidc, Slack, Confluence, Microsoft, Google,
    MailServerSingleSignOn, MailServerPassword, MailServerAppPassword,
    Jira,   // 2C, amended January 2026, ADR-106
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind {
    SlackPost, ConnectionWrite, WrightrunRunStart, EmailSend, OriginPush, CopyPublish, TelegramPost,
    PullRequestOpened, HandoverWritten,
}
```

`SourceLink.origin_location` for a SharePoint link is written by this adapter as `graph:drives/<drive-id>/items/<item-id>` (*chosen here*; 1–2048 bytes, `SRC_LOCATION_INVALID` is the Workspace Service's). This adapter never reads `place_id` (the working-copy place belongs to the Workspace Service).

### Interfaces of other components used here (provider text is authoritative; R3, R6, R7)

```rust
// ordin-identity (IdentityAndCredentials.md, *Tokens and state*, *Errors*) — a concrete service.
pub struct AccessToken { pub token: SecretString, pub expires_at: Option<Timestamp>, pub credential_id: CredentialId }
impl IdentityService {
    /// As `access_token(kind, None)`, and the credential's granted scopes must contain every entry
    /// of `scopes`; otherwise CRED_SCOPE_MISSING.
    pub async fn access_token_for(&self, kind: CredentialKind, scopes: &[&str]) -> Result<AccessToken, CredError>;
    /// Refresh with the stored refresh token only, never a prompt; `Ok(false)` when refused.
    pub async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError>;
}
pub enum CredError { Retryable { code: &'static str }, Permanent { code: &'static str } }
impl ordin_types::CodedError for CredError { fn code(&self) -> &'static str; fn retryable(&self) -> bool; /* … */ }

// ordin-journal (JournalAndArtefactStore.md) — a concrete type, cheap to clone; no trait object.
pub type MigrationStep = (i64, &'static str, &'static str);      // (version, description, SQL)
pub struct ModuleMigrations { pub module: &'static str,
                              pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
pub struct NewJournalEntry { pub kind: String, pub component: String, pub run_id: Option<RunId>,
                             pub crew_run_id: Option<CrewRunId>, pub workspace_id: Option<WorkspaceId>,
                             pub summary: String, pub data: serde_json::Value }
pub struct NewExternalAction { pub action_key: ExternalActionKey, pub run_id: Option<RunId>,
                               pub kind: ExternalActionKind, pub destination: String }
impl Journal {
    pub fn datastore(&self) -> &Datastore;            // D4: this crate's tables live in the Journal's datastore
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    /// A conflict on `action_key` fails with `JournalError::Invalid { code: "EXT_KEY_DUPLICATE" }`.
    pub async fn begin_external_action(&self, new: NewExternalAction) -> Result<ExternalAction, JournalError>;
    pub async fn confirm_external_action(&self, key: ExternalActionKey) -> Result<ExternalAction, JournalError>;
}
impl Datastore { pub fn sqlite(&self) -> Option<&sqlx::SqlitePool>; }
// JournalError implements CodedError; this crate matches on `code()`, never on its variants.
```

### Microsoft Graph surface used (08:102; Graph v1.0, `https://graph.microsoft.com/v1.0`)

| Use | Request |
|---|---|
| Find sites | `GET /sites?search={q}&$select=id,displayName,webUrl` |
| Libraries of a site | `GET /sites/{site-id}/drives?$select=id,name,webUrl` |
| Folders matching a word in a library | `GET /drives/{drive-id}/root/search(q='{q}')?$select=id,name,folder,parentReference,webUrl&$top=50` |
| Children of a folder | `GET /drives/{drive-id}/items/{item-id}/children?$select=id,name,folder,file,size,eTag,cTag,lastModifiedDateTime,lastModifiedBy,parentReference&$top=100`, one Graph page per adapter page (step 1.4) |
| One item's metadata | `GET /drives/{drive-id}/items/{item-id}?$select=id,name,eTag,cTag,size,lastModifiedDateTime,lastModifiedBy,parentReference,file` |
| Item by name in a folder | `GET /drives/{drive-id}/items/{parent-id}:/{name}` |
| Content | `GET /drives/{drive-id}/items/{item-id}/content` (follows the 302 to the download URL without sending the token there) |
| Latest version label | `GET /drives/{drive-id}/items/{item-id}/versions?$top=1&$select=id,lastModifiedDateTime` |
| Changes | `GET /drives/{drive-id}/root/delta?token=latest` once, then the stored `@odata.deltaLink`, following `@odata.nextLink` |
| New version, ≤ 4 MiB | `PUT /drives/{drive-id}/items/{item-id}/content` with `If-Match: <eTag>` |
| New version, > 4 MiB | `POST /drives/{drive-id}/items/{item-id}/createUploadSession` with `If-Match: <eTag>` and `{"item":{"@microsoft.graph.conflictBehavior":"replace"}}`, then `PUT` ranges of 10 MiB to `uploadUrl` |
| Create a file, ≤ 4 MiB | `PUT /drives/{drive-id}/items/{parent-id}:/{name}:/content?@microsoft.graph.conflictBehavior=fail` |
| Create a file, > 4 MiB | `POST /drives/{drive-id}/items/{parent-id}:/{name}:/createUploadSession` with `{"item":{"@microsoft.graph.conflictBehavior":"fail"}}` |
| Who can see it | `GET /drives/{drive-id}/items/{item-id}/permissions` |

Delegated scopes requested from Identity (*chosen here*), in the URI form Identity's own examples use: `https://graph.microsoft.com/Files.ReadWrite.All` (read and write the files the person can reach; Graph's least scope that writes to SharePoint libraries other than the person's own OneDrive) and `https://graph.microsoft.com/Sites.Read.All` (site search and permissions read). Identity adds `offline_access` to every Microsoft sign-in itself (IdentityAndCredentials.md step 5). Delegated permissions never exceed what the person can reach in SharePoint. Whether IT's public-client registration grants these is C-14 (below).

### Constants (*chosen here* unless cited)

| Constant | Value |
|---|---|
| `GRAPH_BASE` | `https://graph.microsoft.com/v1.0` |
| `METADATA_TIMEOUT` | 30 s per request |
| `TRANSFER_TIMEOUT` | 120 s per request or per 10 MiB range |
| `SIMPLE_UPLOAD_MAX` | 4 MiB (Graph's simple-upload limit) |
| `UPLOAD_RANGE` | 10 MiB (a multiple of 320 KiB, as Graph requires) |
| `MAX_FILE_BYTES` | 250 MiB per fetch or upload *(assumed here: bounds memory; larger files are listed but not read)* |
| `RETRIES` | 3 after the first attempt, for reads and for writes proven not to have landed |
| `BACKOFF` | 1 s, 2 s, 4 s, each ±20 % jitter; `Retry-After` wins when present |
| `MAX_LIST_ITEMS` | 100,000 items and 20 folder levels per link (A-21) |
| `FIND_LIMIT` | at most 50 locations per `find_locations` |

### Confirmation item bound here (Phase 1, *Confirmation items*)

- **C-14** — a public client registered by IT signs the person in with PKCE and can read, upload and version files with delegated permissions. *Until confirmed* every method works as specified when Identity returns a token; when IT has not registered the application, Identity returns `CRED_PROVIDER_NOT_CONFIGURED` or, when the registration is refused, `CRED_PROVIDER_REFUSED` (its C-14 row), and this adapter returns `SP_NOT_CONFIGURED`; the HLD's fallback is that SharePoint is off for that company (2D-2, "none (SharePoint and Outlook off)").
- **C-42** — Graph reports who can see a folder or file in a form that fits one line. *Fallback (10 C-42):* the card names the site and says "everyone who can open it", and an automatic update cannot detect a grown audience; `audience` then returns `everyone_who_can_open = true`.
- **C-69** — conditional SharePoint write and directory search for handovers belong to the Handover Service's use of this adapter; `upload_version` with `expected_version` gives the conditional write, `item_at` and `create_folder` give the reads and the new handover folder; directory search is not offered here: the person types the receiver's email address (reconciliation ruling R17).

### Reconciliation assumptions

- **SA-RECON-01** — Identity and Journal are used through their own published names (R3, R6): `IdentityService::access_token_for` with `CredError` codes, and the concrete `Journal` with `append_entry`, `begin_external_action`/`confirm_external_action` and `MigrationStep` tuples. On a Graph 401 this adapter calls `refresh_silently` as Identity defines it (Internal Logic 1.3); IdentityAndCredentials.md's caller table lists this adapter for it (closing decision K22).
- **SA-RECON-02** — One external-action bracket per write: this adapter begins and confirms the action with the caller-minted key; the Push Service and the Handover Service do not record it themselves.
- **SA-RECON-03** — This crate's tables live in the Journal's `Datastore` (D4, R20); the program does not hand it a separate pool.
- **SA-RECON-04** — No directory search for handovers: the person types the recipient's email address (R17); Handover's needs map to `list_children`, `item_at`, `fetch`, `upload_version`, `create_file` and `create_folder`.
- **SA-RECON-05** — `sharepoint.find` and `sharepoint.list` are this module's names, adopted by the Service API catalogue (R1).
- **SA-RECON-06** — `Page`, `Cursor` and `PAGE_SIZE` are imported from `ordin_types` (2C C-15); `ExternalAction.run_id` is optional (2C C-7), so an automatic copy update passes `run_id: None`.

## Public Interface

Error prefix: `SP_`.

```rust
pub struct SharePointDeps {
    pub identity: std::sync::Arc<ordin_identity::IdentityService>,
    /// Also the source of this crate's `Datastore` (`journal.datastore()`, D4).
    pub journal: ordin_journal::Journal,
    /// Built by the program with SA-HTTP-02's TLS and proxy settings.
    pub http: reqwest::Client,
}

pub struct SharePointAdapter { /* private */ }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SharePointLocation {
    pub site_id: String,
    pub site_name: String,
    pub drive_id: String,
    pub drive_name: String,
    /// The folder or file; `None` = the library's root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    /// Path from the library root, `/`-separated, empty for the root.
    pub path: String,
    pub is_folder: bool,
    pub web_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SharePointItemRef { pub drive_id: String, pub item_id: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SharePointVersion {
    /// Graph `cTag`: changes when the content changes; the "version read" of A-25.
    pub tag: String,
    /// Graph `eTag`: changes on any change; used for `If-Match`.
    pub etag: String,
    /// SharePoint's version label ("1.0", "4.0") when Graph returns one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub changed_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SharePointItem {
    pub item: SharePointItemRef,
    pub name: String,
    /// Relative to the listed location, `/`-separated; folders end with `/`.
    pub path: String,
    pub is_folder: bool,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<SharePointVersion>,       // None for folders
}

#[derive(Debug, Clone)]
pub struct FetchedItem { pub item: SharePointItemRef, pub bytes: Vec<u8>, pub version: SharePointVersion }

/// Every write's external-action bracket (G8). The caller mints `key` (UUIDv7) and records nothing in the
/// Journal for it: this adapter begins the external action before sending and confirms it after (single
/// bracket, reconciliation rows 711 and 723). `kind` ∈ {`OriginPush`, `CopyPublish`, `HandoverWritten`}.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalWrite { pub key: ExternalActionKey, pub run_id: Option<RunId>, pub kind: ExternalActionKind }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginChange {
    pub source_link_id: SourceLinkId,
    /// Relative to the link's root.
    pub origin_path: String,
    pub new_version: String,                 // the new cTag; empty when removed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    pub changed_at: Timestamp,
    pub removed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkScope { Users, Organization, Anonymous }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Audience {
    /// One line for the card: "Procurement team and 3 people", "Everyone in the company",
    /// "Anyone with the link", or "everyone who can open it" (C-42 fallback).
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub people: Option<u32>,
    pub everyone_who_can_open: bool,
    /// Sorted Graph principal ids (users, groups, site groups) with access; empty under the fallback.
    pub principals: Vec<String>,
    /// The broadest sharing-link scope on the item, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_scope: Option<LinkScope>,
}

impl SharePointAdapter {
    /// Fails with `SP_DATASTORE_UNAVAILABLE` when the Journal's datastore is not SQLite (this crate is not built
    /// into the agent profile).
    pub fn new(deps: SharePointDeps) -> Result<SharePointAdapter, SharePointError>;

    pub async fn find_locations(&self, query: &str, limit: u32) -> Result<Vec<SharePointLocation>, SharePointError>;
    /// The location form stored in SourceLink.origin_location, and back.
    pub fn origin_location(loc: &SharePointLocation) -> String;
    pub async fn resolve_origin(&self, origin_location: &str) -> Result<SharePointLocation, SharePointError>;
    /// The location behind a SharePoint web address (`https://<tenant>.sharepoint.com/…`), as the person or a
    /// handover record gives it. Called by: Handover Service (places and result links).
    pub async fn resolve_web_url(&self, web_url: &str) -> Result<SharePointLocation, SharePointError>;
    pub async fn list_children(&self, loc: &SharePointLocation, cursor: Option<Cursor>) -> Result<Page<SharePointItem>, SharePointError>;
    pub async fn fetch(&self, item: &SharePointItemRef) -> Result<FetchedItem, SharePointError>;
    pub async fn item_version(&self, item: &SharePointItemRef) -> Result<SharePointVersion, SharePointError>;
    /// Starts change tracking for a newly active link (stores its delta starting point).
    pub async fn track_link(&self, link: &SourceLink) -> Result<(), SharePointError>;
    pub async fn untrack_link(&self, link: SourceLinkId) -> Result<(), SharePointError>;
    pub async fn check_changes(&self, link: &SourceLink) -> Result<Vec<OriginChange>, SharePointError>;
    pub async fn changes_since(&self, links: &[SourceLinkId], since: Timestamp) -> Result<Vec<OriginChange>, SharePointError>;
    /// Conditional write of a new version: refused with `SP_ORIGIN_CHANGED` unless the item's cTag is `expected_version`.
    pub async fn upload_version(&self, item: &SharePointItemRef, bytes: Vec<u8>, expected_version: &str,
                                action: ExternalWrite) -> Result<SharePointVersion, SharePointError>;
    pub async fn create_file(&self, folder: &SharePointLocation, name: &str, bytes: Vec<u8>,
                             action: ExternalWrite) -> Result<(SharePointItemRef, SharePointVersion), SharePointError>;
    /// Creates the sub-folder `name` of `parent` (conflict → `SP_NAME_EXISTS`); returns its location.
    pub async fn create_folder(&self, parent: &SharePointLocation, name: &str,
                               action: ExternalWrite) -> Result<SharePointLocation, SharePointError>;
    /// Moves an item to the site's recycle bin; with `expected_version`, refused with `SP_ORIGIN_CHANGED`
    /// when the item's cTag differs (a published copy changed at its destination is not removed unseen).
    pub async fn delete(&self, item: &SharePointItemRef, expected_version: Option<&str>,
                        action: ExternalWrite) -> Result<(), SharePointError>;
    pub async fn exists(&self, folder: &SharePointLocation, name: &str) -> Result<bool, SharePointError>;
    /// The item at `path` (one or more `/`-separated segments) under `folder`, with its version; `None` when absent.
    pub async fn item_at(&self, folder: &SharePointLocation, path: &str) -> Result<Option<SharePointItem>, SharePointError>;
    pub async fn audience(&self, target: &SharePointLocation) -> Result<Audience, SharePointError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SharePointError {
    #[error("SP_NOT_CONFIGURED")] NotConfigured,
    #[error("SP_SIGN_IN_NEEDED")] SignInNeeded,
    #[error("SP_ACCESS_DENIED")] AccessDenied,
    #[error("SP_NOT_FOUND")] NotFound,
    #[error("SP_LOCATION_INVALID")] LocationInvalid,
    #[error("SP_QUERY_INVALID")] QueryInvalid,
    #[error("SP_ORIGIN_CHANGED")] OriginChanged { current: SharePointVersion },
    #[error("SP_NAME_EXISTS")] NameExists,
    #[error("SP_TOO_LARGE")] TooLarge,
    #[error("SP_THROTTLED")] Throttled { retry_after_s: u32 },
    #[error("SP_UNAVAILABLE")] Unavailable,
    #[error("SP_OUTCOME_UNKNOWN")] OutcomeUnknown,
    #[error("SP_RESPONSE_INVALID")] ResponseInvalid,
    #[error("SP_NOT_TRACKED")] NotTracked,
    #[error("SP_DATASTORE_UNAVAILABLE")] DatastoreUnavailable,
    #[error("SP_ACTION_KEY_USED")] ActionKeyUsed,
    #[error("SRV_BAD_CURSOR")] BadCursor,
}
impl ordin_types::CodedError for SharePointError { fn code(&self) -> &'static str; fn retryable(&self) -> bool;
                                                   fn repair(&self) -> Option<ordin_types::RepairAction>; }

/// Migrations registered by the program with the Journal's `MigrationRegistry` (R3): module `sharepoint`,
/// `sqlite: &[(1, "sharepoint tables", <the Data Model SQL>)]`, `postgres: &[]`.
pub fn migrations() -> ModuleMigrations;
```

### Service API commands (*chosen here*; used by **Add to this workspace**, 11 §5.13)

| Command | Input | Result |
|---|---|---|
| `sharepoint.find` | `{ "query": string (1–200), "limit": u32 (1–50) }` | `Vec<SharePointLocation>` |
| `sharepoint.list` | `{ "location": SharePointLocation, "cursor": Cursor? }` | `Page<SharePointItem>` |

These are the catalogue's rows (ServiceApi.md, `sharepoint.find` and `sharepoint.list`, scope Any, Read; R1); the screens reach them through the Service API catalogue.

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| `origin_location`, `resolve_origin`, `list_children`, `fetch`, `track_link`, `untrack_link`, `check_changes` | Workspace Service |
| `changes_since`, `item_version` | Home and Search |
| `resolve_origin`, `item_at`, `item_version`, `fetch`, `upload_version`, `create_file`, `exists`, `audience`, `delete` (`OriginPush`, `CopyPublish`) | Push Service |
| `resolve_web_url`, `resolve_origin`, `audience`, `list_children` (folders), `item_at`, `fetch`, `upload_version`, `create_file`, `create_folder` (`HandoverWritten`) | Handover Service |
| `find_locations`, `list_children` (`sharepoint.find`, `sharepoint.list`) | Service API catalogue (**Add to this workspace**) |

The Knowledge Service reads SharePoint versions only through the Workspace Service's `origin_version` (no edge to this adapter).

### Example

```rust
let found = sp.find_locations("supplier contracts", 10).await?;
// [SharePointLocation { site_id: "acme.sharepoint.com,1f0c…,9a2e…", site_name: "Procurement",
//    drive_id: "b!x9Fq…", drive_name: "Documents", item_id: Some("01ABCDEF2GHIJ"),
//    path: "Suppliers/Contracts", is_folder: true, web_url: "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents/Suppliers/Contracts" }]

let page = sp.list_children(&found[0], None).await?;
// Page { items: [SharePointItem { item: SharePointItemRef { drive_id: "b!x9Fq…", item_id: "01ABCDEF7KLMN" },
//   name: "Kestrel MSA.docx", path: "Kestrel MSA.docx", is_folder: false, size_bytes: 88210,
//   version: Some(SharePointVersion { tag: "\"c:{6D1A…},4\"", etag: "\"{6D1A…},7\"", label: Some("4.0"),
//   changed_at: 2026-01-10T13:40:00.000Z, changed_by: Some("Sam Lee") }) }], next_cursor: None }

let v = sp.upload_version(&page.items[0].item, new_bytes, "\"c:{6D1A…},4\"",
                          ExternalWrite { key, run_id: Some(run_id), kind: ExternalActionKind::OriginPush }).await;
// Ok(SharePointVersion { tag: "\"c:{6D1A…},5\"", label: Some("5.0"), .. })
// or Err(SharePointError::OriginChanged { current }) when Sam Lee changed it after it was read.
```

## Internal Logic

### 1. Calling Graph (every request)

1. Get a token: `identity.access_token_for(CredentialKind::Microsoft, &["https://graph.microsoft.com/Files.ReadWrite.All", "https://graph.microsoft.com/Sites.Read.All"])`. Map `CredError::code()`: `CRED_SIGN_IN_REQUIRED`, `CRED_SCOPE_MISSING`, `CRED_KEYCHAIN_MISSING`, `CRED_REFRESH_REFUSED`, `CRED_AMBIGUOUS` → `SP_SIGN_IN_NEEDED` (the caller shows "Sign in to SharePoint so Ordin can read the supplier folder." with **Sign in again**, 11 §5.13, and starts the sign-in with these scopes through Identity's `begin_sign_in`); `CRED_PROVIDER_NOT_CONFIGURED`, `CRED_PROVIDER_REFUSED` → `SP_NOT_CONFIGURED`; any code with `retryable() == true` (`CRED_PROVIDER_UNREACHABLE`, `CRED_KEYCHAIN_UNAVAILABLE`, others) → `SP_UNAVAILABLE`; any other permanent code → `SP_SIGN_IN_NEEDED`.
2. Send with `Authorization: Bearer`, `METADATA_TIMEOUT` or `TRANSFER_TIMEOUT`.
3. Map the response: 200/201 → parse (a body that does not parse → `SP_RESPONSE_INVALID`, permanent); 401 → `identity.refresh_silently(CredentialKind::Microsoft)` once and repeat with a new `access_token_for` (`Ok(false)` → `SP_SIGN_IN_NEEDED`); a second 401 → `SP_SIGN_IN_NEEDED`; 403 → `SP_ACCESS_DENIED`; 404 → `SP_NOT_FOUND`; 409 → `SP_NAME_EXISTS`; 412 → `SP_ORIGIN_CHANGED` (current version read again as in step 6.4); 413 → `SP_TOO_LARGE`; 429, 503, 504 → wait `Retry-After` seconds (or the next `BACKOFF` step) and retry up to `RETRIES`, then `SP_THROTTLED` or `SP_UNAVAILABLE`; other 5xx and network errors → retry as 429 for reads; for writes see step 7.
4. Graph paging: `find_locations` and change tracking follow `@odata.nextLink` inside the adapter until the limit or the end (SA-PAGE-01). `list_children` reads one Graph page of `$top=100` per call; when Graph returns `@odata.nextLink`, the adapter's `next_cursor` is `Cursor::encode(&ChildrenCursor { drive_id, item_id, skiptoken })`, where `skiptoken` is the `$skiptoken` query value of that link (*chosen here*: the cursor holds no URL, so it cannot redirect a request). On the next call the adapter decodes it with `Cursor::decode`, checks that `drive_id` and `item_id` equal those of `loc`, and rebuilds the children request with `&$skiptoken=`; a decode failure, a mismatch, or a cursor that would exceed `CURSOR_MAX_CHARS` → `SRV_BAD_CURSOR`. A `@odata.nextLink` with no `$skiptoken` → `SP_RESPONSE_INVALID`.
5. Log `graph_request {operation, status, elapsed_ms, attempt}`; never the URL's query, item names, the token or content.

### 2. Finding and listing

1. `find_locations`: `query` trimmed, 1–200 chars (else `SP_QUERY_INVALID`); `limit` 1–50. Search sites; for each site (up to 10) list its libraries; return libraries whose name contains the query, and for each library (up to 10 in all) folders from `root/search` with a `folder` facet; order: sites' libraries first, then folders, each by name; cut at `limit`. Each site, library and folder becomes a `SharePointLocation` (for a library, `item_id = None`, `path = ""`).
2. `origin_location(loc)` = `graph:drives/{drive_id}/items/{item_id or "root"}`; `resolve_origin` parses that form (else `SP_LOCATION_INVALID`) and reads the item's metadata and its drive's site to rebuild the location.
3a. `resolve_web_url(url)`: must be `https`, host ending in `.sharepoint.com`, ≤ 2,048 characters (else `SP_LOCATION_INVALID`); `GET /shares/u!{base64url(url) without padding}/driveItem` (Graph's sharing-URL form) → the item and its `parentReference.driveId`; then build the `SharePointLocation` as `resolve_origin` does. 404 → `SP_NOT_FOUND`; 403 → `SP_ACCESS_DENIED`.
3. `list_children`: children of the folder (or root), mapped to `SharePointItem`; items beyond `MAX_FILE_BYTES` are listed with their size; `version` from `cTag`, `eTag`, `lastModifiedDateTime`, `lastModifiedBy.user.displayName`. The label is not read per item while listing (one request per item); it is filled by `fetch` and `item_version`.

### 3. Fetching

1. `fetch`: read metadata (step 1); `size > MAX_FILE_BYTES` → `SP_TOO_LARGE`; read the content; re-read metadata and compare `cTag` with the first read; if it changed during the download, repeat once, then `SP_ORIGIN_CHANGED`. Read the latest version label. Return bytes and `SharePointVersion`.
2. `item_version`: metadata and latest version label only.

### 4. Change tracking (D-8; A-25)

1. `track_link(link)`: resolve its location; call `root/delta?token=latest` on its drive; store `delta_link`, `drive_id`, the link's `path_prefix` (folder path from the library root, or the file's path), `tracked_at`. Upsert `sharepoint_items` for the link from a full `list_children` walk (≤ `MAX_LIST_ITEMS`, ≤ 20 levels) so later changes can be compared with what was read.
2. `check_changes(link)`: called by the Workspace Service at workspace open, at start and every 15 minutes (B7 contract D-8). Read `delta_link`, follow it and every `@odata.nextLink`; keep entries whose `parentReference.path` + name lies under `path_prefix`. For each kept entry: `deleted` facet → `OriginChange { removed: true }` and mark the item removed; a file whose `cTag` differs from `sharepoint_items.ctag` → `OriginChange { new_version: cTag, changed_by, changed_at }` and update the item row; a new file → an `OriginChange` and a new item row. Folders produce no change. Store the new `@odata.deltaLink` and `last_checked_at` in the same transaction as the inserted `sharepoint_origin_changes` rows.
3. A 410 Gone (delta token expired) → call `track_link` again (full walk) and report every item whose `cTag` differs from the stored one, then continue. Logged `delta_resync {source_link_id}`.
4. A link not tracked → `SP_NOT_TRACKED` (the Workspace Service calls `track_link` on activation and at start for links without state).
5. `changes_since(links, since)`: rows of `sharepoint_origin_changes` for those links with `found_at > since`, newest first, at most 1,000 *(chosen here)*.
6. Origin changes older than 90 days are deleted at each `check_changes` *(assumed here: Home shows "since you last looked"; marks older than that are not useful)*.

### 5. Existence and audience

1. `exists(folder, name)`: `name` 1–255 chars, no `/ \ : * ? " < > |` (SharePoint's forbidden characters), else `SP_LOCATION_INVALID`; `GET …/{parent-id}:/{name}` → 200 true, 404 false.
2. `audience(target)`: read `permissions`. Under C-42 confirmed: collect principals (`grantedToV2.user.id`, `grantedToV2.group.id`, `grantedToV2.siteGroup.id`, and each `grantedToIdentitiesV2` entry), sorted; the broadest `link.scope` (`anonymous` > `organization` > `users`). Line: `anonymous` → "Anyone with the link"; `organization` → "Everyone in the company"; else up to two group or site-group display names joined with " and ", followed by "and N people" for individual users (`people = N` when there are no groups). A 403 on `permissions` (the person cannot read permissions), or a response with no principals and no links, returns the fallback `Audience { line: "everyone who can open it", everyone_who_can_open: true, principals: [], .. }` (10 C-42). The line is data, localised by the Approval Service's card text around it.

### 6. Upload of a new version (`upload_version`)

1. `bytes.len() > MAX_FILE_BYTES` → `SP_TOO_LARGE`.
2. Read the item's metadata. `cTag != expected_version` → `SP_ORIGIN_CHANGED { current }` before anything is sent (09 ADR-029: "the push stops and Ordin shows both versions").
3. `journal.begin_external_action(NewExternalAction { action_key: action.key, run_id: action.run_id, kind: action.kind, destination: "SharePoint › <site> › <library> › <path>" })`. An error whose `code()` is `EXT_KEY_DUPLICATE` → `SP_ACTION_KEY_USED` (the caller passed a key already used; nothing is sent); `JRNL_DATASTORE_UNAVAILABLE` → `SP_DATASTORE_UNAVAILABLE` (nothing is sent). This is the only external-action record for the write: callers do not open one of their own (step 9.1).
4. Upload with `If-Match: <eTag just read>`, simple or by upload session by size. 412 → `SP_ORIGIN_CHANGED` with the current version read again; the external action stays `sent` and the Push Service resolves it as not pushed (the origin was not overwritten).
5. Success → read the new `SharePointVersion`, `journal.confirm_external_action(action.key)`, update `sharepoint_items` with the new tags (so the next `check_changes` does not report ordin's own push as a change), and `journal.append_entry(NewJournalEntry { kind: "sharepoint_pushed", component: "sharepoint", run_id: action.run_id, crew_run_id: None, workspace_id: None, summary: "A new version was sent to SharePoint.", data: {"kind": <external action kind>, "version_label": <label or null>} })`. A failure of `confirm_external_action` or `append_entry` after Graph accepted the upload is logged `sharepoint_confirm_failed {action_key}` and the method still returns `Ok` (the Journal's start-up recovery settles a `sent` row; the Push Service reads it with `unsettled_external_actions`).
6. Timeout or connection loss after sending: read the item's metadata; `cTag` still equals `expected_version` → the upload did not land: retry once from step 4 with the same key; otherwise → `SP_OUTCOME_UNKNOWN` (retry never automatic, G8; 04:200 "never resent automatically"); the Push Service shows **Try again** after comparing versions.

### 7. Creating a file (`create_file`)

1. Validate `name` as 5.1; size as 6.1.
2. `begin_external_action` with `action.kind` (the Push Service passes `CopyPublish`; the Handover Service `HandoverWritten`) and the destination folder path.
3. Upload with `conflictBehavior=fail`. 409 → `SP_NAME_EXISTS` (the external action stays `sent`, nothing was written; the card offers **Use another name**, 09 ADR-069).
4. Success → `confirm_external_action`, return the new item and version.
5. Timeout after sending → `exists(folder, name)`; absent → retry once; present → `SP_OUTCOME_UNKNOWN` (the Push Service decides).

### 8. Folders, lookups and removal

1. `create_folder(parent, name, action)`: `parent.is_folder` must be true (else `SP_LOCATION_INVALID`); validate `name` as 5.1; `begin_external_action` as 7.2; `POST …/{parent-id}/children` with `{"name", "folder": {}, "@microsoft.graph.conflictBehavior": "fail"}`; 409 → `SP_NAME_EXISTS`; success → `confirm_external_action`, return the new folder's `SharePointLocation` (its `path` is the parent's path plus `/name`). Timeout after sending → `exists(parent, name)`; absent → retry once; present → `SP_OUTCOME_UNKNOWN`.
2. `item_at(folder, path)`: each segment validated as 5.1; `GET …/{folder-id}:/{path}` → 200 → `SharePointItem` with its version (label read as in step 3.2); 404 → `None`.
3. `delete(item, expected_version, action)`: read metadata (404 → `SP_NOT_FOUND`); `expected_version` present and different from `cTag` → `SP_ORIGIN_CHANGED { current }` before anything is sent; `begin_external_action` as 7.2; `DELETE …/items/{item-id}` with `If-Match: <eTag just read>` (Graph moves it to the recycle bin; nothing is purged); 204 → `confirm_external_action`, delete the item's `sharepoint_items` row so `check_changes` does not report ordin's own removal; 412 → `SP_ORIGIN_CHANGED`. Timeout after sending → read metadata: 404 → confirm and return `Ok`; still present with the same `cTag` → retry once; otherwise `SP_OUTCOME_UNKNOWN`.

### 9. One external-action bracket per write

1. Every write method begins the external action itself with the caller's `ExternalWrite` and confirms it on success. A caller that also opened a record for the same key would make this adapter return `SP_ACTION_KEY_USED` and send nothing; the Push Service and the Handover Service therefore mint the key and leave the record to this adapter (SA-RECON-02).

## Data Model

SQLite (desktop, personal server), in the Journal's `Datastore` (D4). This adapter runs on no agent-profile server, so `migrations()` has an empty `postgres` slice. The SQL below is migration step 1 of module `sharepoint`.

```sql
CREATE TABLE sharepoint_link_state (
    source_link_id   TEXT PRIMARY KEY,
    drive_id         TEXT NOT NULL,
    root_item_id     TEXT NOT NULL,
    path_prefix      TEXT NOT NULL,                  -- folder or file path from the library root
    delta_link       TEXT NOT NULL CHECK (length(delta_link) <= 4096),
    tracked_at       TEXT NOT NULL,
    last_checked_at  TEXT,
    last_error       TEXT                            -- error code only, no message text
);

CREATE TABLE sharepoint_items (
    source_link_id   TEXT NOT NULL REFERENCES sharepoint_link_state(source_link_id) ON DELETE CASCADE,
    item_id          TEXT NOT NULL,
    origin_path      TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    ctag             TEXT NOT NULL,
    etag             TEXT NOT NULL,
    version_label    TEXT,
    changed_at       TEXT NOT NULL,
    changed_by       TEXT,
    removed          INTEGER NOT NULL DEFAULT 0 CHECK (removed IN (0,1)),
    PRIMARY KEY (source_link_id, item_id)
);
CREATE INDEX sharepoint_items_path ON sharepoint_items (source_link_id, origin_path);

CREATE TABLE sharepoint_origin_changes (
    id               TEXT PRIMARY KEY,               -- UUIDv7
    source_link_id   TEXT NOT NULL REFERENCES sharepoint_link_state(source_link_id) ON DELETE CASCADE,
    origin_path      TEXT NOT NULL,
    new_version      TEXT NOT NULL,
    changed_by       TEXT,
    changed_at       TEXT NOT NULL,
    removed          INTEGER NOT NULL CHECK (removed IN (0,1)),
    found_at         TEXT NOT NULL
);
CREATE INDEX sharepoint_origin_changes_found ON sharepoint_origin_changes (source_link_id, found_at);
```

`untrack_link` deletes the link's `sharepoint_link_state` row (cascading). No statement deletes or changes anything in SharePoint other than the writes of Internal Logic 6, 7 and 8 (a removal moves the item to the site's recycle bin). Rollback of the migration is the SQLite backup (SA-XC-07).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The company has not registered ordin with Microsoft (no tenant or client in the profile) | `Err(permanent)` | `SP_NOT_CONFIGURED` | `{"error":"SharePoint isn't set up for your company in ordin.","code":"SP_NOT_CONFIGURED","action":"send_to_it"}` |
| No Microsoft sign-in, refused, or a second 401 | `Err(permanent)` until sign-in | `SP_SIGN_IN_NEEDED` | `{"error":"Sign in to SharePoint so Ordin can read this location.","code":"SP_SIGN_IN_NEEDED","action":"sign_in_again"}` |
| Graph 403 | `Err(permanent)` | `SP_ACCESS_DENIED` | `{"error":"You don't have access to this SharePoint location.","code":"SP_ACCESS_DENIED","action":null}` |
| Graph 404 | `Err(permanent)` | `SP_NOT_FOUND` | `{"error":"This SharePoint location no longer exists.","code":"SP_NOT_FOUND","action":null}` |
| Stored origin form or a file name is malformed | `Err(permanent)` | `SP_LOCATION_INVALID` | `{"error":"ordin can't use that SharePoint location.","code":"SP_LOCATION_INVALID","action":null}` |
| Search text empty or over 200 characters | `Err(permanent)` | `SP_QUERY_INVALID` | `{"error":"Type what to look for in SharePoint.","code":"SP_QUERY_INVALID","action":null}` |
| Origin version differs from the one read (before sending, or 412) | `Err(permanent)` for this push | `SP_ORIGIN_CHANGED` | `{"error":"This document was changed in SharePoint after Ordin read it.","code":"SP_ORIGIN_CHANGED","action":null}` |
| A file of that name exists (409 on create) | `Err(permanent)` | `SP_NAME_EXISTS` | `{"error":"A file with that name is already in that folder.","code":"SP_NAME_EXISTS","action":null}` |
| File above 250 MiB, or Graph 413 | `Err(permanent)` | `SP_TOO_LARGE` | `{"error":"This file is too large for ordin to send to SharePoint.","code":"SP_TOO_LARGE","action":null}` |
| 429 after three retries | `Err(retryable)` | `SP_THROTTLED` | `{"error":"SharePoint asked ordin to slow down. ordin will try again shortly.","code":"SP_THROTTLED","action":"try_again"}` |
| 5xx, 503, 504 or network failure after retries; Identity unreachable | `Err(retryable)` | `SP_UNAVAILABLE` | `{"error":"Can't reach SharePoint right now.","code":"SP_UNAVAILABLE","action":"try_again"}` |
| Upload or create timed out after sending and the origin shows a change | `Err(permanent)`, never retried automatically | `SP_OUTCOME_UNKNOWN` | `{"error":"ordin isn't sure whether SharePoint received this change.","code":"SP_OUTCOME_UNKNOWN","action":null}` |
| Graph body does not match the expected shape | `Err(permanent)` | `SP_RESPONSE_INVALID` | `{"error":"SharePoint sent an answer ordin couldn't read.","code":"SP_RESPONSE_INVALID","action":"send_to_it"}` |
| `check_changes` on a link with no tracking state | `Err(permanent)` (caller defect) | `SP_NOT_TRACKED` | — (internal) |
| The caller's external-action key was already used | `Err(permanent)` (caller defect) | `SP_ACTION_KEY_USED` | — (internal; nothing sent) |
| Own tables unreachable | `Err(retryable)` | `SP_DATASTORE_UNAVAILABLE` | `{"error":"ordin couldn't reach its own records.","code":"SP_DATASTORE_UNAVAILABLE","action":"try_again"}` |
| Malformed paging cursor | `Err(permanent)` | `SRV_BAD_CURSOR` | `{"error":"ordin couldn't load more of this list.","code":"SRV_BAD_CURSOR","action":"try_again"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: SharePoint Adapter

  Background:
    Given a Graph test double at GRAPH_BASE and Identity returning a Microsoft token

  Scenario: Happy path — find and list a supplier folder
    Given the site "Procurement" has library "Documents" with folder "Suppliers/Contracts" holding 3 files
    When find_locations("supplier contracts", 10) is called
    Then the folder location is returned with path "Suppliers/Contracts"
    And list_children of it returns 3 SharePointItems, each with a cTag version

  Scenario: Edge case — a listing longer than one Graph page is read to the end
    Given the folder holds 450 files returned 100 per Graph page with @odata.nextLink
    When list_children is called and its pages are followed by next_cursor
    Then 450 items are returned in all over 5 adapter pages
    And the last page has next_cursor null

  Scenario: Error path — a cursor for another folder is refused
    Given a next_cursor returned by list_children for folder A
    When list_children is called for folder B with that cursor
    Then it fails with SRV_BAD_CURSOR and no Graph request is sent

  Scenario: Happy path — an approved push uploads a new version
    Given "Kestrel MSA.docx" has cTag "c4" and the working copy read "c4"
    When upload_version is called with expected_version "c4" and key K
    Then the external action K is begun before the PUT and confirmed after it
    And the PUT carries If-Match with the eTag just read
    And the returned version has a new cTag

  Scenario: Error path — the origin changed since it was read
    Given "Kestrel MSA.docx" now has cTag "c5"
    When upload_version is called with expected_version "c4"
    Then it fails with SP_ORIGIN_CHANGED carrying the current version
    And no PUT was sent and no external action was begun

  Scenario: Error path — the origin changes between the check and the PUT
    Given the Graph double answers the PUT with 412
    When upload_version is called with the matching expected_version
    Then it fails with SP_ORIGIN_CHANGED and nothing at the origin was overwritten

  Scenario: Error path — a timeout after sending is never retried when the origin changed
    Given the PUT times out and the item's cTag afterwards is "c5"
    When upload_version is called with expected_version "c4"
    Then it fails with SP_OUTCOME_UNKNOWN and exactly one PUT was sent

  Scenario: Happy path — a copy's first publish creates the file
    Given no file "Supplier day rates.docx" exists in "Procurement › Rates"
    When create_file is called with that name
    Then the file is created with conflictBehavior fail and its id and version are returned

  Scenario: Error path — the name is taken at the destination
    Given "Supplier day rates.docx" exists in the folder
    When create_file is called with that name
    Then it fails with SP_NAME_EXISTS

  Scenario: Happy path — a change by someone else is found at the next check
    Given the link "Suppliers/Contracts" is tracked
    And Sam Lee saves a new version of "Kestrel MSA.docx" in SharePoint
    When check_changes is called for the link
    Then one OriginChange is returned for "Kestrel MSA.docx" changed by "Sam Lee"
    And changes_since(the link, a time before the check) returns it

  Scenario: Edge case — ordin's own push is not reported as a change
    Given upload_version succeeded for "Kestrel MSA.docx"
    When check_changes is called
    Then no OriginChange is returned for "Kestrel MSA.docx"

  Scenario: Edge case — an expired delta token resynchronises
    Given the stored delta link answers 410 Gone
    When check_changes is called
    Then the link is walked again and only items whose cTag differs are reported

  Scenario: Edge case — permissions cannot be read (C-42 fallback)
    Given the permissions request answers 403
    When audience is called
    Then the line is "everyone who can open it" and everyone_who_can_open is true

  Scenario: Error path — no Microsoft sign-in
    Given Identity's access_token_for fails with CRED_SIGN_IN_REQUIRED
    When find_locations is called
    Then it fails with SP_SIGN_IN_NEEDED and the action is sign_in_again

  Scenario: Happy path — a handover folder is created with its files
    Given the place "Procurement › Handovers" exists and has no folder "2026-01-10-northwind"
    When create_folder is called with that name and kind handover_written, then create_file for "handover.md" and "handover.yaml" in it
    Then the folder and both files exist and three external actions of kind handover_written are begun and confirmed

  Scenario: Happy path — a handover record is read and written conditionally
    Given "2026-01-10-northwind/handover.yaml" exists with cTag "c2"
    When item_at is called with that path
    Then the item with version tag "c2" is returned
    And upload_version with expected_version "c2" writes the appended record

  Scenario: Edge case — item_at on a missing path
    When item_at is called with "2026-01-10-northwind/missing.yaml"
    Then it returns None

  Scenario: Happy path — a published copy is removed to the recycle bin
    Given the copy "Supplier day rates.docx" has cTag "c3"
    When delete is called with expected_version "c3" and kind copy_publish
    Then a DELETE with If-Match is sent and the external action is confirmed

  Scenario: Error path — a copy changed at its destination is not removed
    Given the copy now has cTag "c4"
    When delete is called with expected_version "c3"
    Then it fails with SP_ORIGIN_CHANGED and no DELETE was sent

  Scenario: Error path — a caller that opened the record itself is refused
    Given the Journal already holds external action K
    When create_file is called with key K
    Then it fails with SP_ACTION_KEY_USED and nothing is sent

  Scenario: Error path — throttled beyond the retries
    Given Graph answers 429 with Retry-After 2 four times
    When item_version is called
    Then it waits as told between attempts and fails with SP_THROTTLED

  Scenario: Happy path — a handover place given as a web address is resolved
    Given the folder "Procurement › Handovers" has web address "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents/Handovers"
    When resolve_web_url is called with that address
    Then its SharePointLocation is returned with path "Handovers"

  Scenario: Error path — a web address outside SharePoint is refused
    When resolve_web_url is called with "https://example.com/x"
    Then it fails with SP_LOCATION_INVALID and no Graph request is sent
```

## Performance / Security / Observability

- **Performance targets.** Excluding Graph's own time: request building and parsing ≤ 5 ms p95 per call (*chosen here*). `check_changes` with an empty delta ≤ 1 s p95 on the company network; `find_locations` first results ≤ 3 s p95 (network-bound; the screen shows progress at once, 11 §9). Memory: at most one 10 MiB upload range plus the file being sent (≤ 250 MiB) per call; at most 4 concurrent Graph requests per adapter (*chosen here*).
- **Security.** The person's delegated token only (09 ADR-029); the token never leaves the `Authorization` header to Graph, is not sent to the download redirect host, and is never logged or stored by this crate. Every outbound call uses the system proxy and trust store (A-39, SA-HTTP-02). Responses are parsed into typed structs; names and text from SharePoint are data. A push checks the origin's version first and sends with `If-Match`, so an origin is never overwritten (ADR-029). Every write is an external action begun before sending and never resent automatically after an unknown outcome (G8). The paging cursor is accepted only when it decodes to a Graph URL.
- **Never logged.** Tokens, file content, file and folder names, search text, delta links, user names from `lastModifiedBy`.
- **Observability (logs only; SA-XC-04/05).** `tracing` events with `component = "sharepoint"`: `graph_request {operation, status, elapsed_ms, attempt}`, `graph_throttled {operation, retry_after_s}`, `sharepoint_link_tracked {source_link_id, items}`, `sharepoint_changes_found {source_link_id, count}`, `delta_resync {source_link_id}`, `sharepoint_pushed {kind: "version"|"create", size_bytes}`, `sharepoint_outcome_unknown {action_key}`, `sharepoint_confirm_failed {action_key}`.

## Gaps

None.
