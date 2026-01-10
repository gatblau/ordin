# SPEC: Telegram Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-telegram-service` · **Build phase:** 11 · **Programs:** D P
> **Dependencies:** Telegram Adapter, MCP Front, Rules Engine, Approval Service, File Service, Workspace Service, Organisation Profile, Journal

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

---

## Purpose

The Telegram Service gives agents and Ordin Telegram as a service (ADR-075, 02:621–628): it keeps the messages the installation's bot receives in Telegram groups, forum topics and channels that a workspace links, from the moment of linking; answers the five Telegram tools on the MCP Front server `telegram` (SA-NAME-02) only for the session's own workspace and only for Ordin or agents granted **Read Telegram** / **Read and draft Telegram** there; keeps drafts; and posts a draft only after the person presses **Post** on an approval card the Approval Service builds from the draft itself, recording each post as an external action closed by Telegram's message identifier. It never reads or writes the paired private chat and never treats a linked chat's message as a command (02:626, 07:67). On the desktop it is offered only when the Telegram app is found for the user, through a bot of the desktop's own whose key the person pastes on a guided screen (ADR-077, 02:627).

## Approach

The service is a provider of the MCP Front (`ToolProvider`, ADR-100) and a subscriber of the Telegram Adapter: the Adapter owns the bot connection (polling, sending, rate limits, keychain access through Identity) and pushes group and channel updates to the service through a sink trait the Adapter defines, so the service never talks to the Bot API itself. Kept messages live in the service's own SQLite tables with an FTS5 index for *search*; approval outcomes arrive as `ApprovalDecided` events on the Journal's `EventSink` (2B-2 D1), never as a call from the Approval Service. Rejected: polling Telegram from this crate (a second `getUpdates` receiver conflicts with the Adapter's, C-58); asking the Approval Service to call back (it depends on the Journal only, 2B-2 D1).

The service owns the error prefix `TG_` (shared with 2C's Telegram types).

Reconciliation assumptions (Phase 3 reconciliation):

- **SA-RECON-01** The Service API catalogue adopts this module's `telegram.*` command names, inputs and results (ruling R1): `telegram.messages` returns `Page<TelegramMessageOut>` (the view adds the chat title and the instruction mark to 2C's `KeptTelegramMessage`), and there is no `telegram.draft_save` or `telegram.ask_to_send` — drafts are written only by the tools; the person approves or refuses on the card and uses `telegram.try_again` after a refusal (reconciliation rows 351, 352).
- **SA-RECON-02** The Telegram Service does not register itself with the MCP Front: it exposes `tool_provider()` and the program registers it; it keeps the `ToolRegistry` handle for `tools_changed("telegram")` (rulings R2, R6).
- **SA-RECON-04** Journal writes use `append_entry` (kinds `telegram_<name>`), `begin_external_action` / `confirm_external_action` / `external_action`; the approval task is a Journal `EventSink`; migrations use the Journal's `MigrationStep` tuples (ruling R3). The Journal edge is in the 2B-3 row (Phase 2 correction), which closes the earlier dependency Gap.
- **SA-RECON-05** Calls use each owner's names (ruling R6): Approval Service `request` and `get` (cards are immutable, so a post's outcome is this service's own record — no `note_outcome`); Workspace Service `get` and `telegram_grant`; File Service `save_incoming`, `read_for_agent`, `read_file`. `PlaceReadCheck` is owned here and implemented by the program over `RulesEngine::may_read`; `TelegramRulesSource` is built by the program from the Organisation Profile (D3).
- **SA-CLOSE-01** (*chosen here*; closes the forum-topic title question; closing decision K22) A linked forum topic's title is the workspace's name cut to 128 characters (Telegram's limit for a topic name), set when the topic is linked; the topic's own Telegram name is never read, because the Telegram Adapter delivers no `forum_topic_created` or `forum_topic_edited` update. A topic not yet linked is offered as "[chat title] › topic [topic_id]". A later workspace rename does not change the link's title.
- **SA-RECON-03** The block check reaches `resolve_list_block` through the program's `RefResolver` for `EntityRef::TelegramLink` (reconciliation row 353).

## Shared Context

### Shared types used (copied from Phase 2C, unchanged)

```rust
// ordin-types — conventions: every …Id is a UUIDv7 newtype (serde transparent); Timestamp is RFC 3339 UTC ms "Z".
uuid_id!(WorkspaceId, AgentInstanceId, CrewRunId, RunId, ApprovalId, PlaceId, ExternalActionKey,
         TelegramLinkId, KeptTelegramMessageId, TelegramDraftId, ScopeId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TelegramLink {
    pub id: TelegramLinkId,
    pub workspace_id: WorkspaceId,
    pub chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<i64>,
    pub title: String,
    pub kind: TelegramChatKind,
    pub bot_state: TelegramBotState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_count: Option<u32>,
    pub state: TelegramLinkState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_at: Option<Timestamp>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramChatKind { Group, ForumTopic, Channel }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramBotState { Member, Administrator, Removed }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramLinkState { Offered, Linked }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct KeptTelegramMessage {
    pub id: KeptTelegramMessageId,
    pub telegram_link_id: TelegramLinkId,
    pub message_id: i64,
    pub sender_name: String,
    pub sent_at: Timestamp,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    pub files: Vec<TelegramFile>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TelegramFile { pub file_id: String, pub file_name: String, pub size_bytes: u64 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TelegramDraft {
    pub id: TelegramDraftId,
    pub telegram_link_id: TelegramLinkId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    pub text: String,
    pub files: Vec<EmailAttachment>,
    pub drafted_by: DraftedBy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_key: Option<ExternalActionKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram_message_id: Option<i64>,
    pub state: TelegramDraftState,
    pub created_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramDraftState { Draft, Posted }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailAttachment { pub place_id: PlaceId, pub path: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DraftedBy {
    AgentInstance { agent_instance_id: AgentInstanceId },
    CrewMember { crew_run_id: CrewRunId, member: AgentRef },
    Person,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExternalAction {
    pub action_key: ExternalActionKey,
    /// Absent for an action the person started outside any run; 2C, decision 80: C-7.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,      // this service writes only `TelegramPost`
    pub destination: String,           // 1–2048 chars
    pub state: ExternalActionState,
    pub sent_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<Timestamp>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind {
    SlackPost, ConnectionWrite, WrightrunRunStart, EmailSend, OriginPush, CopyPublish, TelegramPost,
    PullRequestOpened, HandoverWritten,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionState { Sent, Confirmed, UnknownOutcome }

// Wire part (2C §5, §7)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramListBlock { pub telegram_link_id: TelegramLinkId }

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
    // other variants (Command, Changes, Page, Files, Email) are not produced by this service
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectStyle { Reject, NotNow }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState { Pending, Approved, Rejected, Expired }
// ServiceEvent::ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String }
// EventEnvelope { api_version, backend, seq: u64, at: Timestamp, #[serde(flatten)] event: ServiceEvent }
// ProgressCount { done: u64, total: Option<u64> }
```

2C validation rules this service enforces (codes verbatim): `TG_TOPIC_MISMATCH` (`topic_id` present iff `kind = forum_topic`), `TG_TITLE_INVALID` (title 1–255), `TG_LINK_DUPLICATE` (unique `(chat_id, topic_id)`), `TG_MESSAGE_DUPLICATE` (unique `(telegram_link_id, message_id)`), `TG_TEXT_TOO_LONG` (kept text ≤ 4,096), `TG_DRAFT_TEXT_INVALID` (draft text 1–4,096), `TG_DRAFT_FILES_TOO_MANY` (≤ 10, LD-54), `TG_POST_FIELDS_MISMATCH` (`action_key`, `telegram_message_id` present iff `state = posted`), `EXT_KEY_DUPLICATE` (Journal, on action-key reuse). Approval card limits (2C §5): question 1–120 chars ending "?", verb 1–32, where line 1–200, affects 1–50 items of 1–200 chars, changes 1–1,000, `Message.text` 1–100,000.

### `ToolProvider` and `ToolRegistry` — owned by the MCP Front (McpFront.md, ADR-100; copied, ruling R2)

```rust
// crate ordin-mcp-front (owner). This crate implements ToolProvider; the program registers it.
pub use ordin_types::session::ToolCaller;   // Ordin | Agent | CrewMember { crew_run_id } | HostedAgent
pub struct ServerName(String);              // ^[a-z][a-z0-9_]{0,31}$
pub enum ProgramProfile { Desktop, PersonalServer, AgentProfile }

pub struct SessionGrant {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,          // agent profile: the one workspace the Agent Loader assigns
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    pub profile: ProgramProfile,
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,          // agent profile only; None = agent-wide or no scope
    pub coding_task_id: Option<CodingTaskId>,
}
pub struct ToolKey { pub server: ServerName, pub tool: String }   // tool ^[A-Za-z][A-Za-z0-9_-]{0,63}$
pub struct ServerSpec { pub name: ServerName, pub tools: Vec<ToolSpec> }
pub struct ToolSpec {
    pub name: String,
    pub description: String,                // one line; 1–120 chars (1–1,024 in the agent profile)
    pub input_schema: serde_json::Value,    // JSON Schema, "type": "object"
    pub argument_form: String,              // 1–300 chars, quoted to the model on a wrong call
    pub sandbox_callable: bool,             // offered in the Sandbox's ordin_tools module
    pub plain: String,                      // start-up report sentence, 1–200 chars
}
pub enum CallVia { Harness, Sandbox }
pub struct ProgressSink { /* private */ }
impl ProgressSink { pub fn report(&self, done: u64, total: Option<u64>); }
pub struct ToolCallContext {
    pub run_id: RunId, pub workspace_id: WorkspaceId, pub agent_instance_id: AgentInstanceId,
    pub caller: ToolCaller, pub scope_id: Option<ScopeId>, pub coding_task_id: Option<CodingTaskId>,
    pub call_id: ToolRequestId, pub correlation_id: String,
    pub deadline: std::time::Instant, pub cancel: tokio_util::sync::CancellationToken,
    pub progress: ProgressSink, pub only_needed_parts: bool, pub via: CallVia,
}
pub struct ToolOutput { pub text: String, pub structured: Option<serde_json::Value> }
pub struct ToolFailure { pub code: String, pub message: String /* 1–300 chars */, pub retryable: bool }
pub struct ArgumentProblem { pub at: String /* JSON Pointer */, pub reason: String /* 1–300 chars */ }

#[async_trait::async_trait]
pub trait ToolProvider: Send + Sync + 'static {
    fn provider_id(&self) -> &'static str;                 // ^[a-z][a-z0-9-]{0,39}$
    fn servers(&self) -> Vec<ServerSpec>;
    fn server_prefix(&self) -> Option<&'static str> { None }
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>;   // within 2 s
    fn check_arguments(&self, key: &ToolKey, arguments: &serde_json::Value) -> Result<(), ArgumentProblem>;
    async fn call(&self, ctx: ToolCallContext, key: &ToolKey, arguments: serde_json::Value)
        -> Result<ToolOutput, ToolFailure>;
    async fn session_closed(&self, run_id: RunId);
}
pub trait ToolRegistry: Send + Sync {
    fn register(&self, provider: std::sync::Arc<dyn ToolProvider>) -> Result<(), McpError>;
    fn tools_changed(&self, server: &ServerName);
    fn served_tools(&self, server: &ServerName) -> Option<Vec<String>>;
}
```

How this spec reads the earlier shape (SA-RECON, reconciliation rows 279, 283–286, 294–297): what it called `tools(session)` is `servers()` (the full tool list with `ToolSpec`s) plus `offer(&grant)` (the keys this session gets); `session.caller`, `ctx.workspace_id`, `session.scope_id` are the grant's or the context's fields of the same names, and the agent instance is `agent_instance_id`; `session.program` is `grant.profile` (`AgentSingle` / `AgentScaled` both read as `AgentProfile`; a provider that must tell the single and scaled shapes apart reads its own configuration, row 285); the session's progress channel is `ctx.progress`. A `ToolError::InvalidArguments { code, reason }` raised by a schema or type check is returned from `check_arguments` as `ArgumentProblem { at, reason }` (the MCP Front answers `MCP_ARGUMENTS_INVALID`); an invalid-argument finding that needs state (a missing record) stays in `call` as `ToolFailure { code, message: reason, retryable: false }`. `ToolError::Refused { code, reason }` → `ToolFailure { code, message: reason, retryable: false }`; `ToolError::Unavailable { code, reason, retryable }` → `ToolFailure { code, message: reason, retryable }`; `ToolError::Internal { code }` → `ToolFailure { code, message: <the error table's sentence>, retryable: false }`. `ToolResult { content }` → `ToolOutput { text: <content as compact JSON, or the result text where this spec gives one>, structured: Some(content) for JSON content, None for a text result }`. `session_ended` / session end → `session_closed(run_id)`.

`ProgramKind` in this spec reads as `ProgramProfile` (the Telegram Service is desktop and personal server only).

### Journal (`ordin-journal`, JournalAndArtefactStore.md; ruling R3)

```rust
#[derive(Clone)]
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
impl Journal {   // the concrete `ordin_journal::Journal`
    pub fn datastore(&self) -> &Datastore;
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    pub async fn begin_external_action(&self, new: NewExternalAction) -> Result<ExternalAction, JournalError>;
    pub async fn confirm_external_action(&self, key: ExternalActionKey) -> Result<ExternalAction, JournalError>;
    pub async fn external_action(&self, key: ExternalActionKey) -> Result<Option<ExternalAction>, JournalError>;
    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId;
}
pub struct NewJournalEntry { pub kind: String /* ^[a-z][a-z0-9_]{0,63}$ */, pub component: String,
    pub run_id: Option<RunId>, pub crew_run_id: Option<CrewRunId>, pub workspace_id: Option<WorkspaceId>,
    pub summary: String, pub data: serde_json::Value }
pub struct NewExternalAction { pub action_key: ExternalActionKey, pub run_id: Option<RunId>,
    pub kind: ExternalActionKind /* TelegramPost */, pub destination: String }
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    fn name(&self) -> &'static str;
    async fn deliver(&self, event: &JournalEvent);     // this service reads kind "approval_decided"
    async fn lagged(&self, next_seq: u64);
}
```
Every journal write of this spec (`append telegram.<name>`) is `journal.append_entry(NewJournalEntry { kind: "telegram_<name>", component: "telegram", … })`. `record_external_action(ExternalAction { … })` is `begin_external_action(NewExternalAction { action_key, run_id, kind: TelegramPost, destination })`; `confirm_external_action(key)` takes no time. `events.subscribe()` is a Journal `EventSink` registered with `journal.subscribe`.

```rust
// ordin-telegram-adapter (owner; shape required here — interaction "needs callee to define").
#[async_trait::async_trait]
pub trait TelegramGroupSink: Send + Sync + 'static {
    /// Called once per update from a group, supergroup or channel (never from a private chat),
    /// in the order Telegram delivered them, after the Adapter has advanced its polling offset.
    async fn on_group_update(&self, update: TelegramGroupUpdate);
}
#[derive(Debug, Clone)]
pub enum TelegramGroupUpdate {
    /// `message` or `channel_post` in a chat the bot is in.
    Message(TelegramIncomingMessage),
    /// `my_chat_member`: the bot was added, promoted, demoted or removed.
    BotMembership { chat: TelegramChatInfo, bot_state: TelegramBotState },
    /// `new_chat_title` service message.
    ChatTitle { chat_id: i64, title: String },
}
#[derive(Debug, Clone)]
pub struct TelegramChatInfo { pub chat_id: i64, pub title: String, pub chat_type: TelegramChatType, pub is_forum: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramChatType { Private, Group, Supergroup, Channel }
#[derive(Debug, Clone)]
pub struct TelegramIncomingMessage {
    pub chat: TelegramChatInfo,
    /// `message_thread_id` when the chat is a forum and the message is in a topic.
    pub topic_id: Option<i64>,
    pub message_id: i64,
    /// First and last name, or the channel title for a channel post; never the user identifier.
    pub sender_name: String,
    pub sent_at: Timestamp,
    /// `text`, or `caption` when the message carries a file.
    pub text: String,
    pub reply_to_message_id: Option<i64>,
    pub files: Vec<TelegramFile>,
}
#[derive(Debug, Clone)]
pub struct BotInfo { pub username: String, pub display_name: String, pub can_read_all_group_messages: bool }
#[derive(Debug, Clone)]
pub struct OutgoingPost {
    pub chat_id: i64,
    pub topic_id: Option<i64>,
    pub reply_to_message_id: Option<i64>,
    pub text: String,                       // 1–4,096 chars
    pub files: Vec<OutgoingFile>,           // ≤ 10, each ≤ 50 MB
}
#[derive(Debug, Clone)]
pub struct OutgoingFile { pub file_name: String, pub bytes: bytes::Bytes }
#[derive(Debug, Clone)]
pub struct PostedMessage { pub message_id: i64, pub file_message_ids: Vec<i64> }
#[derive(Debug, Clone, thiserror::Error)]
pub enum TelegramBotError {
    #[error("no bot configured")] NotConfigured,
    #[error("bot key refused")] KeyInvalid,
    #[error("telegram unreachable")] Unreachable,
    #[error("telegram refused: {description}")] Refused { description: String },
    #[error("file larger than allowed")] FileTooLarge,
}
#[async_trait::async_trait]
pub trait TelegramBot: Send + Sync + 'static {
    fn set_group_sink(&self, sink: std::sync::Arc<dyn TelegramGroupSink>);
    async fn bot_info(&self) -> Result<BotInfo, TelegramBotError>;
    async fn member_count(&self, chat_id: i64) -> Result<u32, TelegramBotError>;
    /// getFile + download; refuses with FileTooLarge above `max_bytes` without downloading.
    async fn download_file(&self, file_id: &str, max_bytes: u64) -> Result<bytes::Bytes, TelegramBotError>;
    /// sendMessage (text) then sendDocument per file, all replying to `reply_to_message_id`,
    /// within the Adapter's rate limits (1/s per chat, 20/min per group; C-52).
    async fn post(&self, post: OutgoingPost) -> Result<PostedMessage, TelegramBotError>;
    /// Desktop only: checks the key with getMe, stores it in the keychain through Identity and
    /// Credentials, and starts long polling while ordin is open (ADR-077).
    async fn connect_desktop_bot(&self, key: String) -> Result<BotInfo, TelegramBotError>;
    /// Desktop only: stops polling and deletes the key from the keychain.
    async fn remove_desktop_bot(&self) -> Result<(), TelegramBotError>;
}
```

```rust
// Other boundaries, with each owner's exact signature (ruling R6).

