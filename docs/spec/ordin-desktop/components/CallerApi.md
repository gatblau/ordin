# SPEC: Caller API

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-caller-api` · **Build phase:** 9 · **Programs:** A
> **Dependencies:** Caller Authentication, Run Queue, File Service, Settings Store, Knowledge Service, Retention and Erasure, Rules Engine, Telemetry, Approval Service

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
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)

## Purpose

The Caller API is the agent profile's HTTP server, served by instances in the `requests` role and in the single shape (02:793–795). It serves five groups of routes on one port: the **caller interface**, wrightrun-compatible — start a run on a new or existing thread, resume a paused run, read a run — plus the two routes ADR-098 item 2 adds, one run's **event stream** (Server-Sent Events with `Last-Event-ID`) and **cancel**; one **intake** route per configured intake, each starting one fixed command from a signed or token-bearing event; the **operator** routes (runs, settings by scope, jobs, notes, knowledge entries, erasure, duties); the **work-folder** routes; and **health and metrics**. It executes no run: it records an accepted run in the Run Queue and answers with the thread and run identifiers; a thread takes one run at a time. It owns the routes, the error form and the page form (02:794), all kept from wrightrun so callers written for wrightrun work unchanged (ADR-098 item 2; SA-ENV-02; SA-PAGE-01; SA-TIME-01 exception).

## Approach

One `axum` 0.8 router served by `axum-server` 0.8 (SA-HTTP-01), built by one function from the objects it is given (2B-1 rule 3: the program constructs and passes them). Every route follows the same steps: check the token and least role through Caller Authentication before reading a body, read the body under the route's ceiling, check the request against its schema in this crate's code, call one method of one module, render JSON in wrightrun's forms. Two needs fall outside the 2B-3 dependency list: the job routes need the Job Runner (same build phase, so no edge is allowed) and the intake secrets and manifest facts need the Agent Configuration and Agent Loader; this crate owns a small trait (`JobDesk`) and plain settings structs, and the program fills them (ADR-100, 2B-1 rule 4). Error prefixes owned here: `SRV_`, `OPS_`, `INTAKE_` (wrightrun's, unchanged); codes of called modules pass through with the status this spec gives them. Rejected: one generic route taking an action name — paths would tell an ingress rule or a log reader nothing (wrightrun OperatorRoutes). Rejected: axum's `DefaultBodyLimit` alone — its refusal is not in the error form.

### Reconciliation assumptions

- **SA-RECON-01** The resume route ignores `Principal.approver` (reconciliation row 431): ADR-097 item 4 sends approvals to the approvers' cards and keeps resume for operators where approvers are configured; letting an approver resume here would skip the asker rule the Approval Service applies.
- **SA-CLOSE-01** *(K15, chosen here)* wrightrun's spec (`agent-wright-runtime/docs/spec/`, read January 2026) defines no approver route, so the approver cards of ADR-097 item 9 are served by two routes of this crate: 31 `GET /v1/approvals?state=pending` (the approver's open cards) and 32 `POST /v1/approvals/{approval_id}/decision` with `{ "decision": "approve" | "reject", "note": string? }`. The approver is the token's holder: `Principal.subject` with `Principal.approver = true`; the Approval Service applies the approver and asker rules.
- **SA-CLOSE-02** *(K14)* The resume route reads `PendingItem.approval_id` (RunQueue.md SA-CLOSE-01): for each pending tool call that carries one, it calls `ApprovalService::decide_by_operator(approval_id, decision, principal.subject)` before `runs.resume`, so the card and the run agree (closes O50).
- **SA-CLOSE-03** *(chosen here)* Route 32 passes `principal.groups` as `PressOrigin::Approver.groups`, so an approver named in the configuration only by group is recognised by the Approval Service.
- **SA-RECON-02** The readiness check is asynchronous (`ReadyCheck`), because the program's check pings the datastore (reconciliation row 429, ruling R6).
- **SA-RECON-03** The Caller API crate owns `health_router` and `OperatorClient` (reconciliation rows 428 and 430, ruling R6): the health routes and the operator client are the two halves of wrightrun's routes this crate already owns. `health_router` takes the Telemetry handle as well as the readiness check so that the roles without caller routes still serve `/metrics`.
- **SA-RECON-04** The callees are used under their own names (rulings R4, R5, R6): the File Service's `WorkFolder::operator_*`, the Knowledge Service's `KnowledgeIndex` (`None` for the agent-wide scope), `RulesEngine::{take_intake_place, lock_held}` with `RULE_STATE_UNAVAILABLE`, `Telemetry::record` and the OpenMetrics `render`, and 2C `Page<T>`/`Cursor`.

## Shared Context

### Shared types (Phase 2C, `ordin-types`)

`Timestamp` (RFC 3339 ms `Z` internally), `ScopeId` (UUIDv7), `TaskArgument { name, value }`, and:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunStarter {
    Caller { caller_id: String }, ScheduleEntry { entry: String },
    Intake { intake: String }, ChatConnection { connection: String, asker: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunRequest { Command { command: String, arguments: Vec<TaskArgument> }, Message { text: String } }
pub enum RunRecordState { Queued, Executing, Paused, Ended, Cancelled, Interrupted }
pub enum RunEndKind { Completed, Control, Failed, RunLimit }
pub struct RunRecord { pub id, pub run_id: String, pub thread_id: String, pub started_by: RunStarter,
    pub scope_id: ScopeId, pub request: RunRequest, pub gathered_context, pub state: RunRecordState,
    pub end_kind: Option<RunEndKind>, pub lease_holder, pub lease_expires_at, pub result: Option<String>,
    pub stretch_summaries: Vec<String>, pub queued_at: Timestamp }
```

Paging (2C `ordin_types::paging`, decision 80: C-15): `Page<T> { items: Vec<T>, next_cursor: Option<Cursor> }` (`next_cursor` always serialised, `null` on the last page), `PAGE_SIZE = 100`, `Cursor::encode`, `Cursor::decode` (a refusal → `SRV_BAD_CURSOR`). A query's `cursor` text is passed to each callee in the form the callee takes.

### From Caller Authentication (`ordin-caller-auth`)

`Verifier::verify(Option<&str>) -> Result<Principal, AuthError>`; `check_request(&Verifier, method, route_path, header) -> Result<Option<Principal>, AuthError>`; `required_role(method, route_path) -> Result<LeastRole, AuthError>`; `require_role(&Principal, Role)`; `may_act(&Principal, &RunStarter) -> bool`; `Principal { subject, role: Role::{Caller, Operator}, approver: bool, groups: Vec<String> }` (`groups` = the values of the configured `group_claim`, CallerAuthentication.md SA-CLOSE-01); `AuthError::{code(), http_status()}` with codes `AUTH_MISSING` 401, `AUTH_INVALID` 401, `AUTH_NO_ROLE` 403, `AUTH_FORBIDDEN` 403, `AUTH_ROUTE_UNMAPPED` 403, `AUTH_KEYS_UNAVAILABLE` 503. The route table there lists every route of this crate, routes 31 `GET /v1/approvals` and 32 `POST /v1/approvals/{approval_id}/decision` included with least role caller (CallerAuthentication.md SA-CLOSE-02); `required_role` ignores the query string.

### From Run Queue (`ordin-run-queue`)

`RunQueue::{enqueue(NewRun), thread_owner(&str), get(&str) -> Option<RunView>, resume(&str, Value), cancel(&str) -> CancelOutcome, events_after(&str, u64, u32) -> Vec<StoredRunEvent>, list(Option<&[RunRecordState]>, Option<&str>) -> Page<RunView>}`. `RunView { record, outcome: Option<RunOutcome>, created_at, started_at, ended_at, cancel_requested }`; `RunOutcome { reply, ending, pending: Vec<PendingItem>, …, error_code, safe_reply, ended_by_control }`; `PendingItem { kind: "tool-call" | "refused-ending", tool, gate, reason, approval_id: Option<ApprovalId> }`; `CancelOutcome::{Cancelled(RunView), Requested(RunView)}`; `StoredRunEvent { seq, kind: RunEventKind::{RunStateChanged, RunProgress}, data, at }`. Codes: `RUN_THREAD_NOT_FOUND`, `RUN_THREAD_BUSY`, `RUN_DUPLICATE_ID`, `RUN_BAD_INPUT`, `RUN_NOT_PAUSED`, `RUN_NOT_CANCELLABLE`, `SRV_BAD_CURSOR`, `DB_UNAVAILABLE`.

### From File Service (`ordin-files`, copied from FileService.md *Work folder*)

```rust
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
pub fn normalise_path(path: &str) -> Result<String, FileError>;   // FileError::WorkFolder("WF_INVALID_PATH")
pub fn is_bundle_path(normalised: &str) -> bool;
pub struct WorkFileEntry { pub path: String, pub size_bytes: u64, pub modified_at: Timestamp, pub bundle: bool }
impl WorkFolder {   // operator methods: no writable-path or protected-path test
    pub async fn operator_list(&self, prefix: &str, cursor: Option<Cursor>) -> Result<Page<WorkFileEntry>, FileError>;
    pub async fn operator_read(&self, path: &str) -> Result<Vec<u8>, FileError>;              // WF_NOT_FOUND
    pub async fn operator_write(&self, path: &str, content: Vec<u8>) -> Result<(), FileError>; // WF_BUNDLE_PATH, WF_TOO_LARGE
    pub async fn operator_remove(&self, path: &str) -> Result<(), FileError>;
    pub async fn operator_exists(&self, path: &str) -> Result<bool, FileError>;
}
// FileError::code(): for the work folder FileError::WorkFolder(code) with WF_INVALID_PATH, WF_BUNDLE_PATH,
// WF_NOT_FOUND, WF_TOO_LARGE, WF_STORE_UNAVAILABLE; a bad cursor → SRV_BAD_CURSOR
```

### From Settings Store (`ordin-settings-store`)

`SettingsStore::{declarations(), get(ScopeId, &str), all(ScopeId), set(ScopeId, &str, Value, &SettingWriter, Option<i64>)}`; `SettingValue { name, kind, value, version, is_default }`; `SettingWriter::Operator { subject }`; declaration fields `name`, `kind`, `label`, `who`; codes `SET_UNKNOWN`, `SET_INVALID`, `SET_NOT_ALLOWED`, `SET_VERSION_CONFLICT`, `SET_UNAVAILABLE`.

### From Knowledge Service (`ordin-knowledge`, copied from KnowledgeService.md; the program passes `KnowledgeService::index()`)

```rust
pub enum ItemKind { ChatHistory, Entries, Notes }
pub struct KnowledgeItem { pub item_id: uuid::Uuid, pub source: String, pub kind: ItemKind,
    pub scope_id: Option<ScopeId> /* None = agent-wide */, pub origin_id: Option<String>,
    pub origin_time: chrono::DateTime<chrono::Utc>, pub sender: Option<String>, pub sender_name: Option<String>,
    pub from_program: bool, pub own: bool, pub text: String, pub thread: Option<String>,
    pub verified: Option<bool>, pub run_id: Option<RunId>, pub retired: bool }
trait KnowledgeIndex {   // the methods used here
    async fn open(&self, item_id: &str, scope: Option<ScopeId>) -> Result<Option<KnowledgeItem>, KnowledgeError>;
    async fn notes(&self, scope: Option<ScopeId>, cursor: Option<&str>) -> Result<Page<KnowledgeItem>, KnowledgeError>;
    async fn note_retire(&self, note_id: &str, scope: Option<ScopeId>) -> Result<(), KnowledgeError>;       // KNOW_NOTE_NOT_FOUND
    async fn entries(&self, scope: ScopeId, cursor: Option<&str>) -> Result<Page<KnowledgeItem>, KnowledgeError>;
    async fn entry_add(&self, source: &str, scope: ScopeId, text: &str, by: &str) -> Result<uuid::Uuid, KnowledgeError>; // KNOW_UNKNOWN_SOURCE, KNOW_ENTRY_INVALID
    async fn entry_verify(&self, entry_id: &str, by: &str) -> Result<(), KnowledgeError>;           // KNOW_ENTRY_NOT_FOUND
    async fn entry_remove(&self, entry_id: &str) -> Result<(), KnowledgeError>;                     // KNOW_ENTRY_NOT_FOUND
}
```

Scope mapping: the agent-wide scope is `None` for `open`, `notes` and `note_retire`; a channel or direct chat is `Some(scope.id)`. Entries exist in channel and direct-chat scopes only, so routes 21–24 with `{scope}` = `agent-wide` answer 404 `OPS_NOT_FOUND`. *Item* `time` is `origin_time` in `wire_time` form; `id` is `item_id`.

### From Retention and Erasure (`ordin-retention`)

`Retention::{scopes() -> Arc<dyn ScopeRegistry>, erase_person(connection, user, dry_run, operator), erase_scope(&ScopeAddress, dry_run, operator)}`; `ScopeRegistry::{agent_wide(), resolve(&ScopeAddress), get(ScopeId)}`; `ScopeEntry { scope: Scope, connection }`; `parse_scope_text(&str) -> Result<ScopeAddress, RetentionError>`; `scope_text(&ScopeEntry)`; `ErasureReport { target, dry_run, counts: ErasureCounts { knowledge_items, memory_items, chat_replies, pending_questions, runs, threads, settings, jobs, channels }, covers, does_not_cover }`; codes `ERASE_BAD_TARGET`, `ERASE_INCOMPLETE`, `ERASE_UNAVAILABLE`.

### From Rules Engine (`ordin-rules`, copied from RulesEngine.md; ruling R5)

```rust
impl RulesEngine {
    /// Agent profile: Caller API intake limit (wrightrun take_intake_place).
    pub async fn take_intake_place(&self, intake: &str, run_id: &str, limit: u32) -> Result<bool, RulesError>;
    /// Agent profile: whether a lock is held now, for the operator's duty routes.
    pub async fn lock_held(&self, key: &str) -> Result<bool, RulesError>;   // RULE_STATE_UNAVAILABLE
}
pub fn duty_lock_key(duty: &str) -> String;
```

### From Approval Service (`ordin-approval`, copied from ApprovalService.md)

`ApprovalService::decide_by_operator(&self, approval_id: ApprovalId, decision: PressDecision, operator: &str) -> Result<ApprovalOutcome, ApprovalError>` — "Called by: Caller API, in the operator's resume of a paused run, before it resumes the run in the Run Queue"; `APPR_OPERATOR_NOT_ALLOWED` (409 here) when approvers decide. `PressDecision::{Approve, Reject}`.

Routes 31–32 (SA-CLOSE-01) also use:

```rust
impl ApprovalService {
    pub async fn decide(&self, press: ApprovalPress, origin: PressOrigin) -> Result<ApprovalOutcome, ApprovalError>;
    pub async fn get(&self, approval_id: ApprovalId) -> Result<ApprovalRecord, ApprovalError>;
    pub async fn list_pending(&self, workspace: Option<WorkspaceId>, cursor: Option<Cursor>)
        -> Result<Page<ApprovalRecord>, ApprovalError>;
}
pub struct ApprovalPress { pub card_ref: CardRef, pub decision: PressDecision }
pub enum PressOrigin {
    LocalScreen,
    PairedDesktop { computer_name: String },
    PairedChat { channel: ChatChannel },
    Approver { subject: String, groups: Vec<String>, via: ApproverVia },   // the only variant this crate builds
}
pub enum ApproverVia { Chat, Desktop }
pub struct ApprovalRecord {
    pub card: ApprovalCard,            // fields read: asked_by, question, where_line, created_at, buttons.{approve, reject}
    pub proposal: ActionProposal,
    pub state: ApprovalState,
    pub decided_by: Option<String>,
    pub decided_on: Option<String>,
    pub decided_at: Option<Timestamp>,
}
pub struct ApprovalOutcome { pub approval_id: ApprovalId, pub state: ApprovalState, pub decided_on: String }
```

Codes read on routes 31–32: `APPR_NOT_FOUND`, `APPR_NOT_AN_APPROVER`, `APPR_ASKER_MAY_NOT_APPROVE`, `APPR_CARD_ALREADY_DECIDED`, `APPR_CARD_CLOSED`, `APPR_CARD_REF_UNKNOWN`, `APPR_CARD_REF_MISMATCH`.

### From Telemetry (`ordin-telemetry`, copied from Telemetry.md; ruling R4)

`Telemetry::record(&self, event: TelemetryEvent)`; `Telemetry::render(&self) -> (Vec<u8>, &'static str)` (OpenMetrics bytes and `OPENMETRICS_CONTENT_TYPE` = `application/openmetrics-text; version=1.0.0; charset=utf-8`). Variants used here: `HttpRequest { route: &'static str, status: u16 }` (40), `HttpRefusal { code: &'static str }` (41), `WorkFolderChange { action: WorkFolderAction::{Write, Remove} }` (42), `OperatorRequest { route: &'static str, outcome: OperatorOutcome::{Done, Refused} }` (43), `IntakeAccepted { intake: String }` (7), `IntakeRefused { intake: String, code: &'static str }` (8).

### Settings the program passes (this crate depends on neither Agent Configuration nor Agent Loader)

```rust
pub struct CallerApiSettings {
    pub port: u16,                                  // ORDIN_PORT (2D-1a), default 8080
    pub commands: std::collections::BTreeSet<String>,  // the release's command slugs
    pub duties: Vec<String>,                        // recurring duty slugs, manifest order
    pub chat_history_sources: Vec<(String, String)>,   // (source name, connection) of knowledge[] kind chat-history
    pub approvers_configured: bool,                 // the configuration names approvers (ADR-097 item 4)
    pub intakes: Vec<IntakeSettings>,
}
pub struct IntakeSettings {                         // wrightrun intakes[] (EventIntake.md)
    pub name: String,                               // slug
    pub command: String,                            // command slug
    pub accept_token: bool,                         // default true
    pub secret: Option<IntakeSecret>,               // resolved by the program from the secret store or ORDIN_SECRETS_DIR (2D-1d)
    pub signature_header: String,                   // default "X-Wrightrun-Signature"
    pub timestamp_header: Option<String>,
    pub max_body_bytes: usize,                      // 1..=1_048_576, default 65_536
    pub max_runs_in_progress: u32,                  // 1..=1000, default 10
}
/// Holds a shared secret; Debug prints "[redacted]".
pub struct IntakeSecret(String);
```

The header names `X-Wrightrun-Signature` (default) and `X-Wrightrun-Modified-At` are kept from wrightrun so senders and tools written for wrightrun work unchanged (ADR-098 item 2) *(assumed here: ADR-098 keeps the routes "as wrightrun's 08 states them", which includes these header names)*.

### Constants

| Constant | Value | Source |
|---|---|---|
| `MAX_START_BODY_BYTES` | 1,048,576 | wrightrun `RuntimeServer.md` |
| `MAX_DECISION_BODY_BYTES` | 65,536 | wrightrun |
| `MAX_TEXT_CHARS` | 262,144 | wrightrun |
| `MAX_DECISION_MESSAGE_CHARS` | 2,000 | wrightrun |
| `MAX_OPERATOR_BODY_BYTES` | 65,536 | wrightrun `OperatorRoutes.md` |
| `MAX_CONNECTIONS` | 100 | wrightrun |
| `SHUTDOWN_SECONDS` | 25 | wrightrun |
| `SIGNATURE_PREFIX` | `sha256=` | wrightrun `EventIntake.md` |
| `TIMESTAMP_TOLERANCE_SECONDS` | 300 | wrightrun |
| `RETRY_AFTER_SECONDS` | 30 | wrightrun |
| `MIN_SECRET_CHARS` | 32 | 2D-4 *Intake shared secret* |
| `STREAM_POLL_MILLIS` | 1,000 | *chosen here*: how often an open event stream reads new events |
| `STREAM_HEARTBEAT_SECONDS` | 20 | ADR-096 item 3 (the resume rule ADR-098 item 2 applies to this stream) |
| `STREAM_MAX_SECONDS` | 3,600 | *chosen here*: an open stream is closed after one hour; the client reconnects with `Last-Event-ID` |

## Public Interface

```rust
// crate ordin-caller-api
pub struct CallerApiDeps {
    pub verifier: std::sync::Arc<ordin_caller_auth::Verifier>,
    pub runs: std::sync::Arc<dyn ordin_run_queue::RunQueue>,
    pub work_folder: std::sync::Arc<ordin_files::WorkFolder>,
    pub settings: std::sync::Arc<dyn ordin_settings_store::SettingsStore>,
    pub knowledge: std::sync::Arc<dyn ordin_knowledge::KnowledgeIndex>,
    pub retention: std::sync::Arc<ordin_retention::Retention>,
    pub controls: std::sync::Arc<ordin_rules::RulesEngine>,
    pub approvals: std::sync::Arc<ordin_approval::ApprovalService>,
    pub telemetry: std::sync::Arc<ordin_telemetry::Telemetry>,
    pub jobs: std::sync::Arc<dyn JobDesk>,
    pub ready: ReadyCheck,
}

/// The readiness check the program supplies: resolves to true when the instance may take requests
/// (OrdinServer: false once stopping began; otherwise `journal.ping(2 s)` and, in a chat role, every chat link open).
/// Asynchronous because the check reaches the datastore (reconciliation row 429).
pub type ReadyCheck = std::sync::Arc<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'static>> + Send + Sync>;

/// The job routes' view of the Job Runner, implemented by the program (same build phase).
#[async_trait::async_trait]
pub trait JobDesk: Send + Sync {
    async fn request(&self, kind: JobKindText, key: &str, requested_by_subject: &str) -> Result<(JobRow, bool), DeskError>;
    async fn list(&self, cursor: Option<&str>) -> Result<ordin_types::paging::Page<JobRow>, DeskError>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum JobKindText { Ingest, Retention }
/// The *Job* body of the operator routes (wrightrun schema), times already in the wire form.
#[derive(Debug, Clone, Serialize)]
pub struct JobRow {
    pub job_id: String, pub kind: &'static str, pub key: String, pub state: &'static str,
    pub requested_by: Option<serde_json::Value>,
    pub progress: JobProgressRow, pub error_code: Option<String>,
    pub created_at: String, pub ended_at: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct JobProgressRow { pub items_read: u64, pub total: Option<u64>, pub started_at: Option<String>,
                            pub share_done: Option<f64>, pub seconds_left: Option<u64> }
#[derive(Debug, Clone)] pub struct DeskError { pub code: String, pub message: String }

/// Builds the router; fails with AUTH_ROUTE_UNMAPPED (a route missing from Caller Authentication's
/// table), SRV_ROUTE_CONFLICT (two routes answer one method and path), INTAKE_SECRET_MISSING or
/// INTAKE_SECRET_INVALID (an intake secret absent or shorter than 32 characters).
pub fn build_router(settings: CallerApiSettings, deps: CallerApiDeps) -> Result<axum::Router, CallerApiError>;
/// Routes 28–30 only (health and metrics, no token), for instances in the `runs`, `chat` and `jobs` roles,
/// which serve no caller route (reconciliation row 428). The full router of `build_router` includes the same three routes.
pub fn health_router(ready: ReadyCheck, telemetry: std::sync::Arc<ordin_telemetry::Telemetry>) -> axum::Router;
/// Serves until `stop` is cancelled; open requests get up to 25 s to finish.
pub async fn serve(router: axum::Router, port: u16, stop: tokio_util::sync::CancellationToken) -> Result<(), CallerApiError>;
/// HMAC-SHA256 check of an intake event (pure).
pub fn verify_signature(secret: &IntakeSecret, body: &[u8], signature: Option<&str>, timestamp: Option<&str>,
                        signed_timestamp: bool, now_unix: i64) -> bool;
/// "2026-01-10T09:15:02.418000Z": UTC, six fractional digits, Z (wrightrun's form, SA-TIME-01 exception).
pub fn wire_time(t: &ordin_types::Timestamp) -> String;
/// The wrightrun wire state of a run.
pub fn wire_state(view: &ordin_run_queue::RunView) -> &'static str;

#[derive(Debug, thiserror::Error)]
pub enum CallerApiError {
    #[error("no role is stated for this route ({0})")]        RouteUnmapped(String),
    #[error("two routes answer {0}")]                         RouteConflict(String),
    #[error("the intake {0} names a secret that is not set")] IntakeSecretMissing(String),
    #[error("the intake {0}'s secret must hold at least 32 characters")] IntakeSecretInvalid(String),
    #[error("the port {0} cannot be opened")]                 PortUnavailable(u16),
    #[error("the server stopped without being told to")]      Stopped,
}
impl CallerApiError { pub fn code(&self) -> &'static str; }

/// wrightrun's ServerClient: the client the operator commands of group S (`ordin-server agent duty status`,
/// `runs …`, `files …`, `settings …`, `jobs …`, `notes …`, `entries …`, `erase …`) use to call a running
/// hosted agent's routes. Holds the token; `Debug` prints "[redacted]" for it (reconciliation row 430).
pub struct OperatorClient { /* reqwest::Client (rustls, no redirect), base URL, bearer token, timeout */ }
impl OperatorClient {
    /// From ORDIN_SERVER_URL and the token held in the file ORDIN_TOKEN_FILE names (2D-1).
    /// Fails with SRV_URL_MISSING (unset, empty or refused address) or SRV_TOKEN_MISSING (unset, unreadable or empty file).
    pub fn from_env() -> Result<Self, OperatorClientError>;
    /// `base_url`: https, or http for localhost and 127.0.0.1 only; no user information, no fragment, ≤ 2,048 characters.
    pub fn new(base_url: &str, token: OperatorToken, timeout: std::time::Duration) -> Result<Self, OperatorClientError>;
    /// Sends one request and no second one. A status below 400, or one in `accept`, returns the response.
    pub async fn call(&self, method: http::Method, path: &str, query: &[(&str, String)],
                      body: Option<OperatorBody>, accept: &[u16], timeout: Option<std::time::Duration>)
                      -> Result<OperatorResponse, OperatorClientError>;
    /// Every page of a listing route (`next_cursor` followed until null); the items in order.
    pub async fn pages(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<serde_json::Value>, OperatorClientError>;
}
/// An operator's token read from ORDIN_TOKEN_FILE; `Debug` prints "[redacted]".
pub struct OperatorToken(String);
pub enum OperatorBody { Json(serde_json::Value), Bytes(Vec<u8>) }
pub struct OperatorResponse { pub status: u16, pub headers: http::HeaderMap, pub body: Vec<u8> }

#[derive(Debug, thiserror::Error)]
pub enum OperatorClientError {
    #[error("the environment variable ORDIN_SERVER_URL is not set")]       UrlMissing,
    #[error("expected an https address, or an http address of localhost or 127.0.0.1 (ORDIN_SERVER_URL)")] UrlRefused,
    #[error("the environment variable ORDIN_TOKEN_FILE is not set or names no readable, non-empty file")] TokenMissing,
    #[error("the server could not be reached")]                           Unreachable,
    #[error("the server refused the token")]                              Unauthorised,
    #[error("{0}")]                                                       Forbidden(String),
    #[error("{0}")]                                                       Response(String),
}
impl OperatorClientError { pub fn code(&self) -> &'static str; }  // SRV_URL_MISSING (UrlMissing, UrlRefused), SRV_TOKEN_MISSING, SRV_UNREACHABLE, SRV_UNAUTHORISED, SRV_FORBIDDEN, SRV_RESPONSE
```

**Called by.**

