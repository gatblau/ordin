# SPEC: Channel Gateway

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-channel-gateway` · **Build phase:** 9 · **Programs:** P A
> **Dependencies:** Ordin Host, Approval Service, Workspace Service, Organisation Profile, Journal, Retention and Erasure

## Table of contents

1. [Purpose](#purpose)
2. [Approach](#approach)
3. [Shared Context](#shared-context)
4. [Public Interface](#public-interface)
5. [Internal Logic](#internal-logic)
6. [Data Model](#data-model)
7. [Error Table](#error-table)
8. [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
9. [Performance, Security, Observability](#performance-security-observability)
10. [Gaps](#gaps)
11. [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)

## Purpose

The Channel Gateway is the one channel-neutral interface between the ordin server's core and a chat channel (02:598–604, ADR-073). Chat adapters (Telegram, Slack) plug into it; everything that decides *who* is answered, *what* is sent and *which button means what* lives here, so the rules hold the same on every channel.

- **Personal profile.** It keeps the pairings (one chat account per channel kind, 04:261–264 as amended in revision 0.48.0), answers only the paired person in the paired private chat, routes the person's text to Ordin's conversation, saves files to the workspace's inbox (or hands them to a chat trigger), answers voice notes with fixed text (ADR-078), carries the commands **/stop**, **/status**, **/run**, **/triggers**, sends Ordin's replies and everything *Needs you* would show as it happens — approval and question cards with buttons whose data is a random single-use reference, files, images and the text form of every catalogue block — edits cards once decided, keeps each channel's send queue within its rate limit, and, with Telegram and Slack both paired, sends a card to both so the first press decides (ADR-076).
- **Agent profile.** It is the channel-neutral half of the hosted agent's chat connection (02:290, ADR-097, ADR-098): it decides whether a message is addressed to the agent and allowed, keeps scopes (one per channel, one per direct chat, one agent-wide), queues one run per question, delivers the reply (or the configuration's approval or failure text), carries reply actions, chat commands, the settings form and entry commands in fixed code, and sends approval cards to approvers' direct chats.

## Approach

A **hub with adapters** (2B-1 rule 4): the gateway owns the `ChannelAdapter` and `HostedChatLink` traits that the Telegram and Slack adapters implement and register at start, the `ChatTriggerHandler` trait that the Trigger Service implements, and the `PersonalCore` and `HostedAgentCore` traits through which it reaches the rest of the core — both implemented by the server program: `PersonalCore` over the Ordin Host's `send_message` and `answer_block`, the Session Supervisor, the Journal and the Workspace Service (personal; row 471, SA-RECON-01), `HostedAgentCore` over the Run Queue, Settings Store, Job Runner and Knowledge Service (agent), as 2B-1 rule 3 has programs pass trait objects between modules. Events reach it through the Journal's `EventSink` (2B-2 D1). Every inbound item becomes one channel-neutral value before any rule reads it, so no rule names a Telegram or Slack field. Rejected: one connector per channel holding its own rules (each channel would re-implement pairing, cards and approvals — ADR-073 "one channel-neutral interface"); waiting in a task for a hosted run's reply (a chat instance would hold work it cannot hand over; wrightrun's pending-question design is kept).

**Closing assumption** (*chosen here*): **SA-CLOSE-01** (closing decision K18) — chat files and saved block files are written by the Workspace Service's `save_to_inbox` (into `Inbox/` of the workspace's own folder) and `save_generated` (into `Results/`, recorded against the workspace's current Ordin run, which `PersonalCore::ordin_run` gives); both return `PlaceFileRef`, whose `display_path` the reply quotes.

**Reconciliation assumptions** (decision 80): **SA-RECON-01** — `PersonalCore` is implemented by the personal-server program, not in this crate, because several of its reads belong to the Session Supervisor and the Journal, which this crate does not depend on (rows 378, 471). **SA-RECON-02** — the server program stops the gateway by cancelling the token it passed to `start`; there is no `stop_accepting` (row 461). **SA-RECON-03** — the hosted agent's scopes are Retention and Erasure's table, reached through `ScopeRegistry`; this crate keeps only `chat_scope_notes` (X4, R13). **SA-RECON-04** — Slack's button limit is the Slack Channel Adapter's (10, row 468).

Error prefixes: **`CHAN_`** and **`CARD_`** (2C) for the personal profile; **`CHAT_`** for the agent profile, keeping wrightrun's chat codes unchanged so operators' log searches keep working (SA-ERR-01 carries wrightrun prefixes; `CHAT_` added here).

## Shared Context

### Shared types (Phase 2C, copied; not redefined)

```rust
// ---- ordin-types: conventions ----
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`
uuid_id!(ChannelPairingId, CardReferenceId, ApprovalId, WorkspaceId, RunId, CrewRunId,
         ArtefactId, PlaceId, ScopeId, TriggerId); // UUIDv7, #[serde(transparent)]

// ---- ordin-types: ChannelPairing (2C domain) ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChannelPairing {
    pub id: ChannelPairingId,
    pub channel_kind: ChannelKind,
    #[serde(skip_serializing_if = "Option::is_none")] pub chat_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub private_chat_id: Option<String>,
    /// The setup-command invocation that made it.
    pub paired_by: String,
    pub code_expires_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")] pub paired_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")] pub ended_at: Option<Timestamp>,
    pub state: PairingState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind { Telegram, Slack }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingState { Pending, Paired, Ended }

// ---- ordin-types: CardReference (2C domain) ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CardReference {
    pub id: CardReferenceId,
    pub channel_pairing_id: ChannelPairingId,
    pub reference: String,
    pub card: CardSubject,
    pub button: CardButton,
    pub state: CardReferenceState,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")] pub used_at: Option<Timestamp>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardSubject { Approval { approval_id: ApprovalId }, Question { question_id: String } }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardButton { Approve, NotNow, ShowDetails }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardReferenceState { Open, Used, Closed }

// ---- ordin-types: Scope (2C domain, agent profile) ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Scope {
    pub id: ScopeId,
    pub kind: ScopeKind,
    #[serde(skip_serializing_if = "Option::is_none")] pub chat_identifier: Option<String>,
    /// The chat connection the scope belongs to; None for the agent-wide scope (2C, decision 80: C-28).
    #[serde(skip_serializing_if = "Option::is_none")] pub connection: Option<String>,
    pub state: ScopeState,
    #[serde(skip_serializing_if = "Option::is_none")] pub erased_at: Option<Timestamp>,
    pub enabled_at: Timestamp,
}
pub enum ScopeKind { Channel, DirectChat, AgentWide }
pub enum ScopeState { Enabled, Disabled, Erased }

// ---- ordin-service-api: approval card and press (2C wire §5) ----
pub struct ApprovalCard {
    pub approval_id: ApprovalId, pub backend: BackendId, pub workspace_id: WorkspaceId,
    pub asked_by: ApprovalAsker, pub question: String, pub verb: String, pub where_line: String,
    pub affects: Vec<AffectedItem>, pub changes: String,
    pub content: Option<ApprovalContent>, pub reject_style: RejectStyle,
    pub buttons: ApprovalButtons, pub created_at: Timestamp,
}
pub enum ApprovalAsker { Run { run_id: RunId }, CrewRound { crew_run_id: CrewRunId, round: u32 }, Person }
pub enum AffectedItem {
    File { label: String, version: Option<String>, place_id: Option<PlaceId> },
    Record { label: String },
    Recipient { name: String, address: String, outside_company: bool },
}
pub enum RejectStyle { Reject, NotNow }
pub struct ApprovalButtons { pub approve: CardRef, pub reject: CardRef, pub show_details: Option<CardRef> }
pub struct CardRef(String); // 22 chars base64url, 16 random bytes
pub enum ApprovalContent {
    Message { label: String, text: String, destination_note: Option<String> },
    Command { label: String, command: String, runs_in: String },
    Changes { label: String, change_set: ContentRef },
    Page { label: String, page: ContentRef },
    Files { label: String, files: Vec<ContentRef> },
    Email { label: String, email: EmailApproval },
}
pub enum ContentRef { Artefact { artefact_id: ArtefactId }, PlaceFile { place_id: PlaceId, relative_path: String } }
pub struct EmailApproval { pub draft_id: EmailDraftId, pub from: String, pub to: Vec<EmailRecipient>,
    pub cc: Vec<EmailRecipient>, pub bcc: Vec<EmailRecipient>, pub subject: String, pub body: String,
    pub attachments: Vec<EmailAttachmentLine>, pub drafted_by: Option<String>, pub in_reply_to: Option<String> }
pub struct EmailRecipient { pub name: Option<String>, pub address: String, pub outside_company: bool }
pub struct EmailAttachmentLine { pub file_name: String, pub comes_from: String, pub bytes: u64 }
pub struct ApprovalPress { pub card_ref: CardRef, pub decision: PressDecision }
pub enum PressDecision { Approve, Reject }
pub enum ApprovalState { /* 2C domain: Pending, Approved, Rejected, Expired, … */ }

// ---- ordin-service-api: events (2C wire §1), as the Journal delivers them ----
pub enum ServiceEvent {
    RunStateChanged { run_id: RunId, state: RunState },
    RunProgress { run_id: RunId, progress: RunProgress },
    PlanChanged { run_id: RunId, plan_version: u32, steps: Vec<RunStep> },
    StepMarked { run_id: RunId, plan_version: u32, position: u8, mark: StepMark },
    StartUpReportReady { subject: StartUpSubject, items: Vec<StartUpReportItem> },
    ApprovalRequested { card: ApprovalCard },
    ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String },
    CrewRoundChanged { crew_run_id: CrewRunId, round: u32, state: CrewRunState },
    BlockShown { workspace_id: WorkspaceId, block_id: BlockId, block: Block },
    Reset,
}
pub enum StepMark { Next, Now, Done, Skipped }
pub enum RunState { Queued, Running, Paused, Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply }
pub enum CrewRunState { Starting, Running, Paused, Finished, Escalated, StoppedAtLimit, Killed, Failed }

// ---- ordin-service-api: blocks this gateway turns into text (2C wire §7) ----
pub enum Block { Message(MessageBlock), Question(QuestionBlock), Form(FormBlock), Documents(DocumentsBlock),
    Preview(PreviewBlock), Document(DocumentBlock), Changes(ChangesBlock), PlanProgress(PlanProgressBlock),
    Table(TableBlock), Chart(ChartBlock), KeyFigures(KeyFiguresBlock), Report(ReportBlock),
    EmailList(EmailListBlock), Email(EmailBlock), TelegramList(TelegramListBlock), Result(ResultBlock),
    AgentProposal(AgentProposalBlock), AgentLibrary(AgentLibraryBlock), CrewPlan(CrewPlanBlock),
    Memory(MemoryBlock), Knowledge(KnowledgeBlock) }
pub struct MessageBlock { pub text: String, pub attachments: Vec<ContentRef> }
pub struct QuestionBlock { pub question: String, pub choices: Vec<String>, pub allow_other: bool }
pub struct FormBlock { pub title: String, pub verb: String, pub target: RunTarget, pub command: Option<String>, pub fields: Vec<FormField> }
pub struct FormField { pub name: String, pub label: String, pub required: bool, #[serde(flatten)] pub input: FormInput, pub filled_by_ordin: bool }
pub enum FormInput { Text { value: Option<String>, placeholder: Option<String> }, Document { value: Option<ContentRef> },
    Place { value: Option<PlaceId> }, Choice { options: Vec<String>, value: Option<String> } }
pub struct DocumentsBlock { pub workspace_id: WorkspaceId, pub items: Vec<ContentRef> }
pub struct PreviewBlock { pub item: ContentRef }
pub struct DocumentBlock { pub item: ContentRef }
pub struct ChangesBlock { pub before: ContentRef, pub after: ContentRef, pub view: ChangeView }
pub struct PlanProgressBlock { pub subject: StartUpSubject }
pub struct TableBlock { pub title: String, pub columns: Vec<TableColumn>, pub rows: Vec<Vec<Cell>>, pub source: PartSource }
pub struct TableColumn { pub name: String, #[serde(flatten)] pub column_type: ColumnType }
pub enum ColumnType { Text, Number { decimals: Option<u8> }, Money { currency: String }, Percentage, Date, DocumentLink }
pub enum Cell { Empty, Number(f64), Text(String), Link(ContentRef) }
pub struct ChartBlock { pub title: String, #[serde(flatten)] pub chart: ChartKind, pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<Cell>>, pub x: String, pub series: Vec<String>, pub x_title: Option<String>, pub y_title: Option<String>, pub source: PartSource }
pub struct KeyFiguresBlock { pub figures: Vec<KeyFigure>, pub source: PartSource }
pub struct KeyFigure { pub label: String, pub value: f64, pub unit: Option<String>, pub change: Option<FigureChange> }
pub struct FigureChange { pub against: String, pub earlier_value: f64 }
// ReportBlock, EmailListBlock, EmailBlock, TelegramListBlock, ResultBlock, AgentProposalBlock,
// AgentLibraryBlock, CrewPlanBlock, MemoryBlock, KnowledgeBlock: read only as `Block` values and
// serialised whole (step H.2, "no text form").
```

### Agent-profile configuration types (Agent Configuration spec, `ordin_types::agent_profile`, copied)

```rust
pub struct ChatConnection {
    pub name: String, pub kind: ChatKind, pub bot_token_env: String, pub app_token_env: String,
    pub triggers: Vec<ChatTrigger>, pub context: ChatContextConfig /* "ChatContext" in that spec */,
    pub status: StatusSignals, pub reply_actions: Vec<ReplyAction>, pub expose_tools: Vec<RuntimeTool>,
    pub failure_text: String, pub approval_text: String, pub entry_commands: Vec<EntryCommand>,
    pub job_commands: Vec<JobCommand>, pub version_command: Option<String>, pub settings_command: Option<String>,
    pub serves: Vec<String>,
}
pub struct ChatTrigger { pub kind: TriggerKind, pub name: Option<String>, pub starts: Option<String>, pub delivery: Delivery }
pub enum TriggerKind { Mention, Command, ThreadMention }
pub enum Delivery { Asker, Thread }
pub struct StatusSignals { pub received: String, pub working: String, pub done: String, pub failed: String }
pub struct ReplyAction { pub id: String, pub kind: ReplyActionKind, pub label: String, pub url_setting: Option<String>, pub notify_setting: Option<String>,
    // ADR-107 — hand-off route keys
    pub target_channel_setting: Option<String>, pub mention_users_setting: Option<String>,
    pub mention_groups_setting: Option<String>, pub enabled_setting: Option<String>,
    // ADR-107 — list-backed link keys
    pub list_setting: Option<String>, pub label_field: Option<String>, pub url_field: Option<String>,
    pub keywords_field: Option<String>, pub style: LinkStyle }
pub enum ReplyActionKind { Share, HandOff, Link }
pub enum LinkStyle { Url, WorkflowLink }   // default Url
pub struct EntryCommand { pub command: String, pub setting: String, pub action: EntryAction, pub fill: BTreeMap<String, FillSource> }
pub enum EntryAction { Add, Remove, Show }
pub struct JobCommand { pub command: String, pub does: JobCommandKind, pub source: String, pub who: Who }
pub enum JobCommandKind { Ingest, Progress }
pub enum Who { Members, OperatorsOnly }
pub struct ReachConfig { pub channels: Option<Vec<String>>, pub users: Option<Vec<String>>, pub direct_message_users: Vec<String> }
pub struct ApproversConfig { pub users: Vec<ApproverUser>, pub groups: Vec<String>, pub group_claim: String, pub asker_may_approve_internal: bool }
pub struct ApproverUser { pub chat_user: Option<String>, pub subject: Option<String> }
// The package declaration's chat triggers (`Manifest.declaration.chat_triggers: Vec<ChatTrigger>`).
```