// Approval Service (ApprovalService.md) — direct edge.
pub async fn request(&self, req: ApprovalRequest) -> Result<ApprovalCard, ApprovalError>;   // card.approval_id
pub async fn get(&self, approval_id: ApprovalId) -> Result<ApprovalRecord, ApprovalError>;   // `.state`
pub struct ApprovalRequest { pub workspace_id: WorkspaceId, pub asked_by: ApprovalAsker, pub question: String,
    pub verb: String, pub where_line: String, pub affects: Vec<AffectedItem>, pub changes: String,
    pub content: Option<ApprovalContent>, pub reject_style: RejectStyle, pub show_details: bool,
    pub proposal: ActionProposal, pub asker: Option<String>, pub leaves_company: bool }
// A card is immutable after `request` (ApprovalService.md): the outcome of a post ("Posted 10:44",
// refused with Try again, unknown outcome) is this service's own record, shown from its draft and
// post-request states — there is no `note_outcome`.

// Workspace Service (WorkspaceService.md) — direct edge.
pub async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;   // own_folder_place_id; WS_NOT_FOUND
pub async fn telegram_grant(&self, id: WorkspaceId, agent: AgentInstanceId) -> Result<Option<TelegramGrant>, WorkspaceError>;
pub enum TelegramAccess { ReadTelegram, ReadAndDraftTelegram }      // TelegramGrant.access (2C, C-25)