| Public item | Called by |
|---|---|
| `build_router`, `serve`, `CallerApiSettings`, `CallerApiDeps`, `JobDesk` (implemented over the Job Runner's `JobStore`) | the server program (OrdinServer, agent profile, `requests` role and single shape) |
| `health_router` | the server program, in the `runs`, `chat` and `jobs` roles (OrdinServer step 6) |
| `OperatorClient`, `OperatorClient::from_env`, `call`, `pages` | the server program's operator commands of group S (OrdinServer step 10) |
| `verify_signature`, `wire_time`, `wire_state` | this crate's routes; exposed for the conformance tests |
| Routes 1–6 (caller interface) | callers written for wrightrun; the Hosted Agent Adapter (desktop, a hosted agent by role) |
| Routes 7–27 | the operator commands through `OperatorClient` |
| Routes 28–30 | the deployment's probes and metrics scraper |
| Routes 31–32 | an approver's ordin desktop that reaches the hosted agent by role (ADR-097 item 9; 03 *A desktop uses a hosted agent by role*) |

**`wire_state`:** `Queued` → `queued`; `Executing` → `executing`; `Paused` → `paused`; `Ended`+`Completed` → `finished`; `Ended`+`Control` → `ended-by-control`; `Ended`+`Failed` → `failed`; `Ended`+`RunLimit` → `ended-at-limit`; `Interrupted` → `interrupted`; `Cancelled` → `cancelled` (an added value, for the cancel route ADR-098 item 2 adds).

### Routes

"Owner" means `may_act(principal, started_by)`: the principal is an operator, or the record's starter is `Caller { caller_id }` equal to the principal's subject.

| # | Method and path | Name | Least role | Ownership | Success |
|---|---|---|---|---|---|
| 1 | `POST /v1/runs` | `runs.start` | caller | — | 202 *Accepted* |
| 2 | `POST /v1/threads/{thread_id}/runs` | `threads.runs.start` | caller | owner of the thread | 202 *Accepted* |
| 3 | `GET /v1/runs/{run_id}` | `runs.read` | caller | owner of the run | 200 *Run* |
| 4 | `POST /v1/runs/{run_id}/resume` | `runs.resume` | caller (operator when approvers are configured) | owner | 202 *Accepted* |
| 5 | `GET /v1/runs/{run_id}/events` | `runs.events` | caller | owner | 200 `text/event-stream` |
| 6 | `POST /v1/runs/{run_id}/cancel` | `runs.cancel` | caller | owner | 202 *Cancelled* |
| 7 | `GET /work-folder/files` | `files.list` | operator | — | 200 *FileList* |
| 8 | `GET /work-folder/files/{*path}` | `files.read` | operator | — | 200 bytes |
| 9 | `PUT /work-folder/files/{*path}` | `files.write` | operator | — | 200 *FileWritten* |
| 10 | `DELETE /work-folder/files/{*path}` | `files.remove` | operator | — | 200 *FileRemoved* |
| 11 | `POST /intakes/{name}` | `intakes.receive` | none (token or signature checked here) | — | 202 *IntakeAccepted* |
| 12 | `GET /operator/runs` | `operator.runs.list` | operator | — | 200 page of *RunRow* |
| 13 | `GET /operator/scopes/{scope}/settings` | `operator.settings.list` | operator | — | 200 page of *Setting* |
| 14 | `GET /operator/scopes/{scope}/settings/{name}` | `operator.settings.read` | operator | — | 200 *Setting* |
| 15 | `PUT /operator/scopes/{scope}/settings/{name}` | `operator.settings.write` | operator | — | 200 *Setting* |
| 16 | `GET /operator/jobs` | `operator.jobs.list` | operator | — | 200 page of *Job* |
| 17 | `POST /operator/jobs` | `operator.jobs.request` | operator | — | 201 created / 200 open; *Job* with `created` |
| 18 | `GET /operator/scopes/{scope}/notes` | `operator.notes.list` | operator | — | 200 page of *Item* |
| 19 | `GET /operator/scopes/{scope}/notes/{note_id}` | `operator.notes.read` | operator | — | 200 *Item* |
| 20 | `POST /operator/scopes/{scope}/notes/{note_id}/retire` | `operator.notes.retire` | operator | — | 200 *Item* |
| 21 | `GET /operator/scopes/{scope}/entries` | `operator.entries.list` | operator | — | 200 page of *Item* |
| 22 | `POST /operator/scopes/{scope}/entries` | `operator.entries.add` | operator | — | 201 *Item* |
| 23 | `POST /operator/scopes/{scope}/entries/{entry_id}/verify` | `operator.entries.verify` | operator | — | 200 *Item* |
| 24 | `DELETE /operator/scopes/{scope}/entries/{entry_id}` | `operator.entries.remove` | operator | — | 200 `{"entry_id": …}` |
| 25 | `POST /operator/erase` | `operator.erase` | operator | — | 200 *ErasureReport* |
| 26 | `GET /operator/duties` | `operator.duties.list` | operator | — | 200 `{"items":[{"duty","held"}],"next_cursor":null}` |
| 27 | `GET /operator/duties/{duty}/lock` | `operator.duty.lock` | operator | — | 200 `{"duty","held"}` |
| 28 | `GET /healthz` | `health.live` | none | — | 200 `{"status":"ok"}` |
| 29 | `GET /readyz` | `health.ready` | none | — | 200 `{"status":"ready"}` / 503 `{"error":"the instance is not ready","code":"SRV_NOT_READY"}` |
| 30 | `GET /metrics` | `metrics` | none | — | 200 `application/openmetrics-text; version=1.0.0; charset=utf-8` |
| 31 | `GET /v1/approvals?state=pending` | `approvals.list` | caller, and `Principal.approver` | — | 200 *ApproverCards* |
| 32 | `POST /v1/approvals/{approval_id}/decision` | `approvals.decide` | caller, and `Principal.approver` | — | 200 `{"approval_id", "state"}` |

Routes 5 and 6 are the two additions ADR-098 item 2 names, with paths *chosen here* (08 "paths fixed in `/spec`"). Every JSON response has `Content-Type: application/json`. Every time on the wire is `wire_time` form. A `POST /v1/runs` sent twice starts two runs (no idempotency key, as wrightrun). `{scope}` is `agent-wide` or `<connection>:<chat identifier>`.

**Error form.** Every refusal of every route: `{"error": "<one sentence>", "code": "<CODE>"}` in that order, with the status of the Error Table; `error` never holds a token, a body, a file's content, a message's text, an argument's value or a person's identifier. No `action` key (SA-ENV-02).

**Request *StartRun*** (routes 1, 2; body ≤ 1,048,576 bytes): a JSON object with only the keys `command` (string matching `^[a-z0-9]+(-[a-z0-9]+)*$`, ≤ 64), `arguments` (string ≤ 262,144 chars, only with `command`), `message` (string 1–262,144 chars with a non-white-space char); exactly one of `command` and `message`; no NUL in any text.

**Request *Resume*** (route 4; body ≤ 65,536 bytes): `{"decisions": [{"type": "approve"|"reject", "message"?: string ≤ 2,000, only with reject}] (≤ 100), "set_aside"?: [string 1–128] (≤ 20, unique)}`; no other key.

**Response *Accepted*** (202): `{"thread_id": "<uuid>", "run_id": "<uuid>", "state": "queued"}`.

**Response *Cancelled*** (202): `{"thread_id": "<uuid>", "run_id": "<uuid>", "state": "cancelled" | "executing"}` — `executing` means the cancel was requested and the run's instance stops it within one lease renewal (≤ 10 s).

**Response *Run*** (200): `{"run_id", "thread_id", "state", "started_by": {"kind": "caller"|"schedule"|"intake"|"chat", "id", "asker": string|null}, "command": string|null, "created_at", "started_at": string|null, "ended_at": string|null, "result": null | {"reply", "ending", "pending": [{"kind","tool","gate","reason"}], "error_code", "safe_reply"}}`. `result` is null while queued or executing (and for a cancelled run that never paused or ended). `started_by` maps `Caller{caller_id}` → (`caller`, caller_id, null), `ScheduleEntry{entry}` → (`schedule`, entry, null), `Intake{intake}` → (`intake`, intake, null), `ChatConnection{connection, asker}` → (`chat`, connection, asker). The run's input, gathered context and stretch summaries are never returned.

**Event stream** (route 5): `text/event-stream`; each stored run event is sent as `id: <seq>`, `event: run_state_changed` or `event: run_progress`, `data: <JSON>` where `run_state_changed` data is `{"run_id", "state": <wire state>, "at": <wire time>}` and `run_progress` data is `{"run_id", "line", "at"}`. The event names reuse the Service API event kinds (08 *Service API*, revision 0.48.0). A `Last-Event-ID` header (a non-negative integer) resumes after that sequence number; absent → from the first event. A comment line `: keep-alive` every 20 s. The stream closes after it sends a state of `finished`, `failed`, `ended-at-limit`, `ended-by-control`, `interrupted` or `cancelled`, or after 3,600 s.

**Work-folder schemas** (wrightrun): *FileList* `{"items": [{"path", "size_bytes", "modified_at"}], "next_cursor"}`; *FileWritten* `{"path", "size_bytes"}`; *FileRemoved* `{"path"}`. Route 7 takes `prefix` (default `/`) and `cursor`. Route 8 answers the bytes with `application/octet-stream` and `X-Wrightrun-Modified-At: <wire time>`. Route 9 takes the raw bytes (≤ 10,485,760). The URL path is the work-folder path without its leading `/`.

**Intake** (route 11): body = the event's UTF-8 text, 1..=`max_body_bytes`; token form `Authorization: Bearer <JWT>` (caller or operator role); signature form `<signature_header>: sha256=<hex HMAC-SHA256 of the body>` — of `<timestamp>.<body>` when `timestamp_header` is set, with that header carrying Unix seconds. Success 202 `{"intake": "<name>", "thread_id", "run_id"}`.

**Operator schemas** are wrightrun's (`OperatorRoutes.md`), with one addition: *ErasureReport* `counts` holds `memory_items` (ordin keeps memory pages by scope, ADR-097 item 5). *RunRow* `{"run_id","thread_id","state","started_by","command","scope","pending","created_at","started_at","ended_at"}`; *Setting* `{"name","kind","label","who","value","version","is_default"}`; *SettingChange* `{"value", "version"?: int|null}`; *Job* as `JobRow` plus `created` on route 17; *JobRequest* `{"kind": "ingest"|"retention", "key"?: string ≤ 200}`; *Item* `{"id","source","kind": "entries"|"notes","scope","text","time","run_id","retired","verified"}`; *EntryAdd* `{"source": slug ≤ 64, "text": 1–20,000 chars}`; *Erase* `{"target": "person"|"scope", "dry_run": bool, "connection"+"user" | "scope"}` (`dry_run` required).

### Example

```text
POST /v1/runs
Authorization: Bearer eyJhbGciOiJSUzI1NiIs…
{"command": "run-go-no-go", "arguments": "RC-12"}

202
{"thread_id": "01929a3e-7c2e-7d7a-8f0b-0d2f6b1c8e11", "run_id": "01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b", "state": "queued"}

GET /v1/runs/01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b/events
Authorization: Bearer eyJhbGciOiJSUzI1NiIs…

200 text/event-stream
id: 1
event: run_state_changed
data: {"run_id":"01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b","state":"queued","at":"2026-01-10T09:15:02.418000Z"}

id: 2
event: run_state_changed
data: {"run_id":"01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b","state":"executing","at":"2026-01-10T09:15:03.102000Z"}

id: 3
event: run_progress
data: {"run_id":"01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b","line":"Reading the RC-12 checklist","at":"2026-01-10T09:15:05.880000Z"}

POST /v1/runs/01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b/cancel
202
{"thread_id": "01929a3e-7c2e-7d7a-8f0b-0d2f6b1c8e11", "run_id": "01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b", "state": "executing"}

GET /v1/runs/01929a3e-7c31-7f7a-8b19-5c3d2e1f0a9b      (token of another caller)
404
{"error": "no run with this identifier", "code": "SRV_RUN_NOT_FOUND"}
```

## Internal Logic

**`build_router`**

1. Build the 32 routes of the table. For every (method, path) call `required_role`; an error → `RouteUnmapped` (the program exits 1). Two routes sharing a path with an overlapping method → `RouteConflict`.
2. For each intake with `secret = None` and `accept_token = false` → `IntakeSecretMissing(name)`; a secret shorter than 32 chars → `IntakeSecretInvalid(name)`. (The program reports a named variable it could not resolve before calling this.)
3. Layers: a panic-catching layer that answers 500 `SRV_INTERNAL` and logs `server_error` (ERROR; `error` = the panic's type, `method`) — `tower-http`'s `CatchPanicLayer` (`memory/Cargo.toml:41`); a load-shedding concurrency limit of 100 (`tower::limit::ConcurrencyLimitLayer` behind `tower::load_shed::LoadShedLayer`, `memory/Cargo.toml:40`) whose overload answers 503 `SRV_REFUSED`; a fallback answering 404 `SRV_NO_ROUTE`; a method-not-allowed answer 405 `SRV_METHOD_NOT_ALLOWED` with the `Allow` header; any other refusal raised by axum (an extractor rejection) 400 `SRV_BAD_REQUEST` or, for a status axum gives, that status with `SRV_REFUSED`.
4. A request with an `Origin` header is answered 403 `SRV_REFUSED` (SA-XC-11: no CORS; callers are servers and desktops, not browser pages). No CORS header is ever sent.
5. Log `server_routes_built` (INFO; `routes` = 30, `intakes` = count). Return the router.

**Every endpoint first** (except 11, 28–30)

6. `principal = check_request(verifier, method, <declared path>, Authorization)`; an `AuthError` → its status and code. The token is verified before the body is read.
7. Body routes: `Content-Length` above the ceiling → 413 `SRV_BODY_TOO_LARGE` (operator routes: `OPS_BODY_TOO_LARGE`) without reading; else read chunks and refuse as soon as the sum exceeds the ceiling.
8. On return, `telemetry.record(TelemetryEvent::HttpRequest { route: name, status })`; on a refusal also `record(HttpRefusal { code })` and log `request_refused` (INFO; `code`, `method`).

**Routes 1 and 2 — start**

9. Parse the body as a JSON object (else 400 `SRV_BAD_REQUEST` "the body is not a JSON object") and check *StartRun* in order: unknown key; non-string value; both or neither of `command`/`message`; `arguments` without `command`; `command` not a slug or > 64; text > 262,144 chars; NUL; `message` blank. Each → 400 `SRV_BAD_REQUEST` with a message naming the key, never its value.
10. `command ∉ settings.commands` → 400 `SRV_UNKNOWN_COMMAND` "unknown command; the commands are: <sorted, comma-separated>" (or "the package has no command").
11. Route 2: `thread_id` not a UUID, or `runs.thread_owner(thread_id)` is `None`, or `!may_act(principal, owner)` → 404 `SRV_THREAD_NOT_FOUND` "no thread with this identifier" (a thread of another caller answers exactly as one that does not exist).
12. `scope = retention.scopes().agent_wide()` (a run through this interface has the agent-wide scope). `runs.enqueue(NewRun { started_by: Caller { caller_id: subject }, scope_id, request: Command { command, arguments: [TaskArgument { name: "arguments", value: arguments or "" }] } | Message { text }, gathered_context: {}, thread_id, run_id: None })`. `RUN_THREAD_NOT_FOUND` → 404 `SRV_THREAD_NOT_FOUND`; `RUN_THREAD_BUSY` → 409; `DB_UNAVAILABLE` → 503.
13. Log `run_accepted` (INFO; `run_id`, `thread_id`, `command` or `"none"`, `role`, `new_thread`). Answer 202 *Accepted*.

**Route 3 — read**

14. `run_id` not a UUID, or `runs.get` is `None`, or `!may_act(principal, view.record.started_by)` → 404 `SRV_RUN_NOT_FOUND`. Answer *Run* with `command` from `RunRequest::Command`, `result` from `outcome` (`reply`, `ending`, `pending`, `error_code`, `safe_reply`) when the state is paused, ended, or interrupted/cancelled with a stored outcome.

**Route 4 — resume**

15. Find the run as step 14. When `settings.approvers_configured` and `principal.role != Operator` → 403 `AUTH_FORBIDDEN` (ADR-097 item 4: approvals go to the configured approvers; operators may still resume, as in wrightrun). `principal.approver` is not consulted here: an approver decides on the Approval Service's card in chat or on a desktop, where the rule barring the asker from approving their own outbound action is applied; the resume route carries no such check, so an approver flag does not open it (SA-RECON-01). Parse and check *Resume*; a `message` beside `approve` → 400 `SRV_BAD_REQUEST`.
16. State not paused → 409 `RUN_NOT_PAUSED` "The run is not paused.". `calls` = pending items of kind `tool-call`, `gates` = gates of `refused-ending` items. `len(decisions) != len(calls)` → 400 `SRV_DECISION_MISMATCH` "the run awaits <n> decisions and the request holds <m>". A `set_aside` name not in `gates` → 400 `SRV_UNKNOWN_GATE`.
16a. *(SA-CLOSE-02)* For the i-th pending tool call whose `approval_id` is `Some(id)`: `approvals.decide_by_operator(id, <Approve when decisions[i].type is approve, else Reject>, principal.subject)`, in pending order, before step 17. `APPR_OPERATOR_NOT_ALLOWED` → 409 `APPR_OPERATOR_NOT_ALLOWED` (approvers decide this card); `APPR_CARD_ALREADY_DECIDED` → 409 `SRV_APPROVAL_ALREADY_DECIDED` "this approval was already decided"; `APPR_CARD_CLOSED` → 409 `RUN_NOT_PAUSED`; `APPR_NOT_FOUND` → the item is resumed without a card decision and `approval_missing` is logged (WARN; `run_id`, `approval_id`); a store error → 503 `SRV_APPROVAL_UNAVAILABLE`. A decision already recorded for one card stays recorded when a later one fails; the run is then not resumed and the operator sends the resume again (an already-decided card answers 409, so the operator reads the run first).
17. `runs.resume(run_id, {"decisions": [...], "set_aside": [...]})`; `RUN_NOT_PAUSED` (a concurrent resume won) → 409. Log `run_resumed` (INFO; `subject`, `role`, `approved`, `rejected`, `set_aside`); decision messages are never logged. Answer 202 *Accepted*.

**Route 31 — an approver's open cards** (SA-CLOSE-01)

17a. Token and least role as every route; `principal.approver` false → 403 `AUTH_FORBIDDEN`. Query `state` must be `pending` (absent → `pending`; another value → 400 `SRV_BAD_REQUEST`). Read `approvals.list_pending(None, None)` page by page until 100 cards are kept or the pages end. Keep a card only when its `asked_by` is `ApprovalAsker::Run { run_id }` and `runs.get(run_id)` shows a starter other than the principal (`Caller { caller_id }` with `caller_id != principal.subject`, or any other starter) — the asker never sees their own run's card here *(chosen here; the Approval Service applies its asker rule again on the decision)*. Answer the shape the Hosted Agent Adapter reads (HostedAgentAdapter.md *ApproverCards*): `{"approvals": [{"approval_id", "run_id", "asked_by": <the run's started_by.asker when present, else its started_by.id>, "question": <card.question>, "scope": <card.where_line>, "asked_at": <card.created_at, RFC 3339>}]}`, at most 100 entries, oldest first. No button reference leaves the agent. A store error → 503 `SRV_APPROVAL_UNAVAILABLE`. Log `approvals_listed` (INFO; `subject`, `count`).

**Route 32 — an approver's decision** (SA-CLOSE-01)

17b. Token and least role; `principal.approver` false → 403 `AUTH_FORBIDDEN`. `approval_id` not a UUIDv7 → 404 `SRV_APPROVAL_NOT_FOUND`. Body (ceiling 8 KiB) `{"decision": "approve"|"reject", "note": string?}` with no other key; `note` 0–2,000 characters; else 400 `SRV_BAD_REQUEST`. The note is accepted for the approver's own record on the desktop and is neither stored nor logged here *(chosen here: the Approval Service keeps no note)*.
17c. `approvals.get(approval_id)`: `APPR_NOT_FOUND` → 404 `SRV_APPROVAL_NOT_FOUND`. `card_ref` = `card.buttons.approve` or `card.buttons.reject` by `decision`. `approvals.decide(ApprovalPress { card_ref, decision }, PressOrigin::Approver { subject: principal.subject, groups: principal.groups.clone() (SA-CLOSE-03), via: ApproverVia::Desktop })`. Errors: `APPR_NOT_AN_APPROVER`, `APPR_ASKER_MAY_NOT_APPROVE` → 403 with that code; `APPR_CARD_ALREADY_DECIDED` → 409 `SRV_APPROVAL_ALREADY_DECIDED`; `APPR_CARD_CLOSED` → 409 `APPR_CARD_CLOSED`; `APPR_CARD_REF_UNKNOWN`, `APPR_CARD_REF_MISMATCH` → 409 `SRV_APPROVAL_ALREADY_DECIDED` (the reference was spent); a store error → 503 `SRV_APPROVAL_UNAVAILABLE`. Success → 200 `{"approval_id", "state": "approved"|"rejected"}` from `ApprovalOutcome` (the shape HostedAgentAdapter.md reads). The Approval Service wakes the paused run itself; this route does not call `runs.resume`. Log `approval_decided_by_approver` (INFO; `subject`, `approval_id`, `state`); the note is never logged.

**Route 5 — event stream**

18. Find the run as step 14. Read `Last-Event-ID`: absent → 0; not a non-negative integer → 400 `SRV_BAD_REQUEST`.
19. Answer 200 with an axum `Sse` stream: loop — `runs.events_after(run_id, last, 500)`; send each event (its `seq` as `id`, its kind as `event`, data per *Public Interface*); `last = seq`; a sent state that is final → end the stream; else sleep 1 s. Send `: keep-alive` every 20 s (`axum::response::sse::KeepAlive`). After 3,600 s, end the stream. A datastore error while streaming → end the stream (the client reconnects with `Last-Event-ID`) and log `stream_ended` (INFO; `run_id`, `reason`). On opening, log `stream_opened` (INFO; `run_id`); no counter, because Telemetry's family list is closed and holds none for streams.

**Route 6 — cancel**

20. Find the run as step 14. `runs.cancel(run_id)`: `Cancelled(view)` → 202 `{"state": "cancelled"}`; `Requested(view)` → 202 `{"state": "executing"}`; `RUN_NOT_CANCELLABLE` → 409. Log `run_cancel_asked` (INFO; `run_id`, `subject`, `role`, `outcome`).

**Routes 7–10 — work folder** (wrightrun RuntimeServer steps 22–26)

21. `path = normalise_path("/" + path)` (`WF_INVALID_PATH` 400); route 9 with a path ending `/` → 400 `WF_INVALID_PATH`; `is_bundle_path` → 400 `WF_BUNDLE_PATH` before the body is read.
22. 7: `prefix = normalise_path(query prefix or "/")`; `work_folder.operator_list(prefix, cursor)`; times in `wire_time`. 8: `operator_read` → bytes; `X-Wrightrun-Modified-At` is the `modified_at` of the entry `operator_list(path, None)` returns for exactly that path (the read returns bytes only). 9: body ≤ 10,485,760 (else 413 `SRV_BODY_TOO_LARGE`); `operator_write`; log `work_folder_written` (INFO; `path`, `bytes`, `subject`); `record(WorkFolderChange { action: Write })`. 10: `operator_exists` false → 404 `WF_NOT_FOUND`; `operator_remove`; log `work_folder_removed`; `record(WorkFolderChange { action: Remove })`. An operator may remove a duty's `STOP` and `escalation.md` (the agent may not, ADR-098 item 3).

**Route 11 — intake** (wrightrun EventIntake steps 4–11)

23. Unknown `name` → 404 `INTAKE_UNKNOWN` (logged and counted with `connection="unknown"`).
24. Body above `max_body_bytes` (by `Content-Length` or while reading) → 413 `INTAKE_TOO_LARGE`.
25. Sender: when the intake has a secret and the request carries `signature_header` → `verify_signature(secret, body, header, timestamp header, timestamp_header.is_some(), now)`; false → 401 `INTAKE_UNAUTHENTICATED` (a failed signature is never followed by a token try). Else when `accept_token` → `verifier.verify` then `require_role(Caller)`: `AUTH_MISSING`/`AUTH_INVALID` → 401 `INTAKE_UNAUTHENTICATED`; `AUTH_NO_ROLE`/`AUTH_FORBIDDEN` → 403 `INTAKE_FORBIDDEN`; `AUTH_KEYS_UNAVAILABLE` → 503 `INTAKE_UNAVAILABLE`. Else → 401 `INTAKE_UNAUTHENTICATED`.
26. Text: not UTF-8, blank after trimming, or holding NUL → 400 `INTAKE_NOT_TEXT`.
27. `run_id` = a new UUIDv7. `controls.take_intake_place(name, run_id, max_runs_in_progress)`: false → 429 `INTAKE_BUSY` with `Retry-After: 30`.
28. `runs.enqueue(NewRun { started_by: Intake { intake: name }, scope_id: agent_wide, request: Command { command, arguments: [("arguments", text)] }, gathered_context: {}, thread_id: None, run_id: Some(run_id) })`. Any error in step 27 or 28 → log `intake_start_failed` (ERROR; `intake`, `error` = code) and 503 `INTAKE_START_FAILED`; a taken place stops counting after 60 s.
29. `record(IntakeAccepted { intake: name })`; log `intake_accepted` (INFO; `intake`, `thread_id`, `run_id`, `bytes`); 202. Every refusal: `record(IntakeRefused { intake: <name or "unknown">, code })` and log `intake_refused` (WARN; `intake`, `reason`). No header value, signature, token, secret or event content is logged, counted or answered.
30. `verify_signature`: signature absent or not starting `sha256=` → false. With a signed timestamp: it must match `^\d{1,12}$` and be within 300 s of now, else false; signed bytes = `<timestamp>.<body>`; else the body. Compute HMAC-SHA256 with the secret (`hmac` 0.12, `sandbox/sandbox-host/Cargo.toml:45`; `sha2` 0.10, `memory/Cargo.toml:44`), decode the hex after the prefix (lower-cased; invalid hex → false) and compare with `Mac::verify_slice` (constant time).

**Routes 12–27 — operator** (wrightrun OperatorRoutes steps 2–23)

31. `{scope}`: `parse_scope_text` (`ERASE_BAD_TARGET` → 400 `OPS_BAD_SCOPE`); `retention.scopes().resolve(address)`; none → 404 `OPS_NOT_FOUND` "no such scope". Settings and entries routes on `agent-wide` → 400 `OPS_SCOPE_NOT_ALLOWED`. Bodies: JSON object, schema checked → 400 `OPS_BAD_REQUEST` naming the key, never its value.
32. 12: `state` query values (repeatable) must be wire states; map to `RunRecordState` (`finished`, `ended-by-control`, `ended-at-limit`, `failed` → `Ended`, filtered further in this crate by end kind); `runs.list(states, cursor)`; render *RunRow* (`scope` = `scope_text` of `scopes.get(scope_id)`, or `"unknown"` when the scope row is gone; `pending` = count of pending items). Input, arguments, message and reply are not returned.
33. 13–15: `settings.all/get/set` with `SettingWriter::Operator { subject }`; route 13 pages the sorted declared names (cursor = one text part, the last name); `SET_UNKNOWN` 404, `SET_INVALID` 400, `SET_NOT_ALLOWED` 403, `SET_VERSION_CONFLICT` 409, `SET_UNAVAILABLE` 503. The store logs the change; this crate writes no line for it.
34. 16–17: `jobs.list(cursor)`; `jobs.request(kind, key, subject)` after checking: ingest key `<source>/<connection>/<chat identifier>` with (`source`, `connection`) ∈ `chat_history_sources`; retention key absent or `retention` → else 400 `OPS_BAD_REQUEST`. A `DeskError` passes its code: `JOB_BAD_KEY` 400, `JOB_REQUEST_CONFLICT` 409, `JOB_DB_UNAVAILABLE` 503. 201 when created, else 200. Log `job_requested` (INFO; `kind`, `key`, `created`, `subject`).
35. 18–20: `knowledge.notes/open/note_retire`; an item that is absent, not a note, or of another scope → 404 `OPS_NOT_FOUND`; `KNOW_NOTE_NOT_FOUND` 404. Log `note_retired`.
36. 21–24: `knowledge.entries/entry_add/entry_verify/entry_remove`; `open` checks the entry belongs to the path's scope (else 404 `OPS_NOT_FOUND`); `KNOW_UNKNOWN_SOURCE` 404, `KNOW_ENTRY_INVALID` 400, `KNOW_ENTRY_NOT_FOUND` 404. Logs `entry_added`, `entry_verified`, `entry_removed` (never the text).
37. 25: person → `connection` a slug, `user` matching `^[^\s\x00-\x1f\x7f]{1,128}$` (else 400 `OPS_BAD_REQUEST`) → `retention.erase_person(connection, user, dry_run, subject)`; scope → `parse_scope_text` (400 `OPS_BAD_SCOPE`) → `erase_scope`. Answer *ErasureReport*. `ERASE_BAD_TARGET` 400, `ERASE_INCOMPLETE` 500 (the eraser's text), `ERASE_UNAVAILABLE` 503. Not dry → log `erase_done` (WARN; `target`, `subject`, counts, `scope` for a scope); a person's identifier is never logged.
38. 26–27: for each duty (manifest order) `controls.lock_held(duty_lock_key(duty))`; route 27 with a duty not in `settings.duties` → 404 `OPS_NOT_FOUND`; `RULE_STATE_UNAVAILABLE` → 503.
39. Each operator route: `record(OperatorRequest { route, outcome: Done | Refused })`; refusals logged `operator_refused` (INFO; `route`, `code`).

**Routes 28–30 — health and metrics** (no token; SA-XC-08). `build_router` and `health_router` mount the same three handlers.

40. 28: 200 while the process serves. 29: `(deps.ready)().await` → 200, else 503 `SRV_NOT_READY` *(chosen here: wrightrun's HealthChecks code set is not in this workspace; a readiness refusal needs a code in the error form)*. 30: `let (body, media_type) = telemetry.render()`; answer `body` with `Content-Type: media_type` (OpenMetrics).

**`serve(router, port, stop)`**

41. Bind `0.0.0.0:<port>` with `axum_server::bind` and a `Handle`; a bind error → `PortUnavailable(port)` (the program exits 1 with `SRV_PORT_UNAVAILABLE`). HTTP/1.1 and HTTP/2 cleartext; TLS ends at the ingress or reverse proxy in front of the instance *(assumed here, as wrightrun: "the organisation's ingress ends TLS"; SA-HTTP-01 puts TLS 1.3 on the Remote API only)*. On `stop`: `handle.graceful_shutdown(Some(25 s))`; open event streams are ended; return `Ok(())` and log `server_stopped`. The server ending without `stop` → `Stopped`.


**`health_router(ready, telemetry)`**

42. Build an `axum::Router` holding routes 28, 29 and 30 only, with the step 40 handlers, the error form for a refusal and `SRV_NO_ROUTE` 404 for any other path. No token is checked and Caller Authentication is not called. A readiness check that has not resolved within 2 s counts as not ready (503 `SRV_NOT_READY`). The router is served with `serve`.

**`OperatorClient`** (wrightrun ServerClient, OperatorCommands.md steps 4–6)

43. `from_env`: `url = ORDIN_SERVER_URL`; unset or empty → `UrlMissing`. The address check: scheme `https`, or `http` with host `localhost` or `127.0.0.1`; no user information; no fragment; at most 2,048 characters; else `UrlRefused` (the address is not printed). `ORDIN_TOKEN_FILE` unset or empty, or the file unreadable, or its content empty after trimming white space → `TokenMissing`. Then `new(url, token, 30 s)`.
44. `new`: the same address check; build one `reqwest::Client` with rustls, redirects off, the timeout given, and the default header `Authorization: Bearer <token>`.
45. `call`: send one request and no second one. A transport error or a timeout → `Unreachable`. Status below 400 or in `accept` → `OperatorResponse`. 401 → `Unauthorised`. 403 → `Forbidden("the server refused the request, <code>: <error>")` when the body is in the error form, else `Forbidden("the token does not carry the operator role")`. Any other status → `Response("the server answered <status> <code>: <error>")`, or `Response("the server answered <status>")` when the body is not in the error form. No message holds the token or a request body.
46. `pages`: `call(GET, path, query + cursor)` repeatedly, appending `items`, until `next_cursor` is null; a body without `items` → `Response("the server answered 200 with no items")`.

## Data Model

N/A — this crate owns no table. Runs and threads are the Run Queue's; work-folder files the File Service's; settings the Settings Store's; notes and entries the Knowledge Service's; scopes and erasure records Retention and Erasure's; intake places and locks the Rules Engine's; job records the Job Runner's.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No or malformed `Authorization` | 401 | `AUTH_MISSING` | `{"error": "a bearer token is required", "code": "AUTH_MISSING"}` |
| Token fails verification | 401 | `AUTH_INVALID` | `{"error": "the request's token was not accepted", "code": "AUTH_INVALID"}` |
| Token with neither role | 403 | `AUTH_NO_ROLE` | `{"error": "the token carries no role of this agent", "code": "AUTH_NO_ROLE"}` |
| Caller on an operator route; caller resuming when approvers are configured | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| Identity provider keys unreadable | 503 | `AUTH_KEYS_UNAVAILABLE` | `{"error": "the identity provider's keys cannot be read; try again later", "code": "AUTH_KEYS_UNAVAILABLE"}` |
| Body above the route's ceiling (caller, work-folder routes) | 413 | `SRV_BODY_TOO_LARGE` | `{"error": "the body exceeds 1048576 bytes", "code": "SRV_BODY_TOO_LARGE"}` |
| Body not a JSON object or breaking *StartRun*/*Resume*; bad `Last-Event-ID` | 400 | `SRV_BAD_REQUEST` | `{"error": "give either command or message, not both", "code": "SRV_BAD_REQUEST"}` |
| Unknown command | 400 | `SRV_UNKNOWN_COMMAND` | `{"error": "unknown command; the commands are: cut-release, run-go-no-go", "code": "SRV_UNKNOWN_COMMAND"}` |
| Thread absent or another caller's | 404 | `SRV_THREAD_NOT_FOUND` | `{"error": "no thread with this identifier", "code": "SRV_THREAD_NOT_FOUND"}` |
| Run absent or another caller's | 404 | `SRV_RUN_NOT_FOUND` | `{"error": "no run with this identifier", "code": "SRV_RUN_NOT_FOUND"}` |
| Resume of a run that is not paused | 409 | `RUN_NOT_PAUSED` | `{"error": "The run is not paused.", "code": "RUN_NOT_PAUSED"}` |
| A run on a thread whose run has not ended | 409 | `RUN_THREAD_BUSY` | `{"error": "The thread has a run that has not ended.", "code": "RUN_THREAD_BUSY"}` |
| Cancel of a run that has ended | 409 | `RUN_NOT_CANCELLABLE` | `{"error": "The run has ended and cannot be cancelled.", "code": "RUN_NOT_CANCELLABLE"}` |
| Approver route without the approver flag | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| Approver not accepted by the Approval Service; asker approving their own outbound action | 403 | `APPR_NOT_AN_APPROVER` / `APPR_ASKER_MAY_NOT_APPROVE` | `{"error": "the person who asked may not approve this action", "code": "APPR_ASKER_MAY_NOT_APPROVE"}` |
| Approval absent | 404 | `SRV_APPROVAL_NOT_FOUND` | `{"error": "no approval with this identifier", "code": "SRV_APPROVAL_NOT_FOUND"}` |
| Approval already decided, or its reference spent | 409 | `SRV_APPROVAL_ALREADY_DECIDED` | `{"error": "this approval was already decided", "code": "SRV_APPROVAL_ALREADY_DECIDED"}` |
| Card closed because its run ended | 409 | `APPR_CARD_CLOSED` | `{"error": "the card closed because its run ended", "code": "APPR_CARD_CLOSED"}` |
| Operator resume of a card the approvers decide | 409 | `APPR_OPERATOR_NOT_ALLOWED` | `{"error": "approvers decide this approval", "code": "APPR_OPERATOR_NOT_ALLOWED"}` |
| Approval Service store unreachable | 503 | `SRV_APPROVAL_UNAVAILABLE` | `{"error": "approvals cannot be read right now; try again later", "code": "SRV_APPROVAL_UNAVAILABLE"}` |
| Decisions count ≠ pending tool calls | 400 | `SRV_DECISION_MISMATCH` | `{"error": "the run awaits 1 decisions and the request holds 2", "code": "SRV_DECISION_MISMATCH"}` |
| `set_aside` names a gate the run was not paused for | 400 | `SRV_UNKNOWN_GATE` | `{"error": "the run has no refused ending for the gate work-reviewer", "code": "SRV_UNKNOWN_GATE"}` |
| Work-folder path invalid / bundle path | 400 | `WF_INVALID_PATH` / `WF_BUNDLE_PATH` | `{"error": "the path belongs to the bundle and is not in the work folder", "code": "WF_BUNDLE_PATH"}` |
| Work-folder file absent | 404 | `WF_NOT_FOUND` | `{"error": "no file exists at this path", "code": "WF_NOT_FOUND"}` |
| Work-folder write over 10 MiB refused by the store | 413 | `WF_TOO_LARGE` | `{"error": "a work-folder file holds at most 10485760 bytes", "code": "WF_TOO_LARGE"}` |
| Work-folder store unreachable | 503 | `WF_STORE_UNAVAILABLE` | `{"error": "the work folder cannot be reached", "code": "WF_STORE_UNAVAILABLE"}` |
| Unreadable cursor | 400 | `SRV_BAD_CURSOR` | `{"error": "the cursor is not one this listing returned", "code": "SRV_BAD_CURSOR"}` |
| Unknown intake | 404 | `INTAKE_UNKNOWN` | `{"error": "no such intake", "code": "INTAKE_UNKNOWN"}` |
| Intake body over its limit | 413 | `INTAKE_TOO_LARGE` | `{"error": "the event exceeds the intake's size limit", "code": "INTAKE_TOO_LARGE"}` |
| No valid token or signature; stale timestamp | 401 | `INTAKE_UNAUTHENTICATED` | `{"error": "the event carries no valid token or signature", "code": "INTAKE_UNAUTHENTICATED"}` |
| Valid token without a role | 403 | `INTAKE_FORBIDDEN` | `{"error": "the token may not start this intake's command", "code": "INTAKE_FORBIDDEN"}` |
| Keys unreadable for an intake token | 503 | `INTAKE_UNAVAILABLE` | `{"error": "the token cannot be verified now", "code": "INTAKE_UNAVAILABLE"}` |
| Intake body empty, not UTF-8 or with NUL | 400 | `INTAKE_NOT_TEXT` | `{"error": "the event is not UTF-8 text", "code": "INTAKE_NOT_TEXT"}` |
| Intake at its runs-in-progress limit | 429, `Retry-After: 30` | `INTAKE_BUSY` | `{"error": "the intake has its limit of runs in progress; try again later", "code": "INTAKE_BUSY"}` |
| Place or run could not be recorded | 503 | `INTAKE_START_FAILED` | `{"error": "the run could not be recorded", "code": "INTAKE_START_FAILED"}` |
| Operator body over 65,536 bytes | 413 | `OPS_BODY_TOO_LARGE` | `{"error": "the body exceeds 65536 bytes", "code": "OPS_BODY_TOO_LARGE"}` |
| Operator body breaks its schema; bad `state`, key or name | 400 | `OPS_BAD_REQUEST` | `{"error": "the key dry_run is required", "code": "OPS_BAD_REQUEST"}` |
| `{scope}` of another form | 400 | `OPS_BAD_SCOPE` | `{"error": "a scope is agent-wide or <connection>:<channel>", "code": "OPS_BAD_SCOPE"}` |
| Settings or entries on the agent-wide scope | 400 | `OPS_SCOPE_NOT_ALLOWED` | `{"error": "the agent-wide scope holds no setting", "code": "OPS_SCOPE_NOT_ALLOWED"}` |
| Scope, note, entry or duty not found | 404 | `OPS_NOT_FOUND` | `{"error": "no such note in this scope", "code": "OPS_NOT_FOUND"}` |
| Settings Store refusals | 404 / 400 / 403 / 409 / 503 | `SET_UNKNOWN` / `SET_INVALID` / `SET_NOT_ALLOWED` / `SET_VERSION_CONFLICT` / `SET_UNAVAILABLE` | the store's sentence, e.g. `{"error": "the list was changed since it was read; read it again", "code": "SET_VERSION_CONFLICT"}` |
| Knowledge refusals | 404 / 400 / 404 / 404 | `KNOW_UNKNOWN_SOURCE` / `KNOW_ENTRY_INVALID` / `KNOW_ENTRY_NOT_FOUND` / `KNOW_NOTE_NOT_FOUND` | the service's sentence |
| Job refusals | 400 / 409 / 503 | `JOB_BAD_KEY` / `JOB_REQUEST_CONFLICT` / `JOB_DB_UNAVAILABLE` | the runner's sentence |
| Erasure target invalid / stopped part-way / store away | 400 / 500 / 503 | `ERASE_BAD_TARGET` / `ERASE_INCOMPLETE` / `ERASE_UNAVAILABLE` | `{"error": "the erasure stopped at the step \"runs\" after 412 knowledge items and 37 chat reply records; repeat it to finish", "code": "ERASE_INCOMPLETE"}` |
| Duty lock state unreadable | 503 | `RULE_STATE_UNAVAILABLE` | `{"error": "the control state could not be read or written", "code": "RULE_STATE_UNAVAILABLE"}` |
| Run Queue datastore away | 503 | `DB_UNAVAILABLE` | `{"error": "The runtime database did not answer.", "code": "DB_UNAVAILABLE"}` |
| Instance not ready | 503 | `SRV_NOT_READY` | `{"error": "the instance is not ready", "code": "SRV_NOT_READY"}` |
| No route has this path | 404 | `SRV_NO_ROUTE` | `{"error": "no such route", "code": "SRV_NO_ROUTE"}` |
| Route does not accept the method | 405, `Allow` | `SRV_METHOD_NOT_ALLOWED` | `{"error": "the route does not accept this method", "code": "SRV_METHOD_NOT_ALLOWED"}` |
| A request with an `Origin` header; more than 100 open requests; another framework refusal | 403 / 503 / its status | `SRV_REFUSED` | `{"error": "the request was refused", "code": "SRV_REFUSED"}` |
| Any unnamed failure or panic | 500 | `SRV_INTERNAL` | `{"error": "the server could not answer the request", "code": "SRV_INTERNAL"}` |
| A route missing from the role table, at build | exit 1 | `AUTH_ROUTE_UNMAPPED` | `error[AUTH_ROUTE_UNMAPPED]: no role is stated for this route (GET /operator/unlisted)` |
| Two routes for one method and path, at build | exit 1 | `SRV_ROUTE_CONFLICT` | `error[SRV_ROUTE_CONFLICT]: two routes answer POST /v1/runs` |
| An intake secret absent or under 32 characters, at build | exit 1 | `INTAKE_SECRET_MISSING` / `INTAKE_SECRET_INVALID` (*chosen here*) | `error[INTAKE_SECRET_INVALID]: the intake release-candidate's secret must hold at least 32 characters` |
| Port cannot be opened | exit 1 | `SRV_PORT_UNAVAILABLE` | `error[SRV_PORT_UNAVAILABLE]: the port 8080 cannot be opened (ORDIN_PORT)` |
| Server ended without being told to stop | exit 1 | `SRV_STOPPED` | `error[SRV_STOPPED]: the server stopped without being told to` |
| Operator client: `ORDIN_SERVER_URL` unset, empty or a refused address | exit 1 | `SRV_URL_MISSING` | `error[SRV_URL_MISSING]: the environment variable ORDIN_SERVER_URL is not set` |
| Operator client: `ORDIN_TOKEN_FILE` unset, unreadable or empty | exit 1 | `SRV_TOKEN_MISSING` | `error[SRV_TOKEN_MISSING]: the environment variable ORDIN_TOKEN_FILE is not set or names no readable, non-empty file` |
| Operator client: transport error or timeout | exit 1 | `SRV_UNREACHABLE` | `error[SRV_UNREACHABLE]: the server could not be reached` |
| Operator client: the server answers 401 | exit 1 | `SRV_UNAUTHORISED` | `error[SRV_UNAUTHORISED]: the server refused the token` |
| Operator client: the server answers 403 | exit 1 | `SRV_FORBIDDEN` | `error[SRV_FORBIDDEN]: the server refused the request, AUTH_FORBIDDEN: the token's role does not permit this request` |
| Operator client: any other status of 400 or above not in `accept` | exit 1 | `SRV_RESPONSE` | `error[SRV_RESPONSE]: the server answered 409 RUN_NOT_PAUSED: The run is not paused.` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Caller API

  Background:
    Given the router built with a SQLite Run Queue, a Settings Store, in-memory doubles of the File Service work folder, the Knowledge Service index, the Rules Engine and JobDesk, a Retention on the same datastore, and an identity provider stub
    And caller tokens for "bot-a" and "bot-b", an operator token for "ops-1"
    And the commands "run-go-no-go" and "cut-release", the duty "watch-readiness", and the intake "release-candidate" (command run-go-no-go, secret of 40 characters, timestamp header X-Wrightrun-Timestamp, max-runs-in-progress 2)

  Scenario: Happy path — a caller starts a run and reads it
    When POST /v1/runs is sent by "bot-a" with {"command": "run-go-no-go", "arguments": "RC-12"}
    Then the answer is 202 with thread_id, run_id and state "queued"
    And the Run Queue holds a record started by Caller "bot-a" with the agent-wide scope
    When GET /v1/runs/<run_id> is sent by "bot-a"
    Then the answer is 200 with state "queued", started_by {"kind":"caller","id":"bot-a","asker":null} and result null

  Scenario: Error path — no token
    When POST /v1/runs is sent with no Authorization header
    Then the answer is 401 with code AUTH_MISSING and no record exists

  Scenario: Error path — a caller cannot read or continue another caller's run
    Given "bot-a" started run R on thread T
    When GET /v1/runs/R is sent by "bot-b"
    Then the answer is 404 with code SRV_RUN_NOT_FOUND
    When POST /v1/threads/T/runs is sent by "bot-b"
    Then the answer is 404 with code SRV_THREAD_NOT_FOUND, the same body as for an unknown thread

  Scenario: Error path — a thread whose run has not ended
    Given "bot-a"'s run on thread T is paused
    When POST /v1/threads/T/runs is sent by "bot-a"
    Then the answer is 409 with code RUN_THREAD_BUSY

  Scenario: Happy path — an operator resumes a paused intake run
    Given a run R started by Intake "release-candidate", paused with one pending tool-call "create_ticket"
    When POST /v1/runs/R/resume is sent by "ops-1" with {"decisions": [{"type": "approve"}]}
    Then the answer is 202 and R is queued with that decision

  Scenario: Happy path — an operator's resume decides the card too
    Given approvers are configured with no users and no groups
    And run R is paused with one pending tool call whose approval_id is A
    When POST /v1/runs/R/resume is sent by "ops-1" with {"decisions": [{"type": "approve"}]}
    Then decide_by_operator(A, Approve, "ops-1") is called before R is queued
    And the answer is 202

  Scenario: Error path — an operator resume when approvers decide the card
    Given approvers are configured with users ["u-7"] and run R paused with a pending tool call carrying approval_id A
    When POST /v1/runs/R/resume is sent by "ops-1" with one approve decision
    Then the answer is 409 with code APPR_OPERATOR_NOT_ALLOWED and R stays paused

  Scenario: Happy path — an approver lists and decides a card from a desktop
    Given "u-7" is an approver and approval A is pending
    When GET /v1/approvals?state=pending is sent by "u-7"
    Then the answer is 200 and approvals holds one entry with approval_id A, its run_id, question, scope and asked_at, and no button reference
    When POST /v1/approvals/A/decision is sent by "u-7" with {"decision": "approve"}
    Then the answer is 200 with body {"approval_id": A, "state": "approved"}

  Scenario: Happy path — an approver named only by group decides a card (SA-CLOSE-03)
    Given approvers are configured with groups ["leads"] and no users
    And "u-9"'s token carries the group "leads" under the configured group claim, and approval A is pending
    When POST /v1/approvals/A/decision is sent by "u-9" with {"decision": "reject"}
    Then decide is called with PressOrigin::Approver { subject "u-9", groups ["leads"], via Desktop }
    And the answer is 200 with body {"approval_id": A, "state": "rejected"}

  Scenario: Error path — a caller who is not an approver
    When GET /v1/approvals?state=pending is sent by "bot-a", whose token is not an approver's
    Then the answer is 403 with code AUTH_FORBIDDEN

  Scenario: Edge case — two approvers press at once
    Given approval A is pending and "u-7" and "u-8" are approvers
    When both send POST /v1/approvals/A/decision at the same instant
    Then one answer is 200 and the other is 409 with code SRV_APPROVAL_ALREADY_DECIDED

  Scenario: Error path — a caller resumes when approvers are configured
    Given approvers_configured is true and "bot-a"'s run is paused
    When POST /v1/runs/<run_id>/resume is sent by "bot-a"
    Then the answer is 403 with code AUTH_FORBIDDEN

  Scenario: Error path — wrong number of decisions
    Given "bot-a"'s run paused with one pending tool call
    When resume is sent with two approve decisions
    Then the answer is 400 with code SRV_DECISION_MISMATCH

  Scenario: Edge case — twenty resumes at the same instant
    Given "bot-a"'s run paused with one pending tool call
    When 20 resume requests are sent at once
    Then exactly one answer is 202 and nineteen are 409 with code RUN_NOT_PAUSED

  Scenario: The event stream follows a run and resumes with Last-Event-ID
    Given "bot-a"'s run R with events queued, executing and one progress line
    When GET /v1/runs/R/events is sent by "bot-a"
    Then the stream sends ids 1, 2, 3 with event names run_state_changed, run_state_changed, run_progress
    When the client reconnects with Last-Event-ID 2
    Then the first event sent has id 3
    When R ends finished
    Then the stream sends the state "finished" and closes

  Scenario: Cancel a queued run and an executing run
    Given "bot-a"'s queued run Q and executing run E
    When POST /v1/runs/Q/cancel is sent by "bot-a"
    Then the answer is 202 with state "cancelled"
    When POST /v1/runs/E/cancel is sent by "bot-a"
    Then the answer is 202 with state "executing" and the next renewal of E returns CancelRequested

  Scenario: Error path — cancel of an ended run
    Given "bot-a"'s run ended finished
    When POST /v1/runs/<run_id>/cancel is sent
    Then the answer is 409 with code RUN_NOT_CANCELLABLE

  Scenario: Error path — command and message together
    When POST /v1/runs is sent with {"command": "run-go-no-go", "message": "hello"}
    Then the answer is 400 with code SRV_BAD_REQUEST and the text holds neither "hello" nor "run-go-no-go"

  Scenario: Error path — an unknown command lists the known ones
    When POST /v1/runs is sent with {"command": "run-go-nogo"}
    Then the answer is 400 with code SRV_UNKNOWN_COMMAND and the text lists cut-release, run-go-no-go

  Scenario: Error path — a body above the ceiling
    When POST /v1/runs is sent with 1048577 bytes
    Then the answer is 413 with code SRV_BODY_TOO_LARGE

  Scenario: Happy path — a signed intake event starts its command
    When POST /intakes/release-candidate is sent with body "RC-12", a current timestamp and a valid signature
    Then the answer is 202 with intake, thread_id and run_id
    And the Run Queue holds a run started by Intake "release-candidate" with argument "RC-12"

  Scenario: Error path — an intake event with a bad signature
    When the same event is sent with a signature of another secret
    Then the answer is 401 with code INTAKE_UNAUTHENTICATED and no run is recorded
    And no log line holds the signature or "RC-12"

  Scenario: Error path — an intake at its limit
    Given RulesEngine::take_intake_place returns false
    When a valid event is sent
    Then the answer is 429 with Retry-After 30 and code INTAKE_BUSY

  Scenario: Error path — an intake's shared secret is not a token
    When POST /v1/runs is sent with "Authorization: Bearer <the intake secret>"
    Then the answer is 401 with code AUTH_INVALID

  Scenario: Happy path — an operator writes, reads and removes a work-folder file
    When PUT /work-folder/files/plans/rc-12.md is sent by "ops-1" with "# Plan"
    Then the answer is 200 with {"path": "/plans/rc-12.md", "size_bytes": 6}
    When DELETE /work-folder/files/plans/rc-12.md is sent
    Then the answer is 200 and a GET answers 404 with code WF_NOT_FOUND

  Scenario: Error path — a caller on a work-folder route
    When DELETE /work-folder/files/plans/rc-12.md is sent by "bot-a"
    Then the answer is 403 with code AUTH_FORBIDDEN

  Scenario: Operator routes — settings of a channel and refusal for the agent-wide scope
    Given the channel scope "team-slack:C0123" is enabled
    When PUT /operator/scopes/team-slack:C0123/settings/search-results is sent with {"value": 8}
    Then the answer is 200 with value 8
    When GET /operator/scopes/agent-wide/settings is sent
    Then the answer is 400 with code OPS_SCOPE_NOT_ALLOWED

  Scenario: Operator routes — erase dry run lists what is covered
    When POST /operator/erase is sent with {"target": "person", "connection": "team-slack", "user": "U07QK2M4P", "dry_run": true}
    Then the answer is 200 with counts including memory_items, 4 covers lines and 8 does_not_cover lines
    And no log line holds "U07QK2M4P"

  Scenario: Error path — an erase request without dry_run
    When POST /operator/erase is sent with {"target": "scope", "scope": "team-slack:C0123"}
    Then the answer is 400 with code OPS_BAD_REQUEST

  Scenario: Health and metrics need no token
    When GET /healthz, GET /readyz and GET /metrics are sent with no Authorization header
    Then each answer is 200

  Scenario: An instance in the runs role serves the health router only
    Given health_router(ready, telemetry) is served and ready resolves to false
    When GET /readyz is sent
    Then the answer is 503 with code SRV_NOT_READY
    And GET /v1/runs/<run_id> answers 404 with code SRV_NO_ROUTE

  Scenario Outline: The operator client's refusals
    Given <condition>
    When the operator command "ordin-server agent runs list" runs
    Then it exits 1 with code <code> and standard error holds no token

    Examples:
      | condition                                                  | code              |
      | ORDIN_SERVER_URL is unset                                  | SRV_URL_MISSING   |
      | ORDIN_SERVER_URL is "http://agents.example.org"            | SRV_URL_MISSING   |
      | ORDIN_TOKEN_FILE names an empty file                       | SRV_TOKEN_MISSING |
      | the server answers 401                                     | SRV_UNAUTHORISED  |
      | the server answers 403 with code AUTH_FORBIDDEN            | SRV_FORBIDDEN     |

  Scenario: The operator client reads every page
    Given the server holds 230 runs
    When OperatorClient::pages("/operator/runs", []) is called
    Then it returns 230 items after three requests

  Scenario: Error path — an unknown route answers in the error form
    When GET /v2/runs is sent
    Then the answer is 404 with {"error": "no such route", "code": "SRV_NO_ROUTE"}

  Scenario: Error path — a browser origin is refused
    When GET /v1/runs/<run_id> is sent with Origin "https://example.org"
    Then the answer is 403 with code SRV_REFUSED and no CORS header

  Scenario: Error path — a route with no stated role stops the start
    Given a router built with an extra route "GET /operator/unlisted"
    Then build_router fails with code AUTH_ROUTE_UNMAPPED

  Scenario: Error path — the Run Queue's datastore is away
    Given the datastore is closed
    When POST /v1/runs is sent with a valid token and body
    Then the answer is 503 with code DB_UNAVAILABLE and no log line holds a connection string

  Scenario: The server stops when told to
    Given serve runs on a free port with one request open
    When stop is cancelled
    Then the open request receives its answer, new connections are refused, and serve returns within 25 seconds
