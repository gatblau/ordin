# SPEC: Approval Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-approval` · **Build phase:** 2 · **Programs:** D P A
> **Dependencies:** Journal

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

## Purpose

The Approval Service turns every "ask a person" decision into an approval: a record of the exact action, its destination, the records it affects and its consequence, and the card the person decides on (02:658). It holds the asking run paused until a person approves or rejects, and gives the decision back to whichever component waits for it. It is the only producer of `ApprovalCard` (02:334, 11 §5.5): Ordin and agents can ask for an approval, but never write one. A pending approval survives a restart and is never approved by timeout (04:200–202, 11 §5.5). In the agent profile it applies the approver rules of ADR-097 item 4: a card goes to the configuration's approvers, the first to decide decides once, and the asker may not approve their own run's action that leaves the company (02:659).

## Approach

The service is a small state machine over one table per concern (`approvals`, `approval_buttons`), with one in-memory `tokio::sync::watch` channel per pending approval so that waiting components wake without polling. It writes every change through the Journal, and receives run-ended events back through the Journal's `EventSink` (2B-2 D1), so it depends on the Journal only. Error prefix: `APPR_` (2C). Rejected: a callback per caller registered at request time — callbacks do not survive a restart, and the Approval Service would then depend on its callers. Rejected: keeping buttons only in the Channel Gateway's `card_references` — desktop and local presses need single-use references too, and only the Approval Service can decide whether a press is valid.

## Shared Context

### Types from `ordin-types` (2C, copied; do not redefine)

```rust
// Identifiers: newtypes over UUIDv7, lowercase hyphenated (2C domain, Conventions).
pub struct ApprovalId(pub Uuid);  pub struct RunId(pub Uuid);  pub struct CrewRunId(pub Uuid);
pub struct WorkspaceId(pub Uuid); pub struct PlaceId(pub Uuid); pub struct ArtefactId(pub Uuid);
pub struct EmailDraftId(pub Uuid); pub struct BackendId(uuid::Uuid);
pub struct AgentInstanceId(pub Uuid);
/// RFC 3339, UTC, millisecond precision, `Z` suffix.
pub struct Timestamp(pub DateTime<Utc>);

// 2C wire §5 ApprovalCard
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalCard {
    pub approval_id: ApprovalId,
    pub backend: BackendId,
    pub workspace_id: WorkspaceId,
    pub asked_by: ApprovalAsker,
    pub question: String,          // "[Verb] [object]?"
    pub verb: String,              // approve button label
    pub where_line: String,
    pub affects: Vec<AffectedItem>,
    pub changes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ApprovalContent>,
    pub reject_style: RejectStyle,
    pub buttons: ApprovalButtons,
    pub created_at: Timestamp,
}
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectStyle { Reject, NotNow }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalButtons {
    pub approve: CardRef,
    pub reject: CardRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_details: Option<CardRef>,
}
/// 16 random bytes, base64url without padding, 22 characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardRef(String);

// 2C wire §5 ApprovalContent (with ContentRef, EmailApproval, EmailRecipient, EmailAttachmentLine)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalContent {
    Message { label: String, text: String, destination_note: Option<String> },
    Command { label: String, command: String, runs_in: String },
    Changes { label: String, change_set: ContentRef },
    Page { label: String, page: ContentRef },
    Files { label: String, files: Vec<ContentRef> },
    Email { label: String, email: EmailApproval },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentRef { Artefact { artefact_id: ArtefactId }, PlaceFile { place_id: PlaceId, relative_path: String } }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailApproval {
    pub draft_id: EmailDraftId, pub from: String,
    pub to: Vec<EmailRecipient>, pub cc: Vec<EmailRecipient>, pub bcc: Vec<EmailRecipient>,
    pub subject: String, pub body: String, pub attachments: Vec<EmailAttachmentLine>,
    #[serde(skip_serializing_if = "Option::is_none")] pub drafted_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub in_reply_to: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailRecipient { pub name: Option<String>, pub address: String, pub outside_company: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailAttachmentLine { pub file_name: String, pub comes_from: String, pub bytes: u64 }

// 2C wire §5 ApprovalPress (the input of the Service API command `approval.decide`)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApprovalPress { pub card_ref: CardRef, pub decision: PressDecision }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressDecision { Approve, Reject }

// 2C domain — entity Approval (owner: Approval Service)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Approval {
    pub id: ApprovalId,
    pub subject: ApprovalSubject,
    pub action: String,
    pub destination: String,
    pub affected_records: Vec<String>,
    pub consequence: String,
    pub state: ApprovalState,
    #[serde(skip_serializing_if = "Option::is_none")] pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub decided_at: Option<Timestamp>,
    pub created_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalSubject { Run { run_id: RunId }, CrewRound { crew_run_id: CrewRunId, round_number: u32 } }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState { Pending, Approved, Rejected, Expired }

// 2C wire §3 — what an *ask* proposes (produced by the Rules Engine)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActionProposal { pub action: String, pub destination: String, pub affects: Vec<AffectedItem>, pub consequence: String }

// 2C wire §1 ServiceEvent — the two variants this service emits
//   ApprovalRequested { card: ApprovalCard }
//   ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String }
// and the variant it consumes:
//   RunStateChanged { run_id: RunId, state: RunState }    (RunState ended states:
//   completed, failed, stopped_at_limit, cancelled, interrupted, abandoned, no_reply)
//   CrewRoundChanged { crew_run_id: CrewRunId, round: u32, state: CrewRunState }
//   (CrewRunState ended states: finished, escalated, stopped_at_limit, killed, failed)
```

Validation limits copied from 2C (LW-25 to LW-27, LD-44): `question` 1–120 characters ending in "?"; `verb` 1–32 characters, not "OK", "Yes" or "Continue"; `where_line` 1–200; `affects` 1–50 items, each label 1–200; `changes` 1–1,000; `Message.text` 1–100,000; `Command.command` 1–8,000; `Files.files` 1–100; `to + cc + bcc` 1 to the profile's maximum recipients (default 20). `Approval.action` ≤ 500, `destination` ≤ 2,048, `consequence` ≤ 2,000, `affected_records` ≤ 1,000. `CardRef`: 22 characters base64url. `ApprovalDecided.decided_on`: 1–80 characters.

