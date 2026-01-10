# SPEC: Jira Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · added January 2026 (ADR-106, decision 86)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-jira` · **Build phase:** 2 · **Programs:** D
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

Line references: `02 Jira Adapter` is the Jira Adapter entry under *Component responsibilities* in `docs/design/ordin-desktop/02-architecture.md`; `08`, `10`, `11` are `08-interfaces.md`, `10-risks.md`, `11-ux-guidelines.md`; `09 ADR-NNN` is an ADR in `09-decisions.md`; `2C` is `docs/spec/ordin-desktop/phase-2c-shared-types-*.md`.

## Purpose

The Jira Adapter is ordin's typed client for the company's Jira Cloud site, reached with a credential the person holds (02 Jira Adapter; 09 ADR-106; decision 86). It parses Jira project and issue addresses, finds the people who can be assigned issues in a project, searches issues with queries it builds itself, reads an issue with its description, comments, attachments, sub-tasks and linked issues, downloads an attachment, and makes the four writes a handover needs: create an issue, add a label, add a comment and assign the issue. It also checks an API token the person pasted and hands it to Identity to keep. It owns nothing but the calls: it keeps no tables, decides nothing about handovers or workspace links, and asks no approval. The Handover Service and the Workspace Service own those decisions and ask the person first (09 ADR-092 as amended by ADR-106; ADR-029). Every write is an external action in the journal with a unique key, never resent blindly after an unknown outcome.

## Approach

A typed REST client over Jira Cloud's platform REST API v3, reached in one of two ways that the person chooses (09 ADR-106):

- **API token** (no IT set-up): Basic authentication `email:token` sent to `https://{site}/rest/api/3/…`, where `{site}` is the person's `*.atlassian.net` host.
- **OAuth** (when the organisation profile carries `confluence.oauth_client_id`): an Atlassian 3LO bearer token sent through Atlassian's API gateway `https://api.atlassian.com/ex/jira/{cloudId}/rest/api/3/…`, with `cloudId` resolved from `accessible-resources`, as the Confluence Adapter does for Confluence.