The `ChatContext` type of the Agent Configuration spec (*amended January 2026, ADR-107*: `Fixed` with the five keys of before, or `Steps`, a list of context steps; `ChatContext::steps()` reads either as steps, K.3.7) is referred to here as `ChatContextConfig`, to keep it apart from this crate's `ChatContext`; the program imports it with `use ordin_types::agent_profile::ChatContext as ChatContextConfig`.

*Amended January 2026 (ADR-107):* `ChatContextConfig` is either the former five keys or a list of context steps, and `ChatContextConfig::steps()` (in `ordin_types::agent_profile`; AgentConfiguration.md V-18) gives the ordered steps either way, the former keys widened to the first three:

```rust
pub enum ContextStep {
    Transcript { max_messages: u32 },                                   // 0..=200
    Images { max_bytes: u32, max_total_bytes: u32 },
    ChannelDocuments { max_bytes: u32 },
    KnowledgeLookup { source: String, limit: u32, max_bytes: u32 },     // limit 1..=20
    RecentMessages { source: String, limit: u32, max_bytes: u32, when: Vec<String> },  // limit 1..=50; when = keywords
    PrecedenceNote { text: String },
}
```

### Configuration (Phase 2D)

| Source | Key | Default | Use |
|---|---|---|---|
| Organisation profile | `channels.allowed[]` (`telegram`, `slack`) | `[slack]` | channels the company allows (A-40); with no profile, the server owner's choice from the setup command |
| Setup command → server settings | *Needs you* to both paired channels | `false` | step I.5 |
| Setup command → server settings | chat channels chosen by the server owner (no profile) | none | A-40 |
| Environment | `TZ` | system zone | times shown on cards ("10:44") |
| 2D-4 | Channel outage reported on the other channel | after 10 minutes, once | step J.6 |
| 2D-4 | Telegram limits / Slack limits | as the adapters report them in `ChannelLimits` | step J |
| Agent configuration | `chat[]`, `reach`, `approvers`, the package's `chat_triggers` | — | step K |

### Calls on dependencies (as required here)

| Callee | Call | Defined in |
|---|---|---|
| Approval Service | `ApprovalService::decide(&self, press: ApprovalPress, origin: PressOrigin) -> Result<ApprovalOutcome, ApprovalError>`; `get(&self, ApprovalId) -> Result<ApprovalRecord, ApprovalError>`; `list_pending(&self, Option<WorkspaceId>, Option<Cursor>) -> Result<Page<ApprovalRecord>, ApprovalError>`; `PressOrigin::{PairedChat { channel: ChatChannel }, Approver { subject, groups, via: ApproverVia::Chat }}` | Approval Service spec |
| Journal | `Journal::subscribe(&self, Arc<dyn EventSink>) -> SubscriptionId`; `events_since(after_seq, limit)`; `Datastore`; `ModuleMigrations` | Journal and Artefact Store spec |
| Organisation Profile | `OrganisationProfileService::current() -> Arc<ProfileSnapshot>`, `subscribe()`; reads `document.channels.allowed: Vec<ChatChannel>` (never absent; default `[slack]`; mapped to `ChannelKind`) and `source` (`Verified` or `Defaults`) | OrganisationProfile.md |
| Workspace Service | `WorkspaceService::save_to_inbox(&self, ws: WorkspaceId, name: &str, bytes: Bytes, origin: InboxOrigin) -> Result<PlaceFileRef, WorkspaceError>`; `save_generated(&self, ws: WorkspaceId, name: &str, bytes: Bytes, by: RunId) -> Result<PlaceFileRef, WorkspaceError>`; `PlaceFileRef { place_id, relative_path, display_path }`; `InboxOrigin::Chat { channel: ChannelKind }` (SA-CLOSE-01) | WorkspaceService.md |
| Retention and Erasure | `ScopeRegistry::{agent_wide, enable(kind, connection, chat_identifier), disable(connection, chat_identifier), find(connection, chat_identifier), enabled(connection), get(id)}` returning `ScopeEntry { scope: Scope, connection }`; `RetainedContent` (implemented here) | RetentionAndErasure.md |
| Ordin Host | through `PersonalCore` (below), implemented by the server program over `OrdinHost::send_message` and `OrdinHost::answer_block` (OrdinHost.md X-22) and over the Session Supervisor and Journal for runs, step texts and content | OrdinHost.md; program wiring |
| Run Queue, Settings Store, Job Runner, Knowledge Service | through `HostedAgentCore` (below), implemented by the server program | program wiring |

### Libraries