Fixed card lines ("From ordin — built from the action, not written by an agent"; "Nothing is sent or changed outside ordin until you choose.") are not in the payload; the screens draw them from the message catalogue (2C wire §5, 11 §5.5). The `ApprovalContent.label` values are the fixed labels of 11 §5.5: "See the changes", "See the page", "See the files", "See the command", "See what will be sent".

### Journal crate (`ordin-journal`) — used, owned by the Journal writer

```rust
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
#[async_trait] pub trait EventSink: Send + Sync {
    fn name(&self) -> &'static str;
    async fn deliver(&self, event: &JournalEvent);
    async fn lagged(&self, next_seq: u64);
}
pub struct JournalEvent { pub backend: uuid::Uuid, pub seq: u64, pub at: Timestamp, pub kind: String, pub data: serde_json::Value }
#[async_trait] pub trait PruneParticipant: Send + Sync {
    fn module(&self) -> &'static str;
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure>;
}
pub struct PruneRequest { pub cutoff: Timestamp, pub runs_in_progress: Vec<RunId> }
pub struct PruneFailure { pub code: &'static str }
impl Journal {
    /// Appends one Service API event (kind from 2C `EventKind::as_str()`, data = the variant's
    /// fields without `kind`) and delivers it to every EventSink.
    pub async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>;
    pub fn register_prune_participant(&self, participant: Arc<dyn PruneParticipant>);
}
// 2C domain: pub struct Page<T> { pub items: Vec<T>, pub next_cursor: Option<Cursor> }
```

### Configuration

| Source | Key | Use |
|---|---|---|
| Agent configuration (agent profile, read by the program and passed in) | approvers (users, identity-provider groups); whether the asker may approve actions that stay inside the company (04:382–385) | `ApproverPolicy` |
| Program | the computer's display name (desktop) | `decided_on` for presses on this computer |
| 2D-4 | none | The service has no fixed constant of its own. |

## Public Interface

All names below are in crate `ordin-approval` and *chosen here* unless marked 2C.