```

## Performance / Security / Observability

- **Performance targets:** with keys cached, p95 `POST /v1/runs` ≤ 100 ms and `GET /v1/runs/{run_id}` ≤ 50 ms at 200 requests/s for one 1-CPU requests instance (wrightrun targets); an intake event of 64 KiB with a signature ≤ 100 ms p95 at 50 events/s; an operator listing of 100 rows ≤ 200 ms p95, a single operator read/write ≤ 100 ms; a work-folder write of 10 MiB ≤ 2 s. At most 100 open requests per instance (above that, 503); bodies held at once bounded by 100 × 10 MiB; the Helm values' memory request for the requests role is 512 MiB (wrightrun). An open event stream costs one `events_after` statement per second; at 100 open streams, 100 statements/s. `serve` returns within 25 s of stop.
- **Security:** every route but intakes, health and metrics checks a token and its least role before reading a body (ADR-098 item 6). A caller reads, follows, resumes, cancels and continues only what it started; another caller's record and a missing record give the same 404. Resume is operator-only when approvers are configured (ADR-097 item 4). Intake events are authenticated by a token or a constant-time HMAC check; an intake starts one fixed command and can read nothing. Bodies are bounded on every route; JSON bodies refuse unknown keys (SA-XC-12). No CORS; a request with `Origin` is refused (SA-XC-11). No request can name a tool's arguments: approve runs the call unchanged, reject stops it. Error texts and log lines never hold a token, a body, file content, an argument, a decision message, an intake secret or signature, or a chat user's identifier; a work-folder path and an operator's subject are logged.
- **Observability:** agent profile only. Counters (names unchanged, SA-OBS-02): `wrightrun_http_requests_total{route, status}`, `wrightrun_http_refusals_total{code}`, `wrightrun_work_folder_changes_total{action}`, `wrightrun_intake_events_accepted_total{connection}`, `wrightrun_intake_events_refused_total{connection, reason}`, `wrightrun_operator_requests_total{route, outcome}`, each through `Telemetry::record` (families 40, 41, 42, 7, 8, 43). Log events (target `ordin_caller_api`): `server_routes_built`, `server_stopped`, `run_accepted`, `run_resumed`, `run_cancel_asked`, `stream_opened`, `stream_ended`, `work_folder_written`, `work_folder_removed`, `intake_accepted`, `intake_refused` (WARN), `intake_start_failed` (ERROR), `job_requested`, `note_retired`, `entry_added`, `entry_verified`, `entry_removed`, `erase_done` (WARN), `operator_refused`, `request_refused`, `server_error` (ERROR). No trace span from this crate (wrightrun ADR-013; spans are fixed in Phase 4).

## Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (open question 6: "the agent's owners, through the agent's management screen, scope by scope"). Owner of the commands: `components/MemoryService.md` (amendment January 2026: the `memory.*` commands with `scope_id`, `MemoryPlace::Scope`). Counterparts: `components/CallerAuthentication.md` (amendment January 2026: the route's least role), `components/OrdinServer.md` (amendment January 2026: the `MemoryDesk` implementation), `components/ScreensModule.md` (amendment January 2026, step 9: the desktop's hosted-agent section that calls this route).
> Every rule below adds to the sections above. Agent profile only, as the whole crate.

### Purpose (addition)

A hosted agent's owners — its operators — see and govern its memory scope by scope from their ordin desktop: the things, facts and links of each channel, direct chat and the agent-wide scope, with the same actions the person has on the desktop except **Remember everywhere**. Channel members do not reach this route; they see **What I used** on answers in their channel (ChannelGateway.md amendment January 2026).

### Approach (addition)

One route carries the Memory Service's person commands, as the Remote API carries Service API commands on the personal profile: `POST /operator/memory/{verb}` with the command's JSON input as the body and its result as the answer, so the command list stays the Memory Service's and nothing is restated here. The crate reaches the Memory Service through a trait it owns, `MemoryDesk`, implemented by the program (the pattern of `JobDesk`), so the crate gains no dependency edge. Rejected: *one REST route per command* — thirteen routes restating a schema the Memory Service already owns; *serving the personal profile's Remote API in the agent profile* — the agent profile's callers and operators already authenticate here with the identity provider, and a second listener would need a second key system.

Assumptions (*chosen here*):

- **SA-CAPI-AMEM-01** The body's `scope_id` must name a scope the agent's `ScopeRegistry` resolves (enabled or disabled, not erased); an erased or unknown scope answers 404 `OPS_NOT_FOUND` before the Memory Service is called.
- **SA-CAPI-AMEM-02** Every successful call on this route is journalled `memory_operator_call {verb, scope_id, operator_subject_digest}` (the subject as its SHA-256 hex, never the subject), as the other operator routes record through `record(OperatorRequest …)`.

### Shared Context (additions)

```rust
/// The memory route's view of the Memory Service, implemented by the agent-profile program.
#[async_trait::async_trait]
pub trait MemoryDesk: Send + Sync {
    /// `verb` is one of MEMORY_VERBS; `input` is the command's JSON input (with `scope_id`).
    /// Returns the command's JSON result, or the Memory Service's error.
    async fn call(&self, verb: &str, input: serde_json::Value) -> Result<serde_json::Value, MemoryDeskError>;
}
pub struct MemoryDeskError { pub code: &'static str, pub message: String /* the Memory Service's sentence */, pub kind: MemoryDeskErrorKind }
pub enum MemoryDeskErrorKind { Invalid, Refused, NotFound, Unavailable, Internal }   // MemoryError's variants
pub const MEMORY_VERBS: &[&str] = &["things", "thing", "fact_history", "fact_origins", "keep", "used",
    "remembered_in_run", "correct_link", "forget_link", "thing_count", "forget_thing", "same_as", "forget_count"];
