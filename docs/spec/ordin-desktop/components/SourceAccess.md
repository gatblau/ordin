# SPEC: Source Access

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · added January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-source-access` · **Build phase:** 2 · **Programs:** A
> **Dependencies:** Agent Configuration, Journal, Telemetry

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
- [Assumptions](#assumptions)
- [Confirmation items](#confirmation-items)
- [Gaps](#gaps)

## Purpose

Source Access is a knowledge agent's only client of the document systems for anything other than fetching Git repositories (ADR-115). It applies one rule on the agent's behalf: **knowledge inherits its readers** — a person receives an item derived from a document only if the system that holds the document would let that person open it at the time of the question. To do this it keeps **no copy of any reader list**. It does five things:

1. **Links a person's accounts.** Given a token the person's own ordin already holds for GitHub, GitLab, Confluence or SharePoint, it asks that system who the token belongs to, returns the account identifier and drops the token.
2. **Answers access checks.** Given a person's linked accounts and a list of **access units** (a repository, a Confluence page or a SharePoint item), it asks each system, within a deadline, whether that person may read each unit, and returns `Allowed`, `Refused` or `Unchecked` per unit.
3. **Remembers answers briefly.** It keeps each `Allowed` or `Refused` answer in the running program's own memory (RAM) for `remember-seconds` (60 by default, 0 to 300), never in the datastore, and forgets it early when a sharing-change notice names the unit.
4. **Accepts sharing-change notices** from GitHub and GitLab webhooks, verifies their signature, and forgets every remembered answer for the units they name.
5. **Reads Confluence spaces and SharePoint libraries** for the Knowledge Reader: lists documents, finds those changed since a mark, fetches each document's text and version, and states whether a document is restricted beyond its space or library.

It answers no query and decides no admission: the Knowledge Query Server calls it before ranking, and the Knowledge Reader calls it during a reading cycle.

## Approach

One crate with no knowledge of memory or of the query pipeline, built on `reqwest` 0.13.5 (rustls) as every adapter is, with a table of four **check routes**, one per `SourceSystem`, behind one internal trait `SystemRoute` (`whoami`, `check`, optional `read`). Checks for one question run as independent futures, at most `CHECK_CONCURRENCY_PER_SYSTEM` (16) in flight per system, under one `tokio::time::timeout_at(deadline)`; anything not answered by the deadline is `Unchecked`. The short memory is a bounded `HashMap` behind a `tokio::sync::Mutex` in the running program's memory (no new library), keyed by (company subject, access unit), so no stored row ever ties a person to a document; in the scaled shape each request instance has its own memory, which is safe because every remembered answer came from the source system within `remember-seconds`. Rejected: copying reader lists into the agent's datastore and refreshing them — the lists are large, they change without notice, and a copy decides access instead of the source system (ADR-115). Rejected: asking the person's computer to open each document — it would have to be told which documents to try, and all four systems hide restricted items from people who may not read them (ADR-115 *Never reveal*). Rejected: a datastore-backed shared memory of answers — it would store who may read what. Rejected: a cache library — a bounded map covers the need without adding a member to the tech stack.

## Shared Context

### Shared types (Phase 2C wire, `ordin-types`; copied)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSystem { Github, Gitlab, Confluence, Sharepoint }

/// The thing an owner grants access to in a source system (ADR-115).
/// Git: the repository key. Confluence: "<site host>/pages/<page id>". SharePoint: "<drive id>/<item id>".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AccessUnit { pub system: SourceSystem, pub unit: String /* 1..=2,048 */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome { Allowed, Refused, Unchecked }

/// The person's account in one source system, learned from the system itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkedAccount { pub system: SourceSystem, pub account_id: String /* 1..=256 */, pub account_label: String /* 1..=256 */,
    pub linked_at: Timestamp, pub last_used_at: Timestamp }

pub struct Timestamp(chrono::DateTime<Utc>);   // RFC 3339 ms "Z" on the wire
```

A **repository key** (KnowledgeReader.md SA-KRD-12, copied) is `<host>/<owner path>` of the URL without scheme, port, user information or `.git`, lower-cased, then `@<branch>` and, with a sub-path, `:<path>` — for example `git.example.org/supply/contracts@release`. The Git access unit is `<host>/<owner path>` only — the repository key with `@<branch>` and `:<path>` removed (SA-SRC-03) — because access is granted to the whole repository; the Knowledge Query Server derives units in the same form.

**Origins** (memory source `origin_ref`, copied from ADR-115): Git `git:<repository key>/<path>`; Confluence `confluence:<site host>/pages/<page id>`; SharePoint `sharepoint:<drive id>/<item id>`. The access unit of a Confluence or SharePoint origin is the origin without its `confluence:` or `sharepoint:` prefix.

### From the agent configuration (copied from AgentConfiguration.md amendment January 2026, ADR-115; YAML section `knowledge-agent`)

| Key | Rule | Default |
|---|---|---|
| `repositories[].audience-may-read` | required; `false` means per-person checks for that repository | — |
| `repositories[].host-kind` | `github` \| `gitlab`; required when `audience-may-read: false` | absent |
| `repositories[].check-every-question` | bool; only with `audience-may-read: false` | `false` |
| `repositories[].credential-env` | variable holding the agent's read token for the repository (existing key) | — |
| `confluence.site` | https rule | — |
| `confluence.account-email-env`, `confluence.token-env` | variables holding the agent's own Atlassian account e-mail and API token | — |
| `confluence.person-checks` | bool; `true` only when the company has given the agent's account the Confluence Administrator permission | `false` |
| `confluence.spaces[]` | 1..=100: `key` (1..=255), `audience-may-read` (bool), `check-every-question` (bool, default false) | — |
| `sharepoint.tenant-id`, `sharepoint.client-id` | the agent's own Microsoft Entra app registration | — |
| `sharepoint.client-secret-env` | variable holding the app's secret | — |
| `sharepoint.read-token-env` | variable holding the refresh token of the agent's own Microsoft account (delegated `Files.Read.All offline_access`) | — |
| `sharepoint.libraries[]` | 1..=100: `site-url` (https rule), `library` (1..=255), `path` (optional, 1..=512), `check-every-question` (bool, default false) | — |
| `access-checks.remember-seconds` | 0..=300 | 60 |
| `notices.github-secret-env`, `notices.gitlab-token-env` | optional; variables holding the webhook secret / token | absent |

The program resolves every `*-env` variable before calling `SourceAccess::new` and passes the values as `SecretString`; this crate reads no environment variable itself (2D-1d).

### Delegation (Phase 2C wire, `ordin_types::wire`; imported, copied here)