```rust
pub struct ApprovalService { /* private */ }

#[derive(Debug, Clone)]
pub struct ApprovalConfig {
    /// This installation's backend identifier (2C BackendId), copied into every card.
    pub backend: BackendId,
    /// Shown as `decided_on` for a press made on this computer's own screens ("Asha's laptop").
    pub local_computer_name: String,
    /// Agent profile only; `None` on the desktop and the personal server.
    pub approvers: Option<ApproverPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApproverPolicy {
    /// Identity-provider subject identifiers of individual approvers.
    pub users: Vec<String>,
    /// Identity-provider group names; a member of any listed group is an approver.
    pub groups: Vec<String>,
    /// ADR-097 item 4: whether the asker may approve their own run's actions that stay inside the company.
    pub asker_may_approve_internal: bool,
}
// The agent configuration's approvers are `ApproversConfig` (Agent Configuration owns it). The
// agent-profile program maps it here: `users` = every `users[].subject` and every
// `users[].chat_user` (`<connection>/<user>`, the subject a chat press carries), `groups` =
// `groups`, `asker_may_approve_internal` unchanged; `group_claim` is Caller Authentication's (it
// fills `PressOrigin::Approver.groups`) and is not used here.

/// What an asking component hands over. Built by the Rules Engine from an `ActionProposal`, by the
/// Crew Orchestrator for "Ask me first" marks and person-decided conditions, and by the Mail,
/// Push, Telegram, Slack and Handover services and the Confluence Adapter for the person's own requests.
/// Every caller builds this struct as defined here, including `proposal`, `show_details`,
/// `asker` and `leaves_company`; no caller defines its own request type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApprovalRequest {
    pub workspace_id: WorkspaceId,
    pub asked_by: ApprovalAsker,
    pub question: String,
    pub verb: String,
    pub where_line: String,
    pub affects: Vec<AffectedItem>,
    pub changes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ApprovalContent>,
    pub reject_style: RejectStyle,
    /// Whether a **See …** button is offered; true only when `content` is present.
    pub show_details: bool,
    /// The proposal as the Rules Engine stated it; stored as the `Approval` entity's fields.
    pub proposal: ActionProposal,
    /// Agent profile: who asked (identity-provider subject, or `<connection>/<user>` for chat).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asker: Option<String>,
    /// Agent profile: the action leaves the company (outside email address, unlisted channel,
    /// a connection marked external). Ignored outside the agent profile.
    pub leaves_company: bool,
}

/// Where a press came from. The program, the Remote API, the Channel Gateway and the Caller API fill it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PressOrigin {
    /// The screens of the computer this core runs on.
    LocalScreen,
    /// A paired desktop through the Remote API (personal server).
    PairedDesktop { computer_name: String },
    /// A chat button (Telegram or Slack) from the paired private chat.
    PairedChat { channel: ChatChannel },
    /// Agent profile: an approver identified by the identity provider or the chat link.
    Approver { subject: String, groups: Vec<String>, via: ApproverVia },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel { Telegram, Slack }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApproverVia { Chat, Desktop }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalOutcome {
    pub approval_id: ApprovalId,
    pub state: ApprovalState,
    pub decided_on: String,
}

/// The stored record with its card. `to_domain` gives the 2C `Approval` entity where the asker
/// is a run or a crew round (an `ApprovalAsker::Person` card has no `ApprovalSubject`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalRecord {
    pub card: ApprovalCard,
    pub proposal: ActionProposal,
    pub state: ApprovalState,
    #[serde(skip_serializing_if = "Option::is_none")] pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub decided_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub decided_at: Option<Timestamp>,
}
impl ApprovalRecord { pub fn to_domain(&self) -> Option<Approval>; }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome { Decided(ApprovalState), DeadlinePassed }

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("approval request invalid: {code}")] Invalid { code: &'static str, field: &'static str },
    #[error("card reference unknown")] CardRefUnknown,
    #[error("card reference does not match the decision")] CardRefMismatch,
    #[error("already decided")] AlreadyDecided { state: ApprovalState, decided_on: String },
    #[error("card closed")] CardClosed,
    #[error("not an approver")] NotAnApprover,
    #[error("asker may not approve")] AskerMayNotApprove,
    #[error("operator decision not allowed")] OperatorNotAllowed,
    #[error("approval not found")] NotFound,
    #[error("cursor not valid")] BadCursor,   // SRV_BAD_CURSOR (H.2)
    #[error("datastore unavailable")] Store(#[source] sqlx::Error),
    #[error("journal unavailable")] Journal(#[source] JournalError),
}
impl ApprovalError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }

impl ApprovalService {
    pub const MODULE: &'static str = "approval";
    pub fn migrations() -> ModuleMigrations;
    /// Opens the store, loads every pending approval into memory (recovery), and returns.
    /// Called by: the desktop and server programs at start.
    pub async fn new(datastore: Datastore, journal: Arc<Journal>, config: ApprovalConfig)
        -> Result<Arc<Self>, ApprovalError>;
    /// Creates a pending approval, mints its buttons, journals `approval_requested`.
    /// Callers take `card.approval_id` from the returned card. Called by: Rules Engine (every *ask*),
    /// Crew Orchestrator ("Ask me first" steps and approval points), Mail Service (sending an email,
    /// `content: Some(ApprovalContent::Email { .. })`), Push Service and Handover Service (sending
    /// to an origin), Telegram Service (posting), Slack Service (posting a draft, a reminder or a notice that has no card of its own; amended January 2026, ADR-102), Confluence Adapter (publishing a page).
    pub async fn request(&self, req: ApprovalRequest) -> Result<ApprovalCard, ApprovalError>;
    /// Service API `approval.decide` (2C), and chat buttons. Called by: the program for the local
    /// screens (Command Bridge, `LocalScreen`), Remote API (`PairedDesktop`), Channel Gateway
    /// (`PairedChat` on the personal server, `Approver` in the agent profile), Caller API (route 32,
    /// `Approver` with `via: Desktop`).
    pub async fn decide(&self, press: ApprovalPress, origin: PressOrigin) -> Result<ApprovalOutcome, ApprovalError>;
    /// Agent profile with no approvers named: the operator decides. Called by: Caller API, in the
    /// operator's resume of a paused run, before it resumes the run in the Run Queue.
    pub async fn decide_by_operator(&self, approval_id: ApprovalId, decision: PressDecision, operator: &str)
        -> Result<ApprovalOutcome, ApprovalError>;
    /// The asking service takes back an approval it opened, before anyone decides (ADR-035: a local
    /// edit while a push waits). Pending → `expired`, buttons closed, `approval_decided` emitted.
    /// Called by: Push Service; Handover Service, in `take_back` (HandoverService.md Internal Logic F.6,
    /// SA-CLOSE-02: a handover's still-pending hand-over card; SA-CLOSE-01). (The Mail Service never withdraws: a draft whose
    /// email waits for approval cannot be changed, MailService.md state table.)
    pub async fn withdraw(&self, approval_id: ApprovalId) -> Result<(), ApprovalError>;
    /// Current state; a receiver that changes once, from `Pending` to a final state.
    /// `wait` is built on it; no other component calls it.
    pub fn watch(&self, approval_id: ApprovalId) -> Result<tokio::sync::watch::Receiver<ApprovalState>, ApprovalError>;
    /// Waits for a decision until `deadline` (`None` waits without limit). Called by: Rules Engine
    /// (`wait_for_approval`, on behalf of the MCP Front, the Project Service and the Hook Relay),
    /// Hook Relay (late decisions, G), Crew Orchestrator (step approvals).
    pub async fn wait(&self, approval_id: ApprovalId, deadline: Option<tokio::time::Instant>)
        -> Result<WaitOutcome, ApprovalError>;
    /// The record with its card and state; `get(id)?.state` is the one way to read an approval's
    /// state (there is no separate `state` call). Called by: Rules Engine (approved once, D.6),
    /// Mail Service, Push Service, Telegram Service, Slack Service (a post's authority, checked before sending), Channel Gateway (**See …** in chat), Caller API
    /// (route 32), Service API `approval.get`.
    pub async fn get(&self, approval_id: ApprovalId) -> Result<ApprovalRecord, ApprovalError>;
    /// Service API `approval.list_pending` (chosen here); SA-PAGE-01 page form, page size 100.
    /// Called by: the program for the screens and for the Remote API's `approval.list_pending`
    /// command, Channel Gateway (on connect), Caller API (route 31).
    pub async fn list_pending(&self, workspace: Option<WorkspaceId>, cursor: Option<Cursor>)
        -> Result<Page<ApprovalRecord>, ApprovalError>;
    /// Expires every pending approval of a run (or crew run) that ended first.
    /// Called by: this service's event sink; Crew Orchestrator (`expire_for_crew_run`, when a crew ends).
    pub async fn expire_for_run(&self, run_id: RunId) -> Result<Vec<ApprovalId>, ApprovalError>;
    pub async fn expire_for_crew_run(&self, crew_run_id: CrewRunId) -> Result<Vec<ApprovalId>, ApprovalError>;
    /// The `EventSink` the program registers with the Journal (run-ended and crew-ended events).
    /// Called by: the desktop and server programs (`journal.subscribe(approvals.event_sink())`).
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink>;
    /// The Journal's pruning hook for decided approvals (Data Model). Called by: the programs.
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant>;
}

```

Service API commands (SA-API-01; the program implements `ServiceApi` and calls the method named):