`tokio`, `tokio-util` (`CancellationToken`), `async-trait`, `serde_json`, `sqlx` (through the Journal's `Datastore`), `rand` 0.9 and `base64` (action references), `chrono` (`Local` for `TZ`), `unicode-normalization` (NFC). `rand`, `base64`, `tokio-util`, `async-trait` and `unicode-normalization` were added to 05 under decision D2; their licences are confirmed by the licence gate of the Phase 5 scaffolding step (LicenceCheck.md Internal Logic 8.3), which stops the build if either reports an expression outside `allow`.

## Public Interface

```rust
// crate ordin-channel-gateway

// ======================= adapter side (personal profile) =======================

/// What an adapter can carry. Reported once at registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLimits {
    pub max_text_chars: u32,          // Telegram 4096, Slack 4000
    pub max_caption_chars: u32,       // Telegram 1024, Slack 4000
    pub max_buttons: u8,              // per message; Telegram 8 (chosen in its spec), Slack 10 (chosen in its spec)
    pub max_button_label_chars: u8,   // 64 / 75
    pub max_send_file_bytes: u64,     // Telegram 50 MB / Slack 100 MB (chosen in its spec)
    pub max_receive_file_bytes: u64,  // Telegram 20 MB / Slack 100 MB (chosen in its spec)
    pub min_send_interval_ms: u32,    // per chat: 1000 / 1000
}

/// Where to send. For the personal profile `chat_id` is the paired private chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAddress { pub chat_id: String }

#[derive(Debug, Clone, PartialEq)]
pub struct OutboundMessage {
    /// Plain text with `**bold**` and `*italic*` marks only; the adapter converts them.
    pub text: String,
    pub buttons: Vec<OutboundButton>,
    pub file: Option<OutboundFile>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundButton {
    pub label: String,
    /// Opaque, at most 24 bytes: `c:` + 22-char card reference, or `x:` + 22-char action reference.
    pub data: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct OutboundFile { pub file_name: String, pub media_type: String, pub bytes: bytes::Bytes, pub is_image: bool }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentMessage { pub chat_id: String, pub message_id: String }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChannelSendError {
    #[error("retryable: {code}")] Retryable { code: &'static str, retry_after_ms: Option<u64> },
    #[error("permanent: {code}")] Permanent { code: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelStatus {
    NotStarted,
    Connected,
    Reconnecting { since: Timestamp },
    CredentialsRefused { since: Timestamp },
}

/// Implemented by the Telegram Adapter and the Slack Channel Adapter (personal profile).
#[async_trait::async_trait]
pub trait ChannelAdapter: Send + Sync + 'static {
    fn kind(&self) -> ChannelKind;
    fn limits(&self) -> ChannelLimits;
    async fn send(&self, to: &ChannelAddress, msg: &OutboundMessage) -> Result<SentMessage, ChannelSendError>;
    /// Replaces the text and the buttons of a sent message (empty `buttons` removes them).
    async fn edit(&self, sent: &SentMessage, text: &str, buttons: &[OutboundButton]) -> Result<(), ChannelSendError>;
    /// Downloads a received file; refuses above `max_bytes` without downloading.
    async fn fetch_file(&self, file: &InboundFileRef, max_bytes: u64) -> Result<bytes::Bytes, ChannelSendError>;
    /// Acknowledges a button press to the chat service (stops the spinner); no text.
    async fn ack_press(&self, press: &PressAck) -> Result<(), ChannelSendError>;
    fn status(&self) -> ChannelStatus;
}

/// One item from the person's chat, already reduced to channel-neutral form by the adapter.
#[derive(Debug, Clone)]
pub struct InboundItem {
    pub channel: ChannelKind,
    pub sender_id: String,
    pub chat_id: String,
    /// True when the chat is the bot's private chat or the app's direct message with the sender.
    pub is_private: bool,
    pub message_id: String,
    pub received_at: Timestamp,
    pub body: InboundBody,
}
#[derive(Debug, Clone)]
pub enum InboundBody {
    Text { text: String },
    Command { command: ChatCommand, rest: String },
    Voice,
    File(InboundFile),
    /// Stickers, locations, contacts, polls and anything else.
    Unsupported,
    Press { data: String, ack: PressAck },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatCommand { Stop, Status, Run, Triggers }
#[derive(Debug, Clone)]
pub struct InboundFile { pub file: InboundFileRef, pub file_name: String, pub media_type: Option<String>, pub size_bytes: Option<u64>, pub caption: Option<String> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundFileRef(pub String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PressAck(pub String);

// ======================= trigger hub (personal profile) =======================

#[derive(Debug, Clone)]
pub struct WorkspaceRef { pub id: WorkspaceId, pub name: String }
#[derive(Debug, Clone)]
pub struct ChatContext { pub channel: ChannelKind, pub pairing_id: ChannelPairingId, pub message_id: String, pub workspace: Option<WorkspaceRef> }
#[derive(Debug, Clone)]
pub struct SavedChatFile { pub file_name: String, pub media_type: Option<String>, pub size_bytes: u64, pub content: ContentRef, pub display_path: String }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcome { Started, NotMatched }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOwner { Gateway, Triggers }
#[derive(Debug, Clone)]
pub struct ActionButton { pub label: String, pub owner: ActionOwner, /** ≤ 2 KiB serialised */ pub payload: serde_json::Value }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRoute { LastChannel, AllPaired }
#[derive(Debug, Clone)]
pub struct PersonMessage { pub text: String, pub buttons: Vec<ActionButton>, pub attachment: Option<OutboundFile>, pub route: MessageRoute }
#[derive(Debug, Clone)]
pub struct ChatForm { pub title: String, pub verb: String, pub fields: Vec<ChatFormField>, pub owner: ActionOwner, pub payload: serde_json::Value }
#[derive(Debug, Clone)]
pub struct ChatFormField { pub name: String, pub label: String, pub required: bool, pub kind: ChatFieldKind, pub value: Option<ChatFieldValue>, pub note: Option<String> }
#[derive(Debug, Clone)]
pub enum ChatFieldKind { Text { max_chars: u32 }, Choice { options: Vec<String> }, Document }
#[derive(Debug, Clone)]
pub enum ChatFieldValue { Text(String), Document(ContentRef) }
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct HandlerError { pub code: &'static str, pub retryable: bool }

/// Implemented by the Trigger Service; registered once at start.
#[async_trait::async_trait]
pub trait ChatTriggerHandler: Send + Sync + 'static {
    async fn on_file(&self, ctx: &ChatContext, file: &SavedChatFile) -> Result<FileOutcome, HandlerError>;
    async fn on_run_command(&self, ctx: &ChatContext, rest: &str) -> Result<(), HandlerError>;
    async fn on_triggers_command(&self, ctx: &ChatContext) -> Result<(), HandlerError>;
    async fn on_action(&self, ctx: &ChatContext, payload: &serde_json::Value) -> Result<(), HandlerError>;
    async fn on_form_done(&self, ctx: &ChatContext, payload: &serde_json::Value, values: &[(String, ChatFieldValue)]) -> Result<(), HandlerError>;
    async fn on_run_again(&self, ctx: &ChatContext, crew_run_id: CrewRunId) -> Result<(), HandlerError>;
    /// "Started by an event: invoice created, INV-2291" for cards of triggered runs.
    async fn started_by_line(&self, asker: &ApprovalAsker) -> Option<String>;
}

// ======================= the core, personal profile =======================

#[derive(Debug, Clone)]
pub struct ChatTurn { pub channel: ChannelKind, pub text: String, pub attachments: Vec<ContentRef> }
#[derive(Debug, Clone)]
pub struct RunningLine { pub title: String, pub state_words: String }
#[derive(Debug, Clone)]
pub enum NoticeSubject { Run(RunId), CrewRun(CrewRunId) }
#[derive(Debug, Clone)]
pub struct NeedsYouLine {
    /// "Finished: Supplier summary", "Didn't finish: Q3 comparison — a step failed twice".
    pub text: String,
    /// Set for a finished workflow run, so **Run again** is offered.
    pub run_again: Option<CrewRunId>,
}
#[derive(Debug, Clone)]
pub struct ContentFile { pub file_name: String, pub media_type: String, pub bytes: bytes::Bytes, pub display_path: String }
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct CoreError { pub code: &'static str, pub retryable: bool }

/// What the gateway needs from the core in the personal profile. Implemented by the personal-server
/// program (SA-RECON-01): `conversation_turn` → `OrdinHost::send_message`; `answer_question` and
/// `submit_form` → `OrdinHost::answer_block`; the run, step and content reads over the Session
/// Supervisor, the Journal and the Workspace Service, which this crate does not depend on.
#[async_trait::async_trait]
pub trait PersonalCore: Send + Sync + 'static {
    async fn conversation_turn(&self, turn: ChatTurn) -> Result<(), CoreError>;
    async fn current_workspace(&self) -> Result<Option<WorkspaceRef>, CoreError>;
    async fn answer_question(&self, block_id: BlockId, answer: String) -> Result<(), CoreError>;
    async fn submit_form(&self, block_id: BlockId, values: Vec<(String, ChatFieldValue)>) -> Result<(), CoreError>;
    async fn pause_all_runs(&self) -> Result<u32, CoreError>;
    async fn running(&self) -> Result<Vec<RunningLine>, CoreError>;
    /// `None` for runs inside a crew round and for states that are not finished work or a failure.
    async fn needs_you_line(&self, subject: NoticeSubject, ended_state: &str) -> Result<Option<NeedsYouLine>, CoreError>;
    async fn content_file(&self, content: &ContentRef) -> Result<ContentFile, CoreError>;
    async fn content_label(&self, content: &ContentRef) -> Result<String, CoreError>;
    async fn step_texts(&self, run_id: RunId) -> Result<Vec<String>, CoreError>;
    /// The workspace's current Ordin conversation run (program: `OrdinHost::current_run`), the run a saved
    /// block file is recorded against (SA-CLOSE-01); `None` when Ordin has no run there.
    async fn ordin_run(&self, workspace_id: WorkspaceId) -> Result<Option<RunId>, CoreError>;
}

/// The server program computes this after recovery (Always on) and hands it over once.
#[derive(Debug, Clone, Copy)]
pub struct RestartSummary { pub at: Timestamp, pub runs_resumed: u32, pub approvals_waiting: u32 }

// ======================= the gateway =======================

/// Every sentence the gateway sends; en-GB defaults; the program may pass localised values.
#[derive(Debug, Clone)]
pub struct ChatTexts { /* one `String` field per row of step L; `ChatTexts::en_gb()` builds them */ }
impl ChatTexts { pub fn en_gb() -> Self; }

pub struct PersonalDeps {
    pub core: Arc<dyn PersonalCore>,
    pub approvals: Arc<ordin_approval::ApprovalService>,
    pub workspaces: Arc<ordin_workspaces::WorkspaceService>,
    pub profile: Arc<ordin_organisation_profile::OrganisationProfileService>,
    /// The server owner's choice from the setup command, used only with no verified profile (A-40).
    pub owner_channels: Vec<ChannelKind>,
}

pub struct GatewayDeps {
    pub journal: ordin_journal::Journal,
    pub datastore: ordin_journal::Datastore,
    pub texts: ChatTexts,
    pub mode: GatewayMode,
}
pub enum GatewayMode { Personal(PersonalDeps), Hosted(HostedDeps), Setup(SetupDeps) }

/// The setup command's gateway (`ordin-server setup pair-chat`, in-process with no server running):
/// pairing calls only, no conversation and no send queues (row 465).
pub struct SetupDeps {
    pub profile: Arc<ordin_organisation_profile::OrganisationProfileService>,
    pub owner_channels: Vec<ChannelKind>,
}

pub struct ChannelGateway { /* private: Arc<Inner> */ }

impl ChannelGateway {
    pub const MODULE: &'static str = "channel_gateway";
    /// `ModuleMigrations { module: "channel_gateway", sqlite: [...], postgres: [...] }` (Data Model).
    pub fn migrations() -> ordin_journal::ModuleMigrations;

    pub async fn new(deps: GatewayDeps) -> Result<Arc<Self>, GatewayError>;
    /// Registers with the Journal as an `EventSink` named `channel_gateway`, starts the send
    /// queues, the pairing sweeper and (agent profile) the delivery, give-up and comparison loops.
    /// Cancelling `shutdown` is the orderly stop: `receive` and `receive_hosted` refuse new items
    /// with `CHAN_STOPPING`, the outboxes drain for at most 10 s, then every task ends (step A.4).
    pub async fn start(self: &Arc<Self>, shutdown: CancellationToken) -> Result<(), GatewayError>;

    // ---- personal profile ----
    pub fn register_adapter(&self, adapter: Arc<dyn ChannelAdapter>) -> Result<(), GatewayError>;
    pub fn register_trigger_handler(&self, handler: Arc<dyn ChatTriggerHandler>) -> Result<(), GatewayError>;
    /// Called by adapters for every item from a private chat that is not a pairing attempt.
    pub async fn receive(&self, item: InboundItem) -> Result<(), GatewayError>;
    /// Called by adapters for every item that is not from a private chat or not from the paired
    /// account; counted without content.
    pub fn count_ignored(&self, channel: ChannelKind, reason: IgnoredReason);
    pub async fn report_status(&self, channel: ChannelKind, status: ChannelStatus);
    pub fn is_allowed(&self, channel: ChannelKind) -> bool;

    pub async fn begin_pairing(&self, channel: ChannelKind, paired_by: &str) -> Result<ChannelPairing, GatewayError>;
    pub async fn complete_pairing(&self, pairing_id: ChannelPairingId, chat_user_id: &str, private_chat_id: &str) -> Result<ChannelPairing, GatewayError>;
    pub async fn expire_pairing(&self, pairing_id: ChannelPairingId) -> Result<(), GatewayError>;
    pub async fn end_pairing(&self, channel: ChannelKind) -> Result<(), GatewayError>;
    pub async fn active_pairing(&self, channel: ChannelKind) -> Result<Option<ChannelPairing>, GatewayError>;
    pub async fn set_needs_you_both(&self, both: bool) -> Result<(), GatewayError>;

    pub async fn send_to_person(&self, msg: PersonMessage) -> Result<(), GatewayError>;
    pub async fn ask_form(&self, ctx: &ChatContext, form: ChatForm) -> Result<(), GatewayError>;
    pub async fn chat_label(&self, pairing_id: ChannelPairingId) -> Result<String, GatewayError>;
    pub async fn send_restart_notice(&self, summary: RestartSummary) -> Result<(), GatewayError>;
    /// Always on: the harness sign-in has ended (11 §5.23 words), sent at once on the last channel.
    pub async fn send_sign_in_ended(&self) -> Result<(), GatewayError>;
    /// Always on: a newer ordin version is available (`texts.update_available`), sent once on the last channel (row 459).
    pub async fn send_update_available(&self, version: &str) -> Result<(), GatewayError>;

    // ---- agent profile ----
    pub fn register_link(&self, link: Arc<dyn HostedChatLink>) -> Result<(), GatewayError>;
    pub async fn receive_hosted(&self, event: HostedEvent) -> Result<HostedAck, GatewayError>;
    /// Job Runner's `JobOutcomeNotifier`, implemented by the program over this call: a direct chat
    /// to `user` on `connection` through `HostedChatLink::post_direct` (row 469).
    pub async fn notify_job_ended(&self, connection: &str, user: &str, text: &str) -> Result<(), GatewayError>;
    /// Agent profile: the Retention and Erasure providers over `chat_replies` and
    /// `chat_pending_questions` (`ProvidedContent::ChatReplies`, `PendingQuestions`), registered by
    /// the program with `Retention::register` (O124).
    pub fn retained_content(self: &Arc<Self>) -> Vec<Arc<dyn ordin_retention::RetainedContent>>;
}

// Called by:
// - `new`, `start`, `set_needs_you_both`, `send_restart_notice`, `send_sign_in_ended`, `send_update_available`,
//   `end_pairing`: Ordin Server program.
// - `new` with `GatewayMode::Setup`, `end_pairing`, `active_pairing`: the setup command (Ordin Server); pairing starts
//   at the adapters (OrdinServer.md SA-RECON-06).
// - `register_adapter` (each in its `start`), `receive`, `count_ignored`, `report_status`, `is_allowed`,
//   `complete_pairing`, `expire_pairing`, `begin_pairing`, `active_pairing`: Telegram Adapter, Slack Channel Adapter.
// - `send_to_person`, `ask_form`, `chat_label`, `register_trigger_handler`: Trigger Service.
// - `register_link`, `receive_hosted`: Slack Channel Adapter (agent profile).
// - `notify_job_ended`: the program's `JobOutcomeNotifier` (Job Runner).
// - `retained_content`: the program, for Retention and Erasure.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoredReason { NotPaired, OtherUser, NotPrivate, ChannelNotAllowed }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct GatewayError { pub code: &'static str, pub retryable: bool }

// ======================= agent profile =======================

pub struct HostedDeps {
    pub core: Arc<dyn HostedAgentCore>,
    pub approvals: Arc<ordin_approval::ApprovalService>,
    pub connections: Vec<ordin_types::agent_profile::ChatConnection>,
    pub package_chat_triggers: Vec<ordin_types::agent_profile::ChatTrigger>,
    pub reach: Option<ordin_types::agent_profile::ReachConfig>,
    pub approvers: Option<ordin_types::agent_profile::ApproversConfig>,
    /// The instance name used for leases (`hostname`).
    pub instance: String,
    /// Counter sink; the program forwards to Telemetry in the agent profile (as the Run Queue spec's `MetricsSink`).
    pub metrics: Arc<dyn MetricsSink>,
    /// Retention and Erasure's scopes (X4, R13); this crate keeps no scopes table.
    pub scopes: Arc<dyn ordin_retention::ScopeRegistry>,
}

pub trait MetricsSink: Send + Sync {
    fn add(&self, name: &'static str, labels: &[(&'static str, &str)], by: u64);
}

/// Implemented by the Slack Channel Adapter, one per chat connection.
#[async_trait::async_trait]
pub trait HostedChatLink: Send + Sync + 'static {
    fn connection(&self) -> &str;
    fn agent_user_id(&self) -> &str;
    async fn post(&self, post: HostedPost) -> Result<String, LinkError>;          // returns the message id
    async fn post_direct(&self, user: &str, text: &str, buttons: &[HostedButton]) -> Result<(String, String), LinkError>; // (channel, message id)
    async fn update(&self, channel: &str, message: &str, text: &str, buttons: &[HostedButton]) -> Result<(), LinkError>;
    async fn react(&self, channel: &str, message: &str, name: &str) -> Result<(), LinkError>;
    async fn unreact(&self, channel: &str, message: &str, name: &str) -> Result<(), LinkError>;
    async fn thread(&self, channel: &str, thread: &str, limit: u32) -> Result<Vec<HostedMessage>, LinkError>;
    async fn file(&self, file_id: &str, max_bytes: u32) -> Result<ContextItem, LinkError>;
    async fn channel_documents(&self, channel: &str, max_bytes: u32) -> Result<Vec<ContextItem>, LinkError>;
    async fn may_read_thread(&self, user: &str, channel: &str) -> Result<bool, LinkError>;
    async fn member_channels(&self) -> Result<MemberChannels, LinkError>;
    async fn open_form(&self, trigger: &str, form: HostedForm) -> Result<(), LinkError>;
    /// Replaces an ephemeral message through the `response_url` of the event that answered it (ADR-107).
    async fn replace_ephemeral(&self, response_url: &str, text: &str, buttons: &[HostedButton]) -> Result<(), LinkError>;
}

#[derive(Debug, Clone)]
pub struct HostedPost { pub channel: String, pub thread: Option<String>, pub only_to: Option<String>, pub text: String, pub buttons: Vec<HostedButton> }
#[derive(Debug, Clone)]
pub struct HostedButton { pub action: String, pub label: String, pub value: String, pub url: Option<String>,
    /// Read only when `url` is set: a link button or a Slack workflow button (ADR-107).
    pub kind: HostedButtonKind }
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostedButtonKind { #[default] Url, WorkflowLink }
#[derive(Debug, Clone)]
pub struct HostedMessage { pub id: String, pub sender: String, pub from_program: bool, pub text: String, pub files: Vec<HostedFile> }
#[derive(Debug, Clone)]
pub struct HostedFile { pub id: String, pub name: String, pub media_type: String, pub size_bytes: Option<u64> }
/// A context piece for a run, or what was left out and why (wrightrun's ContextPiece/LeftOut).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextItem {
    Piece { piece: String /* "transcript" | "image" | "document" | "note" | "knowledge" | "recent" (ADR-107) */, label: String, media_type: String, content: String, size_bytes: u64, cut: bool,
            /// ADR-107: the context step (position from 1) that gave the piece; None for an item no step gave (the hand-off fact).
            #[serde(default, skip_serializing_if = "Option::is_none")] step: Option<u16>,
            /// ADR-107: messages, images, canvases, documents or hits the piece holds (1 for a single image or document).
            #[serde(default)] items: u32,
            /// ADR-107: the UTF-8 size observed before a cut; None when not cut.
            #[serde(default, skip_serializing_if = "Option::is_none")] observed_bytes: Option<u64> },
    LeftOut { piece: String, label: String, reason: String /* "read-failed" | "wrong-type" | "too-large" | "over-total" | "empty" | "skipped" (ADR-107) */,
              #[serde(default, skip_serializing_if = "Option::is_none")] step: Option<u16>,           // ADR-107
              #[serde(default, skip_serializing_if = "Option::is_none")] observed_bytes: Option<u64> },// ADR-107: the reported size of an item left out for size
}
#[derive(Debug, Clone)]
pub struct MemberChannels { pub channels: Vec<String>, pub complete: bool }
#[derive(Debug, Clone)]
pub struct HostedForm { pub form: String, pub title: String, pub fields: Vec<HostedFormField>, pub metadata: String }
#[derive(Debug, Clone)]
pub struct HostedFormField { pub name: String, pub label: String, pub kind: HostedFieldKind, pub initial: Option<String>, pub optional: bool }
#[derive(Debug, Clone)]
pub enum HostedFieldKind { Flag, Number { min: Option<f64>, max: Option<f64>, decimal: bool }, Text { max_chars: u32, multiline: bool }, Selection { options: Vec<String>, selected: Vec<String> }, Lines { max_chars: u32 } }

/// What the Slack adapter hands over, already parsed.
#[derive(Debug, Clone)]
pub enum HostedEvent {
    Message { connection: String, envelope_id: String, kind: HostedMessageKind, channel: String, thread: Option<String>,
              message: Option<String>, user: String, from_program: bool, text: String, files: Vec<HostedFile>, trigger: Option<String>,
              /// A slash command's address for replacing its ephemeral answers (ADR-107); never logged.
              response_url: Option<String> },
    Action { connection: String, envelope_id: String, action: String, value: String, user: String, channel: String, thread: Option<String>,
             /// The press's address for replacing the ephemeral message it was on (ADR-107); never logged.
             response_url: Option<String> },
    FormSubmitted { connection: String, envelope_id: String, form: String, metadata: String, user: String, values: BTreeMap<String, Vec<String>> },
    Joined { connection: String, envelope_id: String, channel: String },
    Left { connection: String, envelope_id: String, channel: String },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedMessageKind { Mention, ThreadMention, DirectMessage, Command }
/// What the adapter sends back on the envelope's acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedAck { Plain, FormErrors(BTreeMap<String, String>) }
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct LinkError { pub code: &'static str }

/// What the gateway needs from the agent profile's core. Implemented by the server program.
#[async_trait::async_trait]
pub trait HostedAgentCore: Send + Sync + 'static {
    async fn enqueue(&self, req: HostedRunRequest) -> Result<String /* run_id */, CoreError>;
    async fn run_views(&self, run_ids: &[String]) -> Result<Vec<HostedRunView>, CoreError>;
    async fn source_entries(&self, scope: ScopeId, item_ids: &[String]) -> Result<Vec<String>, CoreError>;
    async fn setting_text(&self, scope: ScopeId, name: &str) -> Result<Option<String>, CoreError>;
    async fn available_actions(&self, connection: &str, scope: ScopeId) -> Result<Vec<String>, CoreError>;
    /// The entries of a declared `list` setting in the scope, each a map from field name to text, in stored order (ADR-107).
    async fn setting_entries(&self, scope: ScopeId, name: &str) -> Result<Vec<BTreeMap<String, String>>, CoreError>;
    async fn settings_form(&self, scope: ScopeId) -> Result<Option<HostedForm>, CoreError>;
    async fn submit_settings(&self, scope: ScopeId, metadata: &str, values: &BTreeMap<String, Vec<String>>, by: &str) -> Result<SubmitOutcome, CoreError>;
    async fn entry_command(&self, scope: ScopeId, command: &ordin_types::agent_profile::EntryCommand, rest: &str, by: &str) -> Result<EntryOutcome, CoreError>;
    async fn job_command(&self, scope: ScopeId, command: &ordin_types::agent_profile::JobCommand, asker: &str) -> Result<String /* text */, CoreError>;
    async fn version_lines(&self) -> Result<Vec<String>, CoreError>;
    async fn scope_enabled_first_time(&self, connection: &str, scope: ScopeId) -> Result<u32 /* jobs asked */, CoreError>;
    /// ADR-107: the Knowledge Service's `lookup` for a context step (KnowledgeService.md H.9), visibility rules applied.
    async fn context_lookup(&self, scope: ScopeId, source: &str, question: &str, limit: u32) -> Result<Vec<ContextHit>, CoreError>;
    /// ADR-107: the Knowledge Service's `recent` for a context step, newest first, visibility rules applied.
    async fn context_recent(&self, scope: ScopeId, source: &str, limit: u32) -> Result<Vec<ContextHit>, CoreError>;
}

/// ADR-107: one knowledge item given to a context step; the program maps the Knowledge Service's item.
#[derive(Debug, Clone)]
pub struct ContextHit { pub item_id: String, pub origin_time: Timestamp, pub text: String }

#[derive(Debug, Clone)]
pub struct HostedRunRequest {
    pub connection: String, pub asker: String, pub scope_id: ScopeId, pub run_id: String,
    /// `Some(command)` when the trigger `starts` a command (the text becomes its arguments).
    pub command: Option<String>, pub text: String,
    pub context: Vec<ContextItem>, pub from_program: bool,
}
#[derive(Debug, Clone)]
pub struct HostedRunView {
    pub run_id: String,
    /// `queued` | `executing` | `paused` | `ended` | `cancelled` | `interrupted` (2C RunRecordState).
    pub state: String,
    /// `completed` | `failed` | `run_limit` when `state = ended` (2C RunEndKind).
    pub end_kind: Option<String>,
    pub reply: Option<String>,
    pub safe_reply: bool,
    pub ended_by_control: bool,
    pub delivery: Option<String>, // "asker" | "thread"
    pub actions_attached: Vec<String>,
    pub actions_asked: Vec<String>,
    pub opened_items: Vec<String>,
    pub error_code: Option<String>,
    pub ended_at: Option<Timestamp>,
}
#[derive(Debug, Clone)]
pub enum SubmitOutcome { Saved, FieldErrors(BTreeMap<String, String>) }
#[derive(Debug, Clone)]
pub enum EntryOutcome { Text(String), OpenForm(HostedForm) }
```

### Example

Personal profile — the paired person writes in Telegram, Ordin asks to send an email, the card goes to both channels:

```text
InboundItem { channel: Telegram, sender_id: "48213377", chat_id: "48213377", is_private: true, message_id: "912",
              body: Text { text: "Summarise this week's supplier emails and send Priya the summary" } }
→ PersonalCore::conversation_turn(ChatTurn { channel: Telegram, text: "Summarise…", attachments: [] })
→ Slack (also paired) receives: "Ordin is working on your Telegram request. Cards also appear here."

Journal event approval_requested { card: { question: "Send the supplier summary to Priya Shah?", verb: "Send",
  where_line: "Email, from asha.patel@acme.co.uk", affects: [Recipient { name: "Priya Shah", … }],
  changes: "One email with the summary attached", buttons: { approve: "q8Zt3VxN0pLk2mRb7sYw1A", reject: "Lm4Qp0Rt7Vx2Zb5Nc8Df3g", show_details: "Hh2Kd9Ws1Qe6Zr3Tb8Nm4v" } } }
→ Telegram and Slack each receive:
  "Send the supplier summary to Priya Shah?
   Where: Email, from asha.patel@acme.co.uk
   Affects: Priya Shah
   Changes: One email with the summary attached"
   [Send → c:q8Zt3VxN0pLk2mRb7sYw1A] [Not now → c:Lm4Qp0Rt7Vx2Zb5Nc8Df3g] [Show details → c:Hh2Kd9Ws1Qe6Zr3Tb8Nm4v]

Slack press c:q8Zt3VxN0pLk2mRb7sYw1A → ApprovalService::decide(ApprovalPress { card_ref, decision: Approve }, PairedChat { channel: Slack })
→ Slack card edited: "… Approved by you, 10:44" (no buttons); Telegram card edited: "… Decided in Slack, 10:44" (no buttons)
```

Agent profile — a mention in an enabled channel:

```text
HostedEvent::Message { connection: "support", kind: Mention, channel: "C0HELPDESK", thread: None, message: Some("1729412345.000100"),
  user: "U07ASKER", text: "please email the supplier the corrected invoice", … }
→ scope C0HELPDESK enabled → HostedAgentCore::enqueue(HostedRunRequest { scope_id, run_id: "0192a1e0-…", text: "please email…" })
→ pending question written; reaction "eyes" added; HostedAck::Plain
… run paused → approval_text "This request is waiting for approval." posted to U07ASKER only
… approval card to U07LEAD's direct chat; U07LEAD presses Approve → decide(…, Approver { subject: "00u1lead", groups: [], via: Chat })
… run ended → reply posted to the asker; reaction "white_check_mark"
```

## Internal Logic

**A. Start**

1. `new`: personal mode reads `gateway_settings` (last channel, needs-you-both), sweeps pending pairings whose `code_expires_at` has passed to `ended`; agent mode calls `scopes.agent_wide()` (Retention and Erasure creates it on first call).
2. `start`: `journal.subscribe(self as EventSink)`; start one send-queue worker per registered adapter (step J); start the pairing sweeper (every 60 s, step B.6). Agent mode: start the delivery loop, the give-up loop and one comparison loop per link (step K).
3. `register_adapter`: one adapter per `ChannelKind`; a second registration → `CHAN_ADAPTER_ALREADY_SET`. **Setup mode** (`GatewayMode::Setup`): `new` runs the migrations, `start` is not called, and every call other than `is_allowed`, `begin_pairing`, `complete_pairing`, `expire_pairing`, `end_pairing` and `active_pairing` → `CHAN_SETUP_ONLY`. `register_trigger_handler`: once → else `CHAN_HANDLER_ALREADY_SET`. Without a trigger handler, **/run** and **/triggers** answer `texts.triggers_unavailable` and files always go to Ordin.
4. **Stop.** When the `shutdown` token passed to `start` is cancelled: set *stopping*; `receive` and `receive_hosted` return `CHAN_STOPPING` (retryable) and adapters stop polling; each send-queue worker sends the rows already in its outbox for at most 10 s, then exits leaving the rest for the next start; the Journal subscription is dropped. The program cancels the token; there is no separate stop call (row 461).

**B. Pairing (personal profile)**

1. `is_allowed(kind)`: with a verified profile (`ProfileSnapshot.source = Verified`) → `document.channels.allowed` contains `kind` (absent section → `[slack]`, 2D-2 default); with no verified profile → `owner_channels` contains `kind` (A-40).
2. `begin_pairing(kind, paired_by)`: not allowed → `CHAN_CHANNEL_NOT_ALLOWED`. Insert `channel_pairings (state = pending, code_expires_at = now + 10 min, paired_by)`. A `pending` row of the same kind is ended first (only one code per kind is valid). The adapter generates and holds the code (its own spec); the gateway never sees it.
3. `complete_pairing(id, user, chat)`: the row must be `pending` (else `CHAN_PAIRING_NOT_PENDING`) and `code_expires_at > now` (else `CHAN_CODE_EXPIRED`); `user` and `chat` 1–64 chars (else `CHAN_IDS_MISSING`). In one transaction: end any `paired` row of the same kind (`ended_at = now`, its open `card_references` → `closed`), set this row `paired`, `paired_at = now`. Then send the welcome (step L, *welcome*) to `chat`, set `last_channel = kind` when no channel was last used, journal `chat_paired { channel }`.
4. `expire_pairing(id)`: `pending` → `ended` (used by adapters on expiry or after 5 wrong codes).
5. `end_pairing(kind)`: `paired` or `pending` row → `ended`; open card references of it → `closed`. Only the setup command calls it (`CHAN_NOT_SETUP_COMMAND` is enforced by the program, which exposes it only there).
6. Sweeper: every 60 s, `pending` rows past `code_expires_at` → `ended`.

**C. Receiving (personal profile) — `receive(item)`**

1. `is_allowed(item.channel)` false → `count_ignored(ChannelNotAllowed)`, return `Ok`.
2. Load the `paired` row for `item.channel`. None → `count_ignored(NotPaired)`. `item.sender_id ≠ chat_user_id` or `item.chat_id ≠ private_chat_id` → `count_ignored(OtherUser)`. No reply in either case (03:961).
3. Build `ChatContext { channel, pairing_id, message_id, workspace: core.current_workspace() }` (a `CoreError` → `workspace: None`, logged).
4. **Channel switch.** When `item.body` is not a `Press` and `last_channel ≠ item.channel`: if the previous last channel is still paired, enqueue `texts.other_channel_notice(<this channel's name>)` there; set `last_channel = item.channel` (persisted).
5. Route by body:
   - `Command { Stop, .. }` → D.1; `Status` → D.2; `Run` → `handler.on_run_command(ctx, rest)`; `Triggers` → `handler.on_triggers_command(ctx)`.
   - `Voice` → reply `texts.voice_note` (ADR-078).
   - `Unsupported` → reply `texts.unsupported`.
   - `File(f)` → step E.
   - `Press { data, ack }` → `adapter.ack_press(ack)`, then step F.
   - `Text { text }` → step C.6.
6. Text, in this order:
   1. An active form is waiting for a text field → step G.3.
   2. An open question with `allow_other` is waiting → `core.answer_question(block_id, text)`; the question's buttons are removed (edit).
   3. The text, trimmed, lower-cased, with trailing `.`/`!` removed, is one of `yes`, `y`, `ok`, `okay`, `approve`, `approved`, `go ahead`, `send it`, `do it`, `👍`, and at least one approval card is open → reply `texts.use_buttons` ("Use the buttons on the card to approve.", 11 §5.23). Nothing is forwarded (decision 10).
   4. Otherwise `core.conversation_turn(ChatTurn { channel, text, attachments: [] })`. Text is NFC-normalised and cut at 16,384 chars (LW-10) with `texts.cut_notice` sent when cut. A `CoreError` → reply `texts.core_unavailable`.
7. `count_ignored(channel, reason)` increments an in-memory counter per (channel, reason); every hour (and at shutdown) non-zero counters are written as one journal entry `chat_messages_ignored { channel, reason, count, from, to }` with no content, and reset.

**D. Commands**

1. **/stop** (Slack: the word **stop**): `n = core.pause_all_runs()`; reply `texts.stopped(n)` ("Paused 3 runs." / "Nothing was running.").
2. **/status**: `lines = core.running()`; none → `texts.nothing_running`; else one line per run "{title} · {state words}", at most 20 lines, then "and {n} more".

**E. Files (personal profile)**

1. `size_bytes` known and > `limits.max_receive_file_bytes` → reply `texts.file_too_large_in(limit)` and stop.
2. No workspace in `ctx` → reply `texts.no_workspace_for_file` and stop.
3. `adapter.fetch_file(file, max_receive)`; error → `texts.file_not_received`.
4. `workspaces.save_to_inbox(ws, file_name, bytes, InboxOrigin::Chat { channel: ctx.channel })` → `PlaceFileRef` (SA-CLOSE-01); `saved.content` below is `ContentRef::PlaceFile { place_id, relative_path }` and `display_path` is the reference's. Error → `texts.file_not_saved`.
5. With a trigger handler: `handler.on_file(ctx, SavedChatFile { … })`. `Started` → stop (the handler replied). `NotMatched` or error → continue.
6. `core.conversation_turn(ChatTurn { text: caption or "", attachments: [saved.content] })`: Ordin says where the file went (11 §5.23 *Files*).

**F. Button presses**

1. `data` must be `c:` or `x:` followed by 22 base64url chars; else ignored (logged `chat.press.malformed`).
2. **`c:` — approval card.** Find `card_references` row with `reference = <22 chars>`. None → reply `texts.card_closed` and stop. Its pairing must be the presser's channel's paired row — any paired row of either kind is accepted, because the same card reference goes to both channels (step I.2), and the sender check of C.2 has already passed. `state ≠ open` → reply `texts.card_closed`.
   - `button = show_details` → step I.4; the reference stays `open`.
   - `approve` / `not_now` → `approvals.decide(ApprovalPress { card_ref, decision: Approve | Reject }, PressOrigin::PairedChat { channel })`.
     - `Ok(outcome)` → mark this reference `used` (`used_at`), the card's other references `closed`; remember `pressed_on = channel` for the edit of step I.3 (the edit itself follows the `approval_decided` event).
     - `AlreadyDecided { decided_on, .. }` → reply `texts.already_decided(decided_on)` ("Already decided in Telegram"); close the references.
     - `CardClosed` → `texts.card_closed`. `CardRefUnknown`, `CardRefMismatch` → `texts.card_closed`, log the code.
     - retryable error (`APPR_STORE_UNAVAILABLE`, journal) → `texts.try_again`; the reference stays `open`.
3. **`x:` — chat action.** Find `chat_actions` row by `reference`; none, `used`, or older than 7 days → `texts.action_closed`. Mark it `used` when its `single_use` is true. By `owner`:
   - `Triggers` → `handler.on_action(ctx, payload)`.
   - `Gateway` → by `payload.t`: `choice` (question choice: `core.answer_question(block_id, choices[i])`, then remove the question's buttons and close its other actions), `form_choice` (G.3), `form_submit` (G.4), `show_it` (send every pending approval card again: `approvals.list_pending(None, cursor)` page by page, step I.2 for each), `run_again` (`handler.on_run_again(ctx, crew_run_id)`).

**G. Questions and forms**

1. **Question block** → text "{question}" with one button per choice (`x:` actions, owner Gateway, `{"t":"choice","b":block_id,"i":index}`), plus `texts.or_type_answer` when `allow_other`; record it as the waiting question (one at a time; a newer question replaces the older, whose buttons stay until pressed and then answer `texts.action_closed`).
2. **Form** (a `FormBlock` from Ordin, or `ask_form` from the Trigger Service): one active form at a time; a new form replaces the active one and sends `texts.form_set_aside`. Fields with a value already (`filled_by_ordin` or `value` set) are not asked. Fields are asked one at a time: `Text` → "{label}?" (+ note) ; `Choice` → "{label}?" with one button per option; `Document` → `texts.send_file_for(label)`; `Place` (Ordin's form) → treated as `Text` naming the place.
3. An answer fills the current field: text for `Text` (cut to `max_chars`, with `texts.cut_notice`), a press for `Choice`, a file for `Document` (saved as E.3–E.4; the saved `ContentRef` is the value; the file goes to no trigger and no Ordin turn). A text answer to a `Document` field → `texts.send_file_for(label)` again. `skip` on an optional field leaves it empty.
4. After the last field: one summary message, one line per field "{label}: {value}" (documents by file name), with one button whose label is the form's `verb` (`{"t":"form_submit"}`). Pressing it: Ordin's form → `core.submit_form(block_id, values)`; Trigger Service form → `handler.on_form_done(ctx, payload, values)`. The form ends. A text answer while the summary waits → C.6.4 (the form stays until replaced).
5. Form state is kept in memory; after a restart it is lost and the person starts again (*assumed here*: forms in the chat take minutes; persisting them adds a table for little gain).

**H. Outgoing content (personal profile) — `EventSink::deliver(event)`**

Each `JournalEvent` is decoded by `kind` into the `ServiceEvent` variant; unknown kinds are ignored.

1. `approval_requested` → step I.2. `approval_decided` → step I.3.
2. `block_shown { block }` → text form (to the last channel):
   - `message`: the text; each attachment through `core.content_file` sent as a file (H.4).
   - `question`, `form`: step G.
   - `documents`: "Documents:" then up to 10 `core.content_label` lines, then "and {n} more".
   - `preview`, `document`: the label, then the file (H.4).
   - `changes`: "Changes to {label of after}" and the `after` file.
   - `plan_progress`: the current *Now* step sentence and "step {N} of {M}" from the plan cache (H.3).
   - `table` and `chart`: the title, then up to 10 rows, one line each, cells joined by " · " (money "GBP 412.50" with the column's decimals or 2; percentage ×100 with " %"; numbers with `decimals`; dates as given; empty as "–"; a document link by its label); then "and {n} more" when there are more rows; the full rows always attached as `{title}.csv` (RFC 4180, UTF-8, header row of column names). For a chart, the columns are `x` then the `series` columns.
   - `key_figures`: one line per figure "{label}: {value} {unit}", with " ({up|down} {abs % change} on {against})" when `change` is set and the earlier value is not 0.
   - every other kind (**no text form**, *assumed here*: the HLD defines text forms only for tables, charts, progress and forms, 11 §5.23): `texts.no_text_form(kind words)` and, when `core.ordin_run(workspace_id)` gives a run, the block's JSON saved with `workspaces.save_generated(workspace_id, "{kind}-{HHMM}.json", json, run_id)` (SA-CLOSE-01); the reply names the saved `PlaceFileRef.display_path`. With no Ordin run, or a save error, the reply is `texts.no_text_form` alone and `chat.block_not_saved` is logged.
3. `plan_changed` → cache `run_id → steps` (`core.step_texts`). `step_marked { mark: now }` → progress line `"{step text} · step {position} of {count}"`, sent to the last channel at most once a minute per run; a newer mark inside the minute replaces the waiting one, which is sent when the minute ends (11 §5.23).
4. Files: `bytes.len() ≤ limits.max_send_file_bytes` → sent as `OutboundFile` (`is_image` when `media_type` starts with `image/`); larger → `texts.file_too_large_out(file_name, display_path)` ("Saved as Supplier summary.xlsx in Procurement › Results; it's too large to send here.").
5. `run_state_changed { state ∈ completed, failed, stopped_at_limit }` and `crew_round_changed { state ∈ finished, failed, stopped_at_limit, killed }` → `core.needs_you_line(subject, state)`; `Some(line)` → send `line.text` (with **Run again** `{"t":"run_again","r":id}` when `run_again` is set) on the *Needs you* route (I.5).
6. Other kinds (`run_progress`, `start_up_report_ready`, `reset`) send nothing.
7. `lagged(next_seq)` → re-read with `journal.events_since(next_seq − 1, 500)` until caught up; cards already delivered are not sent twice (step I.2 checks `card_deliveries`).

**I. Approval cards**

1. **Text form of a card** (personal profile): optional first line from `handler.started_by_line(card.asked_by)`; the `question`; "Where: {where_line}"; "Affects: {labels}" (up to 5, then "and {n} more"; a recipient as "{name}" plus " (outside the company)" when `outside_company`); "Changes: {changes}"; when the card is sent more than 60 s after `created_at`, `texts.made_at(HH:MM)`. Buttons: `card.verb` → `c:{buttons.approve}`; "Not now" (`reject_style = not_now`) or "Reject" → `c:{buttons.reject}`; "Show details" → `c:{buttons.show_details}` when present. (*Reading taken*: the approve button carries the card's verb, as on the desktop — "Approve", "Send", "Post" — HLD disagreement B9-D5.)
2. **Sending** (`approval_requested`): for each `paired` row of an allowed channel: insert `card_references` rows for each button (`reference` = the `CardRef` string, `channel_pairing_id` = the pairing of the first channel in this loop, `card = Approval { approval_id }`, `button`, `open`) — once per card; `INSERT OR IGNORE` on the unique reference keeps a second channel or a re-send from duplicating; then, unless a `card_deliveries` row exists for (approval, pairing), enqueue the card message and record the delivery (`message_id` filled after sending).
3. **Decided** (`approval_decided { approval_id, state, decided_on }`): close the approval's open references; for each delivery, enqueue an edit: the card text plus one line — on the channel the press came from: `texts.approved_by_you(HH:MM)` / `texts.not_approved(HH:MM)`; on other channels and when decided elsewhere: `texts.decided_in(decided_on, HH:MM)` ("Decided in Slack, 10:44"); `expired`: `texts.no_longer_needed(HH:MM)`; buttons removed.
4. **Show details**: `approvals.get(id)`; by `content`: `Message` → its text (split per J.2); `Command` → "Runs in {runs_in}:" and the command; `Email` → "From: … To: … Cc: … Subject: …", the body, the attachment lines; `Changes`/`Page` → the file from `core.content_file`; `Files` → each file (H.4); `None` → `texts.no_details`. Sent to the channel of the press only.
5. ***Needs you* route**: `needs_you_both` → every paired channel; otherwise the last channel. Approval cards always go to every paired channel (ADR-076).

**J. Sending, splitting, rate, outages**

1. Every outgoing message is a row of `channel_outbox` (per pairing, FIFO by `id`). One worker per channel takes the oldest row, sends it, deletes it, and waits `min_send_interval_ms` before the next send to the same chat.
2. **Splitting**: text longer than `max_text_chars` (or `max_caption_chars` with a file) is cut at the last blank line within the limit; failing that, the last line break; failing that, the last sentence end (". ", "? ", "! "); failing that, the last space; failing that, at the limit. Buttons go on the last part; a file goes with the first part as its caption. Buttons beyond `max_buttons` are dropped from the end, logged `chat.buttons.cut`.
3. Marks: `**x**` → bold, `*x*` → italic; the adapter converts or strips them.
4. Errors: `Retryable { retry_after_ms }` → keep the row, wait `retry_after_ms` or 1 s doubling to 5 minutes; `Permanent` → drop the row, log `chat.send.dropped {code}`; for a card, the delivery row is deleted so **Show it** can send it again.
5. A pairing's outbox holds at most 1,000 rows; at the limit the oldest row that is not a card is dropped (`CHAN_OUTBOX_FULL` logged).
6. **Outage notice**: on `report_status(kind, Reconnecting { since })` start a 10-minute timer; when it fires and the status is still not `Connected`, and the other channel is paired and `Connected`, enqueue `texts.channel_down(kind)` there once ("Slack hasn't been reachable for 10 minutes. I'm still here; Slack cards wait until it's back."). `CredentialsRefused` → `texts.channel_key_refused(kind)` on the other channel at once (ADR-076 *Before something runs out*). Reset on `Connected`.
7. **Restart notice** (`send_restart_notice`): to the last channel: `texts.restarted(HH:MM, runs_resumed, approvals_waiting)` with **Show it** (`{"t":"show_it"}`) when `approvals_waiting > 0` (03:1086, 11 §5.23).
8. **Sign-in ended**: `texts.sign_in_ended` on the last channel at once (11 §5.23).
8a. **Update available** (`send_update_available(version)`): `version` 1–40 characters → else `CHAN_INPUT_INVALID`; `texts.update_available(version)` on the last channel once. The program records which version was announced; the gateway does not.
9. `send_to_person(msg)`: buttons become `chat_actions` rows (`reference` = 16 random bytes, base64url; `owner`; `payload`; `single_use = true`; `created_at`) and `x:` buttons; then enqueued on the route.

**K. Agent profile (hosted agent)** — wrightrun's chat connector rules (`agent-wright-runtime/docs/spec/components/ChatConnector.md` §E–§L), restated for ordin.

1. **Event record.** `receive_hosted(event)`: insert `(connection, envelope_id)` into `chat_events_seen`; a conflict means another instance or a Slack retry already took it → return `HostedAck::Plain` with nothing else done.
2. **Joined / Left** (ADR-097 item 3): `Joined` → `scopes.enable(ScopeKind::Channel, connection, channel)` (Retention and Erasure keeps the state and the erasure mark, wrightrun ADR-070); when no `chat_scope_notes` row exists for the scope, insert one and call `core.scope_enabled_first_time(connection, scope)` (first ingest jobs). `Left` → `scopes.disable(connection, channel)`. Log `chat.channel.enabled` / `chat.channel.disabled`.
3. **Message addressed to the agent** (`Mention`, `ThreadMention`, `DirectMessage`, and `Command` matched to a command trigger by K.6). The scope of a channel or direct chat is `scopes.find(connection, chat_identifier)` and is enabled when its `Scope.state` is `enabled`; a listed user's first direct message calls `scopes.enable(ScopeKind::DirectChat, connection, chat)` (R13):
   1. *Trigger*: the connection's `triggers`, then each package `chat_triggers` entry whose (kind, name) the connection does not declare; the first whose kind equals the event's (`DirectMessage` uses the `mention` trigger, *assumed here*: a direct message is addressed to the agent as a mention is). None → drop (`reason=no-trigger`).
   2. *Reach*: `Mention`/`ThreadMention`/`Command` in a channel: the channel's scope must be `enabled` (else drop, `reason=not-enabled`, and ask the comparison loop to run early, K.9); with `reach.channels` set, the channel must be listed; with `reach.users` set, the user must be listed. `DirectMessage`: the user must be in `reach.direct_message_users` (else drop, `reason=not-listed`); the direct chat's scope is enabled on the first such message. A dropped message gets no reply and starts nothing; the journal counts it without content (K.12).
   3. *Text*: empty → drop; longer than 12,000 chars → failure text with `CHAT_MESSAGE_TOO_LONG`.
   4. *Thread read*: in a thread, a human sender must be a member of the channel (`link.may_read_thread`); false or error → drop with `CHAT_NOT_PERMITTED`, nothing sent.
   5. *Room*: open pending questions of the connection ≥ 200 → failure text with `CHAT_BUSY`.
   6. *Acknowledge*: `link.react(channel, message, status.received)` (errors logged, ignored).
   7. *Context* (*amended January 2026, ADR-107*: the steps are configured, and ordin runs every one before the model, so what reaches the model does not depend on the model choosing to look). Run `connection.context.steps()` in list order, step `n` numbered from 1; every item a step gives carries `step: Some(n)`. Each step gives one `Piece`, or a `LeftOut` when it fails, finds nothing (`reason: "empty"`) or does not apply (`reason: "skipped"`); a step's failure never stops the others.
      - `Transcript { max_messages }`: `link.thread(channel, thread, max_messages + 1)`, oldest dropped and `cut` set beyond the limit, one line per message `[<id>] <pseudonym>: <text>`, pseudonym = `"person-" + first 8 hex of SHA-256(scope_id ‖ sender)` or `"program-…"` for programs, never a name — wrightrun A-24; `items` = messages kept. With `max_messages` 0 no message is read and the step is `empty`.
      - `Images { max_bytes, max_total_bytes }`: question files then transcript files, newest first, at most 10 and `max_total_bytes` in all, each ≤ `max_bytes`, read with `link.file`; one piece per image (`items` 1); an image above a limit is a `LeftOut` with `observed_bytes` its reported size.
      - `ChannelDocuments { max_bytes }`: `link.channel_documents(channel, max_bytes)`; each returned item takes the step number; none returned → one `LeftOut { reason: "empty" }`.
      - `KnowledgeLookup { source, limit, max_bytes }`: `core.context_lookup(scope, source, text, limit)`; the hits, rank order, become one piece `{ piece: "knowledge", label: source, media_type: "text/plain" }`, one line per hit `[<item id>] <origin time, RFC 3339>: <text>` (no sender, as the transcript names no person), cut at `max_bytes` on a line boundary with `cut` and `observed_bytes` set; `items` = hits kept; no hit → `LeftOut { reason: "empty" }`.
      - `RecentMessages { source, limit, max_bytes, when }`: when `when` is not empty and no keyword of it is found in the question (the Rules Engine's `keywords` match: a keyword is found when all its words are among the question's lower-cased words) → `LeftOut { reason: "skipped" }`; else `core.context_recent(scope, source, limit)`, newest first, formed and cut as `KnowledgeLookup` with `piece: "recent"`.
      - `PrecedenceNote { text }`: only when a `ChannelDocuments` step and a `KnowledgeLookup` step of this run each gave at least one piece → `Piece { piece: "note", label: "precedence", media_type: "text/plain", content: text, items: 1 }` (the piece kind of the hand-off fact below); otherwise `LeftOut { reason: "skipped" }`.
      A read that fails becomes a `LeftOut { reason: "read-failed" }` and is logged `chat.context.step_failed {connection, step, kind, code}`; the next step runs. `CoreError` from a knowledge call is such a failure. The ordered items are the run's gathered context; the Session Supervisor's run summary reports them per step (Logging.md S-1a).
   8. *Queue*: `run_id` = new UUIDv7. Insert the pending question first (`chat_pending_questions`: run_id, connection, scope, channel, thread, question message id, asker, delivery = trigger's, from_program, created_at); then `core.enqueue(HostedRunRequest { command: trigger.starts, text, context, … })`. Pending insert fails → failure text `CHAT_PENDING_FAILED`; enqueue fails → failure text `CHAT_RUN_FAILED` and delete the pending row. Log `chat.run.queued {connection, run_id, kind, context_count, context_bytes}`.
      - *Hand-off fact* (*added January 2026, ADR-107*; SA-HOST-17): when the connection declares a `hand-off` action, before `enqueue` one item is appended to `context`: `ContextItem::Piece { piece: "note", label: "hand-off", media_type: "text/plain", cut: false, step: None, items: 1, observed_bytes: None }` whose `content` is "A hand-off to a person is available in this conversation." when K.8.1 finds a route for the scope, else "No hand-off to a person is available in this conversation; do not offer one." A failed settings read counts as no route. The run's answer text therefore never offers a hand-off the reply will not carry.
      - *Working signal* (*added January 2026, ADR-107*): after `enqueue` succeeds, `link.react(channel, message, status.working)` (errors logged, ignored); it stays until delivery (K.5.e).
   9. Return `HostedAck::Plain` (the adapter acknowledged the envelope on receipt already; C-56).
4. **Delivery loop** (every 1 s while fewer than 10 deliveries are in flight): select up to 50 pending rows of this instance's connections that are not `done` and whose lease is free or expired; `core.run_views(ids)`; for each row whose run is `paused` with `told_approval = 0`, or ended (`ended`, `cancelled`, `interrupted`): take the lease (`UPDATE … SET lease_holder = instance, lease_until = now + 30 s WHERE run_id = ? AND (lease_until IS NULL OR lease_until < now)`, one winner) and deliver (K.5). Paused → post `approval_text` (to the asker only unless `from_program`), set `told_approval = 1`, release the lease.
5. **What is delivered** (ended run):
   - `safe_reply` → the reply text, no actions, no reply record; signal `done`.
   - `ended` + `completed`, or `ended_by_control` with a reply → the reply (K.5 b–d).
   - `ended_by_control` with no reply → nothing sent; signal `done`.
   - `failed`, `run_limit`, `interrupted`, `cancelled`, or no reply text → `failure_text` with `CHAT_RUN_FAILED`; signal `failed`; the run's `error_code` is logged, never sent.
   - Every signal named above follows K.5.e's order: unreact `received` and `working`, then react the named signal (*amended January 2026, ADR-107*).
   - b. *Where*: visible (in the thread / channel) when `from_program` or (`view.delivery` or the trigger's delivery) is `thread`; otherwise to the asker only. Anchor = thread, else the question's message, else none.
   - c. *Content*: the reply, then — when `opened_items` is not empty — an empty line and "Sources: " + `core.source_entries(scope, opened_items)` joined by "; " (left out when none). Before the first reply ever delivered in a scope, one sentence is put first: `texts.scope_notice_channel` ("What's said in this channel is remembered for this channel only.") or `texts.scope_notice_direct` ("What you tell me here stays in this direct chat.") (ADR-097 item 5; *chosen here*), and `scopes.first_reply_sent = 1`.
   - d. *Actions*: candidate order — the first declared `share` action (only when the reply is not visible), then `actions_attached`, then `actions_asked`, no repeats, each kept only when `share` or listed by `core.available_actions(connection, scope)`; at most 6 buttons (`value` = reply id). A `link` action with `url_setting` becomes a URL button from `core.setting_text(scope, url_setting)` when it begins with `https://`, else it is dropped. A `hand-off` action is kept only when K.8.1 finds a route (*amended January 2026, ADR-107*).
     - *List-backed links* (*added January 2026, ADR-107*; SA-HOST-15, SA-HOST-16): for each declared `link` action with `list_setting` that `core.available_actions` lists, in configuration order, whether or not the model asked for it: `core.setting_entries(scope, list_setting)`; an entry matches when one of its keywords is found in the question text (the text the reply record keeps as `question`); the entry's keywords are its `keywords_field` text split at commas, each trimmed, empty ones dropped; a keyword is found when every one of its words is among the question's words, both sides split at Unicode word boundaries and lower-cased — the rule of the Rules Engine's `keywords` condition with `match: any`, applied here in fixed code, never by the model. Each matching entry gives one button: `action` = the action's `id`, `label` = the entry's `label_field` cut to 75 characters, `url` = the entry's `url_field`, `kind` = `WorkflowLink` when `style` is `workflow-link`, else `Url`; an entry whose address does not begin `https://` (or, for `workflow-link`, `https://slack.com/shortcuts/`) is skipped and logged `chat.link.entry_skipped {connection, action}`. Two buttons with the same address are kept once. A list-backed action named by `actions_asked` or `actions_attached` adds only its matching entries. Final order: `share` (when present), the matched workflow buttons, the other candidates in the order above, the matched URL buttons; then the 6-button cap. A failed `setting_entries` read adds no list button and logs `chat.link.entries_failed {connection, action}`; the reply still goes out.
   - e. *Record, send, signal, in this order*: reply record (`chat_replies`: reply_id, run_id unique, scope, asker, question text, content, state `shared` or `asker-only`, actions, done_actions `[]`); a second delivery of the same run reuses the existing record and logs `chat.reply.second_delivery`; `link.post(…)`; set `message_id`; unreact `received` and `working`, react `done`; mark the pending row `done`. A post error leaves the row (no `done`), so the lease lapses and another attempt follows within 30 s, until K.7 gives up.
6. **Commands** (`HostedMessageKind::Command`): normalise white space and case; declared texts = command-trigger names, entry commands, job commands, `version_command`, `settings_command`; the longest declared text equal to the text, or that the text begins with followed by a space, wins; the rest follows it. No match → `TEXT_UNKNOWN_COMMAND` with the declared texts, to the sender only (`CHAT_UNKNOWN_COMMAND`). Every answer goes to the sender only.
   - command trigger → K.3 with the rest as text;
   - job command: `who = operators-only` → `TEXT_NOT_FOR_CHAT`; scope not enabled → `TEXT_NOT_ENABLED`; else `core.job_command(scope, cmd, user)` text;
   - version command → `core.version_lines()` joined by newlines (works in a channel that is not enabled);
   - settings command: scope not enabled → `TEXT_NOT_ENABLED`; `core.settings_form(scope)`: `None` → "This agent has no setting to change here."; `Some(form)` → `link.open_form(trigger, form)` (form metadata names the scope's channel);
   - entry command: scope not enabled → `TEXT_NOT_ENABLED`; `core.entry_command(scope, cmd, rest, user)`: `Text` → send it; `OpenForm` → `link.open_form`.
7. **Give-up loop** (every 60 s): delete pending rows whose run ended more than 7 days ago (or `retention.chat_replies_days`, if smaller) or that were created more than 7 days ago with no run view; count `chat_questions_abandoned`.
8. **Reply actions** (`HostedEvent::Action`, value = reply id): no record → `TEXT_REPLY_GONE`; action not declared or not on the record → `TEXT_ACTION_GONE`; `share` → set state `shared` where it was `asker-only` (one winner; else `TEXT_ALREADY_SHARED`) and post the record's content visibly with its other actions (on a post error set it back and send `failure_text`); `hand-off` → K.8.1 finds no route → `TEXT_NO_HAND_OFF`; else add the action to `done_actions` (one winner; else `TEXT_ALREADY_HANDED_OFF`) and post the escalation (K.8.2), then `TEXT_HANDED_OFF` to the presser only; `link` → nothing (a workflow button starts its Slack workflow in Slack itself). Approval buttons arriving as actions (`action = "approval"`, value = `c:<ref>`) → K.11.
   1. *Hand-off route* (*amended January 2026, ADR-107*; SA-HOST-23). For the scope, read each setting the action names: `enabled_setting` (a flag; its text `false` hides the hand-off whatever else is set), `target_channel_setting`, `mention_users_setting`, `mention_groups_setting`, `notify_setting` (`core.setting_text`). The target is the trimmed text when it matches `^[CG][A-Z0-9]{2,20}$`, else none (logged `chat.handoff.target_invalid {connection}`). Mentions, in this order: each item of the users text (split at commas and white space) matching `^[UW][A-Z0-9]{2,20}$` → `<@U…>`; each item of the groups text matching `^S[A-Z0-9]{2,20}$` → `<!subteam^S…>`, the word `here` → `<!here>`, the word `channel` → `<!channel>`; any other item (a bare name such as `@ops`) is kept as text and is not a mention. A route exists when the hand-off is not hidden and there is a target, at least one mention, or a non-empty notify text. `core.available_actions` applies the same rule, so an action with no route is never offered.
   2. *Escalation post* (*amended January 2026, ADR-107*). First line: the mentions joined by one space, then the notify text, joined by one space; when that line is empty it reads "A person is asked to take this over.", else it ends " — a person is asked to take this over." Then, when posted to a target channel other than the reply's channel, the line "Asked in <#{channel}>." Then "> {question lines}". With a target, `link.post` to the target channel as a new message (no thread); without one, visibly in the reply's thread as before. A post error → `failure_text` with `CHAT_DELIVERY_FAILED` to the presser only and the action is removed from `done_actions`.
   3. *Ephemeral reply replaced* (*added January 2026, ADR-107*; SA-HOST-22). When the pressed reply was `asker-only` and the event carries a `response_url`, after a successful `share` or `hand-off`: `link.replace_ephemeral(response_url, <the record's content>, <its buttons without the one used>)`; an error is logged `chat.ephemeral.replace_failed {connection, code}` and nothing else changes.
9. **Comparison with the chat system's list** (per link): at start, then every 3,600 s, and early when K.3.2 asked (at least 60 s after the last): `link.member_channels()`; `scopes.enabled(connection)` is read; each listed channel with no enabled scope → `scopes.find` then, unless that scope is `erased`, enable (K.2); when `complete`, each enabled channel not listed → disable (K.2). A failed read changes nothing (`chat.reconcile.failed`). Log `chat.reconcile.done {listed, enabled, disabled, kept_erased, forgotten, complete}`.
10. **Settings form submitted** (`FormSubmitted`, `form = "settings"` or `entry:<setting>`): the scope comes from the form's metadata (the channel it was opened in), not from the event; `core.submit_settings(scope, metadata, values, user)`: `FieldErrors` → `HostedAck::FormErrors(map)`; `Saved` → `HostedAck::Plain` and `TEXT_SETTINGS_SAVED` to the sender only.
11. **Approval cards to approvers** (ADR-097 item 4): on `approval_requested`, for each `approvers.users` entry with a `chat_user`, insert `chat_approval_deliveries (approval_id, chat_user)` — one instance wins — then `link.post_direct(chat_user, card text (I.1 without the started-by line, plus "Asked by <@asker>" when the card's asker is a chat user), buttons)` with buttons `action = "approval"`, values `c:<approve>`, `c:<reject>`, `c:<show_details>`; record the (channel, message id). A press: `approvals.decide(press, PressOrigin::Approver { subject: user.subject or "<connection>/<chat_user>", groups: [], via: Chat })`; `AskerMayNotApprove` → `texts.asker_may_not_approve` to the presser only; `NotAnApprover` → `texts.not_an_approver`; `AlreadyDecided` → `texts.already_decided`. On `approval_decided`, every delivery of that approval is updated with the decided line and no buttons (I.3). Approvers named only by group get no chat card (groups are checked where a token is presented — Agent Configuration spec).
12. **Ignored messages** (agent profile): counted per (connection, reason) and written hourly as one journal entry `chat_messages_ignored` without content, as C.7.
13. **Job ended** (`notify_job_ended(connection, user, text)`): the registered link for `connection` → else `CHAT_LINK_UNKNOWN`; `link.post_direct(user, text, &[])`; a `LinkError` → `CHAT_POST_FAILED`. The text is the Job Runner's, passed by the program; the gateway adds nothing.
14. **Retention providers** (`retained_content`): two objects. `ChatReplies`: `delete_older_than(cutoff, limit)` deletes `chat_replies` rows with `created_at < cutoff`, at most `limit`; `count_for_asker`/`delete_for_asker` count or delete the rows whose `asker` matches on that connection's scopes; `count_scope`/`delete_scope` by `scope_id`. `PendingQuestions`: the same over `chat_pending_questions` (`created_at`, `asker`, `scope_id`). A datastore error → `ProviderFailed { code: "CHAT_DATASTORE_FAILED" }`.

**L. Words** — `ChatTexts::en_gb()` (11 §5.23 and §5.24 where given; others *chosen here*):

| Field | Text |
|---|---|
| `welcome` | "I'm your ordin on the server. You can ask me for work, send me files, and approve things here. Editing code, opening files in apps on your computer and working with Confluence aren't available here." + `commands_line` + the channel's two-step line (11 §5.23) |
| `commands_line` (Telegram) | "Commands: /stop pauses every run, /status lists what is running, /run starts a saved workflow, /triggers lists your triggers." (Slack: the same words without "/") |
| `two_step_telegram` | "Please turn on two-step verification in Telegram, because approvals made here count." |
| `two_step_slack` | "Please make sure two-factor authentication is on for your Slack account, because approvals made here count." |
| `voice_note` | "I can't listen to voice notes here. Please type your message." (ADR-078) |
| `use_buttons` | "Use the buttons on the card to approve." (11 §5.23) |
| `other_channel_notice` | "Ordin is working on your {Telegram} request. Cards also appear here." (11 §5.23) |
| `stopped` | "Paused {n} run(s)." / "Nothing was running." |
| `nothing_running` | "Nothing is running." |
| `approved_by_you`, `not_approved`, `decided_in`, `no_longer_needed` | "Approved by you, {10:42}" (11 §5.23), "Not approved, {10:42}", "Decided in {Slack}, {10:44}" (11 §5.23), "No longer needed, {10:44}" |
| `already_decided` | "Already decided in {Telegram}." (03:1089) |
| `made_at` | "Made at {10:31}." (03:964) |
| `restarted` | "ordin restarted at {03:12}; {2 runs} resumed, {1 approval} still waiting." (11 §5.23) with **Show it** |
| `channel_down` | "{Slack} hasn't been reachable for 10 minutes. I'm still here; {Slack} cards wait until it's back." (11 §5.23) |
| `channel_key_refused` | "{Telegram} refused ordin's key, so I can't use {Telegram} until whoever runs your ordin server sets it again." |
| `sign_in_ended` | "Your Claude Code sign-in on the server has ended, so new work waits. Ask whoever runs your ordin server to sign in again." (11 §5.23) |
| `update_available` | "A new version of ordin, {version}, is available. Ask whoever runs your ordin server to install it." (wording from OrdinServer.md step 7.7) |
| `file_too_large_out` | "Saved as {name} in {path}; it's too large to send here." (11 §5.23) |
| `file_too_large_in` | "This file is larger than {20 MB}, so I can't receive it here." |
| `no_workspace_for_file`, `file_not_received`, `file_not_saved` | "Tell me which workspace to work in first, then send the file again.", "I couldn't receive that file. Please send it again.", "I couldn't save that file in the workspace. Please try again." |
| `unsupported` | "I can only take text, files and photos here." |
| `card_closed`, `action_closed`, `try_again`, `no_details` | "This card is no longer open.", "That button is no longer active.", "That didn't go through. Please press the button again.", "There's nothing more to show for this card." |
| `or_type_answer`, `send_file_for`, `form_set_aside`, `cut_notice` | "Or type your answer.", "Send the file for {label}.", "I set the earlier form aside.", "Your message was too long, so I used the first part only." |
| `no_text_form` | "{A report} can't be shown here, so I saved it as {file} in {path}." |
| `core_unavailable`, `triggers_unavailable` | "I can't take that right now. Please try again in a minute.", "Workflows can't be started from the chat right now." |
| `scope_notice_channel`, `scope_notice_direct` | as K.5 c |
| `asker_may_not_approve`, `not_an_approver` | "You asked for this, so someone else needs to approve it.", "Only the agent's approvers can decide this." |
| wrightrun `TEXT_*` (agent profile) | unchanged from `ChatConnector.md`: `TEXT_ACTION_GONE` "This action is no longer offered.", `TEXT_REPLY_GONE` "This reply is no longer available.", `TEXT_ALREADY_SHARED` "This reply is already shared.", `TEXT_HANDED_OFF` "A person has been asked to take this over.", `TEXT_ALREADY_HANDED_OFF` "A person was already asked to take this over.", `TEXT_NO_HAND_OFF` "No hand-off is set up for this channel.", `TEXT_NOT_ENABLED` "The agent is not enabled in this channel.", `TEXT_SETTINGS_SAVED` "Settings saved.", `TEXT_NOT_FOR_CHAT` "This command is for operators.", `TEXT_UNKNOWN_COMMAND` "Unknown command. Available: {commands}" |

## Data Model

SQLite (personal profile; agent single shape) — migration `channel_gateway` 1:

```sql
-- personal profile
CREATE TABLE channel_pairings (
    id                TEXT PRIMARY KEY CHECK (length(id) = 36),
    channel_kind      TEXT NOT NULL CHECK (channel_kind IN ('telegram','slack')),
    chat_user_id      TEXT CHECK (chat_user_id IS NULL OR length(chat_user_id) BETWEEN 1 AND 64),
    private_chat_id   TEXT CHECK (private_chat_id IS NULL OR length(private_chat_id) BETWEEN 1 AND 64),
    paired_by         TEXT NOT NULL CHECK (length(paired_by) BETWEEN 1 AND 200),
    code_expires_at   TEXT NOT NULL,
    paired_at         TEXT,
    ended_at          TEXT,
    state             TEXT NOT NULL CHECK (state IN ('pending','paired','ended')),
    CHECK (state <> 'paired' OR (chat_user_id IS NOT NULL AND private_chat_id IS NOT NULL))
);
-- At most one paired and one pending row per channel kind (04:264 as amended in 0.48.0).
CREATE UNIQUE INDEX channel_pairings_one_paired ON channel_pairings(channel_kind) WHERE state = 'paired';
CREATE UNIQUE INDEX channel_pairings_one_pending ON channel_pairings(channel_kind) WHERE state = 'pending';

CREATE TABLE card_references (
    id                  TEXT PRIMARY KEY,
    channel_pairing_id  TEXT NOT NULL REFERENCES channel_pairings(id),
    reference           TEXT NOT NULL UNIQUE CHECK (length(reference) = 22),
    card                TEXT NOT NULL,          -- JSON CardSubject
    approval_id         TEXT,                   -- copy of card.approval_id, for lookups
    button              TEXT NOT NULL CHECK (button IN ('approve','not_now','show_details')),
    state               TEXT NOT NULL CHECK (state IN ('open','used','closed')),
    created_at          TEXT NOT NULL,
    used_at             TEXT
);
CREATE INDEX card_references_approval ON card_references(approval_id);

CREATE TABLE card_deliveries (
    approval_id         TEXT NOT NULL,
    channel_pairing_id  TEXT NOT NULL REFERENCES channel_pairings(id),
    message_id          TEXT,                   -- NULL until sent
    card_text           TEXT NOT NULL,          -- the text sent, kept to edit it once decided
    pressed             INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (approval_id, channel_pairing_id)
);

CREATE TABLE chat_actions (
    reference    TEXT PRIMARY KEY CHECK (length(reference) = 22),
    owner        TEXT NOT NULL CHECK (owner IN ('gateway','triggers')),
    payload      TEXT NOT NULL CHECK (length(payload) <= 2048),
    single_use   INTEGER NOT NULL DEFAULT 1,
    state        TEXT NOT NULL CHECK (state IN ('open','used')),
    created_at   TEXT NOT NULL
);

CREATE TABLE channel_outbox (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_pairing_id  TEXT NOT NULL REFERENCES channel_pairings(id),
    purpose             TEXT NOT NULL CHECK (purpose IN ('text','card','edit','file')),
    approval_id         TEXT,
    message             TEXT NOT NULL,          -- JSON OutboundMessage (file bytes as an artefact reference, never inline over 64 KiB)
    created_at          TEXT NOT NULL,
    attempts            INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX channel_outbox_order ON channel_outbox(channel_pairing_id, id);

CREATE TABLE gateway_settings (
    key    TEXT PRIMARY KEY CHECK (key IN ('last_channel','needs_you_both')),
    value  TEXT NOT NULL
);
```

Agent profile tables (SQLite in the single shape; PostgreSQL in the scaled shape), migration `channel_gateway` 1 (SQLite statements in the same migration; PostgreSQL migration 1 below):

```sql
-- SQLite
-- Scopes are Retention and Erasure's table (`ScopeRegistry`, X4, R13); this crate keeps only its own note per scope.
CREATE TABLE chat_scope_notes (
    scope_id         TEXT PRIMARY KEY,           -- Retention and Erasure's scope id; no FK across modules
    first_reply_sent INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE chat_events_seen (
    connection   TEXT NOT NULL,
    envelope_id  TEXT NOT NULL,
    seen_at      TEXT NOT NULL,
    PRIMARY KEY (connection, envelope_id)
);

CREATE TABLE chat_pending_questions (
    run_id         TEXT PRIMARY KEY,
    connection     TEXT NOT NULL,
    scope_id       TEXT NOT NULL,
    channel        TEXT NOT NULL,
    thread         TEXT,
    question_msg   TEXT,
    asker          TEXT NOT NULL,
    delivery       TEXT NOT NULL CHECK (delivery IN ('asker','thread')),
    from_program   INTEGER NOT NULL,
    told_approval  INTEGER NOT NULL DEFAULT 0,
    lease_holder   TEXT,
    lease_until    TEXT,
    done           INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL
);
CREATE INDEX chat_pending_due ON chat_pending_questions(connection, done, lease_until);

CREATE TABLE chat_replies (
    reply_id      TEXT PRIMARY KEY,
    run_id        TEXT NOT NULL UNIQUE,
    scope_id      TEXT NOT NULL,
    asker         TEXT NOT NULL,
    channel       TEXT NOT NULL,
    thread        TEXT,
    message_id    TEXT,
    question      TEXT NOT NULL,
    content       TEXT NOT NULL,
    state         TEXT NOT NULL CHECK (state IN ('asker-only','shared')),
    actions       TEXT NOT NULL DEFAULT '[]',
    done_actions  TEXT NOT NULL DEFAULT '[]',
    from_program  INTEGER NOT NULL,
    created_at    TEXT NOT NULL
);
CREATE INDEX chat_replies_scope ON chat_replies(scope_id, created_at);

CREATE TABLE chat_approval_deliveries (
    approval_id  TEXT NOT NULL,
    chat_user    TEXT NOT NULL,
    channel      TEXT,
    message_id   TEXT,
    PRIMARY KEY (approval_id, chat_user)
);
```

```sql
-- PostgreSQL (agent profile, scaled shape), migration channel_gateway 1
CREATE TABLE chat_scope_notes (scope_id UUID PRIMARY KEY, first_reply_sent BOOLEAN NOT NULL DEFAULT FALSE);
CREATE TABLE chat_events_seen (connection VARCHAR(64) NOT NULL, envelope_id VARCHAR(128) NOT NULL, seen_at TIMESTAMPTZ NOT NULL, PRIMARY KEY (connection, envelope_id));
CREATE TABLE chat_pending_questions (run_id VARCHAR(64) PRIMARY KEY, connection VARCHAR(64) NOT NULL, scope_id UUID NOT NULL,
    channel VARCHAR(64) NOT NULL, thread VARCHAR(64), question_msg VARCHAR(64), asker VARCHAR(64) NOT NULL,
    delivery VARCHAR(8) NOT NULL CHECK (delivery IN ('asker','thread')), from_program BOOLEAN NOT NULL,
    told_approval BOOLEAN NOT NULL DEFAULT FALSE, lease_holder VARCHAR(255), lease_until TIMESTAMPTZ,
    done BOOLEAN NOT NULL DEFAULT FALSE, created_at TIMESTAMPTZ NOT NULL);
CREATE INDEX chat_pending_due ON chat_pending_questions(connection, done, lease_until);
CREATE TABLE chat_replies (reply_id UUID PRIMARY KEY, run_id VARCHAR(64) NOT NULL UNIQUE, scope_id UUID NOT NULL, asker VARCHAR(64) NOT NULL,
    channel VARCHAR(64) NOT NULL, thread VARCHAR(64), message_id VARCHAR(64), question TEXT NOT NULL, content TEXT NOT NULL,
    state VARCHAR(16) NOT NULL CHECK (state IN ('asker-only','shared')), actions JSONB NOT NULL DEFAULT '[]',
    done_actions JSONB NOT NULL DEFAULT '[]', from_program BOOLEAN NOT NULL, created_at TIMESTAMPTZ NOT NULL);
CREATE INDEX chat_replies_scope ON chat_replies(scope_id, created_at);
CREATE TABLE chat_approval_deliveries (approval_id UUID NOT NULL, chat_user VARCHAR(64) NOT NULL, channel VARCHAR(64), message_id VARCHAR(64), PRIMARY KEY (approval_id, chat_user));
```

Retention: `chat_events_seen` rows older than 1 day are deleted by the give-up loop; `chat_actions` older than 7 days are deleted daily; `channel_outbox` rows of an ended pairing are deleted when it ends; `chat_replies` are kept for `retention.chat_replies_days` (the Retention and Erasure job erases them through the program, see *Interactions*). Rollback: migration 1 only adds tables; restoring the pre-migration backup (SA-XC-07) removes them.

## Error Table

In-process API; Status is the error and whether the caller may retry. The person never sees a code; the words are those of step L.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Channel not allowed by the profile or the owner's choice | `Err(permanent)` | `CHAN_CHANNEL_NOT_ALLOWED` | `GatewayError { code, retryable: false }`; inbound items counted, no reply |
| Pairing completed after the code expired | `Err(permanent)` | `CHAN_CODE_EXPIRED` | as above |
| Item received after the shutdown token was cancelled | `Err(retryable)` | `CHAN_STOPPING` | `GatewayError { code, retryable: true }` |
| Call other than pairing on a setup-mode gateway | `Err(permanent)` | `CHAN_SETUP_ONLY` | as above |
| `send_update_available` with an empty version or one over 40 characters | `Err(permanent)` | `CHAN_INPUT_INVALID` | as above |
| `notify_job_ended` for a connection with no registered link | `Err(permanent)` | `CHAT_LINK_UNKNOWN` | as above |
| `notify_job_ended` post refused by the chat system | `Err(retryable as the link says)` | `CHAT_POST_FAILED` | as above |
| Pairing not pending | `Err(permanent)` | `CHAN_PAIRING_NOT_PENDING` | as above |
| Chat user or chat id missing or over 64 chars | `Err(permanent)` | `CHAN_IDS_MISSING` | as above |
| Second paired row of one kind (unique index) | `Err(permanent)` | `CHAN_ALREADY_PAIRED` | as above |
| Pairing changed outside the setup command | `Err(permanent)` | `CHAN_NOT_SETUP_COMMAND` | raised by the program |
| Second adapter of one kind, or second trigger handler | `Err(permanent)` | `CHAN_ADAPTER_ALREADY_SET`, `CHAN_HANDLER_ALREADY_SET` | as above |
| Message from anyone but the paired account | ignored (counted) | `CHAN_NOT_PAIRED` (log) | — |
| Card reference unknown, closed or already used | reply `card_closed` | `CARD_REFERENCE_INVALID`, `CARD_ALREADY_USED` (log) | — |
| Chat action unknown, used or expired | reply `action_closed` | `CHAN_ACTION_CLOSED` (log) | — |
| Approval decide fails with a retryable error | reply `try_again` | the Approval Service's code (log) | — |
| Received file over the channel's limit | reply `file_too_large_in` | `CHAN_FILE_TOO_LARGE` (log) | — |
| File cannot be downloaded or saved | reply `file_not_received` / `file_not_saved` | `CHAN_FILE_FAILED` (log) | — |
| Core call fails | reply `core_unavailable` | `CHAN_CORE_UNAVAILABLE` (log) | — |
| Send fails, retryable | kept in the outbox, retried 1 s → 5 min | `CHAN_SEND_RETRY` (log) | — |
| Send fails, permanent | row dropped | `CHAN_SEND_DROPPED` (log) | — |
| Outbox at 1,000 rows | oldest text row dropped | `CHAN_OUTBOX_FULL` (log) | — |
| Datastore read or write fails | `Err(retryable)` | `CHAN_DATASTORE_FAILED` | as above |
| Agent profile: question longer than 12,000 chars | failure text | `CHAT_MESSAGE_TOO_LONG` | — |
| Agent profile: 200 open pending questions | failure text | `CHAT_BUSY` | — |
| Agent profile: thread member check false or failed | dropped, nothing sent | `CHAT_NOT_PERMITTED` | — |
| Agent profile: pending question not written | failure text | `CHAT_PENDING_FAILED` | — |
| Agent profile: run not queued, or run ended without a reply | failure text | `CHAT_RUN_FAILED` | — |
| Agent profile: reply not posted | retried through the lease | `CHAT_DELIVERY_FAILED` (log) | — |
| Agent profile: hand-off escalation not posted (ADR-107) | `failure_text` to the presser; the action is offered again | `CHAT_DELIVERY_FAILED` (log) | — |
| Agent profile: a list-backed link's entries cannot be read (ADR-107) | reply sent without list buttons | `CHAT_SETTINGS_UNAVAILABLE` (log) | — |
| Agent profile: an ephemeral reply cannot be replaced (ADR-107) | logged; nothing else changes | the link's code (log) | — |
| Agent profile: unknown chat command | `TEXT_UNKNOWN_COMMAND` to the sender | `CHAT_UNKNOWN_COMMAND` | — |
| Agent profile: settings store unavailable | failure text / form errors | `CHAT_SETTINGS_UNAVAILABLE` (log) | — |
| Agent profile: comparison with the chat system's list failed | nothing changed | `CHAT_RECONCILE_FAILED` (log) | — |
| Agent profile: a retention provider's (`ChatReplies`, `PendingQuestions`) datastore read or delete fails (step 14) | `Err(retryable)` to Retention and Erasure | `CHAT_DATASTORE_FAILED` | `ProviderFailed { code: "CHAT_DATASTORE_FAILED" }` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Channel Gateway

  Background:
    Given the personal profile, a verified profile allowing telegram and slack
    And Telegram is paired with user "48213377" in private chat "48213377"

  Scenario: Happy path — the paired person's text reaches Ordin
    When the Telegram adapter delivers text "Summarise this week's supplier emails" from "48213377" in chat "48213377"
    Then PersonalCore receives one conversation turn with that text on channel Telegram

  Scenario: Messages from anyone else start nothing and reveal nothing
    When the Telegram adapter delivers text "hello" from "99999" in chat "99999"
    Then nothing is sent and no conversation turn is made
    And within the hour one journal entry chat_messages_ignored counts 1 for telegram, reason other_user, without content

  Scenario: A typed yes is not an approval
    Given an approval card is open
    When the person types "Yes!"
    Then the reply is "Use the buttons on the card to approve."
    And the Approval Service receives no decision

  Scenario: A card on two channels; the first press decides
    Given Slack is also paired
    When the journal delivers approval_requested for approval A
    Then both channels receive the card with buttons carrying the same three references
    When the person presses the approve button in Slack
    Then the Approval Service decides A once with origin PairedChat Slack
    And after approval_decided the Slack card reads "Approved by you, 10:44" and the Telegram card reads "Decided in Slack, 10:44", both without buttons

  Scenario: Press on a card already decided
    Given approval A was decided on "Asha's laptop"
    When the person presses its approve button in Telegram
    Then the reply is "This card is no longer open."

  Scenario: Voice note
    When the Telegram adapter delivers a voice note from the paired person
    Then the reply is "I can't listen to voice notes here. Please type your message."

  Scenario: A file goes to a chat trigger first
    Given a trigger handler that reports Started for "invoice-0451.pdf"
    When the paired person sends "invoice-0451.pdf" of 300 KB
    Then the file is saved to the current workspace's inbox
    And no conversation turn is made

  Scenario: A file too large for Telegram
    When the paired person sends a file stated as 25 MB
    Then the reply is "This file is larger than 20 MB, so I can't receive it here." and nothing is downloaded

  Scenario: /stop pauses every run
    Given 3 runs are going
    When the person sends /stop
    Then PersonalCore pauses all runs and the reply is "Paused 3 runs."

  Scenario: A table becomes ten lines and a CSV file
    When the journal delivers block_shown with a table of 24 rows titled "Q3 rates by supplier"
    Then the message holds the title, 10 row lines and "and 14 more"
    And the file "Q3 rates by supplier.csv" with 25 lines is attached

  Scenario: Long reply is split at paragraph ends
    When a message block of 9,000 characters in paragraphs is shown
    Then Telegram receives three messages, none longer than 4,096 characters, each ending at a paragraph end

  Scenario: Channel outage reported on the other channel
    Given Slack is also paired
    When the Slack adapter reports Reconnecting and is not Connected 10 minutes later
    Then Telegram receives "Slack hasn't been reachable for 10 minutes. I'm still here; Slack cards wait until it's back." once

  Scenario: Restart notice with Show it
    When the program sends the restart notice with 2 runs resumed and 1 approval waiting
    Then the last channel receives "ordin restarted at 03:12; 2 runs resumed, 1 approval still waiting." with "Show it"
    When the person presses "Show it"
    Then the waiting card is sent again

  Scenario: New pairing of the same kind ends the old one only
    Given Slack is paired
    When a new Telegram pairing completes
    Then the earlier Telegram pairing is ended and its open card references are closed
    And the Slack pairing stays paired

  Scenario: Agent profile — a mention in an enabled channel queues one run
    Given the agent profile with connection "support" and channel C0HELPDESK enabled
    When a Mention event from U07ASKER arrives twice with the same envelope id
    Then exactly one pending question and one run are queued

  Scenario: Agent profile — direct message from an unlisted user
    Given reach.direct_message_users is ["U07LEAD"]
    When a DirectMessage from "U07OTHER" arrives
    Then nothing is sent and no run is queued

  Scenario: Agent profile — paused run gets the approval text once
    Given a queued run that pauses for approval
    When the delivery loop sees it paused
    Then the configuration's approval text is posted to the asker only, once

  Scenario: Agent profile — the asker may not approve an outside email
    Given approver U07ASKER is also the asker of approval B that leaves the company
    When U07ASKER presses Approve in the direct chat
    Then the reply to U07ASKER only is "You asked for this, so someone else needs to approve it."

  Scenario: Personal profile — cancelling the shutdown token drains the outbox and refuses new items
    Given the outbox for Telegram holds 2 rows
    When the program cancels the shutdown token passed to start
    Then both rows are sent within 10 seconds
    And receive returns CHAN_STOPPING for a new Telegram item

  Scenario: Personal profile — the update announcement uses the gateway's words
    When the program calls send_update_available with "1.4.0"
    Then the last channel receives "A new version of ordin, 1.4.0, is available. Ask whoever runs your ordin server to install it."

  Scenario: Setup mode — only pairing calls are served
    Given a gateway built with GatewayMode Setup
    When send_to_person is called
    Then the reply is CHAN_SETUP_ONLY
    And begin_pairing for an allowed channel succeeds

  Scenario: Agent profile — a job's end reaches the requester's direct chat
    Given connection "acme-slack" has a registered link
    When notify_job_ended is called for user U123 with "Your weekly digest job finished."
    Then the link's post_direct is called for U123 with that text and no buttons

  Scenario: Agent profile — comparison disables a channel only on a complete list
    Given channel C0OLD is enabled and the chat system's list stops at its page limit without C0OLD
    When the comparison runs
    Then C0OLD stays enabled and chat.reconcile.incomplete is logged

  Scenario: A file sent in the chat is saved in the workspace's Inbox
    Given the paired Telegram chat's current workspace is "Q3 supplier review"
    When the person sends "quote.pdf"
    Then save_to_inbox is called with InboxOrigin chat telegram
    And the Ordin turn carries the ContentRef place_file of the saved Inbox path

  Scenario: A block without a text form is saved against Ordin's run
    Given Ordin's current run in "Q3 supplier review" is R
    When a block_shown event carries an email_list block
    Then save_generated is called with name "email_list-HHMM.json" and run R
    And the reply names the saved file's display path

  Scenario: Agent profile — the working signal lasts from queue to delivery (ADR-107)
    Given status received "eyes", working "hourglass_flowing_sand" and done "white_check_mark"
    When a mention is queued and its run later ends with a reply
    Then the question gets eyes, then hourglass_flowing_sand after the run is queued
    And on delivery eyes and hourglass_flowing_sand are removed and white_check_mark is added

  Scenario: Agent profile — hand-off posts to the target channel with mentions (ADR-107)
    Given a hand-off action whose target channel setting is "C0OPS" and mention users setting is "U0ASHA" in the scope
    When Escalate is pressed on a reply in C0HELPDESK
    Then a new message is posted to C0OPS beginning "<@U0ASHA> — a person is asked to take this over."
    And it holds "Asked in <#C0HELPDESK>." and the question lines
    And the presser receives TEXT_HANDED_OFF

  Scenario: Agent profile — a group mention and here (ADR-107)
    Given the mention groups setting is "S0ONCALL, here"
    When Escalate is pressed
    Then the first line begins "<!subteam^S0ONCALL> <!here>"

  Scenario: Agent profile — hand-off is unavailable with no route (ADR-107)
    Given a hand-off action whose target, mention and notify settings are all empty in the scope
    When a run is queued
    Then its context holds the note "No hand-off to a person is available in this conversation; do not offer one."
    And the delivered reply holds no hand-off button

  Scenario: Agent profile — a bare name is not a route (ADR-107)
    Given the mention users setting is "@ops" and no other hand-off setting has a value
    Then the hand-off is not offered

  Scenario: Agent profile — the enabled setting hides the hand-off (ADR-107)
    Given a hand-off with a target channel and its enabled setting false in the scope
    Then the hand-off is not offered and a press on an older reply gets TEXT_NO_HAND_OFF

  Scenario: Agent profile — a keyword match attaches a workflow button (ADR-107)
    Given a link action "workflows" with style workflow-link on a list holding {label "Reset access", keywords "reset password", url "https://slack.com/shortcuts/Ft07RESET/abc"}
    When a reply is delivered to the question "how do I reset password for the VPN"
    Then the reply holds one workflow button labelled "Reset access", placed before the other buttons
    And a reply to "where is the VPN guide" holds none

  Scenario: Agent profile — a list entry with a bad address is skipped (ADR-107)
    Given a matching list entry whose url is "http://intranet/reset"
    Then no button is made from it and chat.link.entry_skipped is logged

  Scenario: Agent profile — sharing an ephemeral reply replaces it (ADR-107)
    Given an asker-only reply with Share and Escalate buttons
    When the asker presses Share and the action carries a response_url
    Then the content is posted in the thread and the ephemeral reply is replaced with the content and the Escalate button only

  Scenario: Agent profile — a knowledge-lookup step supplies hits without the model choosing (ADR-107)
    Given a knowledge-lookup step on source "channel-history" with limit 5
    And context_lookup returns 7 hits for the question
    When a question arrives
    Then the enqueued context holds one piece "knowledge" labelled "channel-history" with 5 lines and items 5
    And it is in the context before the model runs

  Scenario: Agent profile — the steps run in order and a failing step is left out (ADR-107)
    Given steps transcript, channel-documents and knowledge-lookup, and context_lookup fails
    When a question arrives
    Then the context holds the transcript piece with step 1, the document items with step 2, and a LeftOut with step 3 and reason "read-failed"
    And the run is queued

  Scenario: Agent profile — the precedence note is added only when both sources gave a piece (ADR-107)
    Given steps channel-documents, knowledge-lookup and precedence-note "The channel's documents win over history."
    When the channel has a canvas and the lookup finds two hits
    Then the context ends with a piece "note" labelled "precedence" holding that text
    And when the channel has no canvas, the note step is a LeftOut with reason "skipped"

  Scenario: Agent profile — a recent-messages step runs only on its keywords (ADR-107)
    Given a recent-messages step on "channel-history" with when ["this week"]
    When the question is "what happened this week"
    Then context_recent is called
    And for the question "who owns the VPN" the step is a LeftOut with reason "skipped" and context_recent is not called

  Scenario: Agent profile — a configuration with the former keys behaves as before (ADR-107)
    Given chat context {transcript-messages: 50, channel-documents: true, channel-documents-max-bytes: 200000}
    When a question arrives
    Then the steps run are transcript of 50, images, and channel documents of 200000 bytes, in that order
```

## Performance, Security, Observability

- **Performance targets:** inbound item to `conversation_turn` call p95 < 50 ms (excluding the core); journal event to the first outbox row p95 < 30 ms; a card reaches every paired channel's send queue within 100 ms of `approval_requested`; text form of a 10,000-row table (CSV) < 200 ms and < 20 MB memory; outbox ≤ 1,000 rows per pairing. Agent profile: one event to `enqueue` p95 < 300 ms without context reads; delivery loop polls every 1 s, at most 10 deliveries in flight per instance; ≤ 200 open pending questions per connection.
- **Security:** only the paired account in its private chat is answered (ADR-073); pairing changes only through the setup command; buttons carry 128-bit random single-use references, never an identifier or content; a typed word never approves (decision 10); presses are decided by the Approval Service, which refuses used, closed and decided cards; card text is built from the Approval Service's card, never from Ordin's words (2C wire §5); **Show details** content is sent only on press and only on that channel (R-63); text is NFC-normalised and length-limited at entry; no secret is ever asked for or accepted in the chat; ignored messages are counted without content. Agent profile: only enabled channels and listed users (ADR-097 item 3), thread member check, pseudonyms in transcripts, a direct chat's content never written to another scope. **Never logged:** message text, file names of the person's files, card text, chat user identifiers on the personal server (logged as the first 12 hex of SHA-256), tokens.
- **Observability (logs, `tracing`, target `ordin_channel_gateway`):** `chat.paired {channel}`, `chat.pairing.ended {channel}`, `chat.item {channel, body_kind}`, `chat.press {channel, kind: card|action, outcome}`, `chat.send {channel, purpose, parts}`, `chat.send.dropped {channel, code}`, `chat.outbox {channel, depth}`, `chat.channel.status {channel, status}`, `chat.notice {kind}`, `chat.buttons.cut {channel, dropped}`, `chat.press.malformed {channel}`. Agent profile adds wrightrun's lines `chat.run.queued`, `chat.reply.second_delivery`, `chat.reply.undelivered`, `chat.reply.failure_text`, `chat.event.not_permitted`, `chat.channel.enabled`, `chat.channel.disabled`, `chat.reconcile.done`, `chat.reconcile.failed`, `chat.reconcile.incomplete`, `chat.pending.given_up`, and, through the program's `MetricsSink` to Telemetry, wrightrun's counters unchanged (SA-OBS-02): `wrightrun_chat_events_received_total{connection,kind}`, `wrightrun_chat_replies_delivered_total{connection,delivery,kind}`, `wrightrun_chat_actions_total{connection,action}`, `wrightrun_chat_reconciliations_total{connection,outcome}`, `wrightrun_chat_channels_reconciled_total{connection,change}`, `wrightrun_chat_events_dropped_total{connection,reason}`, `wrightrun_chat_questions_abandoned_total{connection}`, `wrightrun_chat_replies_redelivered_total{connection}`. Log lines added January 2026 (ADR-107), never with text, names or addresses: `chat.handoff.target_invalid {connection}`, `chat.link.entry_skipped {connection, action}`, `chat.link.entries_failed {connection, action}`, `chat.ephemeral.replace_failed {connection, code}`. No metrics or spans on the personal server (SA-XC-04/05).

## Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (open question 6: channel members see only **What Ordin used** on answers in their channel; the agent's owners see the rest through the Caller API, CallerApi.md amendment January 2026). Owner of the data: `components/MemoryService.md` (amendment January 2026: `memory_used`, `MemoryUsedView`, the recall log). The answer block's `memory_used` field is 2C wire's (`MessageBlock`, amended January 2026).
> Every rule below adds to the sections above.

### Purpose (addition)

An answer that used remembered facts or links can say which, in the chat it was given in: a **What I used** button on the hosted agent's Slack reply, a **What Ordin used** button on Ordin's reply in the personal server's Telegram or Slack chat, and, on both profiles, the words "what did you use?" sent in the same thread or chat. The answer lists only the facts and links that answer used, each as a plain sentence, and nothing else of the agent's memory.

### Approach (addition)

The gateway keeps no memory data: it asks its core trait for the plain lines of one answer's used items, at the moment the person asks, so a fact forgotten since is not shown. On the personal profile the recall ids travel on Ordin's `message` block (`memory_used`); on the agent profile the hosted agent's recalls are logged by run, and the reply record already holds the run id. Rejected: *listing the used facts under every answer* — it lengthens every reply in a team channel and shows memory to people who did not ask; *a private (ephemeral) answer only* — Telegram has no private message in a chat and the facts of a channel's scope were learned in that channel, so the answer goes where the reply went; *opening the operators' memory views to channel members* — open question 6 keeps those to the owners.

Assumptions (*chosen here*):

- **SA-CHAN-AMEM-01** The answer goes where the reply went: a reply shared in the thread or channel gets its answer in the thread; an asker-only reply gets an asker-only answer (`only_to` the presser); the personal profile answers in the chat the press came from.
- **SA-CHAN-AMEM-02** "what did you use?" is matched after lower-casing, trimming and dropping a trailing "?" and the agent's mention; it applies to the newest reply record of this agent in the same thread (agent profile, a `ThreadMention` or `DirectMessage`) or the newest Ordin `message` block carrying `memory_used` sent to that chat in the last 24 hours (personal profile). With none, `texts.memory_used_none` is sent. The words start no run and reach no trigger.
- **SA-CHAN-AMEM-03** The button counts towards the 6 buttons of K.5.d and is placed last; when 6 actions are already kept, it is left out and the words still work.
- **SA-CHAN-AMEM-04** At most 20 lines are sent, each cut to 300 characters at a word end; the total is split by J's rules.

### Shared Context (additions)

```rust
/// One answer's used memory, as plain lines (the program renders them from the Memory Service's MemoryUsedView).
#[derive(Debug, Clone)]
pub struct MemoryUsedText {
    /// "<statement> (kept <date>)" per fact and "<from> <kind> <to> (since <date>)" per link, in recall order; 0–20.
    pub lines: Vec<String>,
    /// Items recalled for the answer and forgotten since (MemoryUsedView.gone).
    pub gone: u16,
}
```

`MemoryRecallId` is a 2C id (SharedTypes, amended January 2026). `MessageBlock` (2C wire, copied in *Shared Context* above) gains `#[serde(default, skip_serializing_if = "Vec::is_empty")] pub memory_used: Vec<MemoryRecallId>`.

### Public Interface (additions)

```rust
#[async_trait::async_trait]
pub trait PersonalCore: Send + Sync + 'static {
    // … existing methods …
    /// The plain lines of the facts and links these recalls returned (program: `MemoryService::memory_used(Workspace(W), ids)`).
    async fn memory_used(&self, workspace_id: WorkspaceId, recall_ids: &[MemoryRecallId]) -> Result<MemoryUsedText, CoreError>;
}

#[async_trait::async_trait]
pub trait HostedAgentCore: Send + Sync + 'static {
    // … existing methods …
    /// The plain lines of what one hosted run recalled (program: the run's recall ids from the Memory
    /// Service's recall log, then `memory_used(Scope(S), ids)`). Empty `lines` and `gone` 0 = it used nothing.
    async fn memory_used(&self, scope: ScopeId, run_id: &str) -> Result<MemoryUsedText, CoreError>;
}
```

### Internal Logic (additions and changes)

**H.2 (changed) — `message` block.** When the block's `memory_used` is not empty, the text is sent with one more button, `texts.memory_used_button_personal`, an `x:` action (owner Gateway, `single_use` false, payload `{"t":"memory_used","w":<workspace_id>,"r":[<recall ids>]}`, under the 2 KiB limit for 20 ids). The workspace is the block's.

**F.3 (addition) — `memory_used` action.** `core.memory_used(w, r)`: `lines` empty and `gone` 0 → `texts.memory_used_none`; else `texts.memory_used_header`, then the lines one per line prefixed "• " (SA-CHAN-AMEM-04), then `texts.memory_used_gone(gone)` when `gone > 0`; sent to the chat the press came from (SA-CHAN-AMEM-01). A `CoreError` → `texts.try_again`. Pressing again sends the answer again.

**C (addition) — the words, personal profile.** Before C.6 routes text to Ordin, a text that matches SA-CHAN-AMEM-02 is answered as F.3 for the newest `message` block with `memory_used` sent to that chat in the last 24 hours (the gateway keeps the last such block's workspace and ids per pairing in memory; after a restart the words answer `texts.memory_used_none` until the next such block); it does not reach Ordin.

**K.5.d (addition) — the hosted reply's button.** After the actions are chosen: `core.memory_used(scope, run_id)`; non-empty `lines` or `gone > 0` → add `HostedButton { action: "memory_used", label: texts.memory_used_button_hosted, value: <reply id>, url: None, kind: Url }` last (SA-CHAN-AMEM-03) and add `"memory_used"` to the record's `actions`. A `CoreError` → no button, logged `chat.memory_used.failed {connection, code}`; the reply is delivered as before.

**K.8 (addition) — `memory_used` press.** The reply record by `value` (none → `TEXT_REPLY_GONE`); the action must be on the record (else `TEXT_ACTION_GONE`); `core.memory_used(record.scope, record.run_id)` → the answer of F.3's form, posted with `link.post(HostedPost { channel, thread: <the reply's anchor>, only_to: Some(presser) when the record is `asker-only`, else None, text, buttons: [] })` (SA-CHAN-AMEM-01). The action is not added to `done_actions`: it may be pressed again. Log `chat.memory_used.shown {connection, lines, gone}`.

**K.3 (addition) — the words, agent profile.** Before K.3.1 (*Trigger*), a `ThreadMention` or `DirectMessage` whose text matches SA-CHAN-AMEM-02 is answered as the K.8 press for the newest `chat_replies` record of the same scope whose anchor is this thread (direct chat: the newest record of that direct chat's scope); reach rules K.3.2 and the thread-read check K.3.4 apply first; no record → `texts.memory_used_none`. No run is queued.

**L. Words (rows added):**

| Field | Text |
|---|---|
| `memory_used_button_hosted` | "What I used" |
| `memory_used_button_personal` | "What Ordin used" |
| `memory_used_header` | "For that answer I used:" |
| `memory_used_none` | "That answer didn't use anything I remember." |
| `memory_used_gone` | "{n} more item(s) used then have since been forgotten." |

### Data Model

No table or column changes: the personal profile's button is a `chat_actions` row as every `x:` action; the hosted reply's button is in `chat_replies.actions`.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The core cannot read an answer's used memory | logged; the reply is delivered without the button, or the press answers `try_again` | `CoreError` code (the Memory Service's `MEM_*`) | `chat.memory_used.failed {connection, code}` |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Channel Gateway — what an answer used

  Scenario: Agent profile — a reply that used memory carries What I used
    Given a hosted run in channel scope C whose recalls returned 2 facts and 1 link
    When its reply is delivered in the thread
    Then the post carries a button "What I used" with the reply id as value

  Scenario: Agent profile — pressing it answers in the thread with only that answer's items
    Given that delivered reply
    When a channel member presses "What I used"
    Then a post in the same thread starts "For that answer I used:" and has 3 lines
    And no other fact of scope C is in it

  Scenario: Agent profile — an asker-only reply gets an asker-only answer
    Given the reply was delivered to the asker only
    When the asker presses "What I used"
    Then the answer is posted with only_to the asker

  Scenario: Agent profile — the words in the thread
    Given that delivered reply
    When a member writes "@agent what did you use?" in the thread
    Then the same answer is posted and no run is queued

  Scenario: Edge — a reply that used nothing has no button
    Given a hosted run that recalled nothing
    When its reply is delivered
    Then the post has no "What I used" button

  Scenario: Edge — a fact forgotten since
    Given the owners forgot 1 of the 2 facts after the reply
    When the button is pressed
    Then the answer has 2 lines and ends "1 more item(s) used then have since been forgotten."

  Scenario: Personal profile — Telegram button on Ordin's reply
    Given a block_shown message whose memory_used holds R1
    When it is delivered to Telegram
    Then the message carries an inline button "What Ordin used"
    And pressing it sends "For that answer I used:" and the lines of R1

  Scenario: Error — the core fails while delivering
    Given core.memory_used fails with MEM_STORE_UNAVAILABLE
    When a hosted reply is delivered
    Then it is posted without the button and "chat.memory_used.failed" is logged
```

### Performance, Security, Observability (additions)

- **Performance:** one `memory_used` core call per delivered hosted reply (p95 ≤ 100 ms, the Memory Service's target) and one per press; personal replies make no extra call until pressed.
- **Security:** a press or the words show only the facts and links one answer of the same scope used — never another scope's, never the scope's other memory; channel members cannot reach any other memory view (the Caller API's operator route is for owners). Statements are sent to the chat they were used in. Never logged: the lines, statements, thing names.
- **Observability:** logs `chat.memory_used.shown {connection, lines, gone}`, `chat.memory_used.failed {connection, code}`; no new metric.

### Gaps

None.
