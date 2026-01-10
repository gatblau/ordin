# SPEC: Knowledge Query Client

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · added January 2026 (ADR-113) · amended January 2026 (the `knowledge_agents.*` commands take the Service API catalogue's shapes; `add_connection` also returns `about`, `remove_connection` and `details` take the connection id alone) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-knowledge-query-client` · **Build phase:** 2 · **Programs:** D P A
> **Dependencies:** Identity and Credentials, Journal

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
- [Gaps](#gaps)

## Purpose

The Knowledge Query Client is the asking side of knowledge agents (ADR-113). On the desktop and the personal server it keeps each workspace's **front doors** — the knowledge agents a person connected, or the organisation profile named — and sends a recall question to all of them in parallel, within a deadline, merging what arrives into one ranked list for the Memory Service. In the agent profile the same code is the front door's way of asking its **proxies**: in parallel, each with a deadline, with a backup request to a second proxy covering the same repositories, a hop budget and a list of agents already visited. It sends the question and its filters only, never conversation text, workspace facts or file contents. It keeps one open connection per agent, caches each agent's table of contents so the Memory Service can tell when a question names something only a front door knows, and keeps a front door's change stream open so the Knowledge Query Server can drop cached answers within seconds of a repository moving. It answers no query itself: only the Knowledge Query Server, in the agent profile, answers.

## Approach

One crate with no knowledge of memory, built on `reqwest` 0.13.5 (HTTP/2, rustls) and `sse-stream` 0.3.0, both already in HLD 05. The asking path is a fan-out of independent futures under one `tokio::time::timeout_at` per deadline, joined through an `mpsc` channel so that the first deadline returns whatever has arrived and later batches stream on until the second deadline; merging is a pure function (`merge_hits`) so the front door and the desktop order results identically. Connections are stored in the program's datastore through the Journal's `Datastore` and migration registry, as every table-owning module does. The Memory Service calls this crate directly (edge Memory Service → Knowledge Query Client); the Knowledge Query Server calls it to ask proxies and implements this crate's `ChangeListener`. Rejected: putting fan-out inside the Memory Service — the front door, which answers through the Knowledge Query Server, needs the same fan-out and would then depend on a second copy. Rejected: one request per agent waiting for its complete answer — a slow agent would hold every answer past the first deadline.

## Shared Context

### Shared types (Phase 2C, `ordin-types`; copied)

```rust
pub struct WorkspaceId(uuid::Uuid);            // UUIDv7
pub struct Timestamp(chrono::DateTime<Utc>);   // RFC 3339 ms "Z" on the wire
/// ADR-113: one front door connected to one workspace. UUIDv7.
pub struct KnowledgeConnectionId(uuid::Uuid);
/// ADR-113: one served answer, returned only when the answering agent keeps a sources record. UUIDv7.
pub struct KnowledgeAnswerId(uuid::Uuid);
```

### Wire types (Phase 2C wire, module `ordin_types::wire::knowledge_query`; copied — this crate, the Knowledge Query Server, the Knowledge Reader and the Memory Service use them)

`PageTier` (trust 1.0 / 0.9 / 0.8) and `PageStatus` are defined once in `ordin-types` (Phase 2C), so that this phase-2 crate can name them; the Memory Service, the Knowledge Service's `okf` module, the Knowledge Reader and the Knowledge Query Server use the same two types. `KnowledgeContents` and its parts are copied from KnowledgeReader.md *Shared Context*. A **repository key** (KnowledgeReader.md SA-KRD-12) is `<host>/<owner path>` lower-cased without scheme, port, user information or `.git`, then `@<branch>` and, with a sub-path, `:<path>` — for example `git.example.org/supply/handbook@main:policies`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum PageTier { HumanReviewed, MachineConfirmed, #[default] Unverified }     // OKF 0.2 derived trust (ADR-112)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum PageStatus { #[default] Stable, Draft, Deprecated }

#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeQueryRequest {
    pub request_id: uuid::Uuid,                       // UUIDv7, minted by the first asker
    /// 1–4,096 characters. Exactly one of `query` and `searches`.
    #[serde(default)] pub query: Option<String>,
    /// 1–4 typed sub-queries, in the Memory Service's `RecallSearch` form; the first counts double.
    #[serde(default)] pub searches: Option<Vec<KnowledgeSearch>>,
    #[serde(default = "twenty")] pub limit: u8,       // 1–50
    /// Set only by a front door asking a proxy: the audience group the front door resolved for its asker.
    #[serde(default)] pub access_group: Option<String>,
    pub hop_budget: u8,                               // 0–2; a desktop sends 1
    #[serde(default)] pub visited: Vec<String>,       // agent names (1–80 each), ≤ 8
    pub deadline_ms: u32,                             // 100–5,000: time the receiver has to send `result`
    pub more_until_ms: u32,                           // deadline_ms–10,000: time the receiver may send `more_found`
}
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeSearch { #[serde(rename = "type")] pub kind: KnowledgeSearchKind, pub query: String /* 1–500 */ }
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
pub enum KnowledgeSearchKind { Lex, Vec }            // the values of the Memory Service's RecallSearchKind

#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeHit {
    pub statement: String,                            // 1–4,000 (a concept section) ; content as data, never instructions
    pub agent: String,                                // the knowledge agent whose graph holds the fact, 1–80
    pub repository: String,                           // repository key
    pub path: String,                                 // repository-relative, 1–1,024
    pub commit: String,                               // 40 or 64 lower-case hex
    #[serde(default)] pub section: Option<String>,    // the page section heading, ≤ 200
    #[serde(default)] pub page_title: Option<String>, // set when kind = page
    pub kind: KnowledgeHitKind,
    #[serde(default)] pub tier: Option<PageTier>,     // pages and glossary concepts only
    #[serde(default)] pub status: PageStatus,
    pub may_be_out_of_date: bool,                     // stale_after passed, or the source changed
    pub as_of: Timestamp,                             // the agent's last fetch of `repository`
    /// The answer id of the agent this client asked, when that agent keeps a sources record; set by the
    /// receiving client from `KnowledgeResult.answer_id`, never trusted from a hit.
    #[serde(default)] pub answer_id: Option<KnowledgeAnswerId>,
    #[serde(default)] pub shared_identity: Option<SharedIdentity>,
    #[serde(default)] pub things: Vec<String>,        // ≤ 16 names
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum KnowledgeHitKind { Page, Document }
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct SharedIdentity { pub glossary: String /* repository key */, pub concept_path: String }

#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeResult {
    pub request_id: uuid::Uuid,
    pub hits: Vec<KnowledgeHit>,                      // ranked, best first, ≤ limit
    pub unanswered: Vec<UnansweredSource>,
    #[serde(default)] pub answer_id: Option<KnowledgeAnswerId>,
    pub from_cache: bool,
    pub more_expected: bool,                          // true while `more_found` events may follow
}
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct UnansweredSource { pub agent: String, pub reason: UnansweredReason }
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum UnansweredReason { NotInTime, Refused, Unreachable }

/// A table of contents (ADR-113): no statement ever. Copied from KnowledgeReader.md.
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeContents {
    pub agent_name: String,
    pub generated_at: Timestamp,
    pub repositories: Vec<ContentsRepository>,        // ≤ 220 (repositories, glossaries and hot copies)
    pub things: Vec<ContentsThing>,                   // ≤ 500, most-connected first
}
pub struct ContentsRepository {
    pub repository: String,                           // repository key
    pub role: RepositoryRole,                         // reader | glossary | hot
    pub last_commit: Option<String>,
    pub last_fetched_at: Option<Timestamp>,
    pub readable: bool,
    pub fact_count: u32,
}
#[serde(rename_all = "snake_case")] pub enum RepositoryRole { Reader, Glossary, Hot }
pub struct ContentsThing { pub name: String, pub aliases: Vec<String> /* ≤ 8 */, pub link_count: u32,
                           pub shared_identity: Option<SharedIdentity> }

/// The description of a knowledge agent (route `GET /v1/knowledge/about`; no token — SA-KQS-08).
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeAbout {
    pub agent: String,
    pub issuer: String,                               // the identity provider whose tokens it accepts
    pub token_audience: String,                       // the `aud` it requires
    pub sources_record: Option<SourcesRecordPolicy>,  // None when no record is kept
    pub repository_count: u32,
    pub as_of: Option<Timestamp>,                     // its oldest last fetch
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct SourcesRecordPolicy { pub enabled: bool, pub retention_days: u16 }   // 1–3,650

/// One change notice on `GET /v1/knowledge/changes` (Server-Sent Event `repository_moved`).
pub struct RepositoryMoved { pub repository: String /* repository key */, pub commit: String }
```

### From Identity and Credentials (`ordin-identity`, copied from IdentityAndCredentials.md)

```rust
impl Identity {
    /// The person's company (OIDC) access token for one audience. `issuer` and `client_id` must equal those
    /// of the active `Oidc` credential (else CRED_SIGN_IN_REQUIRED). Refreshed when it expires within 5 minutes.
    pub async fn access_token_for_audience(&self, issuer: &str, client_id: &str, audience: &str) -> Result<AccessToken, CredError>;
    /// Drops the cached token of that audience, so the next call refreshes it.
    pub async fn invalidate_audience_token(&self, issuer: &str, client_id: &str, audience: &str);
}
pub struct AccessToken { pub token: SecretString, pub expires_at: Option<Timestamp>, pub credential_id: CredentialId }
pub enum CredError { Retryable { code: &'static str }, Permanent { code: &'static str } }  // CodedError: code(), retryable()
```

### From the Journal (`ordin-journal`, copied from JournalAndArtefactStore.md)

```rust
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }   // engine(), sqlite(), postgres()
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
impl Journal {
    pub fn datastore(&self) -> &Datastore;
    pub async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>;
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
}
pub struct NewJournalEntry { pub kind: String, pub component: String, pub run_id: Option<RunId>, pub crew_run_id: Option<CrewRunId>,
                             pub workspace_id: Option<WorkspaceId>, pub summary: String /* 1–300 */, /* data ≤ 65,536 bytes */ }
```

### From the Organisation Profile (copied; passed by the program, no crate edge)

The profile's `identity_provider` section, `IdentityProviderSection { issuer: String, client_id: String, scopes: Vec<String> }`, gives the person's sign-in (`CompanySignIn` below). The profile key `knowledge_agents.front_doors[]` (OrganisationProfile.md amendment January 2026, ADR-113), copied:

```rust
pub struct CompanyFrontDoor {
    pub key: String,            // stable key
    pub display_name: String,   // shown as the connection's name, with "Set by your company"
    pub url: String,            // https
    pub applies_to: AppliesTo,  // TeamKnowledge (default) | Code — as team_knowledge.read_folders[] uses it
}
```

The program decides which workspaces each entry applies to (by `applies_to`) and passes the per-workspace list to `apply_company_front_doors`.

### From the agent configuration (agent profile; copied from AgentConfiguration.md amendment January 2026, ADR-113; YAML key `knowledge-agent`)

```rust
pub struct KnowledgeProxy {
    pub name: String,        // SLUG; unique
    pub url: String,         // https; the proxy's base address
    pub audience: String,    // 1..=256; the token audience the proxy's `auth` expects
}
pub struct ServiceIdentity {
    pub token_url: String,   // https; the identity provider's token endpoint
    pub client_id: String,   // 1..=256
    pub client_secret_env: String,
    pub scope: Option<String>,   // 1..=512
}
```

### Shared enums (2C domain; copied)

`SetBy { Person, Company }` (serialised `person`, `company`); `AppliesTo { TeamKnowledge, Code }` (OrganisationProfile.md, copied by the program's caller, not read here).

### Constants (2D-4)

| Constant | Value | Source |
|---|---|---|
| `FIRST_DEADLINE_MS` | 1,000 | ADR-113 (decision 92) |
| `MORE_FOUND_UNTIL_MS` | 5,000 | ADR-113 |
| `HEDGE_DELAY_MS` | 300 | ADR-113 |
| `DEFAULT_HOP_BUDGET` | 1 | ADR-113 |
| `MAX_HOP_BUDGET` | 2 | *chosen here* (SA-KQC-03) |
| `MAX_VISITED` | 8 | ADR-113 |
| `NETWORK_ALLOWANCE_MS` | 50 | *chosen here*: subtracted from a deadline before it is sent on |
| `MAX_CONNECTIONS_PER_WORKSPACE` | 8 | ADR-113 |
| `RRF_K` | 60 | 2D-4 *Stage-1 rank fusion* (the memory component's fusion constant) |
| `IDLE_TIMEOUT_SECONDS` | 90 | ADR-113 (open connections) |
| `CONTENTS_REFRESH_MINUTES` | 60 | *chosen here* (SA-KQC-05) |
| `ABOUT_TIMEOUT_SECONDS` | 5 | *chosen here* |
| `CHANGE_STREAM_BACKOFF_SECONDS` | 1, 2, 4, 8, 16, 32, 60 (then 60) | *chosen here* |
| `MAX_HITS_PER_SOURCE` | 50 | ADR-113 (limit ≤ 50) |

## Public Interface

```rust
// crate ordin-knowledge-query-client
pub fn migrations() -> ordin_journal::ModuleMigrations;            // module "knowledge_query_client"

pub struct KnowledgeClientDeps {
    pub journal: Arc<ordin_journal::Journal>,
    /// Some on D and P (the person's sign-in); None on A.
    pub identity: Option<Arc<ordin_identity::Identity>>,
    /// The program's shared HTTP client (reqwest 0.13.5, rustls); this crate derives per-agent clients from its settings.
    pub http: reqwest::Client,
    pub mode: KnowledgeClientMode,
}
pub enum KnowledgeClientMode {
    /// Desktop and personal server: the person's company sign-in.
    Person { sign_in: CompanySignIn },
    /// Agent profile (a front door asking its proxies): the agent's own service identity.
    /// `service_token` is None only when `proxies` is empty (the configuration then needs no `service-identity`,
    /// AgentConfiguration.md SA-ACFG-KA-02); `new` refuses proxies with no service token (KQ_CONFIG_INVALID).
    Agent { proxies: Vec<KnowledgeProxy>, service_token: Option<ServiceTokenSettings> },
}
pub struct CompanySignIn { pub issuer: String, pub client_id: String }
/// `ServiceIdentity` with its secret resolved by the program from `client_secret_env` (2D-1d).
pub struct ServiceTokenSettings { pub identity: ServiceIdentity, pub client_secret: secrecy::SecretString }

pub struct KnowledgeQueryClient { /* deps, per-agent clients, contents cache */ }

impl KnowledgeQueryClient {
    /// Builds the client; opens no connection. `identity` must be Some in `Person` mode (else KQ_CONFIG_INVALID).
    pub fn new(deps: KnowledgeClientDeps) -> Result<Self, KqError>;
    /// Closes every open connection and change stream (graceful shutdown).
    pub async fn close(&self);

    // ---- desktop and personal server: a workspace's front doors ----
    pub async fn list_connections(&self, ws: WorkspaceId) -> Result<Vec<KnowledgeConnectionView>, KqError>;
    pub async fn add_connection(&self, ws: WorkspaceId, url: &str) -> Result<(KnowledgeConnectionView, KnowledgeAbout), KqError>;
    pub async fn remove_connection(&self, id: KnowledgeConnectionId) -> Result<(), KqError>;
    pub async fn details(&self, id: KnowledgeConnectionId) -> Result<KnowledgeConnectionDetails, KqError>;
    /// Reconciles company-named front doors. `per_workspace` holds, for every workspace the program knows,
    /// the entries that apply to it (possibly none).
    pub async fn apply_company_front_doors(&self, per_workspace: Vec<(WorkspaceId, Vec<CompanyFrontDoor>)>) -> Result<(), KqError>;
    /// The live connections of a workspace (not removed by the person); empty on A. Never fails: a store error gives empty.
    pub async fn connections_of(&self, ws: WorkspaceId) -> Vec<KnowledgeConnection>;
    /// Entity names and aliases of the workspace's front doors' cached tables of contents; no network call.
    pub async fn contents_names(&self, ws: WorkspaceId) -> Vec<String>;

    // ---- agent profile: the front door's proxies ----
    /// Opens one change stream per configured proxy and keeps each open; reports moves and refreshed contents.
    pub fn start_change_streams(&self, listener: Arc<dyn ChangeListener>, stop: CancellationToken);
    pub fn proxy_contents(&self) -> Vec<KnowledgeContents>;

    // ---- both ----
    pub async fn ask(&self, ask: KnowledgeAsk) -> Result<KnowledgeAnswerStream, KqError>;
}

pub enum AskTarget {
    /// Every live connection of the workspace (D, P).
    Workspace(WorkspaceId),
    /// The front door's chosen proxies, each with an optional backup covering the same repositories (A).
    Proxies(Vec<ProxyAsk>),
}
pub struct ProxyAsk { pub primary: String /* proxy name */, pub backup: Option<String> }

pub struct KnowledgeAsk {
    pub target: AskTarget,
    pub query: Option<String>,
    pub searches: Option<Vec<KnowledgeSearch>>,
    pub limit: u8,                       // 1–50
    pub access_group: Option<String>,    // A only
    pub hop_budget: u8,                  // budget the receivers get
    pub visited: Vec<String>,            // agents already visited (on A, this agent included)
    pub first_deadline: tokio::time::Instant,
    pub more_until: tokio::time::Instant,
    pub request_id: uuid::Uuid,
}

/// The answer of one ask: the first batch by the first deadline, later batches as More found.
pub struct KnowledgeAnswerStream { /* receiver */ }
impl KnowledgeAnswerStream {
    /// Resolves at the first deadline at the latest.
    pub async fn first(&mut self) -> KnowledgeBatch;
    /// The next batch, re-merged with everything already returned; None after the final batch.
    pub async fn more(&mut self) -> Option<KnowledgeBatch>;
}
pub struct KnowledgeBatch {
    pub hits: Vec<KnowledgeHit>,               // merged, de-duplicated, grouped, ≤ limit
    pub answered: Vec<String>,                 // agents that sent a result
    pub unanswered: Vec<UnansweredSource>,
    /// (agent, repository key, commit) of every repository the merged hits came from — the cache key's commits.
    pub relied_on: Vec<(String, String, String)>,
    pub final_batch: bool,
}

/// Implemented by the Knowledge Query Server.
pub trait ChangeListener: Send + Sync {
    fn repository_moved(&self, proxy: &str, moved: RepositoryMoved);
    fn contents_refreshed(&self, proxy: &str, contents: KnowledgeContents);
    fn proxy_unreachable(&self, proxy: &str);
}

/// Pure: reciprocal rank fusion and the ordering rule (Internal Logic §M).
pub fn merge_hits(lists: Vec<(String /* source agent */, Vec<KnowledgeHit>)>, limit: u8) -> Vec<KnowledgeHit>;

/// The Service API view (2C wire).
#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeConnectionView {
    pub connection_id: KnowledgeConnectionId,
    pub name: String,
    pub url: String,
    pub added_by: SetBy,
    pub sources_record: Option<SourcesRecordPolicy>,   // None when the agent keeps no record
    pub last_answered_at: Option<Timestamp>,
}
/// What the Memory Service reads (no network).
pub struct KnowledgeConnection { pub id: KnowledgeConnectionId, pub name: String, pub url: String, pub added_by: SetBy, pub state: ConnectionState }
#[derive(Debug, Clone, Copy, Serialize)] #[serde(rename_all = "snake_case")]
pub enum ConnectionState { Ready, SignInNeeded, NotInAudience, Unreachable }
#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeConnectionDetails {
    pub connection: KnowledgeConnectionView,
    pub about: Option<KnowledgeAbout>,     // amended January 2026: read fresh by `details`; None when the read failed
    pub state: ConnectionState,
    pub repositories: Vec<ContentsRepository>,
    pub as_of: Option<Timestamp>,
    pub last_problem: Option<String>,      // one plain sentence
}