// File Service (FileService.md) — direct edge.
pub async fn save_incoming(&self, place_id: PlaceId, file_name: &str, bytes: Vec<u8>, origin_note: &str)
    -> Result<SavedFile, FileError>;                 // FileError::IncomingRefused { reason } when unsafe
pub async fn read_for_agent(&self, agent_instance_id: AgentInstanceId, workspace_id: WorkspaceId,
    place_id: PlaceId, relative_path: &str, max_bytes: u64) -> Result<Vec<u8>, FileError>;
pub async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError>;   // Ordin and the person; size tested here
pub struct SavedFile { pub place_id: PlaceId, pub path: String, pub hidden_content_found: bool }

// Place read check — trait owned here, implemented by the program over the Rules Engine's
// `may_read(&PlaceRead) -> bool` (a crew member is mapped to its crew run's charter; RulesEngine.md K.4).
#[async_trait::async_trait]
pub trait PlaceReadCheck: Send + Sync {
    async fn may_read(&self, ws: WorkspaceId, caller: ToolCaller, agent: AgentInstanceId, place: PlaceId, path: &str) -> bool;
}

// ordin-organisation-profile
#[derive(Debug, Clone, PartialEq)]
pub struct TelegramRules {
    pub telegram_allowed: bool,         // "telegram" ∈ channels.allowed[]
    pub tools_enabled: bool,            // telegram.tools_enabled (default true)
    pub allowed_chats: Option<Vec<i64>>,// telegram.allowed_chats[]; None = any chat a workspace links
    pub foot_line_required: bool,       // telegram.foot_line_required (default false)
}
/// Built by the program from the Organisation Profile's `current().document.telegram` and
/// `document.channels.allowed`, following `subscribe()` (OrganisationProfile.md consumer table).
pub trait TelegramRulesSource: Send + Sync {
    fn telegram_rules(&self) -> TelegramRules;
    fn subscribe_changes(&self) -> tokio::sync::watch::Receiver<u64>;
}

// apps/desktop (composition root) — platform probe for the installed Telegram app (C-58, C-59).
pub trait TelegramAppProbe: Send + Sync {
    /// macOS: Launch Services' handler for `tg:` is an app whose bundle identifier is
    /// `ru.keepcoder.Telegram` or `org.telegram.desktop`. Windows: the `tg` URL handler registered
    /// for the current user points at a `Telegram.exe`, or `%APPDATA%\Telegram Desktop\Telegram.exe`
    /// exists. Never reads Telegram's data.
    fn telegram_app_found(&self) -> bool;
}
```

### Configuration (2D)

| Key / constant | Value | Source |
|---|---|---|
| `channels.allowed[]` | contains `telegram` for the service to be offered | 2D-2, A-40 |
| `telegram.tools_enabled` | boolean, default `true` | 2D-2, ADR-075 |
| `telegram.allowed_chats[]` | list of chat identifiers; default none = any chat | 2D-2, ADR-075 |
| `telegram.foot_line_required` | boolean, default `false` | 2D-2, ADR-075 |
| Foot line text | `Drafted with an agent in ordin` (as the profile's email rule, 08:72; message-catalogue id `telegram-post-foot-line`, *chosen here*) | ADR-075 |
| Telegram limits | receive ≤ 20 MB; send ≤ 50 MB per file; text ≤ 4,096; caption ≤ 1,024; ~1 message/s per chat, 20/min per group | 2D-4, C-52 |
| Desktop bot key | keychain, set on the guided screen | 2D-3, ADR-077 |
| Search result cap | 50 per call, default 20 *(chosen here)* | — |
| Read page size | 100 messages per call, default 50 *(chosen here)* | — |

No environment variable is read by this crate.

## Public Interface

### Constructor and registration

```rust
pub struct TelegramService { /* private */ }

pub struct TelegramServiceDeps {
    pub bot: std::sync::Arc<dyn TelegramBot>,
    pub approvals: std::sync::Arc<ordin_approval::ApprovalService>,
    pub workspaces: std::sync::Arc<ordin_workspaces::WorkspaceService>,
    pub files: std::sync::Arc<ordin_files::FileService>,
    /// Implemented by the program over `RulesEngine::may_read` (SA-RECON-05).
    pub places: std::sync::Arc<dyn PlaceReadCheck>,
    /// Built by the program from the Organisation Profile (SA-RECON-05).
    pub rules: std::sync::Arc<dyn TelegramRulesSource>,
    pub journal: ordin_journal::Journal,
    /// Kept for `tools_changed("telegram")`; registration is the program's (SA-RECON-02).
    pub registry: std::sync::Arc<dyn ToolRegistry>,
    /// Desktop: Some(probe). Personal server: None (the server's bot is ADR-073's).
    pub app_probe: Option<std::sync::Arc<dyn TelegramAppProbe>>,
    pub program: ProgramKind,           // Desktop or PersonalServer only
}

impl TelegramService {
    /// This module's migrations (module "telegram_service") for the Journal's registry (ruling R3).
    pub fn migrations() -> ModuleMigrations;   // module "telegram_service" (ModuleMigrations.module)
    /// Builds the service, registers itself as the Adapter's group sink, starts the approval-event task and the profile-change task, and reconciles
    /// post requests left in `posting` by a crash. Refuses `ProgramKind::AgentSingle | AgentScaled`.
    pub async fn start(deps: TelegramServiceDeps) -> Result<std::sync::Arc<Self>, TelegramError>;
    /// Stops the two background tasks within 2 s (graceful shutdown, SA-XC-13).
    pub async fn shutdown(&self);
    /// The `telegram` server's provider, for the program to register with the MCP Front
    /// (SA-RECON-02). Every call returns the same provider.
    pub fn tool_provider(self: &std::sync::Arc<Self>) -> std::sync::Arc<dyn ToolProvider>;
}