```

`CallerApiDeps` gains `pub memory: std::sync::Arc<dyn MemoryDesk>`.

### Public Interface (additions)

| # | Method and path | Name | Least role | Ownership | Success |
|---|---|---|---|---|---|
| 33 | `POST /operator/memory/{verb}` | `operator.memory` | operator | — | 200 the command's result (`ThingPage`, `ThingDetail`, `FactHistoryView`, `OriginsView`, `FactView`, `MemoryUsedView`, `RememberedInRun`, `LinkView`, `LinkForgetReceipt`, `ThingCount`, `ThingForgetReceipt`, `ThingView` or `ForgetCount`, as the Memory Service's command table gives for `memory.<verb>`) |

Example: `POST /operator/memory/things` with `{"scope_id":"0192b7a0-5c1d-7e2f-8a3b-4c5d6e7f8091","search":"borealis"}` → `200 {"items":[{"id":"entity:0192b7a1-…","name":"Borealis","other_names":[],"fact_count":12,"link_count":4}],"next_cursor":null}`.

| Route | Called by |
|---|---|
| Route 33 | an owner's ordin desktop that reaches the hosted agent by role (the hosted-agent section of ScreensModule.md amendment January 2026, step 9) |

### Internal Logic (additions)

40. Route 33:
    1. Token and role by `check_request` (operator; a caller → 403 `AUTH_FORBIDDEN`).
    2. `verb` not in `MEMORY_VERBS` → 404 `SRV_NO_ROUTE` (the existing unknown-route answer).
    3. Body ≤ 65,536 bytes (else 413 `OPS_BODY_TOO_LARGE`); JSON object with a `scope_id` string that is a UUID (else 400 `OPS_BAD_REQUEST`); a `workspace_id` key → 400 `OPS_BAD_REQUEST` (the agent profile has no workspace).
    4. `scopes().resolve` of the scope (SA-CAPI-AMEM-01) → unknown or erased → 404 `OPS_NOT_FOUND`.
    5. `memory.call(verb, body)`. `Ok(v)` → 200 `v`, then `record(OperatorRequest { route: "operator.memory", outcome: Done })` and the journal entry of SA-CAPI-AMEM-02. `Err(e)` → status by `e.kind`: `Invalid` 400, `Refused` 409, `NotFound` 404, `Unavailable` 503 (with `Retry-After: 5`), `Internal` 500; body `{"error": e.message, "code": e.code}`; `record(… Refused)`.

### Data Model

N/A — no table; the journal entry is the Journal's.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Body is not an object with a UUID `scope_id`, or carries `workspace_id` (existing code) | 400 | `OPS_BAD_REQUEST` | `{"error": "the body needs a scope_id and no workspace_id", "code": "OPS_BAD_REQUEST"}` |
| `scope_id` names no scope of this agent, or an erased one (existing code) | 404 | `OPS_NOT_FOUND` | `{"error": "no such scope", "code": "OPS_NOT_FOUND"}` |
| A caller (not an owner) on route 33 (existing code) | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| The Memory Service refuses the command | 400 / 404 / 409 | the Memory Service's `MEM_*` code | `{"error": "<the Memory Service's sentence>", "code": "<code>"}` |
| The Memory Service is unavailable | 503 | `MEM_STORE_UNAVAILABLE` / `MEM_STORE_TIMEOUT` | as above, with `Retry-After: 5` |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Caller API — owners' memory route

  Background:
    Given the router built with a MemoryDesk double and scopes C (channel), D (direct chat) and A (agent-wide)

  Scenario: Happy path — an operator lists a scope's things
    Given an operator token
    When POST /operator/memory/things is sent with {"scope_id": C}
    Then the MemoryDesk is called with verb "things" and that body
    And the answer is 200 with the MemoryDesk's result

  Scenario: Error — a caller cannot reach the memory route
    Given a caller token
    When POST /operator/memory/things is sent
    Then the answer is 403 AUTH_FORBIDDEN and the MemoryDesk is not called

  Scenario: Error — an unknown verb
    When an operator sends POST /operator/memory/remember_everywhere
    Then the answer is 404 SRV_NO_ROUTE

  Scenario: Error — an erased scope
    Given scope D erased
    When an operator sends POST /operator/memory/things with {"scope_id": D}
    Then the answer is 404 OPS_NOT_FOUND

  Scenario: Error — a workspace id in the body
    When an operator sends POST /operator/memory/thing with {"scope_id": C, "workspace_id": "0192…", "id": "entity:…"}
    Then the answer is 400 OPS_BAD_REQUEST

  Scenario: Edge — the Memory Service's refusal is passed through
    Given the MemoryDesk answers Refused with code MEM_THING_COUNT_CHANGED
    When an operator sends forget_thing
    Then the answer is 409 with code MEM_THING_COUNT_CHANGED and the Memory Service's sentence
```