```rust
/// ADR-115: a Microsoft token whose audience is the agent's app (`api://<client-id>/knowledge.check`);
/// sent by the person's ordin with each question; held for that one question, never stored or logged.
pub struct Delegation { pub sharepoint_assertion: SecretString }
```

### From the Journal (`ordin-journal`, copied from JournalAndArtefactStore.md)

```rust
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }   // engine(), sqlite(), postgres()
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
impl Journal {
    pub fn datastore(&self) -> &Datastore;
    pub async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>;
}
```

### From Telemetry (`ordin-telemetry`, copied from Telemetry.md amendment January 2026, ADR-115)

```rust
impl Telemetry {
    pub fn record(&self, event: TelemetryEvent);
}
pub enum TelemetryEvent {
    // … families 1–58 …
    KnowledgeAccessCheck { system: SourceSystem, outcome: CheckOutcome },             // 59
    KnowledgeAccessCheckSeconds { system: SourceSystem, seconds: f64 },               // 60
    KnowledgeAccessRemembered { system: SourceSystem },                               // 61
    KnowledgeAccountLinked { system: SourceSystem, outcome: KnowledgeLinkOutcome },   // 62 (this crate only)
    KnowledgeAccessNotice { system: SourceSystem, outcome: KnowledgeNoticeOutcome },  // 63
    KnowledgeUncheckedAnswer,                                                         // 64 (Knowledge Query Server)
}
pub enum KnowledgeLinkOutcome { Linked, Refused }
pub enum KnowledgeNoticeOutcome { Applied, Duplicate, Ignored, Refused }
```

This crate records families 59–63; family 62 is recorded here only, never by the Knowledge Query Server.

### Constants (2D-4)

| Constant | Value | Source |
|---|---|---|
| `CHECK_DEADLINE_MS` | 300 | ADR-115 (inside the Knowledge Query Server's `FIRST_DEADLINE_MS` 1,000) |
| `REMEMBER_SECONDS_DEFAULT` | 60 (0–300) | ADR-115 |
| `REMEMBER_CAPACITY` | 100,000 entries | *chosen here* (SA-SRC-07) |
| `GRAPH_BATCH_MAX` | 20 | ADR-115; Microsoft Graph JSON batching limit |
| `CHECK_CONCURRENCY_PER_SYSTEM` | 16 | ADR-115 |
| `WHOAMI_TIMEOUT_S` | 5 | *chosen here* |
| `READ_TIMEOUT_S` | 30 per request | *chosen here* |
| `RETRIES`, `BACKOFF` (reads and whoami only) | 3; 1 s, 2 s, 4 s ±20 % jitter, `Retry-After` wins | RateLimiting.md L26 |
| `NOTICE_SEEN_HOURS` | 24 | ADR-115 |
| `NOTICE_BODY_MAX_BYTES` | 1,048,576 | *chosen here* |
| `DOCUMENT_MAX_BYTES` | 1,048,576 | KnowledgeReader.md SA-KRD-01 (same bound as Git documents) |
| `CONFLUENCE_PAGE_LIST_LIMIT` | 100 per request | ConfluenceAdapter.md (change CQL `limit=100`) |
| `MAX_LIBRARY_ITEMS` | 100,000 | A-21 (SharePoint listing bound) |

## Public Interface

```rust
// crate ordin-source-access
pub fn migrations() -> ordin_journal::ModuleMigrations;          // module "source_access"

pub struct SourceAccessDeps {
    pub journal: Arc<ordin_journal::Journal>,
    pub telemetry: Arc<ordin_telemetry::Telemetry>,
    /// The program's shared HTTP client (reqwest 0.13.5, rustls, redirects refused).
    pub http: reqwest::Client,
    pub settings: SourceAccessSettings,
}

pub struct SourceAccessSettings {
    /// Repositories with `audience-may-read: false`, one entry each.
    pub git: Vec<GitUnitSettings>,
    pub confluence: Option<ConfluenceSettings>,
    pub sharepoint: Option<SharePointSettings>,
    pub remember_seconds: u16,                                  // 0..=300
    pub github_notice_secret: Option<SecretString>,
    pub gitlab_notice_token: Option<SecretString>,
}
pub struct GitUnitSettings {
    pub unit: String,                    // "<host>/<owner path>", e.g. "github.com/supply/contracts" (SA-SRC-03)
    pub host_kind: GitHostKind,          // ordin_agent_configuration::GitHostKind (imported)
    pub host: String,                    // lower-case host, e.g. "github.com", "git.example.org"
    pub owner_path: String,              // "supply/contracts"; GitLab: may hold sub-groups
    pub token: SecretString,             // from `credential-env`
    pub check_every_question: bool,
}
// `GitHostKind { Github, Gitlab }` is owned by Agent Configuration and imported (no local host-kind enum).
pub struct ConfluenceSettings {
    pub site: String,                    // https://<site host>
    pub account_email: SecretString, pub api_token: SecretString,
    pub person_checks: bool,
    pub spaces: Vec<ConfluenceSpace>,
}
pub struct ConfluenceSpace { pub key: String, pub audience_may_read: bool, pub check_every_question: bool }
pub struct SharePointSettings {
    pub tenant_id: String, pub client_id: String,
    pub client_secret: SecretString,
    pub read_refresh_token: SecretString,
    pub libraries: Vec<SharePointLibrary>,
}
pub struct SharePointLibrary { pub site_url: String, pub library: String, pub path: Option<String>,
    pub check_every_question: bool }

pub struct SourceAccess { /* routes per system, remember cache, readers' tokens */ }

impl SourceAccess {
    /// Validates the settings; opens no connection. KQ-side configuration errors are the Agent
    /// Configuration's; this guard refuses an inconsistent settings value built in code.
    pub fn new(deps: SourceAccessDeps) -> Result<Self, SrcError>;

    // ---- linking ----
    /// Asks `system` who `token` belongs to. The token is used for this one request and dropped.
    pub async fn link_account(&self, system: SourceSystem, token: SecretString) -> Result<WhoAmI, SrcError>;

    // ---- checks ----
    /// One outcome per unit, in input order. Never fails as a whole: every failure is an Unchecked unit.
    pub async fn check_units(&self, asker: &AskerAccounts, units: &[AccessUnit],
                             delegation: Option<&Delegation>, deadline: tokio::time::Instant) -> Vec<UnitCheck>;
    /// The systems this agent can check per person (for the Knowledge Query Server's "sign in" hint).
    pub fn checkable_systems(&self) -> Vec<SourceSystem>;
    /// Whether a unit must be checked on every question (its source entry says `check-every-question`).
    pub fn always_check(&self, unit: &AccessUnit) -> bool;

    // ---- notices ----
    /// Verifies and applies one webhook delivery. `headers` and `body` are passed untouched by the route.
    pub async fn accept_notice(&self, system: SourceSystem, headers: &http::HeaderMap, body: &[u8])
        -> Result<NoticeOutcome, SrcError>;

    // ---- reading (Knowledge Reader) ----
    pub async fn confluence_changed(&self, space_key: &str, since: Option<Timestamp>) -> Result<Vec<SourceDocumentRef>, SrcError>;
    pub async fn confluence_restricted(&self, page_ids: &[String]) -> Result<Vec<(String, bool)>, SrcError>;
    pub async fn confluence_fetch(&self, page_id: &str) -> Result<SourceDocument, SrcError>;
    pub async fn sharepoint_changes(&self, library: &SharePointLibrary, mark: Option<&str>) -> Result<SharePointChanges, SrcError>;
    pub async fn sharepoint_fetch(&self, drive_id: &str, item_id: &str) -> Result<SourceDocument, SrcError>;