| Command | Input | Result | Method |
|---|---|---|---|
| `approval.decide` (2C) | `ApprovalPress` | `ApprovalOutcome` | `decide(press, PressOrigin::LocalScreen)` on the desktop; the Remote API passes `PairedDesktop` |
| `approval.list_pending` (*chosen here*) | `{ "workspace_id": WorkspaceId \| null, "cursor": string \| null }` | `Page<ApprovalRecord>` | `list_pending` |
| `approval.get` (*chosen here*) | `{ "approval_id": ApprovalId }` | `ApprovalRecord` | `get` |

Events emitted (2C `ServiceEvent`): `approval_requested`, `approval_decided`; and `needs_you_changed { workspace_id }` (2C `EventKind::NeedsYouChanged`) whenever an approval of a workspace becomes pending or stops being pending (B.6, C.8, F.2, K).

What this service does not offer, by decision (January 2026 reconciliation):
- No `state(id)` call: callers read `get(id)?.state`; an unknown id is `APPR_NOT_FOUND`.
- No owner lookup (`owner(id)`): a module that needs to find its own subject from an approval keeps the `approval_id` on its own row (the Confluence Adapter's link row, as the Workspace Service does).
- No change to a card after `request`: a card is immutable. "Posted 10:44"-style outcome notes after an approved action are the asking service's own notice (Telegram Service, Slack Service), not an approval change; 2C `ServiceEvent` has no variant for them here.
- No email-specific opening call: the Mail Service calls `request` with `content: Some(ApprovalContent::Email { .. })`, `show_details: true` and a `proposal`.

### Example

Request from the Rules Engine for a push:

```json
{
  "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
  "asked_by": {"kind": "run", "run_id": "0192a1c2-3344-7556-8677-8899aabbccdd"},
  "question": "Push 3 commits to acme/supplier-portal?",
  "verb": "Push",
  "where_line": "acme/supplier-portal, branch main",
  "affects": [{"kind": "record", "label": "3 commits on main"}],
  "changes": "The three commits become visible to everyone who can open the repository.",
  "reject_style": "reject",
  "show_details": false,
  "proposal": {"action": "Push to GitHub", "destination": "acme/supplier-portal, branch main",
               "affects": [{"kind": "record", "label": "3 commits on main"}],
               "consequence": "The three commits become visible to everyone who can open the repository."},
  "leaves_company": true
}
```

Result: an `ApprovalCard` with `approval_id` `0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f`, `buttons.approve` `q8Zt3VxN0pLk2mRb7sYw1A`, `buttons.reject` `Lm4Qp0Rt7Vx2Zb5Nc8Df3g`, and the event `approval_requested` with sequence number 48213. The person presses **Push**: `decide({"card_ref":"q8Zt3VxN0pLk2mRb7sYw1A","decision":"approve"}, LocalScreen)` returns `{"approval_id":"0192a1c3-…","state":"approved","decided_on":"Asha's laptop"}`, and every waiter on that approval wakes with `Approved`.

## Internal Logic

**A. `new`.**
1. Run nothing against the store beyond reads: the program has already applied `migrations()` through the Journal's migration registry (SA-XC-07).
2. Select every row of `approvals` with `state = 'pending'`; create one `watch::channel(ApprovalState::Pending)` per row in an in-memory map keyed by `ApprovalId`. Log `approval_recovered` (field `count`).
3. A store error returns `ApprovalError::Store` (`APPR_STORE_UNAVAILABLE`, retryable); the program does not start without this service.

**B. `request(req)`.**
1. Validate `req` against the limits in *Shared Context*, in field order; the first failure returns `ApprovalError::Invalid` with the 2C code: `APPR_QUESTION_INVALID` (length, or not ending with "?"), `APPR_VERB_INVALID` (length, or equal ignoring case to "OK", "Yes" or "Continue"), `APPR_WHERE_INVALID`, `APPR_AFFECTS_INVALID`, `APPR_CHANGES_INVALID`, `APPR_CONTENT_LABEL_INVALID`, `APPR_CONTENT_TOO_LARGE`, `APPR_ACTION_INVALID`, `APPR_DESTINATION_INVALID`, `APPR_CONSEQUENCE_INVALID`. Text fields are NFC-normalised and any control character other than `\n` and `\t` is refused with the code of that field (2C wire §0). `show_details = true` with `content = None` returns `APPR_SHOW_DETAILS_WITHOUT_CONTENT` (*chosen here*). Validation errors are permanent.
2. Recipient checks (`to + cc + bcc` count and recipients outside the company, attachments from excluded places) are the Mail Service's and the Rules Engine's, made before the request; this service checks only the card limits of step 1.
3. Mint `approval_id` (UUIDv7) and two or three `CardRef` values: 16 bytes from the operating system's random source (`getrandom`), encoded base64url without padding; `show_details` only when `req.show_details`.
4. In one transaction: insert the `approvals` row (state `pending`, `card` the serialised `ApprovalCard`, `created_at` now) and one `approval_buttons` row per reference (`state = 'open'`).
5. Create the watch channel for the approval.
6. `journal.append_event(EventKind::ApprovalRequested.as_str(), {"card": card})`. A Journal error after the commit is logged as `approval_emit_failed` and retried once after 200 ms; if the retry fails, the approval stays pending in the store (the screens reload pending approvals through `approval.list_pending`) and `request` still returns the card. The person never loses a pending approval.
7. Log `approval_requested` (fields `approval_id`, `workspace_id`, `asked_by_kind`, `run_id` or `crew_run_id`). Return the card.

**C. `decide(press, origin)`.**
1. Look up `press.card_ref` in `approval_buttons`. None → `APPR_CARD_REF_UNKNOWN` (permanent). The button kind must match `press.decision` (`approve` ↔ `Approve`, `reject` ↔ `Reject`); a `show_details` reference or the other button → `APPR_CARD_REF_MISMATCH`.
2. Button `state = 'used'` or `'closed'`, or the approval not `pending`:
   - approval `approved` or `rejected` → `APPR_CARD_ALREADY_DECIDED`, carrying `decided_on` so the caller can show "This was already decided on Asha's laptop." (2C example);
   - approval `expired` → `APPR_CARD_CLOSED`.
3. Agent profile (`config.approvers` is `Some`): `origin` must be `Approver`. `LocalScreen`, `PairedDesktop` or `PairedChat` → `APPR_NOT_AN_APPROVER`. The subject is an approver when it is in `users`, or any of `groups` is in the policy's `groups`; otherwise `APPR_NOT_AN_APPROVER`. When `subject == asker` and (`leaves_company` or not `asker_may_approve_internal`) → `APPR_ASKER_MAY_NOT_APPROVE`. A policy with empty `users` and empty `groups` means "no approvers named": every press is refused with `APPR_NOT_AN_APPROVER` and only `decide_by_operator` decides (ADR-097 item 4).
4. Desktop and personal server (`config.approvers` is `None`): `Approver` origin → `APPR_NOT_AN_APPROVER`; every other origin is accepted (pairing is checked before this call by the Remote API and the Channel Gateway, 2C wire §5: `SVC_UNAUTHENTICATED`).
5. `decided_on`: `LocalScreen` → `config.local_computer_name`; `PairedDesktop` → `computer_name`; `PairedChat` → "Telegram" or "Slack"; `Approver` → the subject. Truncated to 80 characters. `decided_by` stored: `person` for the first three, the subject for `Approver`.
6. One statement decides once, so a press on two channels at the same instant has exactly one winner: `UPDATE approvals SET state = ?, decided_by = ?, decided_on = ?, decided_at = ? WHERE id = ? AND state = 'pending'`. Zero rows → re-read the row and return `APPR_CARD_ALREADY_DECIDED` (or `APPR_CARD_CLOSED` when expired). In the same transaction: the pressed button row → `used` with `used_at`; every other button of the approval → `closed`.
7. Send the final state on the watch channel and drop the channel from the map.
8. `journal.append_event(EventKind::ApprovalDecided.as_str(), {approval_id, state, decided_on})`; on failure, log `approval_emit_failed` and retry once after 200 ms. The decision stands in the store either way.
9. Log `approval_decided` (fields `approval_id`, `state`, `origin_kind`). Never log the subject of an approver in the desktop profiles; in the agent profile the subject is logged as `approver` because operators audit who approved.
10. Return `ApprovalOutcome`.

**D. `decide_by_operator(approval_id, decision, operator)`.**
1. Allowed only when `config.approvers` is `Some` with empty `users` and `groups`; otherwise `APPR_OPERATOR_NOT_ALLOWED` (approvers decide, not operators).
2. Load the approval; absent → `APPR_NOT_FOUND`. Then steps C.6 to C.10 with `decided_on = operator` and `decided_by = operator`.

**E. `watch` and `wait`.**
1. `watch`: a pending approval returns a receiver from the map. A decided or expired approval returns a new channel already holding the final state. Unknown → `APPR_NOT_FOUND`.
2. `wait(id, deadline)`: `watch`, then `receiver.wait_for(|s| *s != Pending)` under `tokio::time::timeout_at(deadline)` when a deadline is given. Timeout → `WaitOutcome::DeadlinePassed`; the approval stays pending (an approval is never decided by timeout, 11 §5.5).

**F. Expiry (`expire_for_run`, `expire_for_crew_run`, and the event sink).**
1. `UPDATE approvals SET state = 'expired', decided_at = now WHERE state = 'pending' AND asked_by_run_id = ?` (or `asked_by_crew_run_id = ?`), returning the ids; every button of those approvals → `closed`.
2. For each id: send `Expired` on its channel, emit `approval_decided` (`append_event`) with `state: expired` — `decided_on` must be 1–80 characters (2C), so the value is the fixed text `run ended` (*chosen here*).
3. The event sink's `deliver` reads `JournalEvent.kind`: `run_state_changed` with an ended `RunState` calls `expire_for_run`; `crew_round_changed` with an ended `CrewRunState` calls `expire_for_crew_run`; every other kind is ignored. `lagged(next_seq)` logs `approval_sink_lagged` and runs one sweep: every pending approval whose run is not known as running is left pending (the asker re-checks on its own restart); no approval is expired on a guess.
4. An approval asked by the person (`ApprovalAsker::Person`) never expires on its own; the asking component closes it by `expire_for_run` only when it has a run.

**G. Restart.** Pending approvals are reloaded by step A.2; nothing is re-emitted at start. The screens, the Remote API and the Channel Gateway list pending approvals through `approval.list_pending` when they connect. In the agent profile a paused run's harness has ended; the Session Supervisor resumes it on `approval_decided` (Session Supervisor spec).

**H. `list_pending(workspace, cursor)`** (Pagination.md; SA-PAGE-01).
1. One keyset query over `approvals`: `state = 'pending'`, and `workspace_id = workspace` when `workspace` is `Some`; order `(created_at ASC, id ASC)` *(chosen here: the oldest waiting approval first)*; `LIMIT 101`; served by the index `approvals_pending`.
2. `cursor` present → `Cursor::decode::<(String, ApprovalId)>()` gives the last row's `(created_at, id)` and the query adds `AND (created_at, id) > (?, ?)`; a cursor that does not decode → `ApprovalError::BadCursor` (`SRV_BAD_CURSOR`), no query run.
3. Return the first 100 rows as `ApprovalRecord`s; when 101 rows came back, `next_cursor = Cursor::encode(&(created_at, id))` of the 100th row, else `None`. A datastore error → `APPR_STORE_UNAVAILABLE`.

**K. `withdraw(approval_id)`.** Callers: the Push Service (a local edit while a push waits, ADR-035) and the Handover Service's `take_back`, for the handover's still-pending hand-over card (HandoverService.md F.6, SA-CLOSE-01).
1. Load the approval; absent → `APPR_NOT_FOUND`.
2. State `expired` already → return `Ok(())` (a second withdraw changes nothing). State `approved` or `rejected` → `APPR_CARD_ALREADY_DECIDED` with `decided_on`: the person decided first, and the caller acts on that decision.
3. One statement: `UPDATE approvals SET state = 'expired', decided_at = now WHERE id = ? AND state = 'pending'`; zero rows → re-read and apply step 2. In the same transaction every button of the approval → `closed`.
4. Send `Expired` on the watch channel and drop it; emit `approval_decided` (`append_event`) with `{approval_id, state: expired, decided_on: "withdrawn"}` (*chosen here*; 1–80 characters as 2C requires), with the retry rule of B.6.
5. Log `approval_withdrawn` {approval_id}. A later press on the card returns `APPR_CARD_CLOSED`.

**L. Needs you.** After each commit that makes an approval pending (B.4) or ends a pending approval (C.6, D, F.1, K.3), emit `needs_you_changed` with `{"workspace_id": …}` through `journal.append_event(EventKind::NeedsYouChanged.as_str(), …)`, once per workspace touched by the statement, with the retry rule of B.6; a failure is logged as `approval_emit_failed` and never fails the call (Home and Search reloads *Needs you* on the next change or connect).

## Data Model

Owned tables, registered as module `approval` (SQLite: desktop, personal server, single shape; PostgreSQL: scaled shape). Forward-only; nothing exists to roll back on first install.

```sql
-- SQLite, migration 1
CREATE TABLE approvals (
    id                    TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id          TEXT NOT NULL CHECK (length(workspace_id) = 36),
    asked_by_kind         TEXT NOT NULL CHECK (asked_by_kind IN ('run', 'crew_round', 'person')),
    asked_by_run_id       TEXT CHECK (asked_by_run_id IS NULL OR length(asked_by_run_id) = 36),
    asked_by_crew_run_id  TEXT CHECK (asked_by_crew_run_id IS NULL OR length(asked_by_crew_run_id) = 36),
    asked_by_round        INTEGER CHECK (asked_by_round IS NULL OR asked_by_round >= 1),
    action                TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 500),
    destination           TEXT NOT NULL CHECK (length(destination) BETWEEN 1 AND 2048),
    affected_records      TEXT NOT NULL,           -- JSON array of strings, ≤ 1000
    consequence           TEXT NOT NULL CHECK (length(consequence) BETWEEN 1 AND 2000),
    card                  TEXT NOT NULL,           -- JSON ApprovalCard
    asker                 TEXT,                    -- agent profile only
    leaves_company        INTEGER NOT NULL DEFAULT 0 CHECK (leaves_company IN (0, 1)),
    state                 TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'expired')),
    decided_by            TEXT,
    decided_on            TEXT CHECK (decided_on IS NULL OR length(decided_on) BETWEEN 1 AND 80),
    decided_at            TEXT,
    created_at            TEXT NOT NULL,
    CHECK ((state IN ('approved', 'rejected')) = (decided_by IS NOT NULL AND decided_at IS NOT NULL)),
    CHECK ((asked_by_kind = 'run') = (asked_by_run_id IS NOT NULL)),
    CHECK ((asked_by_kind = 'crew_round') = (asked_by_crew_run_id IS NOT NULL AND asked_by_round IS NOT NULL))
);
CREATE INDEX approvals_pending ON approvals (state, workspace_id, created_at) WHERE state = 'pending';
CREATE INDEX approvals_run ON approvals (asked_by_run_id) WHERE asked_by_run_id IS NOT NULL;
CREATE INDEX approvals_crew ON approvals (asked_by_crew_run_id) WHERE asked_by_crew_run_id IS NOT NULL;

CREATE TABLE approval_buttons (
    card_ref     TEXT PRIMARY KEY CHECK (length(card_ref) = 22),
    approval_id  TEXT NOT NULL REFERENCES approvals (id) ON DELETE CASCADE,
    button       TEXT NOT NULL CHECK (button IN ('approve', 'reject', 'show_details')),
    state        TEXT NOT NULL CHECK (state IN ('open', 'used', 'closed')),
    used_at      TEXT,
    UNIQUE (approval_id, button)
);
```

```sql
-- PostgreSQL, migration 1 (agent profile, scaled shape)
CREATE TABLE approvals (
    id uuid PRIMARY KEY, workspace_id uuid NOT NULL,
    asked_by_kind text NOT NULL CHECK (asked_by_kind IN ('run', 'crew_round', 'person')),
    asked_by_run_id uuid, asked_by_crew_run_id uuid, asked_by_round integer CHECK (asked_by_round >= 1),
    action text NOT NULL CHECK (char_length(action) BETWEEN 1 AND 500),
    destination text NOT NULL CHECK (char_length(destination) BETWEEN 1 AND 2048),
    affected_records jsonb NOT NULL, consequence text NOT NULL CHECK (char_length(consequence) BETWEEN 1 AND 2000),
    card jsonb NOT NULL, asker text, leaves_company boolean NOT NULL DEFAULT false,
    state text NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'expired')),
    decided_by text, decided_on text CHECK (char_length(decided_on) BETWEEN 1 AND 80),
    decided_at timestamptz, created_at timestamptz NOT NULL
);
CREATE INDEX approvals_pending ON approvals (workspace_id, created_at) WHERE state = 'pending';
CREATE INDEX approvals_run ON approvals (asked_by_run_id) WHERE asked_by_run_id IS NOT NULL;
CREATE TABLE approval_buttons (
    card_ref text PRIMARY KEY CHECK (char_length(card_ref) = 22),
    approval_id uuid NOT NULL REFERENCES approvals (id) ON DELETE CASCADE,
    button text NOT NULL CHECK (button IN ('approve', 'reject', 'show_details')),
    state text NOT NULL CHECK (state IN ('open', 'used', 'closed')),
    used_at timestamptz, UNIQUE (approval_id, button)
);
```

Pruning: a row is never deleted while `pending` (04:386). Decided rows follow the Journal's history retention through the Journal's `PruneParticipant` (A-08): `prune_participant()` returns this module's participant (`module()` = `approval`), which the program registers with `journal.register_prune_participant`; `prune` runs `DELETE FROM approvals WHERE state <> 'pending' AND decided_at < ?cutoff AND (asked_by_run_id IS NULL OR asked_by_run_id NOT IN (?runs_in_progress))` (buttons go by cascade) and returns the count; a store error → `PruneFailure { code: "APPR_STORE_UNAVAILABLE" }`. Migrations are `MigrationStep` tuples (R3).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A card field breaks its limit (B.1) | `Err(permanent)` | `APPR_QUESTION_INVALID`, `APPR_VERB_INVALID`, `APPR_WHERE_INVALID`, `APPR_AFFECTS_INVALID`, `APPR_CHANGES_INVALID`, `APPR_CONTENT_LABEL_INVALID`, `APPR_CONTENT_TOO_LARGE` | `ApprovalError::Invalid { code, field }` |
| A proposal field breaks its limit (B.1) | `Err(permanent)` | `APPR_ACTION_INVALID`, `APPR_DESTINATION_INVALID`, `APPR_CONSEQUENCE_INVALID` | `ApprovalError::Invalid { code, field }` |
| `show_details` without content (B.1) | `Err(permanent)` | `APPR_SHOW_DETAILS_WITHOUT_CONTENT` | `ApprovalError::Invalid` |
| `list_pending` cursor that does not decode (H.2) | `Err(permanent)` | `SRV_BAD_CURSOR` | `ApprovalError::BadCursor` |
| Press with an unknown reference (C.1) | `Err(permanent)`; Service API reply `kind: error` | `APPR_CARD_REF_UNKNOWN` | `{"code":"APPR_CARD_REF_UNKNOWN","message":"This card is no longer open."}` |
| Reference of the other button (C.1) | `Err(permanent)` | `APPR_CARD_REF_MISMATCH` | `{"code":"APPR_CARD_REF_MISMATCH","message":"This card is no longer open."}` |
| Already decided elsewhere (C.2, C.6) | `Err(permanent)` | `APPR_CARD_ALREADY_DECIDED` | `{"code":"APPR_CARD_ALREADY_DECIDED","message":"This was already decided on Asha's laptop."}` |
| The run ended first (C.2) | `Err(permanent)` | `APPR_CARD_CLOSED` | `{"code":"APPR_CARD_CLOSED","message":"This card closed because its work ended."}` |
| Press from someone who is not an approver (C.3, C.4) | `Err(permanent)` | `APPR_NOT_AN_APPROVER` | `ApprovalError::NotAnApprover`; chat reply "Only the approvers of this agent can decide this." |
| Asker approves own action that leaves the company (C.3) | `Err(permanent)` | `APPR_ASKER_MAY_NOT_APPROVE` | `ApprovalError::AskerMayNotApprove`; chat reply "Someone else must approve this, because it leaves the company." |
| Operator decides while approvers are named (D.1) | `Err(permanent)` | `APPR_OPERATOR_NOT_ALLOWED` | Caller API status 409 with `{"error":"approvers decide this request","code":"APPR_OPERATOR_NOT_ALLOWED"}` |
| Unknown approval id (D.2, E.1, `get`) | `Err(permanent)` | `APPR_NOT_FOUND` | `ApprovalError::NotFound` |
| Store read or write fails | `Err(retryable)` | `APPR_STORE_UNAVAILABLE` | `ApprovalError::Store`; Service API sentence "Something went wrong saving this decision. Try again." with action `try_again` |
| Journal emit fails twice (B.6, C.8, K.4, L) | none (logged) | `APPR_EMIT_FAILED` | Log only; the call still succeeds |
| Withdraw of an approval the person already decided (K.2) | `Err(permanent)` | `APPR_CARD_ALREADY_DECIDED` | `ApprovalError::AlreadyDecided { state, decided_on }`; the caller acts on `state` |
| Withdraw of an unknown approval (K.1) | `Err(permanent)` | `APPR_NOT_FOUND` | `ApprovalError::NotFound` |

The person-facing sentences above are written in the message catalogue as `approval-error-closed`, `approval-error-already-decided` (with `$where`), `approval-error-not-approver`, `approval-error-asker` and `approval-error-store` (*chosen here*, SA-I18N-00 pattern).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Approval Service

  Scenario: Happy path — a run asks and the person approves
    Given a run "R1" in workspace "Sales" asks to push 3 commits
    When the Rules Engine calls request with question "Push 3 commits to acme/supplier-portal?" and verb "Push"
    Then a card is returned with two 22-character button references
    And the event approval_requested is journalled with that card
    When the person presses the approve reference on this computer
    Then decide returns state approved with decided_on "Asha's laptop"
    And every waiter on the approval wakes with Approved
    And the event approval_decided is journalled

  Scenario: Edge — two channels press at the same instant
    Given a pending approval with buttons sent to Telegram and to a paired desktop
    When both presses arrive within the same millisecond
    Then exactly one decide returns approved
    And the other returns APPR_CARD_ALREADY_DECIDED naming where it was decided

  Scenario: Edge — a pending approval survives a restart
    Given a pending approval "A1"
    When the application restarts
    Then list_pending returns "A1"
    And a press of its approve reference after the restart decides it

  Scenario: Edge — pending approvals are paged oldest first
    Given 150 pending approvals in workspace "W1", created one second apart
    When list_pending is called for "W1" with no cursor
    Then it returns the 100 oldest, oldest first, and a next_cursor
    When list_pending is called for "W1" with that next_cursor
    Then it returns the remaining 50 and no next_cursor

  Scenario: Error — a malformed cursor
    When list_pending is called with the cursor "not-a-cursor"
    Then it fails with SRV_BAD_CURSOR

  Scenario: Edge — the run ends before anyone decides
    Given a pending approval asked by run "R1"
    When the Journal delivers run_state_changed for "R1" with state cancelled
    Then the approval becomes expired
    And a later press returns APPR_CARD_CLOSED

  Scenario: Edge — waiting with a deadline never decides
    Given a pending approval
    When a waiter waits with a deadline 50 seconds ahead and nobody presses
    Then the waiter gets DeadlinePassed
    And the approval is still pending

  Scenario: Error — the verb is "OK"
    When request is called with verb "OK"
    Then it returns APPR_VERB_INVALID and nothing is stored

  Scenario: Error — the question has no question mark
    When request is called with question "Publish the brief"
    Then it returns APPR_QUESTION_INVALID

  Scenario: Error — unknown reference
    When decide is called with a reference that was never minted
    Then it returns APPR_CARD_REF_UNKNOWN

  Scenario: Error — reject reference pressed as approve
    When decide is called with the reject reference and decision approve
    Then it returns APPR_CARD_REF_MISMATCH and the approval stays pending

  Scenario: Agent profile — the asker may not approve an outside email
    Given the approvers are the group "support-leads" and the asker "u-123" is in that group
    And a pending approval with leaves_company true asked by "u-123"
    When "u-123" presses approve in chat
    Then it returns APPR_ASKER_MAY_NOT_APPROVE
    When "u-456" of "support-leads" presses approve
    Then the approval is approved with decided_on "u-456"

  Scenario: Agent profile — someone who is not an approver presses
    Given the approvers are the user "u-456" only
    When "u-789" presses approve
    Then it returns APPR_NOT_AN_APPROVER

  Scenario: Agent profile — no approvers named, the operator decides
    Given the approver policy names no users and no groups
    When an operator calls decide_by_operator with approve
    Then the approval is approved with decided_on the operator's subject
    And a chat press on the same card would have returned APPR_NOT_AN_APPROVER

  Scenario: Error — operator decides while approvers are named
    Given the approvers are the user "u-456"
    When an operator calls decide_by_operator
    Then it returns APPR_OPERATOR_NOT_ALLOWED

  Scenario: Withdraw a pending approval
    Given a pending push approval "A2" in workspace "Sales"
    When the Push Service calls withdraw for "A2"
    Then the approval is expired and its buttons are closed
    And approval_decided with decided_on "withdrawn" and needs_you_changed for "Sales" are journalled
    And a later press of its approve reference returns APPR_CARD_CLOSED
    When withdraw is called again for "A2"
    Then it returns Ok and nothing changes

  Scenario: The Handover Service withdraws a pending hand-over card on take back (SA-CLOSE-01)
    Given a pending hand-over approval "A4" asked by the Handover Service
    When the Handover Service calls withdraw for "A4" from take_back
    Then the approval is expired with decided_on "withdrawn"

  Scenario: Withdraw after the person decided
    Given approval "A3" was approved on "Asha's laptop"
    When the Push Service calls withdraw for "A3"
    Then it returns APPR_CARD_ALREADY_DECIDED naming "Asha's laptop"

  Scenario: Error — the datastore is unavailable
    Given the SQLite file is locked by another writer beyond the busy timeout
    When request is called
    Then it returns APPR_STORE_UNAVAILABLE marked retryable
```

## Performance / Security / Observability

- **Performance targets:** `request` p95 ≤ 15 ms and `decide` p95 ≤ 10 ms on SQLite on a supported laptop, excluding the Journal's delivery to subscribers; `wait` wakes within 5 ms of the decision. In-memory state is one watch channel per pending approval (under 1 KiB each); at most 1,000 pending approvals (assumed: far above any person's backlog) keep it under 1 MiB. The approval screen's event reaches the screens within the 150 ms of A-03 because the card is complete in the event.
- **Security:** No entry point takes text from Ordin or an agent as card text without the limits of B.1; the card is drawn as text (ADR-028). A `CardRef` carries 128 random bits and is single use; presses are refused after use, after a decision elsewhere and after the run ended (2C). Pairing of desktops and chats is authenticated before `decide` by the Remote API and the Channel Gateway; in the agent profile the approver's identity comes from the identity-provider token or the chat link. A typed "yes" is never an approval: only `decide` with a button reference decides (decision 10). Never logged: card text, message bodies, email content, recipients, the asker's identity (desktop profiles).
- **Observability:** log events (`tracing`, target `ordin_approval`): `approval_requested` {approval_id, workspace_id, asked_by_kind, run_id?, crew_run_id?}; `approval_decided` {approval_id, state, origin_kind, approver? (agent profile only)}; `approval_expired` {approval_id, run_id?}; `approval_withdrawn` {approval_id}; `approval_recovered` {count}; `approval_emit_failed` {approval_id, error_code}; `approval_sink_lagged` {next_seq}. No metric of its own (Metrics.md N-1: only the 50 families of the closed list exist (49 before ADR-107); tool calls paused for approval are counted by the Run Queue's `wrightrun_tool_calls_paused_for_approval_total`). No spans of its own.

Reconciliation assumptions (January 2026):
- **SA-RECON-01** — R6: callers adopt this service's names: `request` returns the card, `get(id)?.state` replaces a `state` call, `withdraw` is the one way to take back an approval, and every caller builds `ApprovalRequest` as defined here.
- **SA-RECON-02** — R3: events go through `Journal::append_event` with 2C `EventKind` names; history is pruned through the Journal's `PruneParticipant`.
- **SA-RECON-03** — R1: the Service API catalogue adopts `approval.decide`, `approval.list_pending` and `approval.get` with the inputs and results above.
- **SA-RECON-04** — R20, D4: the tables live in the Journal's `Datastore`, handed over by the program.
- **SA-CLOSE-01** — *(chosen here)* The Handover Service is a `withdraw` caller in one step only: `take_back` withdraws the handover's still-pending hand-over card before taking it back (HandoverService.md Internal Logic F.6, SA-CLOSE-02); `APPR_CARD_ALREADY_DECIDED` sends it on with the decided state.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

The caller lists above name the Slack Service in place: the `ApprovalRequest` doc comment (it builds requests), `request` (posting a draft, a reminder or a notice that has no card of its own), `get` (a post's authority, checked before sending) and the asking service's own notice. The Handover Service's cards may name Slack posts in `affects` and `changes`; the card is still the Handover Service's. Nothing else changes.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: a card naming a Slack post is decided like any other
    Given the Handover Service requested a card whose affects hold a Record "#design-reviews · Slack · will appear as you"
    When the person presses Hand over
    Then the approval's state is approved and approval_decided is journalled once
    And the card is not changed afterwards
```

### Gaps

None.