### Performance / Security / Observability (additions)

- **Performance:** the route adds ≤ 5 ms to the Memory Service's own targets (MemoryService.md amendment January 2026).
- **Security:** operator role only; the scope must exist; the body is passed as data to the Memory Service, which validates each command's input; **Remember everywhere** is not offered. Never logged: request or response bodies, statements, thing names.
- **Observability:** the existing `wrightrun_http_requests_total{route="operator.memory", status}`; journal `memory_operator_call {verb, scope_id, operator_subject_digest}`.

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents (ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-113 (only knowledge agents answer knowledge queries, on a route group of the Caller API port kept apart from the caller interface, with a read-only MCP endpoint on the same port, on unless the configuration turns it off). Owner of the handlers, the answers and the `KQ_` codes: `components/KnowledgeQueryServer.md`. Counterparts: `components/CallerAuthentication.md` (amendment January 2026 — knowledge agents: `LeastRole::Member`, `check_knowledge_request`, `KnowledgePrincipal`), `components/OrdinServer.md` (the program passes the server only when the configuration has a `knowledge-agent` section).
> Every rule below adds to the sections above. An agent whose configuration has no `knowledge-agent` section serves the 33 routes as before.

### Purpose (addition)

A knowledge agent serves six more routes on the same port: a query, its table of contents as JSON and as an OKF index, a short description of itself, a stream of repository changes for the front doors that use it, and a read-only MCP endpoint. Its operators gain one operator route, route 40, which returns what the reader last reported: the places read, documents waiting for the reading limit and glossary conflicts. This crate mounts them, checks the token before any body is read, applies the body ceiling and the error form, and counts them; what each answers is the Knowledge Query Server's.

### Approach (addition)

The Knowledge Query Server builds its own `axum::Router` for the six paths (`KnowledgeQueryServer::router`), and this crate merges it into its router behind one layer, `knowledge_layer`, that runs `check_knowledge_request` and places the `KnowledgePrincipal` in the request's extensions; route 37 (`GET /v1/knowledge/about`) has least role `none` and passes the layer with no token and no principal (CallerAuthentication.md SA-CAUTH-KA-05). The server's handlers read it there and apply the audience and front-door rules. One port and one listener keep a knowledge agent to the deployment wrightrun's routes already have (ADR-098 item 6). The new edge Caller API → Knowledge Query Server (phase 9 → 6) points downwards. Rejected: *a second listener for knowledge* — a second port, certificate and ingress rule for each knowledge agent; *restating the six routes' bodies here* — the server owns their schemas, as the Memory Service owns route 33's through `MemoryDesk`; *passing a `Principal` with an invented role* — a member of an audience is not a caller of the agent's runs (CallerAuthentication.md, amendment January 2026).

Assumptions (*chosen here*):

- **SA-CAPI-KA-01** Body ceilings: route 34 accepts at most 65,536 bytes and route 39 at most 262,144 bytes (an MCP `tools/call` with its JSON-RPC envelope); the other four are `GET` and read no body. A larger `Content-Length` answers 413 `SRV_BODY_TOO_LARGE` before the body is read, as step 7.
- **SA-CAPI-KA-02** `GET /mcp/knowledge` and `DELETE /mcp/knowledge` (the streamable HTTP transport's server-initiated stream and session end) are not served: the endpoint is stateless and read-only, so they answer the existing 405 `SRV_METHOD_NOT_ALLOWED` with `Allow: POST`. MCP clients that open no server stream work unchanged.
- **SA-CAPI-KA-03** Routes 34 and 38 hold a response open beyond the usual handler time (the **More found** stream until the second deadline; the changes stream until the client leaves). Both count against the concurrency limit of 100 of step 3 like any request; route 38 is additionally limited by the server (one stream per front-door subject).
- **SA-CAPI-KA-04** With `knowledge-agent.mcp.enabled: false` the path `/mcp/knowledge` is not mounted, so it answers as any unknown path: 404 `SRV_NO_ROUTE`.

### Shared Context (additions)

From the Knowledge Query Server (KnowledgeQueryServer.md), the one item this crate calls:

```rust
// crate ordin-knowledge-query-server
impl KnowledgeQueryServer {
    /// The six knowledge routes (34–39 below), or five when the MCP endpoint is turned off.
    /// Each handler reads `ordin_caller_auth::KnowledgePrincipal` from the request extensions.
    pub fn router(&self) -> axum::Router;
}
```

From Caller Authentication (amendment January 2026): `LeastRole::Member`, `check_knowledge_request(&Verifier, method, route_path, header) -> Result<KnowledgePrincipal, AuthError>`, `KnowledgePrincipal { subject, kind, audience_groups }`.

### Public Interface (additions)

`CallerApiDeps` gains:

```rust
    /// Some only when the agent configuration has a `knowledge-agent` section (OrdinServer).
    pub knowledge_query: Option<std::sync::Arc<ordin_knowledge_query_server::KnowledgeQueryServer>>,
```

| # | Method and path | Name | Least role | Ownership | Success |
|---|---|---|---|---|---|
| 34 | `POST /v1/knowledge/query` | `knowledge.query` | member | — | 200 *KnowledgeResult* (`application/json`), or 200 `text/event-stream` with the result, `more_found` events and `done` when the request sends `Accept: text/event-stream` |
| 35 | `GET /v1/knowledge/contents` | `knowledge.contents` | member | — | 200 *KnowledgeContents* |
| 36 | `GET /v1/knowledge/contents/index.md` | `knowledge.contents.index` | member | — | 200 `text/markdown; charset=utf-8` (an OKF 0.2 `index.md`) |
| 37 | `GET /v1/knowledge/about` | `knowledge.about` | none | — | 200 *KnowledgeAbout* (agent name, issuer, token audience, audiences' names, sources-record policy, repository count, as of; no content) |
| 38 | `GET /v1/knowledge/changes` | `knowledge.changes` | member | — | 200 `text/event-stream` of `repository_moved` events |
| 39 | `POST /mcp/knowledge` | `knowledge.mcp` | member | — | 200 a JSON-RPC response (MCP streamable HTTP, tools `knowledge_search` and `knowledge_contents`) |
| 40 | `GET /operator/knowledge` | `operator.knowledge` | operator | — | 200 *ReaderStatus* (KnowledgeReader.md; read through `KnowledgeQueryServer::reader_status()`), or 200 `null` before the reader's first report |

The bodies (*KnowledgeResult*, *KnowledgeContents*, *KnowledgeAbout*, the event forms and the MCP tool schemas) are the Knowledge Query Server's and phase 2C wire's; this crate does not restate them.

| Route | Called by |
|---|---|
| 34–36 | ordin desktops and personal servers through the Knowledge Query Client; other knowledge agents acting as front doors |
| 37 | the Knowledge Query Client before a person connects (no token) and on details |
| 38 | front doors that ask this agent as a proxy |
| 39 | any MCP client signed in with a company identity (a person's or a service's) |
| 40 | the operator screen under **Hosted agents** (ScreensModule.md amendment January 2026, step 10) |

### Internal Logic (additions)

`build_router` (changed): step 1 builds the 33 routes of the table and, when `deps.knowledge_query` is `Some`, merges `knowledge_query.router()` under `knowledge_layer`; every (method, path) of the merged router is checked with `required_role` as the other routes are, so a knowledge path missing from `ROUTE_ROLES` stops the start with `RouteUnmapped`. Step 5 logs `routes` = 33, 39 or 40 (39 when the MCP endpoint is off).

44. **Route 40** is an operator route with steps 6–8 as routes 12–28: `principal.role` must be operator (`AUTH_FORBIDDEN` otherwise); the handler calls `knowledge_query.reader_status()` and answers its value; `Err(KQ_STORE_UNAVAILABLE)` → 503 with that code (retryable). Mounted only when `deps.knowledge_query` is `Some`.

41. **`knowledge_layer`** (routes 34–39), in place of steps 6 and 7 for these routes:
    1. Route 37: no token is read and no principal is inserted; go to step 4 after the handler answers. Every other route: `principal = check_knowledge_request(verifier, method, <declared path>, Authorization)`; an `AuthError` → its status and code (Caller Authentication's error table). The token is verified before the body is read.
    2. Body ceiling by SA-CAPI-KA-01 → 413 `SRV_BODY_TOO_LARGE`.
    3. Insert `principal` into the request extensions and call the merged handler. The handler's answer, including its `KQ_` refusals in the error form `{"error", "code"}`, is passed through unchanged.
    4. Step 8 applies: `telemetry.record(TelemetryEvent::HttpRequest { route: <name>, status })`; on a refusal `HttpRefusal { code }` and `request_refused` (INFO; `code`, `method`). For a stream (34 with `text/event-stream`, 38) the record is made when the response head is sent.
42. Step 4 (an `Origin` header → 403 `SRV_REFUSED`) applies to routes 34–39: callers are programs, desktops and MCP clients, not browser pages.
43. `GET` or `DELETE` on `/mcp/knowledge` → 405 by SA-CAPI-KA-02.

Nothing in this crate reads a query, a statement or a table of contents; none is logged.

### Data Model

N/A — no table.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Route 34 or 39 body over its ceiling (existing code) | 413 | `SRV_BODY_TOO_LARGE` | `{"error": "the body exceeds 65536 bytes", "code": "SRV_BODY_TOO_LARGE"} (262144 on route 39)` |
| `GET` or `DELETE` on `/mcp/knowledge` (existing code) | 405, `Allow: POST` | `SRV_METHOD_NOT_ALLOWED` | `{"error": "the route does not accept this method", "code": "SRV_METHOD_NOT_ALLOWED"}` |
| A knowledge path on an agent without a `knowledge-agent` section, or `/mcp/knowledge` turned off (existing code) | 404 | `SRV_NO_ROUTE` | `{"error": "no such route", "code": "SRV_NO_ROUTE"}` |
| Token refused on routes 34–39 (Caller Authentication's codes) | 401 / 403 / 503 | `AUTH_MISSING`, `AUTH_INVALID`, `AUTH_FORBIDDEN`, `AUTH_KEYS_UNAVAILABLE` | Caller Authentication's bodies |
| The server refuses or fails (passed through) | the server's status | the server's `KQ_*` code, e.g. `KQ_NOT_IN_AUDIENCE` (403) | `{"error": "<the server's plain sentence>", "code": "<code>"}` |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Caller API — knowledge routes

  Background:
    Given the router built with a KnowledgeQueryServer double whose router serves routes 34–39

  Scenario: Happy path — a member's query reaches the server with its principal
    Given a token admitted by check_knowledge_request with audience_groups ["finance"]
    When POST /v1/knowledge/query is sent with a 2 KiB JSON body
    Then the server's handler receives a KnowledgePrincipal with audience_groups ["finance"]
    And the answer is the handler's 200 body, passed through unchanged

  Scenario: Error — no token on a knowledge route
    When GET /v1/knowledge/contents is sent without Authorization
    Then the answer is 401 AUTH_MISSING and the handler is not called

  Scenario: Error — a query body over 64 KiB
    When POST /v1/knowledge/query is sent with Content-Length 70000 and a valid token
    Then the answer is 413 SRV_BODY_TOO_LARGE and the body is not read

  Scenario: Edge — the server's refusal is passed through
    Given the handler answers 403 {"error": "You are not in this knowledge agent's audience.", "code": "KQ_NOT_IN_AUDIENCE"}
    When POST /mcp/knowledge is sent with a valid token
    Then the answer is that 403 body unchanged

  Scenario: Edge — the MCP endpoint turned off
    Given the configuration's knowledge-agent.mcp.enabled is false
    When POST /mcp/knowledge is sent with a valid token
    Then the answer is 404 SRV_NO_ROUTE

  Scenario: Edge — the about route answers with no token
    When GET /v1/knowledge/about is sent without Authorization
    Then the handler is called with no KnowledgePrincipal and its 200 body is passed through unchanged

  Scenario: Edge — an agent with no knowledge-agent section
    Given CallerApiDeps.knowledge_query is None
    When GET /v1/knowledge/about is sent
    Then the answer is 404 SRV_NO_ROUTE and server_routes_built logged routes = 33

  Scenario: Happy path — an operator reads the knowledge status
    Given the server's reader_status returns a status with 2 places, 14 documents unread and 1 glossary conflict
    When GET /operator/knowledge is sent with an operator token
    Then the answer is 200 with that status unchanged
    And with a caller token the answer is 403 AUTH_FORBIDDEN

  Scenario: Error — the server stream path is not served
    When GET /mcp/knowledge is sent with a valid token
    Then the answer is 405 SRV_METHOD_NOT_ALLOWED with Allow: POST
```

### Performance / Security / Observability (additions)

- **Performance:** the layer adds ≤ 2 ms p95 to the server's own targets (front-door overhead ≤ 50 ms p95, cached answer ≤ 50 ms p95; KnowledgeQueryServer.md).
- **Security:** every knowledge route but route 37 needs a verified identity-provider token (route 37 returns no content); the caller interface's ownership rules are untouched, and a knowledge principal never reaches a run route. No CORS. Never logged: request or response bodies, queries, statements, groups.
- **Observability:** the existing `wrightrun_http_requests_total{route, status}` with the six new route names; `server_routes_built` with the new count.

### Gaps

None.


## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (a person's ordin links their source-system accounts to a knowledge agent; GitHub and GitLab tell a knowledge agent when sharing changes; an operator can make the agent forget every remembered check). Owner of the handlers and the `KQ_` codes: `components/KnowledgeQueryServer.md` (amendment January 2026). Counterparts: `components/CallerAuthentication.md` (amendment January 2026: five route-table rows), `components/SourceAccess.md` (new: the notices' checks, `forget_all`).
> Every rule below adds to the sections above. An agent with no `knowledge-agent` section serves the 33 routes as before; a knowledge agent with no `notices` secrets serves routes 34–42 and 45.

### Purpose (addition)

A knowledge agent serves five more routes on the same port: linking and unlinking a member's source-system accounts (41, 42), receiving sharing-change notices from GitHub and from GitLab (43, 44), and an operator route to forget remembered checks (45). This crate mounts them — 41 to 44 behind the existing `knowledge_layer`, 45 as an operator route like route 40 — applies body ceilings and the error form, and counts them; what each answers is the Knowledge Query Server's.

### Approach (addition)

The Knowledge Query Server's router already holds every knowledge path and is merged behind `knowledge_layer`; routes 41–44 arrive the same way. Routes 43 and 44 pass the layer with no token and no principal, as route 37 does, because a repository host has no company token; the body is passed to the server, whose Source Access checks the host's signature or token. Route 45 is this crate's own operator route, as route 40 is, calling `KnowledgeQueryServer::forget_checks()`. Rejected: *a separate listener for webhooks* — a second port and ingress rule per knowledge agent, as rejected for the knowledge routes (amendment January 2026).

Assumptions (*chosen here*):

- **SA-CAPI-PD-01** Body ceilings: route 41 accepts at most 65,536 bytes (three tokens of at most 8,192 bytes with their JSON); routes 43 and 44 at most 1,048,576 bytes (a host's membership and repository events are under 64 KiB; the ceiling leaves room for large organisation payloads); routes 42 and 45 read no body.
- **SA-CAPI-PD-02** Step 4 (an `Origin` header → 403 `SRV_REFUSED`) applies to routes 41–45: callers are desktops, servers, repository hosts and the operator's tools, not browser pages.
- **SA-CAPI-PD-03** Route 39 passes the request header `Ordin-Delegation` through to the handler unchanged; it is never logged, and no layer of this crate reads it.

### Public Interface (additions)

| # | Method and path | Name | Least role | Ownership | Success |
|---|---|---|---|---|---|
| 41 | `POST /v1/knowledge/accounts` | `knowledge.link_accounts` | member | — | 200 `{ "linked": [{system, account_label}], "refused": [{system, code, reason}] }` |
| 42 | `DELETE /v1/knowledge/accounts/{system}` | `knowledge.unlink_account` | member | — | 200 `{}`, whether or not a link existed |
| 43 | `POST /v1/knowledge/notices/github` | `knowledge.notice_github` | none | — | 202 `{}`; mounted only when the GitHub notice secret is configured |
| 44 | `POST /v1/knowledge/notices/gitlab` | `knowledge.notice_gitlab` | none | — | 202 `{}`; mounted only when the GitLab notice token is configured |
| 45 | `POST /operator/knowledge/forget-checks` | `knowledge.forget_checks` | operator | — | 200 `{}` after `KnowledgeQueryServer::forget_checks()` |

| Route | Called by |
|---|---|
| 41, 42 | ordin desktops and personal servers through the Knowledge Query Client |
| 43 | GitHub webhook deliveries a repository's owner set up (events `member`, `membership`, `team_add`, `repository`) |
| 44 | GitLab webhook deliveries a project's or group's owner set up (member events) |
| 45 | the operator, from the server command `ordin-server knowledge forget-checks` (OrdinServer.md amendment January 2026) |

The bodies are the Knowledge Query Server's and phase 2C wire's; this crate does not restate them.

### Internal Logic (additions)

`build_router` (changed): step 5 logs `routes` = 33 with no knowledge agent; otherwise 40 (routes 1–40) + 2 (routes 41, 42) + 1 per mounted notice route + 1 (route 45), less 1 when the MCP endpoint is off — at least 42, at most 45. Every (method, path) of the merged router is still checked with `required_role`, so a route missing from `ROUTE_ROLES` stops the start with `RouteUnmapped`.

41 (changed). **`knowledge_layer`** now covers routes 34–39 and 41–44:
1. Routes 37, 43 and 44: no token is read and no principal is inserted. Routes 34–36, 38, 39, 41, 42: `check_knowledge_request` as before.
2. Body ceilings: SA-CAPI-KA-01 for 34 and 39; SA-CAPI-PD-01 for 41, 43 and 44 → 413 `SRV_BODY_TOO_LARGE`.
3. and 4. unchanged.

45. Routes 43 and 44 never call `check_knowledge_request` (CallerAuthentication.md amendment January 2026, step 20). A notice route not mounted answers as any unknown path: 404 `SRV_NO_ROUTE`.
46. Step 4 (`Origin` → 403) applies to routes 41–45 (SA-CAPI-PD-02).
47. **Route 45** is an operator route with steps 6–8 as route 40: `principal.role` must be operator (`AUTH_FORBIDDEN` otherwise); the handler calls `knowledge_query.forget_checks()` and answers 200 `{}`; journal entry `knowledge_checks_forgotten` (no content). Mounted only when `deps.knowledge_query` is `Some`; no body is read.

Nothing in this crate reads a token, an account, a notice body or the `Ordin-Delegation` header; none is logged.

### Data Model

N/A — no table.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Route 41 body over 65,536 bytes, or route 43/44 body over 1,048,576 bytes (existing code) | 413 | `SRV_BODY_TOO_LARGE` | `{"error": "the body exceeds 65536 bytes", "code": "SRV_BODY_TOO_LARGE"} (1048576 on routes 43 and 44)` |
| A notice route whose secret is not configured (existing code) | 404 | `SRV_NO_ROUTE` | `{"error": "no such route", "code": "SRV_NO_ROUTE"}` |
| Route 45 with a token that is not an operator's (existing code) | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| The server refuses a notice (passed through) | 401 | `KQ_NOTICE_REJECTED` | `{"error": "The notice could not be verified.", "code": "KQ_NOTICE_REJECTED"}` |
| The server refuses an account body (passed through) | 400 | `KQ_BAD_ACCOUNTS` | `{"error": "Give 1 to 3 accounts, one each for GitHub, GitLab or Confluence.", "code": "KQ_BAD_ACCOUNTS"}` |

No `SACC_` code reaches this crate: the Knowledge Query Server maps every Source Access error to a `KQ_` code or logs it.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Caller API — account, notice and forget-checks routes

  Background:
    Given the router built with a KnowledgeQueryServer double whose router serves routes 34–39 and 41–44

  Scenario: Happy path — a member links accounts
    Given a token admitted by check_knowledge_request with audience_groups ["procurement"]
    When POST /v1/knowledge/accounts is sent with a 3 KiB body
    Then the handler receives a KnowledgePrincipal and its 200 body is passed through unchanged

  Scenario: A notice needs no token
    When POST /v1/knowledge/notices/github is sent without Authorization
    Then the handler is called with no KnowledgePrincipal and its 202 body is passed through unchanged

  Scenario: A forged notice is refused by the server
    Given the handler answers 401 {"error": "The notice could not be verified.", "code": "KQ_NOTICE_REJECTED"}
    When POST /v1/knowledge/notices/gitlab is sent
    Then the answer is that 401 body unchanged

  Scenario: Notices off
    Given no notice secret is configured and the MCP endpoint is on
    When POST /v1/knowledge/notices/github is sent
    Then the answer is 404 SRV_NO_ROUTE
    And server_routes_built logged routes = 43

  Scenario: Every route mounted
    Given both notice secrets configured and the MCP endpoint on
    Then server_routes_built logged routes = 45

  Scenario: Error — an account body over 64 KiB
    When POST /v1/knowledge/accounts is sent with Content-Length 70000 and a valid token
    Then the answer is 413 SRV_BODY_TOO_LARGE and the body is not read

  Scenario: Error — no token on the account route
    When DELETE /v1/knowledge/accounts/github is sent without Authorization
    Then the answer is 401 AUTH_MISSING and the handler is not called

  Scenario: An operator makes the agent forget remembered checks
    When POST /operator/knowledge/forget-checks is sent with an operator token
    Then the answer is 200 {} and forget_checks was called once
    And with a caller token the answer is 403 AUTH_FORBIDDEN and forget_checks was not called

  Scenario: The delegation header is not logged
    Given a capture of log lines
    When POST /mcp/knowledge is sent with header Ordin-Delegation and a valid token
    Then the handler receives the header and no log line holds its value
```

### Performance / Security / Observability (additions)

- **Performance:** the layer adds ≤ 2 ms p95 to routes 41–44; route 45 answers within 10 ms p95.
- **Security:** routes 41 and 42 need a verified company token; routes 43 and 44 are verified by the host's signature or token in Source Access before anything changes; route 45 needs an operator token; no CORS. Never logged: account bodies, notice bodies, the delegation header.
- **Observability:** `wrightrun_http_requests_total{route, status}` with the five new route names (`knowledge.link_accounts`, `knowledge.unlink_account`, `knowledge.notice_github`, `knowledge.notice_gitlab`, `knowledge.forget_checks`); `server_routes_built` with the new count; journal entry `knowledge_checks_forgotten`.

### Gaps

None.