#[derive(Debug, thiserror::Error)]
pub enum KqError { /* one variant per code of the Error Table */ }
impl ordin_types::CodedError for KqError { fn code(&self) -> &'static str; fn retryable(&self) -> bool; }
```

**Service API commands** (owner: this crate; the Service API catalogue restates them):

| Command | Input | Result | Method | Kind |
|---|---|---|---|---|
| `knowledge_agents.list` | `{ workspace_id }` | `{ connections: Vec<KnowledgeConnectionView> }` | `list_connections` | Read |
| `knowledge_agents.add` | `{ workspace_id, url }` | `{ connection_id: KnowledgeConnectionId, about: KnowledgeAbout }` | `add_connection` | Change |
| `knowledge_agents.remove` | `{ connection_id: KnowledgeConnectionId }` | `{}` | `remove_connection` | Change |
| `knowledge_agents.details` | `{ connection_id: KnowledgeConnectionId }` | `{ connection: KnowledgeConnectionView, about: KnowledgeAbout }` | `details` | Read |

*Amended January 2026:* the rows are the Service API catalogue's, unchanged. `add` returns the new view's `connection_id` and the `about` read at step B.4. `details` with `about = None` fails the command with `KQ_UNREACHABLE`; the stored view stays as it was.

**Service API event** (owner: this crate): `knowledge_agents_changed { workspace_id }` after every add, remove and company reconciliation that changed a row. The screens re-read `knowledge_agents.list`.

**Called by.**

| Public item | Called by |
|---|---|
| `new`, `close`, `migrations`, `apply_company_front_doors`, `start_change_streams` | the desktop program and the server program (ordin desktop, ordin server) |
| `list_connections`, `add_connection`, `remove_connection`, `details` | the programs' Service API implementation (`knowledge_agents.*`) |
| `connections_of`, `contents_names`, `ask` (`AskTarget::Workspace`) | Memory Service, desktop recall (MemoryService.md amendment January 2026) |
| `ask` (`AskTarget::Proxies`), `proxy_contents`, `merge_hits`, `ChangeListener` | Knowledge Query Server |

### Example

`knowledge_agents.add` with `{"workspace_id":"0192b7a0-5c1d-7e2f-8a3b-4c5d6e7f8091","url":"https://knowledge.procurement.example.org"}` →

```json
{"connection_id":"0192c1d2-0a1b-7c3d-9e4f-5a6b7c8d9e0f",
 "about":{"agent":"procurement-knowledge","issuer":"https://id.example.org","token_audience":"procurement-knowledge",
          "sources_record":{"enabled":true,"retention_days":365},"repository_count":3,"as_of":null}}
```

A request sent by `ask` to that front door: `POST /v1/knowledge/query`, `Accept: text/event-stream`, `Authorization: Bearer …`,

```json
{"request_id":"0192c1d3-77aa-7bcd-8e01-23456789abcd","query":"who supplies packaging for the Leeds site",
 "limit":20,"hop_budget":1,"visited":[],"deadline_ms":950,"more_until_ms":4950}
```

## Internal Logic

**A. `new(deps)`.** `mode = Person` with `identity = None` → `KQ_CONFIG_INVALID`. From `deps.http`'s settings build one client per agent on first use: HTTP/2 preferred (`http2_prior_knowledge` not set; ALPN chooses), rustls with the platform verifier, redirects refused (`redirect::Policy::none()`), `pool_idle_timeout(90 s)`, `pool_max_idle_per_host(4)`, `tcp_keepalive(30 s)`, per-request timeouts set per call. One client per agent URL is created on first use and kept for as long as the program runs (open connections). No network call; nothing logged. `close()` cancels every change stream and drops the per-agent clients, which closes their connections; asks in flight end with their sources listed `Unreachable`.

**B. `add_connection(ws, url)`.**
1. `url` must parse, scheme `https`, no user information, no fragment, ≤ 2,048 characters → else `KQ_URL_INVALID`. Normalise: lower-case host, no trailing `/`.
2. The workspace must have < 8 connections (`removed_by_person = false`) → else `KQ_TOO_MANY_CONNECTIONS`. The URL must not already be connected to the workspace → else `KQ_ALREADY_CONNECTED`.
3. `mode` must be `Person` → else `KQ_NOT_SUPPORTED` (the agent profile has no workspaces).
4. `GET {url}/v1/knowledge/about`, no token, 5 s timeout. Network error or timeout → `KQ_UNREACHABLE`; status ≠ 200 or a body that is not `KnowledgeAbout` → `KQ_NOT_A_KNOWLEDGE_AGENT`.
5. `about.issuer` must equal `sign_in.issuer` (exact string after trimming one trailing `/`) → else `KQ_OTHER_COMPANY`.
6. Token: `identity.access_token_for_audience(sign_in.issuer, sign_in.client_id, about.token_audience)`. `CRED_SIGN_IN_REQUIRED` or another permanent sign-in code → `KQ_SIGN_IN_NEEDED`; a retryable code → `KQ_UNREACHABLE`.
7. `GET {url}/v1/knowledge/contents` with the token, 5 s. 401 → invalidate the audience token, fetch once more and retry once; still 401 → `KQ_SIGN_IN_NEEDED`. 403 with `KQ_NOT_IN_AUDIENCE` → `KQ_NOT_IN_AUDIENCE` (the person is told in one sentence that the front door serves another group). Any other failure → `KQ_UNREACHABLE`.
8. In one transaction insert `kq_connections` (new UUIDv7 id, `added_by = 'person'`, `name = about.agent`, `token_audience`, `issuer`, `sources_record_enabled`, `sources_record_days`, `state = 'ready'`) and `kq_connection_contents` (the contents JSON, ≤ 2 MiB, else stored with `things` emptied and a WARN log `kq_contents_trimmed`).
9. `append_entry { kind: "knowledge_agent_connected", component: "knowledge_query_client", workspace_id: ws, summary: "A knowledge agent was connected to this workspace.", data: {connection_id, added_by} }` — no query text, no URL path beyond the host. `append_event("knowledge_agents_changed", {workspace_id})`.
10. Return the view (`last_answered_at = null`) and the `about` of step 4 (*amended January 2026*).

**C. `remove_connection(id)`.** Row must exist → else `KQ_CONNECTION_NOT_FOUND`; its `workspace_id` is the workspace of the journal entry and the event (*amended January 2026*). A `person` row is deleted with its contents row. A `company` row is kept with `removed_by_person = true` and leaves every ask (so a later reconciliation does not add it again). Journal `knowledge_agent_disconnected {connection_id, added_by}` and event `knowledge_agents_changed`.

**D. `apply_company_front_doors(per_workspace)`.** For each `(ws, entries)`: each entry with no `company` row for `(ws, entry.key)` → insert as step B.8 with `added_by = 'company'`, `company_entry_key = key`, `name = display_name`, `state` left `Unreachable` until the first contents read succeeds (steps B.4–B.7 run in the background, failures recorded in `last_problem`, never shown as an error to the person); each `company` row whose key is absent from `entries` → deleted. A row with `removed_by_person = true` is left as it is. One `knowledge_agents_changed` per changed workspace. Journal `knowledge_agent_connected` / `knowledge_agent_disconnected` with `added_by = company`.

**E. `details(id)`.** Read the row (none → `KQ_CONNECTION_NOT_FOUND`); refresh `about` (step B.4) and contents (B.6–B.7) within 5 s each, `about` returned when read (*amended January 2026*); on failure return the stored values with `about = None` and `last_problem` set to one plain sentence ("The knowledge agent could not be reached at 09:15."). `as_of` = the oldest `last_fetched_at` of its repositories; `state` is the row's.

**F. Contents cache.** Each connection's contents are refreshed every 60 minutes while the program runs, and at once after `add_connection`. `contents_names(ws)`: every thing name and alias of the cached contents of the workspace's live connections, de-duplicated, at most 4,000; the Memory Service decides which of them a query names (MemoryService.md SA-MKA-04). No network call. `connections_of(ws)` reads `kq_connections` rows with `removed_by_person = false`; a store error is logged `kq_store_failed` (ERROR) and gives an empty list, so recall stays local.

**G. `ask(ask)` — validation.**
1. Exactly one of `query`, `searches`; `query` 1–4,096 characters; `searches` 1–4 entries, each 1–500 characters; `limit` 1–50; `hop_budget` ≤ 2; `visited` ≤ 8 names, each 1–80 → else `KQ_BAD_QUERY`.
2. Resolve `ask.target`: `Workspace(ws)` → the connections with `removed_by_person = false` and `state ∈ {Ready, Unreachable}`; none → `KQ_NO_CONNECTIONS`. `Proxies(list)` → the mode must be `Agent` (else `KQ_NOT_SUPPORTED`) and each name a configured proxy → else `KQ_UNKNOWN_PROXY`; a name in `ask.visited` is skipped and reported nowhere (it has already answered this request).
3. The body is built from `ask`'s fields only: `{request_id, query | searches, limit, access_group, hop_budget, visited, deadline_ms, more_until_ms}` where `deadline_ms = (first_deadline − now − 50 ms)` and `more_until_ms = (more_until − now − 50 ms)`, both clamped to [100, 5,000] and [deadline_ms, 10,000]. Nothing else from the caller's context is accepted by `KnowledgeAsk`, so no conversation text, workspace fact or file content can be sent.

**H. `ask` — fan-out.**
1. For each target spawn one task: get a token (Person: `access_token_for_audience` with the connection's `token_audience`; Agent: a client-credentials grant `POST token_url` with `grant_type=client_credentials`, `client_id`, `client_secret`, `scope` when configured and `audience` = the proxy's `audience`, cached per proxy until 5 minutes before `expires_in`; a refused grant → every proxy `Refused` and log `kq_service_token_refused` (ERROR)), then `POST {url}/v1/knowledge/query` with `Accept: text/event-stream`. On receipt, every hit of an agent's `result` and `more_found` gets `answer_id` = that agent's `KnowledgeResult.answer_id` (any value the hit carried is replaced). Every task sends each parsed SSE event to one `mpsc` channel tagged with its agent.
2. SSE events read: `result` (a `KnowledgeResult`), `more_found` (`{hits, unanswered}`), `done` (`{unanswered}`); any other event name is ignored; an event whose data is not valid for its name ends that task and the agent is counted `Unreachable` (logged `kq_bad_event`, WARN, `agent` only).
3. **Backup request (hedge).** For a `ProxyAsk` with a `backup`, the backup task starts only when the primary has sent no `result` within 300 ms. The first `result` of the pair is kept; the other task of the pair is cancelled and its later events ignored. Neither is reported unanswered when one answered.
4. HTTP 401 → invalidate the token, retry once within the same deadline; then `Refused`. 403 → `Refused` (and on the desktop, the connection's `state` becomes `NotInAudience`). 409 `KQ_LOOP` → the agent is dropped from this request silently. Connection failure → `Unreachable` (desktop `state = Unreachable`). No retry after the first deadline.
5. **First deadline.** `first()` resolves at `first_deadline`, or earlier when every target has ended: every `result` received merged (§M); `answered` = the agents that sent one; every target with no `result` yet and not cancelled by the hedge listed `NotInTime`; refused and unreachable targets with their reasons. When no target is still open, the first batch has `final_batch = true` and `more()` returns `None`. On the desktop, `kq_connections.last_answered_at` is set for every answering connection.
6. **More found.** Each `result` or `more_found` arriving after the first deadline and before `more_until` produces one `KnowledgeBatch` re-merged with everything already returned, `final_batch = false`, returned by the next `more()`. At `more_until`, or when every task ended, one last batch with `final_batch = true` lists the agents that never answered as `NotInTime`; open tasks are cancelled; `more()` then returns `None`. Dropping the stream cancels every open task.
7. Logged per ask (DEBUG): `kq_ask {request_id, targets, answered, not_in_time, refused, unreachable, first_ms}`. Never the query, searches or hits.

**M. `merge_hits(lists, limit)`** (pure, deterministic).
1. Drop any hit failing its field bounds (statement 1–4,000, path 1–1,024, commit hex 40 or 64, ≤ 16 things), logging `kq_hit_dropped` (DEBUG, `agent`, `field`).
2. Score each hit `Σ 1/(60 + rank)` over the lists it appears in, where `rank` is its 1-based position in that list and identity is the key `(repository, path, commit, section, sha256(statement))`; the hit kept for a key is the one from the earliest list in input order.
3. When at least one hit has `status ≠ Deprecated`, drop every `Deprecated` hit.
4. Sort by score descending (compared to six decimal places), then — for otherwise equal hits — `tier` (HumanReviewed > MachineConfirmed > Unverified > none), then `kind` (Page before Document), then `(agent, repository, path, section)` ascending.
5. Group: for hits with the same `shared_identity`, move every later hit of that identity to directly after the first one of it, keeping their relative order.
6. Truncate to `limit`.

**N. Change streams (agent profile).** For each proxy keep `GET {url}/v1/knowledge/changes` (`Accept: text/event-stream`, service token) open. Event `repository_moved` → `listener.repository_moved(proxy, moved)` and an immediate contents refresh of that proxy (`GET /v1/knowledge/contents`) → `listener.contents_refreshed`. A closed or failed stream reconnects after 1, 2, 4, 8, 16, 32, then every 60 s; after the second failure in a row `listener.proxy_unreachable(proxy)`. On reconnect the proxy's contents are refreshed, so moves missed while closed are seen through `last_commit`.

## Data Model

Owned tables (module `knowledge_query_client`; SQLite on D, P and the single shape; PostgreSQL in the scaled shape, where the tables exist but hold no rows — the agent profile has no workspaces).

SQLite, version 1:

```sql
CREATE TABLE kq_connections (
    id                     TEXT PRIMARY KEY,                         -- KnowledgeConnectionId, UUIDv7
    workspace_id           TEXT NOT NULL,
    url                    TEXT NOT NULL CHECK (length(url) BETWEEN 9 AND 2048 AND url LIKE 'https://%'),
    name                   TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    issuer                 TEXT NOT NULL CHECK (length(issuer) BETWEEN 9 AND 2048),
    token_audience         TEXT NOT NULL CHECK (length(token_audience) BETWEEN 1 AND 512),
    added_by               TEXT NOT NULL CHECK (added_by IN ('person', 'company')),
    company_entry_key      TEXT CHECK (company_entry_key IS NULL OR length(company_entry_key) BETWEEN 1 AND 64),
    removed_by_person      INTEGER NOT NULL DEFAULT 0 CHECK (removed_by_person IN (0, 1)),
    state                  TEXT NOT NULL CHECK (state IN ('ready', 'sign_in_needed', 'not_in_audience', 'unreachable')),
    sources_record_enabled INTEGER NOT NULL CHECK (sources_record_enabled IN (0, 1)),
    sources_record_days    INTEGER CHECK (sources_record_days IS NULL OR sources_record_days BETWEEN 1 AND 3650),
    last_problem           TEXT CHECK (last_problem IS NULL OR length(last_problem) <= 300),
    added_at               TEXT NOT NULL,
    last_answered_at       TEXT,
    CHECK ((added_by = 'company') = (company_entry_key IS NOT NULL)),
    CHECK (added_by = 'company' OR removed_by_person = 0)
);
CREATE UNIQUE INDEX kq_connections_ws_url ON kq_connections (workspace_id, url);
CREATE UNIQUE INDEX kq_connections_ws_company ON kq_connections (workspace_id, company_entry_key) WHERE company_entry_key IS NOT NULL;

CREATE TABLE kq_connection_contents (
    connection_id TEXT PRIMARY KEY REFERENCES kq_connections(id) ON DELETE CASCADE,
    contents_json TEXT NOT NULL CHECK (length(contents_json) <= 2097152),
    fetched_at    TEXT NOT NULL
);
```

PostgreSQL, version 1: the same tables with `UUID` for the two ids, `BOOLEAN` for the 0/1 columns, `TIMESTAMPTZ` for times, `JSONB` for `contents_json` (`CHECK (octet_length(contents_json::text) <= 2097152)`), the same checks and indexes.

Additive only; no destructive change. Rollback: the pre-migration backup copy (SA-XC-07); a build before this module ignores the two tables.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Address not https, malformed, with user information or a fragment, or longer than 2,048 | 400 (Service API `kind: error`) | `KQ_URL_INVALID` | `{"error": "Use an https address of a knowledge agent.", "code": "KQ_URL_INVALID"}` |
| Workspace already has 8 front doors | 409 | `KQ_TOO_MANY_CONNECTIONS` | `{"error": "A workspace can connect up to 8 knowledge agents.", "code": "KQ_TOO_MANY_CONNECTIONS"}` |
| Front door already connected to the workspace | 409 | `KQ_ALREADY_CONNECTED` | `{"error": "This knowledge agent is already connected.", "code": "KQ_ALREADY_CONNECTED"}` |
| `about` answers, but not as a knowledge agent | 422 | `KQ_NOT_A_KNOWLEDGE_AGENT` | `{"error": "That address is not a knowledge agent.", "code": "KQ_NOT_A_KNOWLEDGE_AGENT"}` |
| The agent trusts another identity provider | 422 | `KQ_OTHER_COMPANY` | `{"error": "This knowledge agent accepts another company's sign-in.", "code": "KQ_OTHER_COMPANY"}` |
| The person must sign in again, or the token is refused twice | 401 | `KQ_SIGN_IN_NEEDED` | `{"error": "Sign in to your company account again to use this knowledge agent.", "code": "KQ_SIGN_IN_NEEDED"}` |
| The person is not in the agent's audience | 403 | `KQ_NOT_IN_AUDIENCE` | `{"error": "This knowledge agent serves another group of people.", "code": "KQ_NOT_IN_AUDIENCE"}` |
| Agent unreachable, timed out, or the sign-in service failed temporarily | 503 | `KQ_UNREACHABLE` | `{"error": "The knowledge agent could not be reached. Try again in a minute.", "code": "KQ_UNREACHABLE"}` |
| Connection id absent for the workspace | 404 | `KQ_CONNECTION_NOT_FOUND` | `{"error": "No such knowledge agent connection.", "code": "KQ_CONNECTION_NOT_FOUND"}` |
| Connection command on the agent profile; proxies asked in `Person` mode | 400 | `KQ_NOT_SUPPORTED` | `{"error": "Knowledge agent connections belong to workspaces.", "code": "KQ_NOT_SUPPORTED"}` |
| `Person` mode built with no Identity | — (start-up) | `KQ_CONFIG_INVALID` | the program stops with the code |
| `ask` input breaks a bound | — (internal) | `KQ_BAD_QUERY` | not shown; the caller's input error |
| `ask` on a workspace with no live connection | — (internal) | `KQ_NO_CONNECTIONS` | not shown; the Memory Service skips the wider search |
| `ask` names a proxy not configured | — (internal) | `KQ_UNKNOWN_PROXY` | not shown; logged ERROR |
| Datastore read or write fails | 503 | `KQ_STORE_UNAVAILABLE` | `{"error": "ordin could not read its knowledge agent list. Try again.", "code": "KQ_STORE_UNAVAILABLE"}` |

Retryable: `KQ_UNREACHABLE`, `KQ_STORE_UNAVAILABLE`. Every other code is not retryable. Per-agent failures inside `ask` are never errors of `ask`: they become `unanswered` entries.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Knowledge Query Client

  Background:
    Given workspace W signed in to the company identity provider "https://id.example.org"

  Scenario: Happy path — a person connects a front door
    Given a knowledge agent at "https://kfd.example.org" whose about names issuer "https://id.example.org" and keeps a sources record for 365 days
    And the person is in its audience
    When the person sends knowledge_agents.add {workspace_id: W, url: "https://kfd.example.org"}
    Then a kq_connections row with added_by "person" exists for W
    And the event knowledge_agents_changed {workspace_id: W} is appended
    And the journal holds "knowledge_agent_connected" with the connection id and no query text
    And the view says the sources record is kept for 365 days

  Scenario: Error — the person is outside the audience
    Given the agent answers GET /v1/knowledge/contents with 403 KQ_NOT_IN_AUDIENCE
    When the person sends knowledge_agents.add for it
    Then the command fails with KQ_NOT_IN_AUDIENCE and no row is written

  Scenario: Edge — a workspace with no front door asks nothing
    Given W has no connection
    When the Memory Service calls connections_of(W)
    Then it returns an empty list and no request leaves the computer

  Scenario: Privacy — only the question and its filters are sent
    Given W has one connection and a capture of outbound requests
    When ask runs with target Workspace(W) and query "who supplies packaging"
    Then the captured body holds exactly the keys request_id, query, limit, hop_budget, visited, deadline_ms and more_until_ms
    And no conversation text, workspace fact or file content appears in it

  Scenario: Deadline — a slow agent is reported and its answer arrives as More found
    Given two front doors A and B, and B waits 3 s before sending result
    When ask runs with the first deadline 1,000 ms and more-found until 5,000 ms
    Then first() resolves by 1,000 ms with A's hits and B listed not_in_time
    And more() returns a batch before 5,000 ms holding B's hits merged with A's
    And the final batch has final_batch true

  Scenario: Backup request — the second proxy answers when the first is slow
    Given proxies P1 and P2 covering the same repositories, and P1 delayed past 300 ms
    When ask runs with target Proxies([{primary: P1, backup: P2}])
    Then P2 is asked after 300 ms, its result is used, P1's task is cancelled
    And first() resolves within 1,000 ms with nobody listed unanswered

  Scenario: Hop loop — a visited agent is never asked
    Given proxy P3 whose name is in visited
    When ask runs with target Proxies([{primary: P3}])
    Then no request is sent to P3

  Scenario Outline: Merge order follows trust, then pages, then rank
    Given hits of equal fused score with tier <a> kind <b> and tier <c> kind <d>
    When merge_hits runs
    Then the first hit is the one with tier <first>
    # tier "none" stands for a document hit with no tier
    Examples:
      | a              | b        | c                 | d        | first             |
      | human_reviewed | page     | machine_confirmed | page     | human_reviewed    |
      | unverified     | page     | none              | document | unverified        |
      | none           | document | machine_confirmed | page     | machine_confirmed |

  Scenario: Merge — the same fact from two proxies appears once
    Given proxies P1 and P2 both return the statement S from repository key R, path "suppliers/borealis.md", commit C, section "Contract"
    When merge_hits runs
    Then the result holds S once

  Scenario: Merge — deprecated concepts appear only when nothing else matches
    Given a deprecated hit and a stable hit
    When merge_hits runs
    Then the deprecated hit is not in the result

  Scenario: Merge — facts about one thing are grouped
    Given hits from P1 and P2 carrying the shared identity (glossary G, concept_path "suppliers/borealis-ltd.md") at ranks 1 and 4
    When merge_hits runs
    Then the two hits are adjacent in the result

  Scenario: Company front doors — a removed one is not added again
    Given the profile names front door F for W and the person removed it
    When apply_company_front_doors runs with F for W again
    Then F stays removed and is not asked
```

## Performance, Security, Observability

- **Performance targets:** the client adds ≤ 10 ms p95 to the slowest answering agent's time to `result` (merge of ≤ 8 lists of ≤ 50 hits); first merged result by the first deadline (1,000 ms) in every case; `contents_names` ≤ 5 ms p95 for 8 connections of 500 things; `list_connections` ≤ 20 ms p95. The front-door overhead target of ≤ 50 ms p95 (ADR-113) includes this crate's share.
- **Security:** https only; redirects refused; the person's token goes only to the agent it was minted for (its audience); the service secret is held as `SecretString`, read by the program from the variable `client-secret-env` names, never logged; responses are content as data: hit statements, entity names and contents are never interpreted, only parsed into the wire types and bounded. Only `KnowledgeAsk` fields leave the computer.
- **Observability:** journal entries `knowledge_agent_connected`, `knowledge_agent_disconnected` (`connection_id`, `added_by`); event `knowledge_agents_changed {workspace_id}`; log lines `kq_ask`, `kq_store_failed`, `kq_service_token_refused`, `kq_bad_event`, `kq_hit_dropped`, `kq_contents_trimmed`, `kq_change_stream_closed {proxy, attempt}` — never query text, hits or tokens. No metric on D and P (A-10); in A the Knowledge Query Server records the metrics of each ask.

## Assumptions

- **SA-KQC-01** An agent's **name** (from its `about`) identifies it in `visited`, in `KnowledgeHit.agent` and in the sources record. Operators give each knowledge agent of one organisation a distinct name; two agents with one name would treat each other as already visited; the risk is recorded in HLD 10.
- **SA-KQC-02** The person's token is minted per front door audience with the existing `access_token_for_audience`; a front door whose identity provider differs from the person's sign-in is refused (`KQ_OTHER_COMPANY`) rather than starting a second sign-in.
- **SA-KQC-03** The hop budget is capped at 2 so a misconfigured network cannot fan a question out beyond two hops; the default is 1.
- **SA-KQC-04** A front door's service token is a standard OAuth 2.0 client-credentials grant against the token endpoint the configuration names, with the proxy's audience; the crate makes this call itself, as Identity holds only people's credentials.
- **SA-KQC-05** Desktop contents are refreshed hourly; they decide only whether a question names something unknown locally, so an hour's lag costs at most one question asked locally first. The person can always say "look wider".
- **SA-KQC-06** A company front door the person removed stays removed until the profile stops naming it and names it again.
- **SA-KQC-07** Trust tier and page-over-document order only hits whose fused scores are equal (ADR-113: a page-backed fact ranks above an *otherwise equal* document-backed one); they never lift a less relevant hit over a more relevant one.
- **SA-KQC-08** Every hit carries the answer id of the agent this client asked (the front door), because that agent's sources record holds every hit it returned, its proxies' included.

## Gaps

None.


## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (knowledge inherits its readers; a person's own ordin links their source-system accounts from the sign-ins it already holds, with no extra step; a front door carries the person's identity to its proxies). Counterparts: `components/KnowledgeQueryServer.md` (amendment January 2026: routes 41 and 42, `delegation`, `asker`, `pending`, `unchecked_sources`), `components/IdentityAndCredentials.md` (amendment January 2026: `access_token_for_scope`), `components/MemoryService.md` (amendment January 2026: `unchecked_sources` in **What Ordin used**), `components/ScreensModule.md` (amendment January 2026: the connection details' sign-in sentence).
> Every rule below adds to the sections above. Against a knowledge agent whose `about` lists no `accounts` and no `sharepoint_check_scope`, the client sends nothing new and every scenario above passes unchanged.

### Purpose (addition)

On the desktop and the personal server, the client now also links the person's GitHub, GitLab and Confluence accounts to each front door, from the sign-ins Identity and Credentials already holds, when the person connects the front door, when the program starts and whenever the person signs in to or out of one of those systems in ordin. The person takes no step for it. For a front door that checks SharePoint, each question carries a short-lived Microsoft token issued for that front door's app, so the front door can ask SharePoint on the person's behalf. In the agent profile, a front door's ask carries the person's identity to its proxies, and the client returns the hits a proxy could not check, for the front door to check. Every answer reports whether some sources could not be checked.

### Approach (addition)

Linking is one `POST /v1/knowledge/accounts` per front door carrying the tokens of the systems that front door lists in its `about.accounts`; the front door asks each system who the token belongs to and keeps nothing but the account id. The token goes only to a front door whose issuer equals the person's company sign-in (§B.5), over https, once per link. Sign-in changes reach the client through Identity's `subscribe()` broadcast, so the link follows the person's sign-ins without polling. The SharePoint delegation is fetched from Identity per ask, per front door (each front door has its own app), and placed only in that front door's request body. Rejected: *linking on first question* — the first answer after connecting would miss every restricted fact; *the desktop stating an account name* — anyone could claim a colleague's account; the system's answer to the token is the only proof (ADR-115); *sending the person's own Microsoft Graph token* — it reads every file the person can read, while a token for the front door's app is limited to what that app's scope allows and is exchanged by the front door for one question (SA-KQC-PD-03).

### Shared Context (additions)

From Phase 2C wire (amended January 2026; copied): `SourceSystem`, `AccessUnit`, `Delegation { sharepoint_assertion }`, `AskerStatement { subject, accounts, issued_at }`, `PendingHit { hit, unit }`, `AccountNeed { system, host }`; `KnowledgeQueryRequest.delegation`, `KnowledgeQueryRequest.asker`; `KnowledgeHit.person_checked`; `KnowledgeResult.unchecked_sources`, `KnowledgeResult.pending`; `KnowledgeAbout.accounts`, `KnowledgeAbout.sharepoint_check_scope` (KnowledgeQueryServer.md amendment January 2026, *Shared Context*).

From Identity and Credentials (copied; `access_token_for_scope` added by IdentityAndCredentials.md amendment January 2026):

```rust
impl Identity {
    pub async fn list(&self) -> Result<Vec<CredentialView>, CredError>;       // active credentials; kind, provider_host, state
    pub async fn access_token_by_id(&self, credential_id: CredentialId) -> Result<AccessToken, CredError>;
    /// A Microsoft access token for one scope of another app (an on-behalf-of assertion for a knowledge agent),
    /// from the stored Microsoft refresh token; never prompts. CRED_SCOPE_MISSING when the person has not consented.
    pub async fn access_token_for_scope(&self, kind: CredentialKind, scope: &str) -> Result<AccessToken, CredError>;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SignInChanged>;
}
pub struct SignInChanged { pub credential_id: CredentialId, pub credential_kind: CredentialKind, pub state: SignInState }
```

Systems and credentials: `github` ↔ `CredentialKind::Github`, `gitlab` ↔ `CredentialKind::Gitlab`, `confluence` ↔ `CredentialKind::Confluence`; SharePoint ↔ `CredentialKind::Microsoft` (delegation only, never linked).

### Public Interface (changes)

```rust
// KnowledgeAsk gains (A only; None on D and P):
//   pub asker: Option<AskerStatement>,
// KnowledgeBatch gains:
//   pub unchecked_sources: bool,                    // OR of every answering agent's flag
//   pub pending: Vec<(String /* proxy */, PendingHit)>,  // A only; never merged into `hits`
// KnowledgeConnectionDetails gains:
//   pub accounts: Vec<ConnectionAccount>,
#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "snake_case")]
pub struct ConnectionAccount { pub system: SourceSystem, pub state: AccountLinkState }
#[derive(Debug, Clone, Copy, Serialize)] #[serde(rename_all = "snake_case")]
pub enum AccountLinkState { Linked, NotSignedIn, Refused }
```

```rust
impl KnowledgeQueryClient {
    /// D, P: the `sharepoint_check_scope` of every live connection's stored `about` (`api://<client-id>/knowledge.check`),
    /// de-duplicated; empty on A. The programs add them to the person's Microsoft sign-in so the delegation (§PD-B) can be issued. No network call.
    pub async fn microsoft_check_scopes(&self) -> Vec<String>;
}
```

Linking itself runs inside the client (§PD-A).

**Called by (additions).** `microsoft_check_scopes` — the desktop and server programs, when building the Microsoft sign-in's scopes (IdentityAndCredentials.md amendment January 2026). `KnowledgeBatch.unchecked_sources` — Memory Service (desktop recall). `KnowledgeAsk.asker`, `KnowledgeBatch.pending` — Knowledge Query Server. `KnowledgeConnectionDetails.accounts` — the programs' Service API (`knowledge_agents.details`), shown by the screens.

### Internal Logic (additions and changes)

**PD-A. Linking accounts (D, P).**
1. **When.** (a) At step B.8 of `add_connection`, after the row is written; (b) for every live connection when the program starts, in the background, after the contents refresh; (c) on each `SignInChanged` from `identity.subscribe()` whose `credential_kind` is `Github`, `Gitlab` or `Confluence`: `SignedIn` → link that system to every live connection whose `about.accounts` lists it; `SignedOut` → `DELETE {url}/v1/knowledge/accounts/{system}` on each of them. A lagged broadcast receiver relinks every system of every live connection.
2. **Which credential.** For each `AccountNeed { system, host }` of the connection's stored `about`: the active `CredentialView` of the matching kind whose `provider_host` equals `host` (Confluence: whose site host equals `host`), not `needs_sign_in`. None → the system's state is `NotSignedIn`, nothing is sent for it.
3. **Send.** GitHub and GitLab tokens through `access_token_by_id`; the Confluence token through `identity.access_token_for(CredentialKind::Confluence, &["read:me"])`, on this linking path only (the question path never asks for it); `CRED_SCOPE_MISSING` or any other `CredError` there → Confluence is left out of the body silently, its state stays as it was, and it is retried at the next trigger; `POST {url}/v1/knowledge/accounts` with the person's company token for the connection's audience (as step H.1), body `{"accounts":[{"system", "token"}]}` holding every system with a credential, 5 s timeout. 401 → invalidate the audience token and retry once. Each `linked` entry → state `Linked`. Each `refused` entry (code `KQ_ACCOUNT_LINK_REFUSED`) by its `reason`: `not_accepted` → state `Refused` and `last_problem` = "GitHub did not accept ordin's sign-in for this knowledge agent." (one sentence per system, naming the system); `try_again` → state unchanged, retried at the next trigger; `not_checked_here` → the system's row is deleted (the front door no longer checks it; its `about` is re-read at the next contents refresh). Network failure → retried at the next trigger; states unchanged.
4. **Store.** Upsert `kq_connection_accounts (connection_id, system, state, changed_at)`; no account id, label or token is stored on the desktop.
5. A token is never logged, never put in the journal, and sent only to the connection's URL. Log `kq_accounts_linked {connection_id, linked: count, refused: count}` (DEBUG).
6. `details(id)` returns `accounts` from `kq_connection_accounts`, one per `about.accounts` entry, defaulting to `NotSignedIn`. The screens show, for each `NotSignedIn` system, the sentence `knowledge-agents-sign-in-to-include` ("Sign in to {system} in ordin to include what you can read there."); it names the system only, never a document. Unlinking (trigger (c), `SignedOut`) is idempotent on the front door: it answers 200 whether or not a link existed, and the row's state becomes `NotSignedIn`.

**PD-B. Delegation (D, P).** Step H.1 (changed): for a target connection whose stored `about.sharepoint_check_scope` is Some, before the request: `identity.access_token_for_scope(CredentialKind::Microsoft, scope)` under the task's deadline; Ok → body gains `"delegation":{"sharepoint_assertion": <token>}`; any error → no delegation is sent, logged `kq_delegation_unavailable {connection_id, code}` (DEBUG) — that front door then withholds restricted SharePoint facts. The token is placed only in that connection's body and dropped when the request is sent.

**PD-C. Asker and pending (A).**
1. Step G.3 (changed): the body built for each proxy also carries `asker` when `ask.asker` is Some. `delegation` is never sent by mode `Agent`.
2. Step H.2 (changed): the `result` and `more_found` events may carry `pending` and `unchecked_sources`; `pending` entries are collected per proxy into the batch's `pending`, never passed to `merge_hits`; `unchecked_sources` of every answering agent is OR-ed into the batch's.
3. The hedge (H.3) keeps the pending entries of the kept task only.

**PD-D. Desktop answers.** `KnowledgeBatch.unchecked_sources` = the OR of every front door's `unchecked_sources` received so far (`result`, `more_found`, `done`); the final batch carries the final value. The Memory Service shows the sentence `memory-used-unchecked` ("Some sources couldn't be checked.") under **What Ordin used** when it is true (MemoryService.md amendment January 2026).

**PD-E. Contents on the desktop.** A desktop or personal server is a member, so the contents it caches (§F) hold only audience-open sources and things (KnowledgeQueryServer.md amendment January 2026, §PD-F); `contents_names` therefore never returns a name known only from a per-person source. In mode `Agent`, `proxy_contents` holds the full rows a proxy gives a front door, with `audience_open`, used only to choose proxies.

**Privacy (step G.3, restated).** The body keys are now `request_id`, `query` or `searches`, `limit`, `hop_budget`, `visited`, `deadline_ms`, `more_until_ms`, and, only where stated, `access_group` and `asker` (A) or `delegation` (D, P). Still no conversation text, workspace fact or file content.

### Data Model (additions)

Module `knowledge_query_client`, version 2. SQLite:

```sql
CREATE TABLE kq_connection_accounts (
    connection_id  TEXT NOT NULL REFERENCES kq_connections(id) ON DELETE CASCADE,
    system         TEXT NOT NULL CHECK (system IN ('github', 'gitlab', 'confluence')),
    state          TEXT NOT NULL CHECK (state IN ('linked', 'not_signed_in', 'refused')),
    changed_at     TEXT NOT NULL,
    PRIMARY KEY (connection_id, system)
);
```

PostgreSQL, version 2: the same with `UUID` for `connection_id` and `TIMESTAMPTZ` for `changed_at` (no rows in the agent profile). Additive only; rollback as version 1. No account id, label or token is stored.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A front door refused a linked system's token with reason `not_accepted` (shown on the connection details, not as a command error) | — | `KQ_ACCOUNT_LINK_REFUSED` (the server's) | the details' `last_problem` sentence naming the system |
| Linking cannot reach the front door | — (background) | — | retried at the next trigger; states unchanged |
| The Microsoft token for a front door's scope is unavailable | — (internal) | `CRED_SCOPE_MISSING`, `CRED_SIGN_IN_REQUIRED` (Identity's) | not shown; the question is sent without delegation |

No new code is owned here.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Knowledge Query Client — per-document read access

  Background:
    Given workspace W connected to front door "https://kfd.example.org"
    And its about lists accounts [{system: github, host: github.com}, {system: confluence, host: acme.atlassian.net}]
      and sharepoint_check_scope "api://7d1c…/knowledge.check"

  Scenario: Linking needs no step from the person
    Given the person is signed in to GitHub (github.com) and Confluence (acme.atlassian.net) in ordin
    When the person connects the front door
    Then one POST /v1/knowledge/accounts is sent with a github and a confluence entry
    And no prompt is shown and no screen is opened
    And details shows github Linked and confluence Linked

  Scenario: A system the person is not signed in to is not linked
    Given the person is signed in to GitHub only
    When the person connects the front door
    Then the accounts body holds only github
    And details shows confluence NotSignedIn with "Sign in to Confluence in ordin to include what you can read there."

  Scenario: Signing in later links at once; signing out unlinks
    When the person signs in to Confluence in ordin
    Then a POST /v1/knowledge/accounts with a confluence entry is sent within 5 s
    When the person signs out of GitHub in ordin
    Then DELETE /v1/knowledge/accounts/github is sent to the front door

  Scenario: Tokens never stay on the desktop's tables or logs
    When linking completes
    Then no table of module knowledge_query_client and no log line holds a token or an account id

  Scenario: A refused link shows on the details; a temporary refusal changes nothing
    Given the front door answers refused [{system: github, code: KQ_ACCOUNT_LINK_REFUSED, reason: not_accepted}]
    When linking completes
    Then details shows github Refused with "GitHub did not accept ordin's sign-in for this knowledge agent."
    Given the front door answers refused [{system: confluence, code: KQ_ACCOUNT_LINK_REFUSED, reason: try_again}]
    When linking completes
    Then confluence's state is unchanged

  Scenario: Delegation travels only to its own front door
    Given a second front door with no sharepoint_check_scope
    When ask runs with target Workspace(W)
    Then the request to "https://kfd.example.org" carries delegation and the request to the second front door does not

  Scenario: Unchecked sources reach the Memory Service
    Given the front door answers result with unchecked_sources true
    When ask runs
    Then the first batch has unchecked_sources true

  Scenario: A front door's asker reaches the proxies; pending hits stay apart
    Given mode Agent and a KnowledgeAsk with asker {subject: "00u1", accounts: {github: "4411"}}
    When ask runs with target Proxies([{primary: P1}])
    Then P1's body carries that asker and no delegation
    And a pending hit P1 returns is in the batch's pending, not in its hits
```