    /// Drops every remembered answer (operator action `knowledge forget-checks`, and graceful shutdown).
    pub fn forget_all(&self);
}

/// The linked accounts of the person a question is for. Built by the Knowledge Query Server from
/// `kq_linked_accounts`, or from a verified asker statement on a proxy.
pub struct AskerAccounts { pub subject: String /* company subject */, pub accounts: BTreeMap<SourceSystem, String /* account_id */> }

// `Delegation` is imported from `ordin_types::wire` (Phase 2C wire; see Shared Context), not defined here.

pub struct WhoAmI { pub system: SourceSystem, pub account_id: String /* 1..=256 */, pub account_label: String /* 1..=256 */ }

pub struct UnitCheck { pub unit: AccessUnit, pub outcome: CheckOutcome, pub reason: Option<CheckReason>, pub remembered: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "snake_case")]
pub enum CheckReason { NotLinked, NotConfigured, NoDelegation, Timeout, Unavailable, Forbidden, RateLimited, DelegationRefused }

pub struct NoticeOutcome { pub units_forgotten: u32, pub duplicate: bool }

pub struct SourceDocumentRef {
    pub origin: String,                  // per *Origins*
    pub unit: AccessUnit,
    pub title: String,                   // 1..=500; data, never instructions
    pub version: String,                 // Confluence version number; SharePoint cTag
    pub changed_at: Timestamp,
    pub removed: bool,
    /// Confluence: the page or an ancestor has a read restriction. SharePoint: always true (SA-SRC-12).
    pub restricted: bool,
}
pub struct SourceDocument { pub reference: SourceDocumentRef, pub body: String /* UTF-8 text, ≤ DOCUMENT_MAX_BYTES */, pub kind: BodyKind }
#[derive(Debug, Clone, Copy)] pub enum BodyKind { Markdown, Text }
pub struct SharePointChanges { pub changed: Vec<SourceDocumentRef>, pub next_mark: String /* deltaLink, ≤ 4,096 */, pub resynced: bool }