**Called by** (each interaction is listed on the caller's side too):

| Public item | Called by |
|---|---|
| `migrations`, `start`, `shutdown`, `tool_provider` | the desktop and personal-server programs (OrdinDesktop, OrdinServer); `tool_provider` is passed to `McpFront::register` |
| `status`, `connect_bot`, `remove`, `offered_chats`, `link`, `unlink`, `links`, `messages`, `save_file`, `try_again` | the program's Service API implementation, for the `telegram.*` commands named beside each (SA-RECON-01) |
| `resolve_list_block` | the program's `RefResolver` (SA-RECON-03) |
| the group sink | Telegram Adapter (`set_group_sink`) |
| tools `telegram_search`, `telegram_read`, `telegram_save_file`, `telegram_draft_post`, `telegram_ask_to_post` | the MCP Front, for desktop and personal-server sessions |
impl TelegramGroupSink for TelegramService { /* see Internal Logic §2 */ }
```

### MCP tools — server `telegram` (SA-NAME-02)

Every argument object refuses unknown fields. Results are JSON objects with snake_case keys (SA-JSON-01).

| Tool | Offered to | Description (one line, ADR-062) | Arguments | Result |
|---|---|---|---|---|
| `telegram_search` | Ordin; agents/crew members with Read Telegram or Read and draft Telegram in the workspace | "Search messages kept from this workspace's linked Telegram chats." | `query` string 1–200; `link_id` TelegramLinkId optional; `since` Timestamp optional; `limit` 1–50, default 20 | `{"messages":[TelegramMessageOut…]}` newest first |
| `telegram_read` | as `telegram_search` | "Read messages of one linked Telegram chat, oldest first." | `link_id` TelegramLinkId; `message_ids` array of 1–100 i64 optional; `before_message_id` i64 optional; `limit` 1–100, default 50 | `{"link":TelegramLinkOut,"messages":[…],"kept_from":Timestamp,"more_before":bool}` |
| `telegram_save_file` | agents/crew members with Read and draft Telegram; Ordin | "Save one file from a linked Telegram message into this workspace's folder, after a safety check." | `link_id`; `message_id` i64; `file_id` string 1–256 | `{"place_id":…,"path":"…","file_name":"…","checked":true}` |
| `telegram_draft_post` | agents/crew members with Read and draft Telegram; Ordin | "Write a draft post or reply for a linked Telegram chat; nothing is posted until the person approves." | `link_id`; `text` 1–4,096; `reply_to_message_id` i64 optional; `files` array ≤ 10 of `{"place_id":PlaceId,"path":string 1–4096 bytes}` | `{"draft_id":…,"text_as_posted":"…","foot_line_added":bool}` |
| `telegram_ask_to_post` | as `telegram_draft_post` | "Ask the person to approve posting a draft; the person decides on a card." | `draft_id` TelegramDraftId | `{"approval_id":…,"state":"waiting_for_you"}` |

```rust
#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub struct TelegramMessageOut {
    pub link_id: TelegramLinkId,
    pub chat_title: String,
    pub message_id: i64,
    pub sender_name: String,
    pub sent_at: Timestamp,
    /// The text as kept; content, never instructions (R-66).
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    pub files: Vec<TelegramFile>,
    /// True when the instruction detector matched (Internal Logic §8).
    pub asks_agent_to_act: bool,
}
#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub struct TelegramLinkOut { pub link_id: TelegramLinkId, pub title: String, pub kind: TelegramChatKind, pub bot_in_chat: bool }
```

### Service API methods (called by the program's `ServiceApi` implementation; command names *chosen here*, SA-API-01)

```rust
impl TelegramService {
    /// `telegram.status` — whether Telegram is offered here and the bot's state.
    pub async fn status(&self) -> Result<TelegramStatus, TelegramError>;
    /// `telegram.connect_bot` (desktop) — the pasted key; checks it, stores it, starts polling.
    pub async fn connect_bot(&self, key: String) -> Result<BotInfo, TelegramError>;
    /// `telegram.remove` (desktop Settings › Remove Telegram) — deletes the key, stops polling.
    pub async fn remove(&self) -> Result<(), TelegramError>;
    /// `telegram.offered_chats` — chats and topics the bot is in, not yet linked (Add to this workspace › Telegram).
    pub async fn offered_chats(&self, ws: WorkspaceId) -> Result<Vec<OfferedChat>, TelegramError>;
    /// `telegram.link` — links one offered chat or topic to the workspace.
    pub async fn link(&self, ws: WorkspaceId, chat_id: i64, topic_id: Option<i64>) -> Result<TelegramLink, TelegramError>;
    /// `telegram.unlink` — after the screen's §5.5 confirmation: deletes the link and its kept messages.
    pub async fn unlink(&self, ws: WorkspaceId, link_id: TelegramLinkId) -> Result<(), TelegramError>;
    /// `telegram.links` — the workspace's links with new-message counts (tree, §5.25).
    pub async fn links(&self, ws: WorkspaceId) -> Result<Vec<LinkSummary>, TelegramError>;
    /// `telegram.messages` — one link's messages, paged (SA-PAGE-01: 100 per page, opaque cursor); marks them seen.
    pub async fn messages(&self, ws: WorkspaceId, link_id: TelegramLinkId, cursor: Option<String>) -> Result<Page<TelegramMessageOut>, TelegramError>;
    /// `telegram.save_file` — the person's **Save file**.
    pub async fn save_file(&self, ws: WorkspaceId, link_id: TelegramLinkId, message_id: i64, file_id: String) -> Result<SavedFile, TelegramError>;
    /// `telegram.try_again` — after a refused post: a new card for the same draft (LD-29 pattern).
    pub async fn try_again(&self, ws: WorkspaceId, draft_id: TelegramDraftId) -> Result<ApprovalId, TelegramError>;
    /// Block Renderer check of a `TelegramListBlock`: the link exists and belongs to `ws`.
    pub async fn resolve_list_block(&self, ws: WorkspaceId, block: &TelegramListBlock) -> Result<(), TelegramError>;
}

#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub struct TelegramStatus { pub offered: bool, pub reason_not_offered: Option<NotOfferedReason>, pub bot: Option<BotInfo>, pub connected: bool }
#[derive(Serialize, Clone, Copy)] #[serde(rename_all = "snake_case")]
pub enum NotOfferedReason { NotAllowedByCompany, ToolsTurnedOff, AppNotOnComputer, NoBot }
#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub struct OfferedChat { pub chat_id: i64, pub topic_id: Option<i64>, pub title: String, pub kind: TelegramChatKind, pub bot_state: TelegramBotState, pub reads_every_message: bool }
#[derive(Serialize)] #[serde(rename_all = "snake_case")]
pub struct LinkSummary { pub link: TelegramLink, pub new_messages: u32, pub kept_from: Timestamp }
pub use ordin_types::paging::{Page, Cursor};   // 2C (C-15): Page<T> { items: Vec<T>, next_cursor: Option<Cursor> }; SA-PAGE-01

#[derive(Debug, thiserror::Error)]
pub enum TelegramError {
    #[error("{code}")] Refused { code: &'static str },      // permanent
    #[error("{code}")] Unavailable { code: &'static str },  // retryable
    #[error("{code}")] Internal { code: &'static str },
}
```

`BotInfo` is serialised with `#[derive(Serialize)]` by the program for the Service API (fields `username`, `display_name`, `can_read_all_group_messages`).

### Example

`telegram_draft_post` from an agent with **Read and draft Telegram** in "Q3 supplier review", profile `telegram.foot_line_required = true`:

```json
{"link_id":"0192a2d0-1111-7222-8333-444455556666","text":"Thanks Ahmed — the revised quote is in our comparison; we will confirm by Friday.","reply_to_message_id":5812,"files":[{"place_id":"0192a0f1-aaaa-7bbb-8ccc-ddddeeeeffff","path":"Q3 comparison.pdf"}]}
```

Result:

```json
{"draft_id":"0192a2d1-0000-7000-8000-00000000b001","text_as_posted":"Thanks Ahmed — the revised quote is in our comparison; we will confirm by Friday.\n\nDrafted with an agent in ordin","foot_line_added":true}
```

`telegram_ask_to_post` with that `draft_id` produces this `ApprovalRequest` (card words from 11 §5.25):

```json
{"workspace_id":"0192a0f1-1111-7222-8333-444455556666","asked_by":{"kind":"run","run_id":"0192a1c2-3344-7556-8677-8899aabbccdd"},
 "question":"Post in Suppliers — Northwind?","verb":"Post",
 "where_line":"Suppliers — Northwind · seen by about 14 people · from your ordin bot",
 "affects":[{"kind":"record","label":"Suppliers — Northwind"},{"kind":"file","label":"Q3 comparison.pdf","version":null,"place_id":"0192a0f1-aaaa-7bbb-8ccc-ddddeeeeffff"}],
 "changes":"A post appears in Suppliers — Northwind from your ordin bot. Replying to Ahmed, 10:31. Drafted by Supplier analyst.",
 "content":{"kind":"message","label":"See what will be sent","text":"Thanks Ahmed — the revised quote is in our comparison; we will confirm by Friday.\n\nDrafted with an agent in ordin","destination_note":"Replying to Ahmed, 10:31 · Drafted by Supplier analyst"},
 "reject_style":"not_now"}
```

Both programs use the words "from your ordin bot" (11 §5.25; *assumed here* for the personal server, whose card ADR-075 describes as "from the bot").

## Internal Logic

### 1. Start (`start`)
1. Refuse `program ∉ {Desktop, PersonalServer}` → `TelegramError::Internal { code: "TG_PROGRAM_UNSUPPORTED" }`; log `telegram.start_refused` at `error`.
2. Call `bot.set_group_sink(self)`; build the provider returned by `tool_provider()` (the program registers it, SA-RECON-02).
3. Register the **approval task** (§6) as a Journal `EventSink` (`journal.subscribe`) and spawn the **profile task** on `rules.subscribe_changes()`: on each change, recompute `TelegramRules` and call `registry.tools_changed(&ServerName::new("telegram"))`; log `telegram.rules_changed` with `telegram_allowed`, `tools_enabled`, `allowed_chat_count`.
4. **Crash reconciliation.** Select `telegram_post_requests` rows with `state = 'posting'`. For each, read `journal.external_action(action_key)`: `Confirmed` → set request `posted` and the draft `posted` if not yet; `Sent` or `UnknownOutcome` or absent → set request `unknown_outcome`, append `telegram.post_unknown_outcome`. Never post again automatically (04:200, `EXT_AUTO_RESEND`).
5. Select requests with `state = 'waiting_for_you'` and read `approvals.get(approval_id)?.state`; an already-decided approval is applied as §6 states (catch-up for events missed while closed).
6. Log `telegram.started` with `program`, `link_count`.