### Performance, Security, Observability (additions)

- **Performance:** linking runs off the question path; one request per front door per sign-in change. The delegation adds at most one Identity call per front door per ask, served from Identity's token cache while valid (≤ 2 ms p95 on a cache hit).
- **Security:** a source-system token leaves the computer only to a front door of the person's own company (issuer check, step B.5), over https, once per link; the front door asks the system who it belongs to and keeps only the account id; nothing about the account is stored on the desktop beyond the link state. The Microsoft delegation is issued for the front door's app scope only and sent only to that front door.
- **Observability:** logs `kq_accounts_linked`, `kq_delegation_unavailable` (counts and codes only).

### Assumptions (added)

- **SA-KQC-PD-01** *(chosen here)* The credential for a system is matched by host — GitHub's `provider_host`, a GitLab host, the Confluence site host — so a person signed in to two GitLab hosts links the one the front door names.
- **SA-KQC-PD-02** *(chosen here)* A link is refreshed at every program start, so a link removed on the front door after 90 days unused comes back at the next start without the person noticing.
- **SA-KQC-PD-03** A Microsoft token for the front door's app scope (`api://<client-id>/knowledge.check`) can be obtained from the person's stored Microsoft refresh token once the person has consented to that app; the consent is the person's own (no administrator), recorded as confirmation item C-104.
- **SA-KQC-PD-04** SharePoint needs no link: the delegation itself identifies the person to SharePoint for each question.

### Gaps

None.