#[derive(Debug, thiserror::Error)]
pub enum SrcError {
    /* one variant per code of the Error Table; SACC_READ_FAILED is: */
    #[error("SACC_READ_FAILED")] ReadFailed { reason: ReadFailure },
    /* … */
}
/// The closed set of reading failures (§J step 8), matched by the Knowledge Reader one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "snake_case")]
pub enum ReadFailure { SignIn, NotReadable, TooLarge, NotUtf8 }
impl ordin_types::CodedError for SrcError { fn code(&self) -> &'static str; fn retryable(&self) -> bool; }
```

**Called by.**

| Public item | Called by |
|---|---|
| `new`, `migrations`, `forget_all` | the server program (agent profile with a `knowledge-agent` section) |
| `link_account`, `check_units`, `checkable_systems`, `always_check`, `accept_notice` | Knowledge Query Server (routes `knowledge.link_accounts`, `knowledge.query`, `knowledge.notice_github`, `knowledge.notice_gitlab`) |
| `confluence_*`, `sharepoint_*` | Knowledge Reader (reading cycle) |

### Example

`check_units` for a person linked to GitHub as `4412907`, with three units and a delegation for SharePoint:

```text
asker = { subject: "00u8x2asha", accounts: { github: "4412907" } }   // SharePoint: by the delegation, never linked
units = [ {github, "github.com/supply/contracts"},
          {sharepoint, "b!Xy3kq0VdH0uM8ZpL/01ABCDEF2GHIJKLMNOPQRSTUVWX"},
          {confluence, "acme.atlassian.net/pages/884736112"} ]
→ [ {unit: …contracts, outcome: allowed, reason: null, remembered: false},
    {unit: …01ABCDEF…, outcome: refused, reason: null, remembered: false},
    {unit: …884736112, outcome: unchecked, reason: not_configured, remembered: false} ]
```

The third unit is `Unchecked` because `confluence.person-checks` is `false`; the Knowledge Query Server withholds it.

GitHub whoami answer used by `link_account(Github, token)`: `GET https://api.github.com/user` → `{"login":"pshah-dev","id":4412907,…}` → `WhoAmI { system: Github, account_id: "4412907", account_label: "pshah-dev" }`.

## Internal Logic

**A. `new(deps)`** — SA-SRC-01.
1. Every `GitUnitSettings.unit` unique; `host` and `owner_path` non-empty; `confluence.spaces` and `sharepoint.libraries` 1..=100 when the section is present; `remember_seconds` ≤ 300 → else `SACC_SYSTEM_NOT_CONFIGURED` (start-up).
2. GitHub API base: host `github.com` → `https://api.github.com`; any other host → `https://{host}/api/v3` (GitHub Enterprise Server). GitLab API base: `https://{host}/api/v4`. Confluence base: `{site}` with HTTP Basic `account_email:api_token`. SharePoint: token endpoint `https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token`, Graph `https://graph.microsoft.com/v1.0`.
3. Build the remember cache: `tokio::sync::Mutex<RememberMap>`, where `RememberMap` holds a `HashMap<(String /* subject */, AccessUnit), (CheckOutcome, Instant /* stored_at */)>` and a `VecDeque<((String, AccessUnit), Instant)>` in insertion order. A read returns an entry only when `now − stored_at < remember_seconds`, and removes it otherwise. An insert that would exceed `REMEMBER_CAPACITY` (100,000) first pops the oldest entries from the queue (skipping queue entries whose map entry was replaced later). Forgetting by unit (§H) removes every map entry whose key holds that unit; stale queue entries are skipped when popped. With `remember_seconds = 0` no map is built and every check goes to the source.
4. Log `src_started {github_units, gitlab_units, confluence: bool, confluence_person_checks: bool, sharepoint: bool, remember_seconds}` (INFO). No network call.

**B. `link_account(system, token)`** — SA-SRC-02.
1. `system` must be `Github`, `Gitlab` or `Confluence`, and checkable on this agent (`checkable_systems` contains it) → else `SACC_SYSTEM_NOT_CONFIGURED`. SharePoint is never linked: the person is identified for SharePoint on each question by the `Delegation` (§G, SA-SRC-18). For Git systems the host is the one host of that kind in `settings.git` (SA-SRC-04).
2. One request, `WHOAMI_TIMEOUT_S`, the token as `Authorization: Bearer` (GitLab: `PRIVATE-TOKEN` is not accepted; Bearer only):
   - GitHub `GET {api}/user` → `account_id = id` (decimal string), `account_label = login`.
   - GitLab `GET {api}/user` → `account_id = id`, `account_label = username`.
   - Confluence `GET https://api.atlassian.com/me` → `account_id`, `account_label = name`.
3. 401 or 403 → `SACC_LINK_REFUSED` (the token is not valid for that system). 429 → `SACC_RATE_LIMITED`. 5xx, timeout or network error → retry per `BACKOFF` up to `RETRIES`, then `SACC_CHECK_UNAVAILABLE`. A 200 body missing the field, or a field outside 1..=256 characters → `SACC_LINK_REFUSED`.
4. The `SecretString` is dropped when the function returns, on every path; it is never cloned, stored, logged or put in an error. Record `TelemetryEvent::KnowledgeAccountLinked { system, outcome }` (`Linked` / `Refused`; family 62). Log `src_link {system, outcome}` (INFO) — no account id, label or subject.

**C. `check_units(asker, units, delegation, deadline)`** — SA-SRC-05, SA-SRC-06.
1. `deadline_eff = min(deadline, now + CHECK_DEADLINE_MS)`.
2. For each unit, in order:
   a. Unit's system not checkable here → `Unchecked`, `NotConfigured`.
   b. Unit is a Git unit not in `settings.git` → `Unchecked`, `NotConfigured` (an audience-open repository never reaches this crate; the caller skips it).
   c. Cache lookup `(asker.subject, unit)` unless `always_check(unit)`; a hit → its outcome, `remembered = true`; record `TelemetryEvent::KnowledgeAccessRemembered { system }` (family 61).
3. Group the remaining units by system and run each system's check (§D–§G) concurrently, at most 16 requests in flight per system, all under `timeout_at(deadline_eff)`. Units with no answer at `deadline_eff` → `Unchecked`, `Timeout`.
4. Every `Allowed` or `Refused` answer from a source is inserted into the cache under `(asker.subject, unit)` unless `always_check(unit)`. `Unchecked` is never remembered.
5. Record `TelemetryEvent::KnowledgeAccessCheck { system, outcome }` (family 59) once per unit answered by a source, and `TelemetryEvent::KnowledgeAccessCheckSeconds { system, seconds }` (family 60) once per system batch. Log `src_checks {units, allowed, refused, unchecked, remembered, ms}` (DEBUG) — counts only.
6. Return one `UnitCheck` per input unit, in input order.

**D. GitHub check** — SA-SRC-08, C-101.
1. Visibility first, so a public repository needs no link.
2. `GET {api}/repos/{owner_path}` with the unit's token. `visibility = "public"` → `Allowed` for anyone, linked or not. 404 or 403 to the agent's own token → `Unchecked`, `Forbidden` (the agent itself can no longer read it; the Knowledge Reader's SA-KRD-09 drops the repository).
3. Not linked → `Unchecked`, `NotLinked` (never `Refused`: refusal is the source's answer, and an unlinked person has not been asked about).
4. `GET {api}/repos/{owner_path}/collaborators/{login}/permission`, where `login` is resolved once per account id by `GET {api}/user/{account_id}` and kept with the cache entry's key material in memory only (SA-SRC-09). Response `permission` ∈ {`admin`, `maintain`, `write`, `triage`, `read`} → `Allowed`; `none` → `Refused`; 404 for the user → `Refused`. For a repository with `visibility = "internal"`, the same endpoint is used; C-101 confirms that it reports the base read permission of enterprise members.
5. 403 to the agent's token on step 4 (missing permission) → `Unchecked`, `Forbidden`, and log `src_check_forbidden {system: "github"}` (ERROR) once per hour. 429 or `x-ratelimit-remaining: 0` → `Unchecked`, `RateLimited` (no retry inside a question). 5xx, network error → `Unchecked`, `Unavailable`.

**E. GitLab check** — SA-SRC-10, C-102.
1. `GET {api}/projects/{url-encoded owner_path}` with the unit's token → `visibility`. `public` → `Allowed` for anyone.
2. Not linked → `Unchecked`, `NotLinked`.
3. `internal` → `GET {api}/users/{account_id}`; `external = false` → `Allowed`; `external = true` → continue at step 4; field absent from the answer (C-102: may be visible to administrators only) → continue at step 4, so an internal project is allowed only to its members — the safe reading.
4. `private` (or internal from step 3) → `GET {api}/projects/{id}/members/all/{account_id}`. 200 with `access_level ≥ 20` (Reporter) and `state = "active"` → `Allowed`; 200 with a lower level, or 404 → `Refused`. Members of an invited group that is neither public nor joined by the agent's account may be absent from this answer (GitLab members API); such a person is `Refused` — the safe reading (SA-SRC-10).
5. Status mapping as §D.5 with `system: "gitlab"`.

**F. Confluence check** — SA-SRC-11, C-103.
1. `confluence.person_checks = false` → every Confluence unit `Unchecked`, `NotConfigured`. The Knowledge Reader reads no restricted page in that case (§I.3), so only pages already open to the audience exist in the graph, and the caller skips them.
2. Not linked → `Unchecked`, `NotLinked`.
3. Per page, `POST {site}/wiki/rest/api/content/{page id}/permission/check` with `{"subject":{"type":"user","identifier":"<account_id>"},"operation":"read"}`. `hasPermission = true` → `Allowed`; `false` → `Refused`; 403 to the agent's account (it lacks Confluence Administrator) → `Unchecked`, `Forbidden`, logged `src_check_forbidden {system: "confluence"}` (ERROR) once per hour; 404 → `Refused` (the page is gone or hidden from the agent too). No batch exists; at most 16 requests in flight.
4. 429 → `Unchecked`, `RateLimited`; 5xx, network error → `Unchecked`, `Unavailable`.

**G. SharePoint check** — SA-SRC-12, SA-SRC-13, C-104.
1. `delegation` absent → every SharePoint unit `Unchecked`, `NoDelegation`.
2. On-behalf-of exchange, once per call: `POST https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token` form `grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer`, `client_id`, `client_secret`, `assertion = <sharepoint_assertion>`, `scope = https://graph.microsoft.com/Files.Read.All`, `requested_token_use = on_behalf_of`. 400 or 401 (`invalid_grant`, consent missing, wrong audience) → every SharePoint unit `Unchecked`, `DelegationRefused`, and log `src_delegation_refused` (WARN, no token, no subject). Network error or 5xx → `Unchecked`, `Unavailable`.
3. The access token from step 2 lives in a local variable of this call only and is dropped when the call returns.
4. Split the units into batches of ≤ 20 and send each as `POST https://graph.microsoft.com/v1.0/$batch` `{"requests":[{"id":"<n>","method":"GET","url":"/drives/{drive id}/items/{item id}?$select=id"}…]}`. Per response: 200 → `Allowed`; 403 or 404 → `Refused`; 429 → `Unchecked`, `RateLimited`; any other status → `Unchecked`, `Unavailable`. A batch whose outer request fails → every unit in it `Unchecked`, `Unavailable`.

**H. `accept_notice(system, headers, body)`** — SA-SRC-14, C-106.
0. The whole call runs under `tokio::time::timeout(10 s)`; on expiry → `SACC_CHECK_TIMEOUT` (retryable; the sender redelivers and the replay guard of step 4 makes the second delivery safe).
1. `body.len() > NOTICE_BODY_MAX_BYTES` → `SACC_NOTICE_SIGNATURE_INVALID`, without reading further.
2. GitHub: `github_notice_secret` absent → `SACC_SYSTEM_NOT_CONFIGURED`. Header `X-Hub-Signature-256` must be `sha256=<64 hex>` and equal, in constant time (`subtle`), to HMAC-SHA256(secret, body) → else `SACC_NOTICE_SIGNATURE_INVALID`. Delivery id = `X-GitHub-Delivery`; event = `X-GitHub-Event`.
3. GitLab: `gitlab_notice_token` absent → `SACC_SYSTEM_NOT_CONFIGURED`. Header `X-Gitlab-Token` must equal the token in constant time → else `SACC_NOTICE_SIGNATURE_INVALID`. Delivery id = `X-Gitlab-Event-UUID`; event = body `event_name` or `object_kind`.
4. Replay guard: `INSERT INTO src_notice_seen (system, delivery_id, received_at) … ON CONFLICT DO NOTHING`; no row inserted → `NoticeOutcome { units_forgotten: 0, duplicate: true }`. A store error → `SACC_CHECK_UNAVAILABLE` (retryable; the sender redelivers).
5. Map the event to the units to forget (only units in `settings.git` count; a notice for anything else forgets nothing):

   | System | Event | Units forgotten |
   |---|---|---|
   | GitHub | `member` (collaborator added, removed, edited) | the repository `repository.full_name` |
   | GitHub | `repository` with action `privatized`, `publicized`, `edited` | the repository |
   | GitHub | `team_add`; `team` with action `added_to_repository`, `removed_from_repository`, `edited` | the repository when named, else every unit whose owner is `organization.login` |
   | GitHub | `membership` (team member added or removed); `organization` with action `member_added`, `member_removed` | every unit whose owner is `organization.login` |
   | GitLab | group member events `user_add_to_group`, `user_remove_from_group`, `user_update_for_group`, `user_access_request_to_group` | every unit whose `owner_path` starts with `group_path` + `/` |
   | GitLab | project events whose `object_kind` is `project` with `event_name` `project_update` or `project_transfer` | the project `path_with_namespace` |
   | any | any other event, including GitHub `ping` | none |

6. Forget: under the mutex, remove every map entry whose unit is in `units`. Record `TelemetryEvent::KnowledgeAccessNotice { system, outcome }` (`Applied` / `Duplicate` / `Ignored` / `Refused`; family 63). Log `src_notice {system, event, units_forgotten}` (INFO) — no repository name.
7. A daily sweep, run by the first `accept_notice` call after midnight UTC and by `new`, deletes `src_notice_seen` rows older than 24 hours.

**I. Confluence reading** — SA-SRC-15, SA-SRC-16, C-103.
1. `confluence_changed(space_key, since)`: the space must be configured → else `SACC_SYSTEM_NOT_CONFIGURED`. `GET {site}/wiki/rest/api/search?cql=space="{key}" AND type=page[ AND lastmodified >= "{yyyy/MM/dd HH:mm}"]&limit=100`, following `_links.next`, with `since` reduced by 2 minutes for clock skew (ConfluenceAdapter.md, same rule). The key is quoted with `"` and `\` escaped. Each result → `SourceDocumentRef { origin: "confluence:{site host}/pages/{id}", unit, title, version: version.number, changed_at: version.when, removed: false, restricted }` with `restricted` from step 2.
2. `confluence_restricted(page_ids)`: per page `GET {site}/wiki/rest/api/content/{id}?expand=ancestors,restrictions.read.restrictions.user,restrictions.read.restrictions.group`, and for each ancestor not seen in this call `GET {site}/wiki/rest/api/content/{ancestor id}/restriction/byOperation/read`. A page is restricted when it or any ancestor lists at least one user or group under `read`. At most 16 requests in flight.
3. Pages of a space with `audience_may_read = true` that are restricted, while `person_checks = false`, are returned with `restricted = true`; the Knowledge Reader does not read them and retires any facts it holds from them (KnowledgeReader.md amendment). The Reader calls `confluence_restricted` for every page it holds, once per reading cycle, so a restriction added without any change to the page is seen within one fetch interval (R-131).
4. `confluence_fetch(page_id)`: `GET {site}/wiki/api/v2/pages/{id}?body-format=storage`. Convert the storage-format XHTML to Markdown with a fixed table: `h1`–`h6` → `#`…`######`; `p` → paragraph; `ul`/`ol`/`li` → `-` / `1.` items; `table`/`tr`/`th`/`td` → pipe rows; `a` → `[text](href)`; `code`/`pre` → backticks / fenced block; `ac:structured-macro` and `ri:*` elements → dropped, text kept; every other element → its text. Body over `DOCUMENT_MAX_BYTES` after conversion → `SACC_READ_FAILED` with reason `too_large`. `kind = Markdown`.
5. Status mapping for reads: 401 → `SACC_READ_FAILED` (`sign_in`); 403, 404 → `SACC_READ_FAILED` (`not_readable`); 429, 5xx, network → retry per `BACKOFF`, then `SACC_RATE_LIMITED` / `SACC_CHECK_UNAVAILABLE`.

**J. SharePoint reading** — SA-SRC-12, SA-SRC-17, C-104.
1. Agent's own token (C-107, R-132): `POST …/oauth2/v2.0/token` with `grant_type=refresh_token`, `client_id`, `client_secret`, `refresh_token`, `scope = https://graph.microsoft.com/Files.Read.All offline_access`. The newest refresh token returned is kept in memory and used next; the one from `read-token-env` is used only at start. A refused refresh (`invalid_grant`) → `SACC_READ_FAILED` (`sign_in`) and log `src_read_sign_in_needed {system: "sharepoint"}` (ERROR): the operator runs `ordin-server knowledge sign-in sharepoint` again. A restart uses the token in `read-token-env`, which may be older than the rotated one held in memory; if Microsoft has invalidated it, reading stops with `SACC_READ_FAILED` (`sign_in`) until the operator signs the agent in again (C-107; risk R-132). Checks (§G) are unaffected: they use the person's delegation, not this token.
2. Resolve the library once: `GET /sites/{host}:/{site path}` → site id; `GET /sites/{site id}/drives` → the drive whose `name` equals `library` (none → `SACC_SYSTEM_NOT_CONFIGURED`).
3. `sharepoint_changes(library, None)`: walk `GET /drives/{drive}/root:/{path}:/children` (or `/root/children` without a path), following `@odata.nextLink`, ≤ 20 levels, ≤ 100,000 items; then `GET /drives/{drive}/root/delta?token=latest` for the starting mark. Return every file as changed, `resynced = true`.
4. `sharepoint_changes(library, Some(mark))`: follow the mark (`@odata.deltaLink`) and every `@odata.nextLink`; keep entries whose `parentReference.path` + name lies under the library's path; `deleted` facet → `removed = true`; files only. 410 Gone → step 3 again, `resynced = true`, logged `src_delta_resync` (INFO).
5. Only files whose name ends in `.md`, `.markdown` or `.txt` (any case), ≤ `DOCUMENT_MAX_BYTES`, are returned (the Knowledge Reader's document rule, SA-KRD-01). `origin = "sharepoint:{drive id}/{item id}"`, `version = cTag`, `restricted = true` for every item (SA-SRC-12).
6. `sharepoint_fetch(drive, item)`: `GET /drives/{drive}/items/{item}/content` (redirect to the download URL followed for this one request only, to a `*.sharepoint.com` host); body must be valid UTF-8 → else `SACC_READ_FAILED` (`not_utf8`). `kind = Markdown` for `.md`/`.markdown`, else `Text`.
7. Status mapping as §I.5.
8. **Reading never emits `SACC_CHECK_FORBIDDEN`**: that code is a check outcome only (§D–§G). Every reading failure is `SACC_READ_FAILED` with exactly one reason, or `SACC_RATE_LIMITED` / `SACC_CHECK_UNAVAILABLE` after retries. The closed set of `SACC_READ_FAILED` reasons, which the Knowledge Reader maps one by one:

   | Reason | Raised by | Meaning |
   |---|---|---|
   | `sign_in` | §I.5 (401), §J.1 (refresh refused) | the agent's own credential for that system is refused |
   | `not_readable` | §I.5, §J.7 (403, 404) | the space, page, library or file is gone or hidden from the agent |
   | `too_large` | §I.4, §J.5 | the body exceeds `DOCUMENT_MAX_BYTES` |
   | `not_utf8` | §J.6 | the file is not valid UTF-8 |

   `SrcError::ReadFailed { reason: ReadFailure }` carries it, with `pub enum ReadFailure { SignIn, NotReadable, TooLarge, NotUtf8 }` (snake_case on render).

**K. `forget_all()`.** Clear the map and the queue under the mutex. Logged `src_forget_all` (INFO).

## Data Model

Owned table (module `source_access`; SQLite in the single shape, PostgreSQL in the scaled shape). It holds webhook delivery identifiers only — no person, account, unit or outcome.

SQLite, version 1:

```sql
CREATE TABLE src_notice_seen (
    system       TEXT NOT NULL CHECK (system IN ('github', 'gitlab')),
    delivery_id  TEXT NOT NULL CHECK (length(delivery_id) BETWEEN 1 AND 128),
    received_at  TEXT NOT NULL,
    PRIMARY KEY (system, delivery_id)
);
CREATE INDEX src_notice_seen_received ON src_notice_seen (received_at);
```

PostgreSQL, version 1: the same table with `TIMESTAMPTZ` for `received_at`, the same checks and index.

Additive only; no destructive change. Rollback: the pre-migration backup copy (SA-XC-07); a build before this module ignores the table. The remember cache lives in the running program's memory (RAM) and has no schema.

## Error Table

Every code here is **internal**: no `SACC_` code leaves the agent. The Knowledge Query Server maps each `SrcError` to its own `KQ_` code on its routes (route statuses and bodies are in KnowledgeQueryServer.md), and the Knowledge Reader maps reading failures to its document outcomes.

| Condition | Surface | Code | What happens |
|---|---|---|---|
| A unit or link names a system this agent has no per-person check for; a notice for a system with no secret; a SharePoint library that does not exist | internal (mapped by the Knowledge Query Server) | `SACC_SYSTEM_NOT_CONFIGURED` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| Settings inconsistent at start-up | internal (start-up) | `SACC_SYSTEM_NOT_CONFIGURED` | the program stops with the code |
| A check is asked for a person with no linked account in that system (the only use of this code; unlinking is idempotent and never returns it) | internal | `SACC_NOT_LINKED` | not shown; the unit is `Unchecked` with reason `not_linked`; the desktop shows `knowledge-agents-sign-in-to-include` |
| Whoami answered 401 or 403 | internal (mapped by the Knowledge Query Server) | `SACC_LINK_REFUSED` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| Whoami answered 200 without a usable account id | internal (mapped by the Knowledge Query Server) | `SACC_LINK_REFUSED` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| Source system unreachable or 5xx after retries (whoami, read) | internal (mapped by the Knowledge Query Server) | `SACC_CHECK_UNAVAILABLE` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| Datastore unavailable while recording a notice | internal (mapped by the Knowledge Query Server) | `SACC_CHECK_UNAVAILABLE` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| A check did not finish by its deadline | internal | `SACC_CHECK_TIMEOUT` | not shown; unit `Unchecked`, reason `timeout`; the answer says "Some sources couldn't be checked." |
| Recording a notice took longer than 10 s (§H step 0) | internal (mapped by the Knowledge Query Server) | `SACC_CHECK_TIMEOUT` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| The agent's own credential lacks the permission to check (GitHub, GitLab, Confluence 403) | internal | `SACC_CHECK_FORBIDDEN` | not shown; unit `Unchecked`, reason `forbidden`; logged ERROR once per hour |
| The agent's own credential can no longer read the repository | internal | `SACC_CHECK_FORBIDDEN` | not shown; unit `Unchecked`; the Knowledge Reader reports the repository unreadable |
| The source answered 429 or reported its limit spent | internal (mapped by the Knowledge Query Server) | `SACC_RATE_LIMITED` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| 429 on a reading request after retries | internal | `SACC_RATE_LIMITED` | not shown; the Knowledge Reader retries the document at its next cycle |
| On-behalf-of exchange refused (`invalid_grant`, consent missing, wrong audience) | internal | `SACC_DELEGATION_REFUSED` | not shown; SharePoint units `Unchecked`, reason `delegation_refused` |
| Delegation assertion expired before the exchange | internal | `SACC_DELEGATION_REFUSED` | not shown; the desktop's next question carries a fresh assertion |
| Webhook signature or token missing or wrong | internal (mapped by the Knowledge Query Server) | `SACC_NOTICE_SIGNATURE_INVALID` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| Webhook body larger than 1 MiB or not JSON | internal (mapped by the Knowledge Query Server) | `SACC_NOTICE_SIGNATURE_INVALID` | returned to the caller as `SrcError`; the Knowledge Query Server answers with its own `KQ_` code |
| A Confluence page or SharePoint file cannot be read: sign-in refused, gone, not readable, too large, not UTF-8 | internal | `SACC_READ_FAILED` | not shown; the Knowledge Reader counts the document `failed` with the reason |
| The agent's SharePoint refresh token is refused | internal | `SACC_READ_FAILED` | not shown; operator status line "Sign the knowledge agent in to SharePoint again." |

Retryable: `SACC_CHECK_UNAVAILABLE`, `SACC_CHECK_TIMEOUT`, `SACC_RATE_LIMITED`. Every other code is not retryable. `check_units` never returns an error: every failure is an `Unchecked` unit with its reason.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Source Access

  Background:
    Given a knowledge agent with Source Access built from settings with
      | git unit                         | host kind | host       |
      | github.com/supply/contracts      | github    | github.com |
      | git.example.org/finance/ledgers  | gitlab    | git.example.org |
    And Confluence site "https://acme.atlassian.net" and SharePoint tenant "acme"
    And remember-seconds 60

  Scenario: Linking — GitHub tells who the token belongs to
    Given GitHub answers GET /user with id 4412907 and login "pshah-dev"
    When link_account(github, a token) runs
    Then it returns account_id "4412907" and account_label "pshah-dev"
    And no table holds the token, and no log line holds the token, the id or the label

  Scenario Outline: Linking — each system's whoami
    Given <system> answers its whoami request with <id> and <label>
    When link_account(<system>, a token) runs
    Then the account id is <id>
    Examples:
      | system     | id                                   | label                 |
      | gitlab     | 9182                                 | priya.s               |
      | confluence | 5b10ac8d82e05b22cc7d4ef5             | Priya Shah            |

  Scenario: Error — a token the system does not accept
    Given GitLab answers GET /api/v4/user with 401
    When link_account(gitlab, a token) runs
    Then it fails with SACC_LINK_REFUSED

  Scenario: Error — a system the agent cannot check
    Given the settings hold no Confluence section
    When link_account(confluence, a token) runs
    Then it fails with SACC_SYSTEM_NOT_CONFIGURED

  Scenario: GitHub — a collaborator is allowed, a stranger refused
    Given the repository supply/contracts is private
    And GitHub reports permission "read" for "pshah-dev" and "none" for "jdoe"
    When check_units runs for each person with unit github.com/supply/contracts
    Then Priya's outcome is allowed and J's outcome is refused

  Scenario: GitHub — a public repository needs no link
    Given the repository is public and the person has no linked GitHub account
    When check_units runs
    Then the outcome is allowed

  Scenario: Not linked — the unit is unchecked, never refused
    Given a private GitLab project and a person with no linked GitLab account
    When check_units runs
    Then the outcome is unchecked with reason not_linked

  Scenario: GitLab — Reporter or above may read a private project
    Given members/all answers access_level 20 for user 9182 and 10 for user 77
    When check_units runs for each
    Then 9182 is allowed and 77 is refused

  Scenario: GitLab — an internal project when the external flag cannot be read
    Given an internal project and GET /users/9182 answers without the "external" field
    And members/all answers 404 for 9182
    When check_units runs
    Then the outcome is refused

  Scenario: Confluence — person checks off by default
    Given confluence.person-checks is false
    When check_units runs with a Confluence unit
    Then the outcome is unchecked with reason not_configured and no request reaches Confluence

  Scenario: Confluence — permission check when the company allowed it
    Given confluence.person-checks is true
    And permission/check answers hasPermission true for page 884736112
    When check_units runs for that page
    Then the outcome is allowed

  Scenario: Confluence — the agent lacks the administrator permission
    Given confluence.person-checks is true and permission/check answers 403
    When check_units runs
    Then the outcome is unchecked with reason forbidden
    And one src_check_forbidden ERROR line is logged within the hour

  Scenario: SharePoint — one batch for twenty files
    Given a delegation whose on-behalf-of exchange succeeds
    And 20 SharePoint units of which Graph answers 200 for 12 and 404 for 8
    When check_units runs
    Then exactly one token request and one $batch request are sent
    And 12 units are allowed and 8 refused
    And the Graph token appears in no table, log line or metric

  Scenario: SharePoint — no delegation, no check
    Given no delegation
    When check_units runs with a SharePoint unit
    Then the outcome is unchecked with reason no_delegation

  Scenario: Error — the on-behalf-of exchange is refused
    Given the token endpoint answers 400 invalid_grant
    When check_units runs with three SharePoint units
    Then all three are unchecked with reason delegation_refused

  Scenario: Deadline — a slow system yields unchecked units
    Given Confluence answers permission/check after 800 ms
    When check_units runs with a deadline 300 ms away
    Then it returns by 300 ms with the Confluence unit unchecked, reason timeout

  Scenario: Rate limit — no retry inside a question
    Given GitHub answers 429
    When check_units runs
    Then the unit is unchecked with reason rate_limited and only one request was sent

  Scenario: Memory — a second question within 60 s asks nothing
    Given check_units answered allowed for Priya and github.com/supply/contracts at 10:00:00
    When check_units runs again for the same person and unit at 10:00:30
    Then the outcome is allowed with remembered true and no request reaches GitHub
    And at 10:01:01 the source is asked again

  Scenario: Memory — check every question bypasses the memory
    Given the unit's source entry says check-every-question
    When check_units runs twice within one second
    Then the source is asked twice

  Scenario: Memory — remember-seconds 0 keeps nothing
    Given remember-seconds is 0
    When check_units runs twice within one second
    Then the source is asked twice

  Scenario: Notice — a GitHub collaborator removal forgets the repository's answers
    Given remembered answers for three people on github.com/supply/contracts
    When a "member" delivery for supply/contracts with a valid X-Hub-Signature-256 arrives
    Then accept_notice returns units_forgotten 1 and the next check asks GitHub

  Scenario: Notice — a replayed delivery is a duplicate
    Given delivery "72d3162e-cc78-11e3-81ab-4c9367dc0958" was accepted
    When the same delivery arrives again
    Then accept_notice returns duplicate true and forgets nothing

  Scenario: Error — a notice with a wrong signature
    Given a GitHub delivery whose signature does not match the secret
    When accept_notice runs
    Then it fails with SACC_NOTICE_SIGNATURE_INVALID and nothing is forgotten

  Scenario: Notice — a GitLab group member removal forgets the group's projects
    Given remembered answers on git.example.org/finance/ledgers
    When a group delivery with event_name "user_remove_from_group", group_path "finance" and a valid X-Gitlab-Token arrives
    Then the answers on git.example.org/finance/ledgers are forgotten

  Scenario: Reading — a Confluence page restricted by an ancestor
    Given page 884736112 has no restriction and its parent page has a read restriction for group "legal"
    When confluence_restricted(["884736112"]) runs
    Then the page is reported restricted

  Scenario: Reading — Confluence storage format becomes Markdown
    Given a page body "<h2>Price review</h2><p>Reviewed each April.</p>"
    When confluence_fetch runs
    Then the body is "## Price review\n\nReviewed each April.\n" and kind is markdown

  Scenario: Reading — SharePoint delta after the starting walk
    Given a library walked once with mark M
    When one file under the library path changes and one outside it changes
    Then sharepoint_changes(library, M) returns only the file under the path, with restricted true

  Scenario: Error — the SharePoint read token is refused
    Given the token endpoint answers invalid_grant to the refresh token
    When sharepoint_changes runs
    Then it fails with SACC_READ_FAILED and the operator status says to sign the agent in again

  Scenario: Error — a file that is not UTF-8
    Given a .txt file holding Latin-1 bytes
    When sharepoint_fetch runs
    Then it fails with SACC_READ_FAILED with reason not_utf8

  Scenario: SharePoint — the delegation and the exchanged token are dropped when the question completes
    Given a delegation whose on-behalf-of exchange succeeds, and a test double holding a Weak reference to the assertion and to the Graph token
    When check_units returns
    Then both Weak references fail to upgrade
    And no table, log line or metric holds the assertion or the Graph token
    And a second check_units without a delegation sends no Graph request

  Scenario: Never reveal — no identifier in logs or metrics
    Given check_units ran with refused and unchecked units
    Then no log line and no metric label holds a subject, account id, unit, title or token
```

## Performance, Security, Observability

- **Performance targets:** `check_units` returns by `min(deadline, now + 300 ms)` in every case; with each source's mock answering in 150 ms, ≤ 180 ms p95 for 30 units across the four systems (one SharePoint batch, ≤ 16 Confluence requests in flight, Git units remembered or one request each). A remembered answer costs ≤ 1 ms. Memory ceiling of the remember cache: ≤ 40 MiB at 100,000 entries. Measured against real systems in C-105.
- **Security:** a person's token (linking) and delegation assertion are `SecretString`, used for one request or one call and dropped; they are never stored, cloned into an error, logged or put in a metric. The Graph token from the exchange lives in one local variable. No table holds a reader list, an outcome, an account or a person: the remember cache lives in the running program's memory (RAM) only. Webhook signatures and tokens are compared in constant time (`subtle`, `hmac`, `sha2`), bodies are bounded to 1 MiB and parsed as data, and deliveries are de-duplicated. Every document title and body read from Confluence or SharePoint is content as data, never instructions. Redirects are refused, except the one SharePoint download redirect to a `*.sharepoint.com` host. CQL text is quoted.
- **Observability:** metric families (closed list, Telemetry amendment, recorded through `Telemetry::record`): `wrightrun_knowledge_access_checks_total{system, outcome}`, `wrightrun_knowledge_access_check_seconds{system}` (histogram, buckets 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1 seconds), `wrightrun_knowledge_access_remembered_total{system}`, `wrightrun_knowledge_accounts_linked_total{system, outcome}`, `wrightrun_knowledge_access_notices_total{system, outcome}` (families 59–63; family 64 is the Knowledge Query Server's). Log events: `src_started`, `src_link`, `src_checks`, `src_check_forbidden`, `src_delegation_refused`, `src_notice`, `src_delta_resync`, `src_read_sign_in_needed`, `src_forget_all`. **Never logged:** tokens, the delegation assertion, account ids and labels, the person's subject, the unit of any refused or unchecked check, document titles and bodies, repository names in notice logs.

## Assumptions

- **SA-SRC-01** The server program constructs Source Access in every role of an agent profile whose configuration has a `knowledge-agent` section (OrdinServer.md SA-SRV-PD-01). With no per-person source — no repository with `audience-may-read: false`, no Confluence section and no SharePoint section — it makes no outbound call, and `checkable_systems` is empty.
- **SA-SRC-02** Linking learns the account from the system itself, never from a name the desktop states, so a person cannot link an account they cannot sign in to (ADR-115).
- **SA-SRC-03** Git access is per repository: the unit is `<host>/<owner path>`, the repository key with `@<branch>` and `:<path>` removed, because GitHub and GitLab grant read access to the whole repository. Two configured entries for one repository (different branches or paths) share one unit.
- **SA-SRC-04** Per-person checks on Git use one host per host kind on one agent (every `host-kind: github` repository with `audience-may-read: false` on one host, and the same for GitLab), because a linked account belongs to one host. The Agent Configuration refuses a second host of the same kind for per-person repositories (`CFG_KNOWLEDGE_HOST_MIXED`).
- **SA-SRC-05** `Unchecked` is never remembered and is always withheld by the caller (ADR-115 *Doubt withholds*).
- **SA-SRC-06** An unlinked person gets `Unchecked` (reason `not_linked`), not `Refused`, so the desktop can tell the person to sign in without implying the source refused them.
- **SA-SRC-07** 100,000 remembered answers per instance cover 2,000 people × 50 units each within one minute; beyond that, the oldest entry is evicted on each insert, which costs extra checks, never a wrong answer.
- **SA-SRC-08** A GitHub repository with `visibility = "public"` is readable by anyone, so no link and no permission request are needed.
- **SA-SRC-09** The GitHub login for an account id is resolved with `GET /user/{id}` and kept only beside the in-memory cache entries, because the permission endpoint takes a login and logins can change.
- **SA-SRC-10** Where GitLab's answer cannot show a person's access (internal project with the external flag hidden; invited group not visible to the agent), the person is refused — under-showing, never over-showing.
- **SA-SRC-11** Confluence per-person checks need the agent's account to hold the Confluence Administrator permission (Atlassian content permissions API: checking another user's access requires it). Off by default (`person-checks: false`); then only unrestricted pages in spaces the operator states the whole audience may read are read.
- **SA-SRC-12** Every SharePoint file is treated as restricted (`restricted = true`) and checked per person, because a file's sharing can be narrowed below its library's with no change the agent sees cheaply; a library therefore has no `audience-may-read` key.
- **SA-SRC-13** The delegation assertion is a Microsoft token whose audience is the agent's app (`api://<client-id>/knowledge.check`), exchanged on behalf of the person for Graph `Files.Read.All` delegated, which needs no administrator consent unless the company has turned user consent off (C-104).
- **SA-SRC-14** GitHub and GitLab sharing-change notices arrive as webhooks the repository or group owners add themselves; with no notice, the remember window (≤ `remember-seconds`) bounds how long a removed reader can still be shown knowledge.
- **SA-SRC-15** Confluence documents are read through the agent's own Atlassian account with an API token over HTTP Basic, which needs no OAuth app and no administrator.
- **SA-SRC-16** A restriction added to a Confluence page without any change to the page is detected by the per-cycle restriction sweep, within one fetch interval (default 15 minutes; R-131).
- **SA-SRC-17** SharePoint is read through the agent's own Microsoft account (delegated `Files.Read.All`, user-consentable); the refresh token returned on each use is kept in memory, and the operator signs the agent in again when the token in `read-token-env` has expired.
- **SA-SRC-18** SharePoint is never linked: `link_account` accepts GitHub, GitLab and Confluence only. The person is identified to Microsoft on each question by the `Delegation`, which the on-behalf-of exchange turns into a Graph token for that person.

## Confirmation items

- **C-101** The permission a GitHub App installation token (or fine-grained token) needs for `GET /repos/{owner}/{repo}/collaborators/{username}/permission`, and whether the endpoint reports enterprise members' base read access on `internal` repositories. *If not confirmed:* a classic token with `repo` scope from the agent's own machine account.
- **C-102** Whether `GET /api/v4/users/:id` shows the `external` flag to a non-administrator token. *If not:* internal projects are allowed only to their members (SA-SRC-10, already the safe behaviour).
- **C-103** That the agent's account can read page restrictions and ancestors without administrator rights, and that `permission/check` for another user requires Confluence Administrator. *If restrictions are not readable:* every Confluence page is treated as restricted and is read only with `person-checks: true`.
- **C-104** The on-behalf-of exchange with a user-registered, user-consented Entra app; delegated `Files.Read.All` needing no administrator consent; Graph answering 403 or 404 per item for a person who cannot open it. *If not confirmed:* the company-wide application permission when the company grants it, otherwise SharePoint is not offered to knowledge agents.
- **C-105** Measured latency against real systems: 20 Confluence `permission/check` requests at 16 in flight, and one Graph `$batch` of 20, each ≤ 300 ms p95.
- **C-107** Whether a refresh token obtained once by `ordin-server knowledge sign-in sharepoint` stays usable after Microsoft has rotated it on use, and for how long without use (risk R-132). *If not:* the operator signs the agent in again after each restart that finds the token refused; reading stops until then, with the operator status line "Sign the knowledge agent in to SharePoint again."
- **C-106** The GitHub repository and organisation webhook events and the GitLab group webhook member events named in §H.5, their payload fields, and which GitLab tiers send group member events.

## Gaps

None. Vendor details that are not yet confirmed are confirmation items C-101 to C-106, each with a stated fallback; none blocks implementation of this crate.