Jira's rich text (Atlassian Document Format, ADF) is turned to and from Markdown in this crate with a bounded element set (Internal Logic 8); everything else is read as plain text. Queries are typed (`JiraQuery`): the adapter builds and escapes every JQL string itself, so no caller and no person's text writes raw JQL. Rejected: an MCP Jira server reached through the Connection Binder (an agent tool, not ordin's own fixed code, and outside the approval path for handover writes; 09 ADR-106 *Alternatives*); keeping issue state in this crate (callers already keep the versions they read, so a second copy would drift).

## Shared Context

### Identifiers and time (2C part 1 *Conventions*)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);       // RFC 3339, UTC, ms, `Z`
pub struct RunId(pub uuid::Uuid);                               // UUIDv7, 36-character lowercase
pub struct ExternalActionKey(pub uuid::Uuid);                   // UUIDv7
pub struct CredentialId(pub uuid::Uuid);
pub struct Sha256Hex(pub String);                               // 64 lowercase hex
```

Jira sends times as `2026-01-10T09:00:00.000+0000`; the adapter parses that form (and RFC 3339) and converts to `Timestamp` in UTC. A time it cannot parse → `JIRA_RESPONSE_INVALID`.

### Domain types read or written (2C part 1, copied unchanged)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind { SlackPost, ConnectionWrite, WrightrunRunStart, EmailSend, OriginPush, CopyPublish, TelegramPost,
                              PullRequestOpened, HandoverWritten }   // 2C C-7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionState { Sent, Confirmed, UnknownOutcome }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind { Github, Gitlab, Oidc, Slack, Confluence, Microsoft, Google, Jira,   // Jira: 09 ADR-106
    MailServerSingleSignOn, MailServerPassword, MailServerAppPassword }
```

### Boundary interfaces of other components (copied from the providers' current specs)

```rust
// ordin-identity — IdentityService (IdentityAndCredentials.md, "Tokens and state", "Mail passwords")
/// A secret that never prints: Debug and Display write "[redacted]"; zeroised on drop.
pub struct SecretString(/* private */);
impl SecretString { pub fn new(s: String) -> Self; pub fn expose(&self) -> &str; }
pub struct AccessToken { pub token: SecretString, pub expires_at: Option<Timestamp>, pub credential_id: CredentialId }
impl IdentityService {
    /// Refreshes the active credential of `kind` now; Ok(false) when nothing needed refreshing.
    pub async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError>;
    /// Marks the credential `needs_sign_in`; later token requests return CRED_SIGN_IN_REQUIRED.
    pub async fn report_refused(&self, credential_id: CredentialId, reason_code: &str) -> Result<(), CredError>;
}
pub enum CredError { Retryable { code: &'static str }, Permanent { code: &'static str } }   // CodedError: code(), retryable()

// ordin-journal — Journal (JournalAndArtefactStore.md)
pub struct NewExternalAction { pub action_key: ExternalActionKey, pub run_id: Option<RunId>,
                               pub kind: ExternalActionKind, pub destination: String }
impl Journal {
    /// A used key → `JournalError` with `code() == "EXT_KEY_DUPLICATE"`.
    pub async fn begin_external_action(&self, new: NewExternalAction) -> Result<ExternalAction, JournalError>;
    pub async fn confirm_external_action(&self, key: ExternalActionKey) -> Result<ExternalAction, JournalError>;
}
```

### Boundary interfaces this spec requires (added by the Identity and Credentials amendment for 09 ADR-106, `IdentityAndCredentials.md`)

These do not exist in `IdentityAndCredentials.md` today; Identity owns them (ruling R6: the provider adds what callers need).

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JiraCredentialMethod {
    /// A pasted Atlassian API token, sent with Basic authentication as `email:token`.
    ApiToken { email: String },
    /// Atlassian 3LO through the gateway; scopes as *Jira Cloud surface used* below.
    Oauth,
}
pub struct JiraAccess {
    /// The person's Jira site host, `{name}.atlassian.net`: the credential's account label.
    pub site: String,
    pub method: JiraCredentialMethod,
    /// The API token, or the OAuth access token refreshed when it expires within 5 minutes.
    pub token: AccessToken,
}
impl IdentityService {
    /// The person's single active Jira credential. Ok(None) when no Jira credential exists (never signed in, or signed out).
    /// OAuth: granted scopes must contain `JIRA_OAUTH_SCOPES`, else CRED_SCOPE_MISSING.
    /// A credential marked needs-sign-in → CRED_SIGN_IN_REQUIRED.
    pub async fn jira_access(&self) -> Result<Option<JiraAccess>, CredError>;
    /// Keeps a pasted API token that this adapter has already checked with `GET /rest/api/3/myself`.
    /// Stored as a `CredentialKind::Jira` credential, label = `site`, method api_token, no refresh token;
    /// the token goes to the platform secret store only. Replaces the token of an active Jira credential
    /// for the same site; a different site replaces nothing and returns CRED_DUPLICATE (one Jira site per person).
    pub async fn save_jira_token(&self, site: &str, email: &str, token: SecretString,
                                 account_id: &str, display_name: &str) -> Result<Credential, CredError>;
}
```

### Jira Cloud surface used (09 ADR-106; C-86, C-87, C-88)

Base: API token → `https://{site}/rest/api/3`; OAuth → `https://api.atlassian.com/ex/jira/{cloudId}/rest/api/3`, where `cloudId` is resolved once per token from `GET https://api.atlassian.com/oauth/token/accessible-resources`, choosing the resource whose `url` equals `https://{site}` and whose `scopes` include `read:jira-work`.

| Use | Request |
|---|---|
| Who the credential is | `GET /myself` (`accountId`, `displayName`, `emailAddress`, `accountType`) |
| Project | `GET /project/{key}` (`id`, `key`, `name`) |
| Assignable people (search) | `GET /user/assignable/search?project={KEY}&query={q}&maxResults={n}` |
| Assignable (one person) | `GET /user/assignable/search?project={KEY}&accountId={id}&maxResults=1` |
| Search issues | `POST /search/jql` `{"jql","fields":["summary","assignee","status","labels","updated"],"maxResults"}` (`issues`, `nextPageToken`) |
| Issue | `GET /issue/{key}?fields=summary,description,assignee,status,labels,updated,attachment,subtasks,issuelinks` |
| Comments | `GET /issue/{key}/comment?startAt={n}&maxResults=100&orderBy=created` |
| Attachment content | `GET /attachment/content/{id}` (redirect followed to Atlassian's media host only) |
| Issue types for create | `GET /issue/createmeta/{KEY}/issuetypes?maxResults=50` |
| Create issue | `POST /issue` `{"fields":{"project":{"key"},"summary","issuetype":{"id"}}}` |
| Add a label | `PUT /issue/{key}` `{"update":{"labels":[{"add":"{label}"}]}}` (`notifyUsers` default) |
| Add a comment | `POST /issue/{key}/comment` `{"body": <ADF document>}` |
| Assign | `PUT /issue/{key}/assignee` `{"accountId"}` |
| Find own just-created issue (unknown outcome only) | `POST /search/jql` with the JQL of Internal Logic 7.4 |

`JIRA_OAUTH_SCOPES` (the least set that covers the table): `read:jira-work`, `write:jira-work`, `read:jira-user`, `offline_access`. They are requested through the same Atlassian OAuth app as Confluence (profile key `confluence.oauth_client_id`, 09 ADR-106) in a separate grant; this crate only asks Identity for the token.

Status categories: Jira `statusCategory.key` `new` → `ToDo`, `indeterminate` → `InProgress`, `done` → `Done`; any other key → `JIRA_RESPONSE_INVALID`.

### Address forms (*chosen here*, 09 ADR-106)

| Kind | Canonical form | Also accepted |
|---|---|---|
| Project (a handover place) | `https://{site}/projects/{KEY}` | `https://{site}/jira/software/projects/{KEY}…`, `/jira/core/projects/{KEY}…`, `/jira/software/c/projects/{KEY}…`, `/browse/{KEY}` |
| Issue (a workspace link, a handover's issue) | `https://{site}/browse/{KEY}-{N}` | any project form above followed by `?selectedIssue={KEY}-{N}`; `/browse/{KEY}-{N}` with a trailing path or query |

Rules: scheme `https` only; the whole address ≤ 2048 characters; `{site}` is a host matching `^[a-z0-9][a-z0-9-]{0,62}\.atlassian\.net$` (lower-cased first), with no port, user information or fragment-only form; `{KEY}` matches `^[A-Z][A-Z0-9_]{1,254}$` after upper-casing; `{N}` matches `^[1-9][0-9]{0,9}$`. Anything else → `JIRA_ADDRESS_INVALID`. Parsing makes no network call.

### Constants (*chosen here* unless cited)

| Constant | Value |
|---|---|
| `REQUEST_TIMEOUT` | 30 s for a call; 120 s for an attachment download |
| `RETRIES`, `BACKOFF` | 3 attempts; 1 s, 2 s, 4 s ±20 % jitter; a `Retry-After` header wins when it is ≤ 60 s |
| `MAX_CONCURRENT_REQUESTS` | 4 per adapter |
| `MAX_ATTACHMENT_BYTES` | 100 MiB (as the Confluence Adapter's attachments, 09 ADR-106) |
| `MAX_COMMENTS` | 2,000 per issue read |
| `MAX_BODY_CHARS` | 32,767 characters of ADF JSON text per comment or description written (Jira Cloud's field limit) |
| `MAX_RESPONSE_BYTES` | 10 MiB per JSON response |
| `MAX_ASSIGNABLE` | 20 per `find_assignable` |
| `MAX_SEARCH` | 50 per `search` (A-54) |
| `MAX_ALSO_KEYS` | 50 keys in `JiraQuery::HandoversFor.also_keys` (A-43, A-54) |
| `LABEL_RULE` | `^[a-z0-9][a-z0-9-]{0,63}$` |

### Confirmation items bound here

- **C-86** — Jira Cloud returns an issue's comments in creation order with a stable `created`, and an edited comment has `updated` later than `created`. This adapter returns comments in the order Jira gives with `orderBy=created`, then sorts stably by `(created, id as integer)`; callers decide what an edit means. *If not confirmed*, the sort on `(created, id)` still gives one order for every reader, and the Handover Service's reading of edits is unaffected.
- **C-87** — an Atlassian API token with Basic authentication reaches every endpoint in the table without admin set-up. *If not confirmed*, or where an organisation admin blocks API tokens (R-103), `connect_with_token` fails with `JIRA_TOKEN_REFUSED` and the person is offered **Sign in with Atlassian** when the profile has the OAuth app; without it, Jira stays off with that message.
- **C-88** — assignable-user search returns `accountId` and `displayName`; `emailAddress` may be absent under the user's privacy settings. `JiraUser.email` is therefore `Option`, and every caller identifies people by `account_id`.

### Reconciliation assumptions

- **SA-JIRA-01** Single external-action bracket (as SA-RECON-01 of the Confluence Adapter): the caller mints the `ExternalActionKey` (UUIDv7) and passes it; this adapter alone calls `begin_external_action` before sending and `confirm_external_action` after. Every write uses `kind: HandoverWritten` and `run_id: None`, because the Handover Service is the only caller that writes to Jira (09 ADR-106: workspace links are read-only).
- **SA-JIRA-02** The destination string of every external action is `Jira › {site} › {KEY-N}` for a write to an issue, and `Jira › {site} › {KEY}` for `create_issue`. The Handover Service's restart recovery matches its own handovers by this form.
- **SA-JIRA-03** One Jira site per person (as Confluence): every call checks that the site in its argument equals `JiraAccess.site`; otherwise `JIRA_SITE_MISMATCH`, nothing sent.
- **SA-JIRA-04** `connect_with_token` is reached through the Service API (command below); the token travels once from the paste field to the core and never back to the WebView, a log line, the journal or a screen payload (`IdentityAndCredentials.md` Purpose).
- **SA-JIRA-05** The adapter is constructed only by the desktop program. The ordin server builds no Jira Adapter (09 ADR-106: desktop only; no Atlassian device grant), and its callers hold `Option<Arc<JiraAdapter>>`.

## Public Interface

Error prefix: `JIRA_`.

```rust
pub struct JiraDeps {
    pub journal: std::sync::Arc<Journal>,
    pub identity: std::sync::Arc<IdentityService>,
    pub http: reqwest::Client,                                     // SA-HTTP-02
}

pub struct JiraAdapter { /* private */ }

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraProjectRef {
    pub site: String,          // host, `{name}.atlassian.net`
    pub key: String,           // `ABC`
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueRef {
    pub site: String,
    pub key: String,           // `ABC-12`
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraProject { pub id: String, pub key: String, pub name: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraUser {
    pub account_id: String,    // 1–128
    pub display_name: String,  // 1–255
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>, // absent under the person's privacy settings (C-88)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraStatusCategory { ToDo, InProgress, Done }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueSummary {
    pub issue: JiraIssueRef,
    pub id: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<JiraUser>,
    pub status_category: JiraStatusCategory,
    pub labels: Vec<String>,
    pub updated: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraCodeBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraComment {
    pub id: String,
    pub author: JiraUser,
    pub created: Timestamp,
    pub updated: Timestamp,
    /// The whole body as Markdown, code blocks included as fenced blocks (Internal Logic 8.2).
    pub body_markdown: String,
    /// Every code block of the body, in document order, as written (no Markdown escaping).
    pub code_blocks: Vec<JiraCodeBlock>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraAttachment { pub id: String, pub filename: String, pub mime_type: String, pub size: u64, pub created: Timestamp }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraLinkedIssue {
    /// Jira's own wording for the link from this issue, e.g. "blocks", "is blocked by", "relates to".
    pub relation: String,
    pub issue: JiraIssueSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueContent {
    pub summary: JiraIssueSummary,
    pub description_markdown: String,          // "" when the issue has no description
    pub comments: Vec<JiraComment>,            // all pages, creation order (C-86)
    pub attachments: Vec<JiraAttachment>,
    pub subtasks: Vec<JiraIssueSummary>,       // as listed on the issue; those issues are not read
    pub linked: Vec<JiraLinkedIssue>,          // as listed on the issue; those issues are not read
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraCommentBody {
    pub markdown: String,
    /// Appended after the Markdown as one ADF code block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_block: Option<JiraCodeBlock>,
}

/// Typed queries; the adapter builds and escapes the JQL (Internal Logic 3). Callers never pass JQL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JiraQuery {
    /// Issues carrying `label` that are assigned to the signed-in person and not in the Done category,
    /// plus the issues named in `also_keys` whatever their assignee or status.
    HandoversFor { label: String, also_keys: Vec<String> },
    /// The person's search text from **Add to this workspace › Jira issue…**.
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JiraDownload { pub bytes_written: u64, pub sha256: Sha256Hex }

impl JiraAdapter {
    pub fn new(deps: JiraDeps) -> std::sync::Arc<JiraAdapter>;

    // Addresses (pure)
    pub fn parse_project_address(address: &str) -> Result<JiraProjectRef, JiraError>;
    pub fn parse_issue_address(address: &str) -> Result<JiraIssueRef, JiraError>;
    pub fn project_address(project: &JiraProjectRef) -> String;   // canonical form
    pub fn issue_address(issue: &JiraIssueRef) -> String;         // canonical form

    // Connection
    /// Checks a pasted API token with `GET /myself`, then keeps it through Identity (`save_jira_token`).
    pub async fn connect_with_token(&self, site: &str, email: &str, token: SecretString) -> Result<JiraUser, JiraError>;
    pub async fn myself(&self, site: &str) -> Result<JiraUser, JiraError>;

    // Reading
    pub async fn project(&self, project: &JiraProjectRef) -> Result<JiraProject, JiraError>;
    pub async fn find_assignable(&self, project: &JiraProjectRef, query: &str, limit: u32) -> Result<Vec<JiraUser>, JiraError>;
    pub async fn is_assignable(&self, project: &JiraProjectRef, account_id: &str) -> Result<bool, JiraError>;
    pub async fn search(&self, site: &str, query: &JiraQuery, limit: u32) -> Result<Vec<JiraIssueSummary>, JiraError>;
    pub async fn issue(&self, issue: &JiraIssueRef) -> Result<JiraIssueContent, JiraError>;
    pub async fn comments(&self, issue: &JiraIssueRef) -> Result<Vec<JiraComment>, JiraError>;
    /// Streams the attachment to `dest_path` (written to `<dest_path>.part`, then renamed).
    pub async fn download_attachment(&self, issue: &JiraIssueRef, attachment_id: &str, dest_path: &std::path::Path)
        -> Result<JiraDownload, JiraError>;

    // Writes (the Handover Service only; SA-JIRA-01)
    pub async fn create_issue(&self, project: &JiraProjectRef, summary: &str, key: ExternalActionKey) -> Result<JiraIssueRef, JiraError>;
    pub async fn add_label(&self, issue: &JiraIssueRef, label: &str, key: ExternalActionKey) -> Result<(), JiraError>;
    pub async fn add_comment(&self, issue: &JiraIssueRef, body: &JiraCommentBody, key: ExternalActionKey) -> Result<JiraComment, JiraError>;
    pub async fn assign(&self, issue: &JiraIssueRef, account_id: &str, key: ExternalActionKey) -> Result<(), JiraError>;

    // Conversions (pure)
    pub fn markdown_to_adf(markdown: &str, code_block: Option<&JiraCodeBlock>) -> Result<serde_json::Value, JiraError>;
    pub fn adf_to_markdown(adf: &serde_json::Value) -> Result<(String, Vec<JiraCodeBlock>), JiraError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum JiraError {
    #[error("JIRA_ADDRESS_INVALID")] AddressInvalid,
    #[error("JIRA_NOT_CONNECTED")] NotConnected,
    #[error("JIRA_SIGN_IN_NEEDED")] SignInNeeded,
    #[error("JIRA_TOKEN_REFUSED")] TokenRefused,
    #[error("JIRA_SITE_MISMATCH")] SiteMismatch,
    #[error("JIRA_ACCESS_DENIED")] AccessDenied,
    #[error("JIRA_NOT_FOUND")] NotFound,
    #[error("JIRA_QUERY_INVALID")] QueryInvalid,
    #[error("JIRA_ISSUE_TYPE_NONE")] IssueTypeNone,
    #[error("JIRA_TOO_LARGE")] TooLarge,
    #[error("JIRA_THROTTLED")] Throttled,
    #[error("JIRA_UNAVAILABLE")] Unavailable,
    #[error("JIRA_RESPONSE_INVALID")] ResponseInvalid,
    #[error("JIRA_OUTCOME_UNKNOWN")] OutcomeUnknown,
    #[error("JIRA_ACTION_KEY_USED")] ActionKeyUsed,
}
impl JiraError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
// retryable: Throttled, Unavailable. Every other variant is permanent.
```

### Service API commands (*chosen here*)

| Command | Input | Result |
|---|---|---|
| `jira.connect_token` | `{ "site": string (host), "email": string (3–254), "token": string (1–1024) }` | `JiraUser` (no token in the result) |
| `jira.find_issues` | `{ "text": string (1–200), "limit": u32 (1–25) }` | `Vec<JiraIssueSummary>` (the connected site) |

`jira.connect_token` is marked *sensitive input* in the catalogue: its input is never logged, journalled or echoed (SA-JIRA-04). The OAuth path is Identity's `begin_sign_in` for `CredentialKind::Jira`, not a command of this crate. Linking an issue is the Workspace Service's command, and handing over is the Handover Service's; receivers are found through the Handover Service's `handover.find_people`, which calls `find_assignable`, so this crate has no command for it.

### Callers of the public interface

| Item | Called by |
|---|---|
| `parse_project_address`, `project`, `find_assignable`, `is_assignable`, `search` (`HandoversFor`), `comments`, `create_issue`, `add_label`, `add_comment`, `assign`, `project_address`, `issue_address` | Handover Service |
| `parse_issue_address`, `issue`, `download_attachment`, `issue_address` | Workspace Service |
| `connect_with_token`, `myself`, `search` (`Text`) | Service API catalogue (`jira.*`) |
| `new` | desktop program |

### Example

```rust
let project = JiraAdapter::parse_project_address("https://acme.atlassian.net/jira/software/projects/ABC/boards/3")?;
// JiraProjectRef { site: "acme.atlassian.net", key: "ABC" }
let people = jira.find_assignable(&project, "sam", 20).await?;
// [JiraUser { account_id: "5b10ac8d82e05b22cc7d4ef5", display_name: "Sam Lee", email: None }]
let issue = JiraIssueRef { site: "acme.atlassian.net".into(), key: "ABC-12".into() };
let c = jira.add_comment(&issue, &JiraCommentBody {
    markdown: "Handed over to Sam Lee".into(),
    code_block: Some(JiraCodeBlock { language: Some("yaml".into()),
        text: "# ordin.handover/v1 event\nkind: sent\n…".into() }),
}, key).await?;
// external action "Jira › acme.atlassian.net › ABC-12" begun before the POST and confirmed after
```

## Internal Logic

### 1. Calling Jira (every request)

1. `identity.jira_access()`: `Ok(None)` → `JIRA_NOT_CONNECTED`; `CredError` by `code()`: `CRED_SIGN_IN_REQUIRED`, `CRED_SCOPE_MISSING`, `CRED_PROVIDER_REFUSED`, `CRED_KEYCHAIN_MISSING` → `JIRA_SIGN_IN_NEEDED`; any other retryable code → `JIRA_UNAVAILABLE`; any other permanent code → `JIRA_SIGN_IN_NEEDED`.
2. The site of the call's argument (`JiraProjectRef.site`, `JiraIssueRef.site` or `site`) must equal `access.site`; otherwise `JIRA_SITE_MISMATCH`, nothing sent (SA-JIRA-03).
3. Base and header: `ApiToken { email }` → `https://{site}/rest/api/3`, `Authorization: Basic base64("{email}:{token}")`; `Oauth` → resolve `cloudId` (cached per `credential_id`; no matching resource → `JIRA_SIGN_IN_NEEDED`), `https://api.atlassian.com/ex/jira/{cloudId}/rest/api/3`, `Authorization: Bearer {token}`. `Accept: application/json` on every call.
4. Status mapping: 401 → OAuth: `identity.refresh_silently(Jira)` once and repeat; API token, or a second 401: `identity.report_refused(credential_id, "JIRA_TOKEN_REFUSED")` then `JIRA_SIGN_IN_NEEDED`. 403 → `JIRA_ACCESS_DENIED`. 404 → `JIRA_NOT_FOUND`. 400 on `/search/jql` → `JIRA_QUERY_INVALID` (a defect in the built JQL, logged with the operation only). 413 → `JIRA_TOO_LARGE`. 429 or 503 → back-off per `RETRIES`/`BACKOFF` (reads always; writes only as step 7.5 allows), then `JIRA_THROTTLED` (429) or `JIRA_UNAVAILABLE` (503). Other 5xx, connection failure or timeout before sending → retried as 429, then `JIRA_UNAVAILABLE`. A body that does not parse, exceeds `MAX_RESPONSE_BYTES`, or lacks a required field → `JIRA_RESPONSE_INVALID`.
5. At most `MAX_CONCURRENT_REQUESTS` in flight (a semaphore). Log `jira_request {operation, status, elapsed_ms, attempt}` only.

### 2. Addresses and connection

1. `parse_project_address` / `parse_issue_address`: apply *Address forms*; `url::Url::parse` first, then host and path rules; the key is upper-cased, the host lower-cased. A project form given to `parse_issue_address` without `selectedIssue` → `JIRA_ADDRESS_INVALID`.
2. `connect_with_token(site, email, token)`: `site` lower-cased must match the host rule; `email` 3–254 characters containing one `@`; `token` 1–1024 bytes; otherwise `JIRA_ADDRESS_INVALID` (site) or `JIRA_TOKEN_REFUSED` (email, token). `GET https://{site}/rest/api/3/myself` with Basic authentication (this call does not go through step 1.1, because nothing is stored yet): 401 or 403 → `JIRA_TOKEN_REFUSED` (R-103: an admin may block API tokens); 404 → `JIRA_ADDRESS_INVALID`; network failure or 5xx after retries → `JIRA_UNAVAILABLE`; `accountType` other than `atlassian` → `JIRA_TOKEN_REFUSED`. Then `identity.save_jira_token(site, email, token, accountId, displayName)`; a `CredError` → `JIRA_SIGN_IN_NEEDED` when permanent, `JIRA_UNAVAILABLE` when retryable. Return the `JiraUser`. The token is never logged, echoed or returned.
3. `myself(site)`: step 1, then `GET /myself` → `JiraUser`.

### 3. Building queries (JQL is never taken from a caller)

1. String values are written inside double quotes with `\` → `\\` and `"` → `\"`; control characters (U+0000–U+001F, U+007F) are removed. Issue keys are written unquoted only after they match `^[A-Z][A-Z0-9_]{1,254}-[1-9][0-9]{0,9}$`.
2. `HandoversFor { label, also_keys }`: `label` must match `LABEL_RULE`; `also_keys` ≤ `MAX_ALSO_KEYS`, each matching the key rule (else `JIRA_QUERY_INVALID`). JQL: `labels = "{label}" AND ((assignee = currentUser() AND statusCategory != Done) OR key in ({k1}, {k2}, …)) ORDER BY updated DESC`; with no `also_keys`, the `OR key in (…)` part is left out. Fields: `summary, assignee, status, labels, updated`. One `POST /search/jql` with `maxResults = limit` (≤ `MAX_SEARCH`, else `JIRA_QUERY_INVALID`); `nextPageToken` is not followed (A-54: one search per check).
3. `Text(s)`: `s` trimmed 1–200 characters (else `JIRA_QUERY_INVALID`). When `s` upper-cased matches the issue-key rule → `GET /issue/{key}?fields=summary,assignee,status,labels,updated` and return that one issue (404 → empty list). Otherwise JQL `text ~ "{s escaped}" ORDER BY updated DESC`, `maxResults = limit` (≤ 25).

### 4. Reading

1. `project`: `GET /project/{key}` → `JiraProject`.
2. `find_assignable(project, query, limit)`: `query` trimmed 1–100 (else `JIRA_QUERY_INVALID`), `limit` 1–`MAX_ASSIGNABLE`; the query is sent URL-encoded; results with `active: false` or `accountType` other than `atlassian` are dropped. A typed email address that Jira does not match returns an empty list; this adapter never accepts an address as a person.
3. `is_assignable(project, account_id)`: `account_id` 1–128 characters of `[A-Za-z0-9:_-]` (else `JIRA_QUERY_INVALID`); the one-person request; `true` when the result holds an active user with that `accountId`.
4. `issue(issue)`: the issue request; `description` through `adf_to_markdown` (absent → ""); attachments from `fields.attachment`; sub-tasks from `fields.subtasks`; links from `fields.issuelinks` (`outwardIssue` with `type.outward`, `inwardIssue` with `type.inward` as `relation`); then `comments(issue)`.
5. `comments(issue)`: page with `startAt` 0, 100, 200… until `startAt + maxResults ≥ total`; more than `MAX_COMMENTS` → `JIRA_TOO_LARGE`. Each body through `adf_to_markdown`; sort stably by `(created, id as integer)` (C-86).
6. `download_attachment(issue, attachment_id, dest_path)`: read the issue's attachment list (step 4.4 fields `attachment` only); the id absent → `JIRA_NOT_FOUND`; `size` above `MAX_ATTACHMENT_BYTES` → `JIRA_TOO_LARGE` before any download. `GET /attachment/content/{id}`; a redirect is followed only to an `https` host ending `.atlassian.net`, `api.atlassian.com` or `api.media.atlassian.com`, otherwise `JIRA_RESPONSE_INVALID`. Stream to `<dest_path>.part` while hashing; more bytes than `MAX_ATTACHMENT_BYTES` → stop, delete the part file, `JIRA_TOO_LARGE`; on success rename to `dest_path` and return the size and SHA-256. Any failure deletes the part file. The Authorization header is never sent to the redirect host.

### 5. Creating an issue (`create_issue`)

1. `summary` trimmed 1–255 characters, with no line break (else `JIRA_QUERY_INVALID`).
2. Issue type: `GET /issue/createmeta/{KEY}/issuetypes`; drop types with `subtask: true`; choose the one named `Task` (case-insensitive) if present, else the first remaining in Jira's order (09 ADR-106; OQ-69). None → `JIRA_ISSUE_TYPE_NONE`, nothing sent.
3. Step 7.1, destination `Jira › {site} › {KEY}`; `POST /issue`; 201 → confirm (step 7.4) and return the new key.
4. Timeout or connection loss after sending (outcome unknown): one search `project = "{KEY}" AND reporter = currentUser() AND summary ~ "{summary escaped}" AND created >= -15m ORDER BY created DESC` with `maxResults = 2`. Exactly one issue whose `summary` equals `summary` → confirm and return it. Otherwise → `JIRA_OUTCOME_UNKNOWN`; the external action stays `sent` and nothing is sent again.

### 6. Label, comment and assignee

1. `add_label(issue, label, key)`: `label` matches `LABEL_RULE`; step 7.1; `PUT /issue/{key}` with the label add; 204 → confirm. Adding a label the issue already has changes nothing in Jira, so after an unknown outcome the write is repeated once with the same key (step 7.5).
2. `add_comment(issue, body, key)`: `adf = markdown_to_adf(body.markdown, body.code_block)`; its serialised length above `MAX_BODY_CHARS` → `JIRA_TOO_LARGE`, nothing sent; step 7.1; `POST /issue/{key}/comment`; 201 → confirm and return the created comment read back from the response (author, `created`, `updated`, body converted as step 4.5). A timeout after sending → `JIRA_OUTCOME_UNKNOWN`, never resent here: a comment is not idempotent, and the caller re-reads `comments` to find its own `event_id` before it decides.
3. `assign(issue, account_id, key)`: `account_id` as step 4.3; step 7.1; `PUT /issue/{key}/assignee`; 204 → confirm. 400 (the person cannot be assigned) → `JIRA_ACCESS_DENIED`. Assigning the same person twice changes nothing, so it is repeated once after an unknown outcome (step 7.5).

### 7. External actions (every write; SA-JIRA-01, SA-JIRA-02)

1. Before sending: `journal.begin_external_action(NewExternalAction { action_key: key, run_id: None, kind: ExternalActionKind::HandoverWritten, destination })`. `JournalError` with `code() == "EXT_KEY_DUPLICATE"` → `JIRA_ACTION_KEY_USED`, nothing sent; any other `JournalError` → `JIRA_UNAVAILABLE`, nothing sent.
2. The request is sent once per attempt with the same body.
3. A 4xx answer means Jira did not apply the write: the error is returned per step 1.4 and the external action is left `sent` for the caller's records (the caller treats the write as refused).
4. Success → `journal.confirm_external_action(key)`; a failure to confirm is logged `jira_confirm_failed {action_key}` and the result is still returned, because the write landed.
5. A 429 or 503 answer to a write means it was not applied: retried per `BACKOFF`. A timeout or connection loss after the request was sent: `add_label` and `assign` are repeated once with the same key; `create_issue` follows step 5.4; `add_comment` returns `JIRA_OUTCOME_UNKNOWN`. Nothing is resent automatically after `JIRA_OUTCOME_UNKNOWN`, and the adapter keeps no queue of writes.

### 8. Conversions (pure)

1. `markdown_to_adf(markdown, code_block)` (`pulldown-cmark`, CommonMark): `doc` version 1; headings → `heading` (`level` 1–6); paragraphs → `paragraph`; emphasis → `text` marks `em`, `strong`, `strike`; inline code → mark `code`; fenced code → `codeBlock` with `attrs.language` when given; bullet and numbered lists → `bulletList` / `orderedList` with `listItem`; links → mark `link` with `href` for `https`, `http` and `mailto` only (others become plain text); block quotes → `blockquote`; rules → `rule`; hard breaks → `hardBreak`. Raw HTML in the Markdown becomes plain text. Tables and images become their source text as a paragraph. `code_block`, when given, is appended as a final `codeBlock` whose text is written exactly as given.
2. `adf_to_markdown(adf)`: the same node set in reverse, plus `mention` → `@{attrs.text}`, `emoji` → `attrs.text`, `inlineCard` / `blockCard` → `<url>`, `panel` → a block quote, `table` → a Markdown table of cell text, `mediaSingle` / `media` → `[attachment: {attrs.alt or id}]`, `status` → `[{text}]`, `date` → the date as `YYYY-MM-DD`. Any other node → its text content, or `[Jira content ordin can't show: {type}]` when it has none. Every `codeBlock` is also returned in the `Vec<JiraCodeBlock>` with its exact text. A value that is not an ADF `doc` → `JIRA_RESPONSE_INVALID`.
3. Text read from Jira is data: the conversions never interpret it, and callers mark instruction-like lines as they do for any linked content (R-81).

## Data Model

None. The adapter keeps no tables and needs no migration: the Handover Service keeps the Jira issue and comment versions of each handover, and the Workspace Service keeps the versions of each linked issue's items (09 ADR-106; one owner per version, as the Approach states). The only state it holds is in memory: the `cloudId` cache per credential id, cleared when Identity publishes a `SignInChanged` for a Jira credential or on a 401.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Address not https, not an `*.atlassian.net` host, bad key or number, over 2048 characters | `Err(permanent)` | `JIRA_ADDRESS_INVALID` | `{"error":"That isn't a Jira Cloud project or issue address.","code":"JIRA_ADDRESS_INVALID","action":null}` |
| No Jira credential | `Err(permanent)` | `JIRA_NOT_CONNECTED` | `{"error":"Connect Jira in Settings to use Jira issues.","code":"JIRA_NOT_CONNECTED","action":"sign_in_again"}` |
| Credential needs sign-in, scope missing, second 401, or OAuth site not granted | `Err(permanent)` until sign-in | `JIRA_SIGN_IN_NEEDED` | `{"error":"Sign in to Jira again to reach this issue.","code":"JIRA_SIGN_IN_NEEDED","action":"sign_in_again"}` |
| Pasted token refused (401/403, blocked by an admin, not an Atlassian account), or email or token out of range | `Err(permanent)` | `JIRA_TOKEN_REFUSED` | `{"error":"Jira didn't accept that email address and API token.","code":"JIRA_TOKEN_REFUSED","action":null}` |
| Argument names a site other than the connected one | `Err(permanent)` | `JIRA_SITE_MISMATCH` | `{"error":"This issue is on a different Jira site from the one you connected.","code":"JIRA_SITE_MISMATCH","action":null}` |
| 403; assignment refused (400 on assign) | `Err(permanent)` | `JIRA_ACCESS_DENIED` | `{"error":"You don't have permission to do this in Jira.","code":"JIRA_ACCESS_DENIED","action":null}` |
| 404; attachment id not on the issue | `Err(permanent)` | `JIRA_NOT_FOUND` | `{"error":"This Jira issue or project no longer exists.","code":"JIRA_NOT_FOUND","action":null}` |
| Search text, label, key list, account id, summary or limit out of range; 400 on a built JQL | `Err(permanent)` | `JIRA_QUERY_INVALID` | `{"error":"Type what to look for in Jira.","code":"JIRA_QUERY_INVALID","action":null}` |
| Project offers no issue type that is not a sub-task | `Err(permanent)` | `JIRA_ISSUE_TYPE_NONE` | `{"error":"This Jira project doesn't let ordin create an issue. Link an existing issue instead.","code":"JIRA_ISSUE_TYPE_NONE","action":null}` |
| Attachment over 100 MiB, over 2,000 comments, comment body over 32,767 characters, or 413 | `Err(permanent)` | `JIRA_TOO_LARGE` | `{"error":"This is too large for ordin to move to or from Jira.","code":"JIRA_TOO_LARGE","action":null}` |
| 429 after retries | `Err(retryable)` | `JIRA_THROTTLED` | `{"error":"Jira asked ordin to slow down. ordin will try again shortly.","code":"JIRA_THROTTLED","action":"try_again"}` |
| 5xx, network failure or timeout before sending, after retries; Journal unavailable | `Err(retryable)` | `JIRA_UNAVAILABLE` | `{"error":"Can't reach Jira right now.","code":"JIRA_UNAVAILABLE","action":"try_again"}` |
| Body unparseable, over 10 MiB, missing a field, unknown status category, redirect to another host | `Err(permanent)` | `JIRA_RESPONSE_INVALID` | `{"error":"Jira sent an answer ordin couldn't read.","code":"JIRA_RESPONSE_INVALID","action":"send_to_it"}` |
| Comment or issue creation timed out after sending and could not be found | `Err(permanent)`, never retried automatically | `JIRA_OUTCOME_UNKNOWN` | `{"error":"ordin isn't sure whether Jira received this change.","code":"JIRA_OUTCOME_UNKNOWN","action":null}` |
| External-action key already used | `Err(permanent)` (caller defect) | `JIRA_ACTION_KEY_USED` | — (internal; nothing sent) |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Jira Adapter

  Background:
    Given a Jira Cloud test double for the site "acme.atlassian.net", reachable directly and behind the Atlassian gateway
    And Identity returning a Jira API-token credential for "acme.atlassian.net" with email "priya.shah@acme.co.uk"

  Scenario: Happy path — addresses parse to the canonical form
    When parse_project_address is called with "https://acme.atlassian.net/jira/software/projects/abc/boards/3"
    Then it returns site "acme.atlassian.net" and key "ABC"
    And project_address of that result is "https://acme.atlassian.net/projects/ABC"
    When parse_issue_address is called with "https://acme.atlassian.net/jira/software/projects/ABC/boards/3?selectedIssue=ABC-12"
    Then it returns key "ABC-12"

  Scenario Outline: Error path — addresses that are not Jira Cloud
    When parse_issue_address is called with "<address>"
    Then it fails with JIRA_ADDRESS_INVALID and no request is sent
    Examples:
      | address                                   |
      | http://acme.atlassian.net/browse/ABC-12   |
      | https://jira.acme.co.uk/browse/ABC-12     |
      | https://acme.atlassian.net/browse/ABC-0   |
      | https://acme.atlassian.net/projects/ABC   |

  Scenario: Happy path — only assignable people are found
    Given the test double lists "Sam Lee" and "Sam Patel" as assignable in project ABC and "Sam Jones" as a user who is not
    When find_assignable is called for ABC with query "sam"
    Then the result holds "Sam Lee" and "Sam Patel" and not "Sam Jones"

  Scenario: Edge case — an email address that is not assignable gives no choice
    When find_assignable is called for ABC with query "someone@other.co.uk"
    Then the result is empty

  Scenario: Happy path — a pasted token is checked and kept without IT set-up
    Given Identity holds no Jira credential and the profile has no Atlassian section
    When connect_with_token is called with site "acme.atlassian.net", email "sam.lee@acme.co.uk" and token "ATATT3x…"
    Then one GET /rest/api/3/myself is sent with Basic authentication
    And save_jira_token receives the site, the email, the token, the account id and the display name
    And the token appears in no log line, no journal entry and no command result

  Scenario: Error path — an admin has blocked API tokens
    Given /myself answers 401 for the pasted token
    When connect_with_token is called
    Then it fails with JIRA_TOKEN_REFUSED and save_jira_token is not called

  Scenario: Happy path — OAuth goes through the gateway
    Given Identity returns an OAuth Jira credential and accessible-resources lists "https://acme.atlassian.net" with cloudId "c1"
    When project is called for ABC
    Then the request goes to "https://api.atlassian.com/ex/jira/c1/rest/api/3/project/ABC" with a Bearer token

  Scenario: Error path — no Jira credential
    Given Identity's jira_access returns none
    When search is called
    Then it fails with JIRA_NOT_CONNECTED and no request is sent

  Scenario: Error path — another site
    When issue is called for "other.atlassian.net" key "ABC-12"
    Then it fails with JIRA_SITE_MISMATCH and no request is sent

  Scenario: Happy path — the handover search is one call with escaped values
    When search is called with HandoversFor label "ordin-handover" and also_keys ["ABC-12", "ABC-13"]
    Then exactly one POST /rest/api/3/search/jql is sent
    And its jql is 'labels = "ordin-handover" AND ((assignee = currentUser() AND statusCategory != Done) OR key in (ABC-12, ABC-13)) ORDER BY updated DESC'

  Scenario: Error path — a key that is not a key is refused before sending
    When search is called with HandoversFor label "ordin-handover" and also_keys ["ABC-12) OR project = HR"]
    Then it fails with JIRA_QUERY_INVALID and no request is sent

  Scenario: Edge case — search text cannot change the query
    When search is called with Text 'rates" OR project = HR OR text ~ "x'
    Then the jql sent is 'text ~ "rates\" OR project = HR OR text ~ \"x" ORDER BY updated DESC'

  Scenario: Happy path — reading an issue brings its parts
    Given ABC-12 has a description, 3 comments, 2 attachments, 1 sub-task and 1 "blocks" link
    When issue is called for ABC-12
    Then the content holds the description as Markdown, 3 comments in creation order, 2 attachments, 1 sub-task and 1 linked issue with relation "blocks"
    And no request reads the sub-task or the linked issue

  Scenario: Happy path — a comment carries a code block that is read back exactly
    When add_comment is called on ABC-12 with markdown "Handed over to Sam Lee" and a yaml code block "# ordin.handover/v1 event\nkind: sent"
    Then the posted ADF has a paragraph followed by a codeBlock with language "yaml" and that exact text
    And the returned comment's code_blocks holds that text unchanged

  Scenario: Happy path — every write is bracketed as an external action
    When assign is called on ABC-12 for account "5b10ac8d82e05b22cc7d4ef5" with key K
    Then the external action K with kind handover_written and destination "Jira › acme.atlassian.net › ABC-12" is begun before the PUT and confirmed after it

  Scenario: Error path — the write is refused
    Given the test double answers 403 to PUT /issue/ABC-12/assignee
    When assign is called
    Then it fails with JIRA_ACCESS_DENIED and the external action is not confirmed

  Scenario: Error path — a comment whose outcome is unknown is not resent
    Given the POST of a comment times out after it was sent
    When add_comment is called with key K
    Then it fails with JIRA_OUTCOME_UNKNOWN and exactly one POST was sent

  Scenario: Edge case — an assignment whose outcome is unknown is repeated once with the same key
    Given the first PUT /issue/ABC-12/assignee times out after sending and the second answers 204
    When assign is called with key K
    Then it succeeds, two PUTs were sent and K is confirmed once

  Scenario: Edge case — an issue created with an unknown outcome is found again
    Given the POST /issue times out after sending and the test double holds one new issue ABC-40 with that summary reported by the person
    When create_issue is called for ABC with summary "Northwind assessment"
    Then it returns ABC-40 and exactly one POST /issue was sent

  Scenario: Happy path — a new issue uses the Task type
    Given project ABC offers the types "Epic", "Task" and "Sub-task" (a sub-task)
    When create_issue is called for ABC
    Then the POST names the "Task" type

  Scenario: Error path — a project with only sub-task types
    Given project ABC offers only "Sub-task"
    When create_issue is called
    Then it fails with JIRA_ISSUE_TYPE_NONE and no POST /issue is sent

  Scenario: Error path — throttling is retried, then reported
    Given the test double answers 429 with Retry-After 1 to every search
    When search is called
    Then 3 attempts are sent and it fails with JIRA_THROTTLED

  Scenario: Edge case — throttling once does not lose the write
    Given the first POST of a comment answers 429 with Retry-After 1 and the second answers 201
    When add_comment is called with key K
    Then it succeeds with one comment created and K confirmed

  Scenario: Error path — an attachment that is too large is not downloaded
    Given attachment 10001 on ABC-12 has size 104857601
    When download_attachment is called
    Then it fails with JIRA_TOO_LARGE, no content request is sent and no file is left at the destination

  Scenario: Edge case — raw HTML and unknown nodes stay text
    When markdown_to_adf is called with "<script>alert(1)</script>"
    Then the result is one paragraph whose text is "<script>alert(1)</script>"
    When adf_to_markdown is called with a doc holding an "extension" node with no text
    Then the Markdown is "[Jira content ordin can't show: extension]"
```

## Performance / Security / Observability

- **Performance targets.** Excluding Jira's own time: `markdown_to_adf` and `adf_to_markdown` of a 100 KiB body ≤ 20 ms p95; `search` adds ≤ 5 ms p95 to the request; a receiving check for one site is one search request (A-54). At most 4 concurrent requests.
- **Security.** The person's own credential only (09 ADR-106): an API token the person created, or an OAuth token through the company's Atlassian app. Tokens are never logged, stored by this crate, returned to a caller or sent to any host but the person's `*.atlassian.net` site and `api.atlassian.com`; an attachment redirect never carries the Authorization header. JQL is built only from typed, validated values with quoted and escaped strings, so no caller or person's text adds a JQL operator. Raw HTML in Markdown is never passed through as markup. Content read from Jira is data, never followed as an instruction (R-81). The adapter writes only the four calls the Handover Service makes after its approval card (09 ADR-092, ADR-106), each an external action begun before sending; it never changes status, fields or work logs and never deletes anything in Jira.
- **Never logged.** Tokens, email addresses, account ids, issue summaries, descriptions, comment bodies, attachment names, search text, display names.
- **Observability (logs only; SA-XC-04/05).** `tracing` events with `component = "jira"`: `jira_request {operation, status, elapsed_ms, attempt}`, `jira_throttled {operation, retry_after_ms}`, `jira_written {operation: "create_issue"|"add_label"|"add_comment"|"assign"}`, `jira_outcome_unknown {operation, action_key}`, `jira_confirm_failed {action_key}`, `jira_token_checked {accepted: bool}`.

## Gaps

None.