### 2. Group updates (`on_group_update`)
1. **Private chats never kept.** If `chat.chat_type = Private`, drop the update, log `telegram.private_update_dropped` at `warn` (no content). (The Adapter is required never to send one; this is the second guard, 02:626.)
2. `BotMembership { chat, bot_state }`: upsert `telegram_bot_chats` (`chat_id`, `title`, `chat_type`, `is_forum`, `bot_state`, `seen_at = now`). For every `telegram_links` row with that `chat_id`, set `bot_state`; when `bot_state = removed`, append `telegram.bot_removed` (`link_id`) — the link stays `linked`, marked (2C state table). When added or promoted, call `bot.member_count(chat_id)` and store it on the bot chat and its links; a failure leaves `member_count` unchanged and logs `telegram.member_count_failed` (`retryable = true`).
3. `ChatTitle { chat_id, title }`: validate 1–255 chars (else truncate to 255 at a scalar boundary and log `telegram.title_truncated`); update `telegram_bot_chats.title` and `telegram_links.title`.
4. `Message(m)`:
   a. Upsert `telegram_bot_chats` for `m.chat` (a message proves membership; `bot_state` unchanged if present, else `member`). If `m.chat.is_forum` and `m.topic_id` is `Some`, upsert `telegram_bot_topics (chat_id, topic_id, first_seen_at)` so the topic can be offered *(chosen here: a topic's title is not in the update; while offered it is shown as "[chat title] › topic [topic_id]"; once linked, its title follows SA-CLOSE-01)*.
   b. Find the link: `(chat_id, topic_id)` for a forum message in a topic, else `(chat_id, NULL)`. If none, or the link's `chat_id` is not in `allowed_chats` (when `Some`), or `telegram_allowed = false`: do not keep; count it in `telegram_bot_chats.unkept_count`; stop.
   c. Validate: `text` ≤ 4,096 (`TG_TEXT_TOO_LONG` → keep the first 4,096 scalars and log `telegram.text_truncated`); `sender_name` 1–255 (empty → `"Unknown"`); `files` each `file_name` 1–255 (empty → `"file"`).
   d. `INSERT … ON CONFLICT (telegram_link_id, message_id) DO NOTHING` into `kept_telegram_messages` and the FTS row in the same transaction. A conflict is `TG_MESSAGE_DUPLICATE`: silently ignored (redelivery after restart).
   e. On a datastore error: log `telegram.keep_failed` (`code = TG_STORE_UNAVAILABLE`, `link_id`, `message_id`) at `error`; the message is lost for ordin (the Adapter has already advanced its offset). *(Assumed here: no retry queue — a write to the local SQLite file fails only when the disk is full or the file is damaged, which the Journal reports on its own.)*
   f. Log `telegram.message_kept` (`link_id`, `message_id`, `file_count`, `text_len`) at `debug`; never the text or sender.
   No kept message starts work, opens a card or is read as a command (ADR-075 "New messages start nothing").

### 3. `servers()` and `offer(grant)`
0. `provider_id()` = `"telegram"`; `servers()` = server `telegram` with the five tools of *Public Interface* (`sandbox_callable = true` for `telegram_search` and `telegram_read`, `false` for the others; `argument_form` from each argument column; `plain` "Can search and read the Telegram chats this workspace links." / "Can save a file from a linked Telegram chat." / "Can draft Telegram posts for you to approve."). `check_arguments` is §4 step 2; `session_closed` holds nothing.
1. `offer(grant)` returns `Ok([])` when: `grant.profile` is `AgentProfile`; `TelegramRules.telegram_allowed = false`; `tools_enabled = false`; on the desktop `app_probe.telegram_app_found() = false` or no bot connected (`bot.bot_info()` → `NotConfigured`; the bot info is cached for 60 s *(chosen here)*); the workspace has no link in state `linked`.
2. Caller → tool set: `Ordin` → all five *(assumed here: Ordin drafts replies through **Reply with Ordin**, 03:1054, which needs `telegram_draft_post` and `telegram_ask_to_post`; ADR-075 names only search and read for Ordin's reads)*; `Agent`/`CrewMember` → `workspaces.telegram_grant(ws, agent)` (its `access`): `None` → `[]`; `ReadTelegram` → search, read; `ReadAndDraftTelegram` → all five; `HostedAgent` → `[]`.
3. A grant-lookup error returns `[]` and logs `telegram.grant_lookup_failed`.

### 4. `call(ctx, key, arguments)` — common steps
Every refusal below is a `ToolFailure { code, message: <the Error Table sentence>, retryable }`; results are `ToolOutput { text: <compact JSON>, structured: Some(<the result object>) }`.
1. Re-run §3 for the caller; a tool not in the caller's current set → `Refused TG_NOT_GRANTED` ("This agent isn't allowed to use Telegram in this workspace; ask the person to allow it."). Tools off or not allowed → `Refused TG_TOOLS_OFF`.
2. Deserialise `arguments` into the tool's argument struct (`deny_unknown_fields`). The same deserialisation is `check_arguments`; its failure is `ArgumentProblem { at, reason }` naming the expected fields (rule `TG_ARGS_INVALID`; the model sees `MCP_ARGUMENTS_INVALID`).
3. Every `link_id` must name a `linked` link whose `workspace_id = ctx.workspace_id`; else `Refused TG_LINK_NOT_FOUND` (never reveals links of other workspaces). A link whose `chat_id` is outside `allowed_chats` → `Refused TG_CHAT_NOT_ALLOWED`.
4. Append nothing extra to the Journal for reads; the MCP Front journals the `ToolRequest` (digest only).

### 5. Per tool
- **`telegram_search`**: `query` 1–200 chars after trimming (else `TG_QUERY_INVALID`). Build an FTS5 `MATCH` expression by splitting the query on whitespace, dropping FTS5 operator characters (`"*:^()-+`), quoting each term, joining with spaces (implicit AND). Restrict to the session workspace's linked links (or `link_id`), `sent_at ≥ since`. Order by `bm25` then `sent_at DESC`; cap at `limit`. Map to `TelegramMessageOut` with `asks_agent_to_act` (§8). Datastore error → `Unavailable TG_STORE_UNAVAILABLE retryable`.
- **`telegram_read`**: with `message_ids`, return those (missing ids skipped; none found → `Refused TG_MESSAGE_NOT_FOUND`). Otherwise the `limit` messages with `message_id < before_message_id` (or newest when absent) returned oldest first; `more_before` true when older kept messages exist. `kept_from` = `linked_at`.
- **`telegram_save_file`**: find the kept message (`TG_MESSAGE_NOT_FOUND`) and the file in its `files` (`TG_FILE_NOT_FOUND`); `size_bytes > 20 MiB` → `Refused TG_FILE_TOO_LARGE` before any download. `bot.download_file(file_id, 20 MiB)`: `Unreachable` → `Unavailable TG_TELEGRAM_UNREACHABLE retryable`; `FileTooLarge` → `TG_FILE_TOO_LARGE`; `Refused` → `Unavailable TG_FILE_UNAVAILABLE` (Telegram no longer serves the file). Then `files.save_incoming(workspaces.get(ws)?.own_folder_place_id, file_name, bytes, "Telegram: <chat title>")`: `Unsafe` → `Refused TG_FILE_UNSAFE` with reason "The file failed the safety check and was not saved."; `Io` → `Unavailable TG_FILE_SAVE_FAILED retryable`. Append `telegram.file_saved` (`link_id`, `message_id`, `place_id`, `bytes`).
- **`telegram_draft_post`**:
  1. Link checks (§4.3); link `bot_state = removed` → `Refused TG_BOT_REMOVED` ("ordin is no longer in this chat; the draft can't be posted.").
  2. `reply_to_message_id`, when given, must be a kept message of that link → else `TG_MESSAGE_NOT_FOUND`.
  3. `files.len() ≤ 10` → else `TG_DRAFT_FILES_TOO_MANY`. Each file: `places.may_read(ws, ctx.caller, ctx.agent_instance_id, place_id, path)` false → `Refused TG_DRAFT_FILE_NOT_READABLE`; read with `files.read_for_agent(agent, ws, place_id, path, 50 MiB)` for an agent or crew member, `files.read_file(place_id, path)` for Ordin — `FileError::NotFound` → `TG_DRAFT_FILE_NOT_FOUND`; `FileError::TooLarge` (or more than 50 MiB) → `Refused TG_DRAFT_FILE_TOO_LARGE`. The bytes are discarded; the draft stores only `(place_id, path)`.
  4. Foot line: when `foot_line_required` and the caller is not the person, `text_as_posted = text + "\n\n" + foot_line`, `foot_line_added = true`. `text_as_posted` must be 1–4,096 chars → else `InvalidArguments TG_DRAFT_TEXT_INVALID` with the remaining room in the reason. With files, a caption is not used: text goes in its own message (§7), so the 1,024 caption limit does not apply.
  5. `drafted_by`: `Ordin`/`Agent` → `AgentInstance { agent_instance_id }`; `CrewMember` → `CrewMember { crew_run_id, member: AgentRef::AgentInstance { … } }`.
  6. Insert `telegram_drafts` (state `draft`, `text` stored as `text_as_posted`) with `run_id = ctx.run_id` in `telegram_draft_runs`. Append `telegram.draft_written` (`draft_id`, `link_id`, `file_count`, `text_len`).
- **`telegram_ask_to_post`**:
  1. Load the draft; it must belong to a link in the session's workspace (`TG_DRAFT_NOT_FOUND`). `state = posted` → `Refused TG_DRAFT_ALREADY_POSTED`. A request for the draft in `waiting_for_you` or `posting` → `Refused TG_DRAFT_ALREADY_ASKED`.
  2. Re-check (§5 draft steps 1, 3, 4) against the current profile and grants (ADR-075: rules checked again before posting). A failure returns that step's code; `TG_CHAT_NOT_ALLOWED` here means "no card; the agent is told the chat is not allowed" (03:1051).
  3. `member_count`: from the link; when absent, try `bot.member_count`; still absent → the where line omits "· seen by about N people".
  4. Build the `ApprovalRequest` exactly as the *Example* (question `"Post in <title>?"`, truncated so it is ≤ 120 chars; verb `Post`; `reject_style = NotNow`; content `Message` label `See what will be sent`, text = draft text; affects = the chat as `Record` plus each file as `File`; `changes` names reply target "Replying to <sender>, <HH:MM>" and "Drafted by <agent display name>"). `asked_by = Run { run_id: ctx.run_id }`.
  5. `approvals.request(req)`: error → `Unavailable TG_APPROVAL_UNAVAILABLE retryable`. Insert `telegram_post_requests` (`state = waiting_for_you`, `approval_id`, `run_id`, `draft_id`). Append `telegram.post_asked`.

### 6. Approval task (posting)
1. For every Journal event of kind `approval_decided` (`data`: `approval_id`, `state`, `decided_on`) whose `approval_id` matches a `telegram_post_requests` row in `waiting_for_you`:
   - `Rejected` → request `rejected`; draft stays `draft`; append `telegram.post_rejected`.
   - `Expired` → request `expired`; append `telegram.post_expired`.
   - `Approved` → §7.
2. `EventSink::lagged(next_seq)`: log `telegram.events_lagged` (`next_seq`), then run §1 step 5 over all `waiting_for_you` requests.

### 7. Post an approved draft
1. Atomically set the request `posting` (`UPDATE … WHERE state = 'waiting_for_you'`; zero rows → another path already moved it on; stop).
2. Re-check link (`bot_state ≠ removed`, chat allowed, Telegram allowed): failure → request `refused` with reason (no **Try again**), append `telegram.post_refused` (`code`).
3. Read each file with `files.read_file(place, path)` (the person approved, so the person's read applies; above 50 MiB → `TG_DRAFT_FILE_TOO_LARGE`); failure → `refused` with `TG_DRAFT_FILE_NOT_FOUND` / `TG_DRAFT_FILE_TOO_LARGE`, `try_again: true`.
4. Mint `action_key` (UUIDv7) and call `journal.begin_external_action(NewExternalAction { action_key, run_id: Some(run_id), kind: TelegramPost, destination: "telegram:<chat_id>[/<topic_id>]" })` **before** sending; store `action_key` on the request. Journal error → request `refused` with `TG_STORE_UNAVAILABLE`, `try_again: true`; nothing was sent.
5. `bot.post(OutgoingPost { chat_id, topic_id, reply_to_message_id, text, files })`.
   - `Ok(PostedMessage { message_id, .. })` → in one transaction set the draft `state = posted`, `action_key`, `telegram_message_id = message_id`; request `posted`; then `journal.confirm_external_action(action_key)`; append `telegram.posted` (`draft_id`, `link_id`, `telegram_message_id`).
   - `Err(Refused { description })` → request `refused` (`refusal_reason` = description ≤ 500 chars, kept for Diagnostics only); external action left `sent` and marked by the Journal per its rules, offered **Try again**; append `telegram.post_refused` (`code = TG_POST_REFUSED`).
   - `Err(Unreachable)` → the outcome is unknown when the Adapter reports it after the request left; the Adapter returns `Unreachable` only when nothing was sent *(required of the Adapter; interaction)*; request `refused` with `TG_TELEGRAM_UNREACHABLE`, `try_again: true`.
   - A crash between steps 4 and 5's completion leaves the request in `posting`; §1 step 4 resolves it to `unknown_outcome`.
6. **Try again** (`try_again`): allowed only for a request in `refused`; creates a new request and card for the same draft (§5 ask steps 2–5). A request in `unknown_outcome` offers no automatic resend; the person may ask Ordin to draft again (*assumed here*, mirrors 04:200).

### 8. Instruction detector (*chosen here*, R-66)
A message `asks_agent_to_act` when its text, lower-cased, matches any of a fixed list compiled into the crate: begins with or contains after a sentence break one of `ignore (all|previous|the above)`, `you are now`, `as an ai`, `system prompt`, `approve `, `post this`, `send this`, `delete `, `run `, `execute `, `forward `, or contains `@<bot username>` followed within 40 characters by one of the same phrases. The flag never changes what is kept or returned; it drives the inline note on the screen (11 §5.25) and a field in tool results. The list is a constant `INSTRUCTION_PATTERNS` matched with plain substring search on the lower-cased text (no pattern library is added).

### 9. Service API methods
- **`status`**: `offered` per §3 step 1 (without the link condition); `reason_not_offered` in this order: `NotAllowedByCompany`, `ToolsTurnedOff`, `AppNotOnComputer` (desktop), `NoBot`. `connected` from `bot.bot_info()` success.
- **`connect_bot(key)`** (desktop only; personal server → `Refused TG_SERVER_BOT_FROM_SETUP`): no app → `Refused TG_APP_NOT_FOUND`; not allowed → `Refused TG_TOOLS_OFF`; key not matching `^[0-9]{5,16}:[A-Za-z0-9_-]{30,64}$` *(chosen here)* → `Refused TG_BOT_KEY_INVALID` without a network call; `bot.connect_desktop_bot(key)`: `KeyInvalid` → `TG_BOT_KEY_INVALID`; `Unreachable` → `Unavailable TG_TELEGRAM_UNREACHABLE`. Success: `registry.tools_changed("telegram")`, append `telegram.bot_connected` (`username`). The key is never logged or journalled.
- **`remove`**: `bot.remove_desktop_bot()`; links and kept messages are kept until the person removes each link *(assumed here: a Settings action does not delete workspace content without the §5.5 confirmation of that content)*; `tools()` returns `[]` from then on (no bot). Append `telegram.bot_removed_by_person`.
- **`offered_chats(ws)`**: rows of `telegram_bot_chats` with `bot_state ≠ removed`, `chat_type ∈ {group, supergroup, channel}`, not linked; for forum chats, each seen topic as `ForumTopic` plus the whole chat as `Group` *(assumed here)*; filtered by `allowed_chats`. `reads_every_message = bot_state = administrator || chat_type = channel || bot_info().can_read_all_group_messages` (C-55) — false drives "ordin's bot is in [group] as an ordinary member, so it sees only messages addressed to it."
- **`link(ws, chat_id, topic_id)`**: `workspaces.get(ws)` returns `WS_NOT_FOUND` → `TG_WORKSPACE_NOT_FOUND`; chat not in `telegram_bot_chats` or removed → `TG_CHAT_NOT_OFFERED`; not allowed → `TG_CHAT_NOT_ALLOWED`; `kind` = `ForumTopic` if `topic_id` is `Some` (else `Channel` for channels, `Group` otherwise), `TG_TOPIC_MISMATCH` when `topic_id` is given for a non-forum chat; duplicate `(chat_id, topic_id)` → `TG_LINK_DUPLICATE`. Insert with `state = linked`, `linked_at = now`, title from the bot chat; for a forum topic, the workspace's name cut to its first 128 characters (SA-CLOSE-01; the workspace is read by the `workspaces.get(ws)` call above). `registry.tools_changed("telegram")`; append `telegram.link_added`.
- **`unlink(ws, link_id)`**: the link must belong to `ws` (`TG_LINK_NOT_FOUND`). In one transaction delete the kept messages, their FTS rows, drafts in state `draft` and their `telegram_draft_runs`, the `telegram_link_views` row, and the link. Posted drafts and post requests stay as post records (their `telegram_link_id` is no longer a foreign key target) until journal retention. `tools_changed`; append `telegram.link_removed` (`kept_deleted` count).
- **`links(ws)`**: each link with `new_messages` = kept messages with `sent_at > telegram_link_views.last_opened_at` (all when no row).
- **`messages(ws, link_id, cursor)`**: page of 100, newest first; cursor = base64 of `(sent_at, message_id)` ≤ 1,024 chars, malformed → `SRV_BAD_CURSOR` (SA-PAGE-01). Upserts `telegram_link_views.last_opened_at = now` on the first page.
- **`save_file`**: as the tool, with caller `Person` semantics (no grant check; `origin` "Telegram: <chat title>").
- **`resolve_list_block`**: link exists, `linked`, and in `ws` → else `TG_LINK_NOT_FOUND` (the Block Renderer maps it to `BLOCK_REF_UNRESOLVED`).

## Data Model

Module `telegram_service`; SQLite only (D P; no agent profile). Migration `0001_init`. Field names equal 2C field names; JSON columns per LD-01.

```sql
-- spec: ordin-desktop/components/TelegramService#telegram_links
CREATE TABLE telegram_links (
  id             TEXT PRIMARY KEY,                 -- TelegramLinkId (UUIDv7)
  workspace_id   TEXT NOT NULL,
  chat_id        INTEGER NOT NULL,
  topic_id       INTEGER,
  title          TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 255),
  kind           TEXT NOT NULL CHECK (kind IN ('group','forum_topic','channel')),
  bot_state      TEXT NOT NULL CHECK (bot_state IN ('member','administrator','removed')),
  member_count   INTEGER CHECK (member_count IS NULL OR member_count >= 0),
  state          TEXT NOT NULL CHECK (state IN ('offered','linked')),
  linked_at      TEXT,
  CHECK ((kind = 'forum_topic') = (topic_id IS NOT NULL))
);
CREATE UNIQUE INDEX telegram_links_chat ON telegram_links (chat_id, IFNULL(topic_id, 0));
CREATE INDEX telegram_links_ws ON telegram_links (workspace_id);

CREATE TABLE kept_telegram_messages (
  id                  TEXT PRIMARY KEY,
  telegram_link_id    TEXT NOT NULL REFERENCES telegram_links(id) ON DELETE CASCADE,
  message_id          INTEGER NOT NULL,
  sender_name         TEXT NOT NULL,
  sent_at             TEXT NOT NULL,
  text                TEXT NOT NULL,
  reply_to_message_id INTEGER,
  files               TEXT NOT NULL DEFAULT '[]',   -- JSON Vec<TelegramFile>
  UNIQUE (telegram_link_id, message_id)
);
CREATE INDEX kept_telegram_messages_time ON kept_telegram_messages (telegram_link_id, sent_at);
CREATE VIRTUAL TABLE kept_telegram_messages_fts USING fts5(
  text, sender_name, content='kept_telegram_messages', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER ktm_ai AFTER INSERT ON kept_telegram_messages BEGIN
  INSERT INTO kept_telegram_messages_fts(rowid, text, sender_name) VALUES (new.rowid, new.text, new.sender_name);
END;
CREATE TRIGGER ktm_ad AFTER DELETE ON kept_telegram_messages BEGIN
  INSERT INTO kept_telegram_messages_fts(kept_telegram_messages_fts, rowid, text, sender_name)
  VALUES ('delete', old.rowid, old.text, old.sender_name);
END;

CREATE TABLE telegram_drafts (
  id                  TEXT PRIMARY KEY,
  telegram_link_id    TEXT NOT NULL,                -- no FK: posted records outlive the link
  reply_to_message_id INTEGER,
  text                TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 4096),
  files               TEXT NOT NULL DEFAULT '[]',   -- JSON Vec<EmailAttachment>, ≤ 10
  drafted_by          TEXT NOT NULL,                -- JSON DraftedBy
  action_key          TEXT,
  telegram_message_id INTEGER,
  state               TEXT NOT NULL CHECK (state IN ('draft','posted')),
  created_at          TEXT NOT NULL,
  CHECK ((state = 'posted') = (action_key IS NOT NULL AND telegram_message_id IS NOT NULL))
);
CREATE INDEX telegram_drafts_link ON telegram_drafts (telegram_link_id, state);

-- chosen here: run that wrote each draft (ExternalAction.run_id needs it)
CREATE TABLE telegram_draft_runs (
  draft_id TEXT PRIMARY KEY REFERENCES telegram_drafts(id) ON DELETE CASCADE,
  run_id   TEXT NOT NULL
);

-- chosen here: one row per approval card asked for a draft
CREATE TABLE telegram_post_requests (
  id             TEXT PRIMARY KEY,
  draft_id       TEXT NOT NULL,
  workspace_id   TEXT NOT NULL,
  run_id         TEXT NOT NULL,
  approval_id    TEXT NOT NULL UNIQUE,
  state          TEXT NOT NULL CHECK (state IN
                   ('waiting_for_you','posting','posted','refused','rejected','expired','unknown_outcome')),
  action_key     TEXT,
  refusal_code   TEXT,
  refusal_reason TEXT CHECK (refusal_reason IS NULL OR length(refusal_reason) <= 500),
  created_at     TEXT NOT NULL,
  decided_at     TEXT
);
CREATE UNIQUE INDEX telegram_post_requests_open ON telegram_post_requests (draft_id)
  WHERE state IN ('waiting_for_you','posting');

-- chosen here: chats and topics the bot is in (the offer list; no workspace yet)
CREATE TABLE telegram_bot_chats (
  chat_id       INTEGER PRIMARY KEY,
  title         TEXT NOT NULL,
  chat_type     TEXT NOT NULL CHECK (chat_type IN ('group','supergroup','channel')),
  is_forum      INTEGER NOT NULL DEFAULT 0,
  bot_state     TEXT NOT NULL CHECK (bot_state IN ('member','administrator','removed')),
  member_count  INTEGER,
  unkept_count  INTEGER NOT NULL DEFAULT 0,
  seen_at       TEXT NOT NULL
);
CREATE TABLE telegram_bot_topics (
  chat_id       INTEGER NOT NULL REFERENCES telegram_bot_chats(chat_id) ON DELETE CASCADE,
  topic_id      INTEGER NOT NULL,
  first_seen_at TEXT NOT NULL,
  PRIMARY KEY (chat_id, topic_id)
);

-- chosen here: when the person last opened each linked chat (new-message counts)
CREATE TABLE telegram_link_views (
  telegram_link_id TEXT PRIMARY KEY REFERENCES telegram_links(id) ON DELETE CASCADE,
  last_opened_at   TEXT NOT NULL
);
```

Rollback: forward-only migration (SA-XC-07); the module's first migration creates tables only. Unlinking is a destructive delete of kept messages, done only after the person's confirmation (02:626, 04:293). `PRAGMA foreign_keys = ON` is the Journal's connection setting (required of the Journal).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Started in an agent-profile program | `TelegramError::Internal` (permanent) | `TG_PROGRAM_UNSUPPORTED` | — (start fails; log only) |
| Telegram not in `channels.allowed`, or `telegram.tools_enabled = false` | `ToolFailure` (`retryable: false`) | `TG_TOOLS_OFF` | "Telegram isn't available in this workspace." |
| Caller lacks the grant the tool needs | `ToolFailure` (`retryable: false`) | `TG_NOT_GRANTED` | "This agent isn't allowed to use Telegram in this workspace; ask the person to allow it." |
| Arguments malformed or unknown field | `ArgumentProblem` (the model sees `MCP_ARGUMENTS_INVALID`) | `TG_ARGS_INVALID` | "Expected link_id, text and optionally reply_to_message_id and files." (per tool) |
| Search query empty or > 200 chars | `ArgumentProblem` (the model sees `MCP_ARGUMENTS_INVALID`) | `TG_QUERY_INVALID` | "The query must be 1 to 200 characters." |
| `link_id` not a linked chat of the session's workspace | `ToolFailure` / `TelegramError::Refused` (permanent) | `TG_LINK_NOT_FOUND` | "No linked Telegram chat with that id in this workspace." |
| Chat not in the profile's allowed chats | `Refused` (permanent) | `TG_CHAT_NOT_ALLOWED` | "Your company doesn't allow this chat; nothing was drafted." |
| Message id not kept for the link | `Refused` (permanent) | `TG_MESSAGE_NOT_FOUND` | "That message isn't among the messages kept for this chat." |
| File id not on the message | `Refused` (permanent) | `TG_FILE_NOT_FOUND` | "That message has no such file." |
| File over 20 MB | `Refused` (permanent) | `TG_FILE_TOO_LARGE` | "Telegram lets bots download files of up to 20 MB only." |
| Telegram no longer serves the file | `Unavailable` (permanent) | `TG_FILE_UNAVAILABLE` | "Telegram no longer offers this file." |
| Incoming-file safety check fails | `Refused` (permanent) | `TG_FILE_UNSAFE` | "The file failed the safety check and was not saved." |
| Writing the saved file fails | `Unavailable` (retryable) | `TG_FILE_SAVE_FAILED` | "The file couldn't be saved; try again." |
| Bot removed from the chat | `Refused` (permanent) | `TG_BOT_REMOVED` | "ordin is no longer in this chat; the draft can't be posted." |
| Draft text with foot line not 1–4,096 chars | `InvalidArguments` (permanent) | `TG_DRAFT_TEXT_INVALID` | "The post must be 1 to 4,096 characters; N characters too long." |
| More than 10 files | `InvalidArguments` (permanent) | `TG_DRAFT_FILES_TOO_MANY` | "A post can carry at most 10 files." |
| Attached file in a place the session may not read | `Refused` (permanent) | `TG_DRAFT_FILE_NOT_READABLE` | "This agent can't read that file, so it can't attach it." |
| Attached file missing | `Refused` (permanent) | `TG_DRAFT_FILE_NOT_FOUND` | "That file doesn't exist." |
| Attached file over 50 MB | `Refused` (permanent) | `TG_DRAFT_FILE_TOO_LARGE` | "Telegram lets bots send files of up to 50 MB only." |
| Draft not found in the workspace | `Refused` (permanent) | `TG_DRAFT_NOT_FOUND` | "No draft with that id in this workspace." |
| Draft already has an open card | `Refused` (permanent) | `TG_DRAFT_ALREADY_ASKED` | "The person already has this draft to decide." |
| Draft already posted | `Refused` (permanent) | `TG_DRAFT_ALREADY_POSTED` | "This draft was already posted." |
| Approval Service fails to open the card | `Unavailable` (retryable) | `TG_APPROVAL_UNAVAILABLE` | "The approval couldn't be opened; try again." |
| Telegram unreachable (download, connect, post before sending) | `Unavailable` (retryable) | `TG_TELEGRAM_UNREACHABLE` | "Telegram can't be reached right now; try again." |
| Telegram refuses the post | card noted Refused, `try_again` (permanent for that send) | `TG_POST_REFUSED` | card: refused, with **Try again** |
| Crash during a post | request `unknown_outcome` (never resent) | `TG_POST_UNKNOWN_OUTCOME` | card noted UnknownOutcome |
| Datastore read or write fails | `Unavailable` (retryable) | `TG_STORE_UNAVAILABLE` | "Telegram messages can't be read right now; try again." |
| Kept text over 4,096 chars | message kept truncated (log) | `TG_TEXT_TOO_LONG` | — |
| Duplicate kept message | ignored | `TG_MESSAGE_DUPLICATE` | — |
| Desktop: Telegram app not found | `TelegramError::Refused` (permanent) | `TG_APP_NOT_FOUND` | screen: "Telegram isn't on this computer. Install it to link Telegram groups here." |
| Desktop: pasted key malformed or refused by Telegram | `Refused` (permanent) | `TG_BOT_KEY_INVALID` | screen: "That isn't a bot key yet…" |
| Server: `connect_bot` called | `Refused` (permanent) | `TG_SERVER_BOT_FROM_SETUP` | — (the server's bot is set by `ordin-server setup`) |
| `link` for a workspace that does not exist | `Refused` (permanent) | `TG_WORKSPACE_NOT_FOUND` | — |
| `link` for a chat the bot is not in | `Refused` (permanent) | `TG_CHAT_NOT_OFFERED` | — |
| `link` with a topic for a non-forum chat | `Refused` (permanent) | `TG_TOPIC_MISMATCH` | — |
| `link` for an already linked chat or topic | `Refused` (permanent) | `TG_LINK_DUPLICATE` | — |
| Malformed page cursor | `Refused` (permanent) | `SRV_BAD_CURSOR` | SA-PAGE-01 |

Service API refusals use the SA-ENV-01 envelope; the sentence comes from the Phase 4 registry, and the person never sees a code (07).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Telegram Service

  Background:
    Given the organisation profile allows "telegram" and telegram.tools_enabled is true
    And workspace "Q3 supplier review" links the group "Suppliers — Northwind" (chat -1001234) since 18 October
    And agent "Supplier analyst" has "Read and draft Telegram" in that workspace

  Scenario: A message in a linked group is kept
    When the Telegram Adapter delivers a message 5812 from "Ahmed" in chat -1001234 with text "Revised quote attached"
    Then kept_telegram_messages holds message 5812 for the link
    And no run, card or command is started

  Scenario: A message in an unlinked chat is not kept
    Given the bot is in the group "Lunch club" which no workspace links
    When a message arrives in "Lunch club"
    Then no kept message is written and telegram_bot_chats.unkept_count for "Lunch club" increases by 1

  Scenario: A private chat update is dropped
    When the Adapter delivers an update whose chat_type is private
    Then nothing is kept and "telegram.private_update_dropped" is logged without content

  Scenario: Agent searches the workspace's linked chats
    When "Supplier analyst" calls telegram_search with query "revised quote"
    Then the result holds message 5812 with sender_name "Ahmed" and asks_agent_to_act false

  Scenario: Agent without a grant gets no tools
    Given agent "Contract checker" has no Telegram grant in the workspace
    Then tools() for its session is empty
    And a telegram_read call from it is refused with TG_NOT_GRANTED

  Scenario: Read-only grant cannot draft
    Given agent "Reader" has "Read Telegram"
    When it calls telegram_draft_post
    Then the call is refused with TG_NOT_GRANTED

  Scenario: A link of another workspace is not visible
    Given workspace "HR" links chat -1009999
    When "Supplier analyst" calls telegram_read with that link's id from "Q3 supplier review"
    Then the call is refused with TG_LINK_NOT_FOUND

  Scenario: Draft with the company's foot line
    Given telegram.foot_line_required is true
    When "Supplier analyst" drafts "Thanks Ahmed" replying to 5812
    Then the draft text ends with "\n\nDrafted with an agent in ordin" and foot_line_added is true

  Scenario: Draft too long with the foot line
    Given telegram.foot_line_required is true
    When the agent drafts a text of 4,080 characters
    Then the call is refused with TG_DRAFT_TEXT_INVALID naming how many characters too long

  Scenario: Draft attaching a file the agent cannot read
    When the agent drafts with a file in a place it has no grant for
    Then the call is refused with TG_DRAFT_FILE_NOT_READABLE and no draft is written

  Scenario: Ask to post opens a card built from the draft
    Given a draft in state draft for "Suppliers — Northwind" with 14 members
    When the agent calls telegram_ask_to_post
    Then the Approval Service receives question "Post in Suppliers — Northwind?" with verb "Post" and where line "Suppliers — Northwind · seen by about 14 people · from your ordin bot"
    And the content label is "See what will be sent" with the whole draft text
    And a post request in waiting_for_you is recorded

  Scenario: Approved post is posted once and recorded
    Given a post request waiting_for_you for approval A
    When the Journal emits ApprovalDecided for A with state approved
    Then an external action of kind telegram_post is recorded before the Adapter is called
    And after the Adapter returns message_id 5820 the draft is posted with telegram_message_id 5820
    And the external action is confirmed and the card is noted "Posted"

  Scenario: Telegram refuses the post
    Given an approved post request
    When the Adapter returns Refused "not enough rights"
    Then the request is refused with code TG_POST_REFUSED and the card offers Try again
    And nothing is posted again unless the person presses Try again

  Scenario: Crash during posting is never resent
    Given a post request in posting whose external action is in state sent
    When the service starts
    Then the request becomes unknown_outcome and the Adapter's post is not called

  Scenario: Rejected card
    When ApprovalDecided arrives with state rejected
    Then the request is rejected and the draft stays in draft

  Scenario: The bot is removed from a linked chat
    When the Adapter delivers BotMembership removed for chat -1001234
    Then the link stays linked with bot_state removed
    And telegram_read still returns kept messages
    And telegram_draft_post for that link is refused with TG_BOT_REMOVED

  Scenario: Chat no longer allowed by the profile
    Given the profile's telegram.allowed_chats changes to [-1007777]
    Then tools_changed("telegram") is called
    And telegram_ask_to_post for a draft in chat -1001234 is refused with TG_CHAT_NOT_ALLOWED and no card is opened

  Scenario: Save a file over Telegram's limit
    Given message 5812 carries "quote.zip" of 25 MB
    When the agent calls telegram_save_file for it
    Then the call is refused with TG_FILE_TOO_LARGE and nothing is downloaded

  Scenario: Save a file that fails the safety check
    When telegram_save_file downloads a file the File Service marks unsafe
    Then the call is refused with TG_FILE_UNSAFE and no file is written

  Scenario: Desktop without the Telegram app
    Given the program is the desktop and the TelegramAppProbe finds no Telegram app
    Then status() reports offered false with reason app_not_on_computer
    And tools() is empty for every session

  Scenario: Desktop bot key malformed
    When the person pastes "hello" on the Set up Telegram screen
    Then connect_bot is refused with TG_BOT_KEY_INVALID without a call to Telegram

  Scenario: Unlinking deletes kept messages
    When the person confirms Remove "Suppliers — Northwind"
    Then the link, its kept messages and its unposted drafts are deleted
    And posted drafts remain as post records

  Scenario: Linking an already linked chat
    When the person links chat -1001234 to workspace "HR"
    Then the call is refused with TG_LINK_DUPLICATE

  Scenario: The program registers the provider
    When the Telegram Service starts on the desktop
    Then it has not registered itself with the MCP Front
    And tool_provider() returns the provider the program registers for server "telegram"

  Scenario: Messages command returns the module's own view
    Given a linked chat with 2 kept messages
    When the Service API calls messages for that link
    Then each item is a TelegramMessageOut with chat_title and asks_agent_to_act

  Scenario: A linked forum topic takes the workspace's name
    Given the bot has seen topic 42 in the forum chat "Supplier desk"
    And the workspace is named "Q3 supplier review"
    When link is called with that chat and topic 42
    Then the new link's kind is forum_topic and its title is "Q3 supplier review"
```

## Performance / Security / Observability

- **Performance targets** *(chosen here)*: keeping one message (`on_group_update`) p95 ≤ 20 ms on SQLite; `telegram_search` p95 ≤ 200 ms over 200,000 kept messages; `telegram_read` p95 ≤ 50 ms; `tools()` p95 ≤ 10 ms (one grant lookup, cached bot info); card opened within 1 s of `telegram_ask_to_post` excluding the Approval Service. Memory: no message cache; at most one file of ≤ 50 MiB per in-flight post and ≤ 20 MiB per save held in memory, at most 2 concurrent saves *(chosen here)* — ceiling about 140 MiB transient.
- **Security:** every tool call is decided by the Rules Engine before it reaches the service, and the service re-checks grant, workspace and profile rules itself (defence in depth). Arguments are typed with unknown fields refused; `link_id` and `draft_id` resolve only inside the session's workspace. The paired private chat is never kept, read or written (two guards: Adapter contract and §2.1). Kept text is returned as data; the instruction detector only flags. Files are saved only after the File Service's incoming-file safety check; links in messages are never followed. No tool posts; posting happens only after `ApprovalDecided { state: approved }` for this service's own request. The desktop bot key passes through `connect_bot` to the Adapter and is never stored by this crate. SQL is parameterised only; FTS5 input is sanitised (§5 search).
- **Never logged:** bot key, message text, captions, sender names, chat titles, file names, draft text, Telegram's refusal description (stored for Diagnostics only, not logged).
- **Observability** (`tracing` JSON; no metrics or spans — D and P only, SA-XC-04/05). Events with fields: `telegram.started {program, link_count}`, `telegram.rules_changed {telegram_allowed, tools_enabled, allowed_chat_count}`, `telegram.private_update_dropped {}`, `telegram.message_kept {link_id, message_id, file_count, text_len}` (debug), `telegram.keep_failed {code, link_id, message_id}`, `telegram.text_truncated {link_id, message_id}`, `telegram.title_truncated {chat_id}`, `telegram.member_count_failed {chat_id}`, `telegram.grant_lookup_failed {workspace_id, agent_instance_id}`, `telegram.tool_refused {tool, code, workspace_id, run_id}`, `telegram.events_lagged {n}`, `telegram.post_state {request_id, draft_id, state, code}`. Journal records (`JournalRecord.kind`, *chosen here*): `telegram.link_added`, `telegram.link_removed`, `telegram.bot_removed`, `telegram.bot_connected`, `telegram.bot_removed_by_person`, `telegram.file_saved`, `telegram.draft_written`, `telegram.post_asked`, `telegram.post_rejected`, `telegram.post_expired`, `telegram.posted`, `telegram.post_refused`, `telegram.post_unknown_outcome` — each with ids and counts only.

## Gaps

None.
