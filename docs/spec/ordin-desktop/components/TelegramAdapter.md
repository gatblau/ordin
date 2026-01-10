# SPEC: Telegram Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-telegram-adapter` · **Build phase:** 10 · **Programs:** D P
> **Dependencies:** Channel Gateway, Identity, Journal

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

## Purpose

The Telegram Adapter is ordin's only connection to the Telegram Bot API (02:606–611, ADR-073, ADR-075, ADR-077, 08:82). It receives updates by long polling (`getUpdates`) and never sets a webhook, so no port is opened. On the **ordin server (personal profile)** it is the Channel Gateway's first adapter: it pairs the person's Telegram account with a one-time code shown by the setup command (valid 10 minutes; 5 wrong codes in 10 minutes stop pairing), passes the paired person's private-chat messages, files, voice notes and button presses to the gateway, and sends and edits what the gateway queues, within Telegram's limits (C-52). On the **server and the desktop** it is also the bot connection of the Telegram Service: it hands every group, forum-topic and channel update to the service's sink, and posts the service's approved drafts, reads files and member counts. It owns the polling offset and, while it is valid, the pairing code.

## Approach

One poll loop per bot, built on `reqwest` (SA-HTTP-02; 05:23 "an HTTP client in the core"), dispatches each update by chat type: the paired private chat goes to the Channel Gateway (`ChannelAdapter`, a hub trait the gateway owns), groups and channels go to the Telegram Service (`TelegramGroupSink`, which this crate owns because the service depends on it). The offset is written after each update, so a crash repeats at most one update. Rejected: a Telegram bot framework crate (none is named in 1A or 05, and the Bot API surface used here is eight methods); webhooks (need an inbound port — ADR-073 *Alternatives*).

Error prefix: **`TGA_`** (*chosen here*; `TG_` belongs to the Telegram Service and 2C's Telegram types).

## Shared Context

### Shared types (Phase 2C, copied; not redefined)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`
uuid_id!(ChannelPairingId); // UUIDv7

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramBotState { Member, Administrator, Removed }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TelegramFile { pub file_id: String, pub file_name: String, pub size_bytes: u64 }

// ChannelPairing, ChannelKind, PairingState: as in the Channel Gateway spec (2C domain §ChannelPairing).
```

### Types from the Channel Gateway (hub; copied from its spec)

```rust
pub struct ChannelLimits { pub max_text_chars: u32, pub max_caption_chars: u32, pub max_buttons: u8,
    pub max_button_label_chars: u8, pub max_send_file_bytes: u64, pub max_receive_file_bytes: u64, pub min_send_interval_ms: u32 }
pub struct ChannelAddress { pub chat_id: String }
pub struct OutboundMessage { pub text: String, pub buttons: Vec<OutboundButton>, pub file: Option<OutboundFile> }
pub struct OutboundButton { pub label: String, pub data: String } // data ≤ 24 bytes: "c:"/"x:" + 22 chars
pub struct OutboundFile { pub file_name: String, pub media_type: String, pub bytes: bytes::Bytes, pub is_image: bool }
pub struct SentMessage { pub chat_id: String, pub message_id: String }
pub enum ChannelSendError { Retryable { code: &'static str, retry_after_ms: Option<u64> }, Permanent { code: &'static str } }
pub enum ChannelStatus { NotStarted, Connected, Reconnecting { since: Timestamp }, CredentialsRefused { since: Timestamp } }
#[async_trait] pub trait ChannelAdapter: Send + Sync + 'static {
    fn kind(&self) -> ChannelKind; fn limits(&self) -> ChannelLimits;
    async fn send(&self, to: &ChannelAddress, msg: &OutboundMessage) -> Result<SentMessage, ChannelSendError>;
    async fn edit(&self, sent: &SentMessage, text: &str, buttons: &[OutboundButton]) -> Result<(), ChannelSendError>;
    async fn fetch_file(&self, file: &InboundFileRef, max_bytes: u64) -> Result<bytes::Bytes, ChannelSendError>;
    async fn ack_press(&self, press: &PressAck) -> Result<(), ChannelSendError>;
    fn status(&self) -> ChannelStatus;
}
pub struct InboundItem { pub channel: ChannelKind, pub sender_id: String, pub chat_id: String, pub is_private: bool,
    pub message_id: String, pub received_at: Timestamp, pub body: InboundBody }
pub enum InboundBody { Text { text: String }, Command { command: ChatCommand, rest: String }, Voice, File(InboundFile), Unsupported, Press { data: String, ack: PressAck } }
pub enum ChatCommand { Stop, Status, Run, Triggers }
pub struct InboundFile { pub file: InboundFileRef, pub file_name: String, pub media_type: Option<String>, pub size_bytes: Option<u64>, pub caption: Option<String> }
pub struct InboundFileRef(pub String);
pub struct PressAck(pub String);
pub enum IgnoredReason { NotPaired, OtherUser, NotPrivate, ChannelNotAllowed }
impl ChannelGateway {
    pub fn register_adapter(&self, adapter: Arc<dyn ChannelAdapter>) -> Result<(), GatewayError>;
    pub async fn receive(&self, item: InboundItem) -> Result<(), GatewayError>;
    pub fn count_ignored(&self, channel: ChannelKind, reason: IgnoredReason);
    pub async fn report_status(&self, channel: ChannelKind, status: ChannelStatus);
    pub fn is_allowed(&self, channel: ChannelKind) -> bool;
    pub async fn begin_pairing(&self, channel: ChannelKind, paired_by: &str) -> Result<ChannelPairing, GatewayError>;
    pub async fn complete_pairing(&self, pairing_id: ChannelPairingId, chat_user_id: &str, private_chat_id: &str) -> Result<ChannelPairing, GatewayError>;
    pub async fn expire_pairing(&self, pairing_id: ChannelPairingId) -> Result<(), GatewayError>;
    pub async fn active_pairing(&self, channel: ChannelKind) -> Result<Option<ChannelPairing>, GatewayError>;
}
```

### Calls on Identity and Credentials (Identity and Credentials spec, *Secret store*)

```rust
pub struct SecretName; impl SecretName { pub fn new(s: &str) -> Result<Self, CredError>; }
impl SecretStore {
    pub async fn put(&self, name: &SecretName, value: &[u8]) -> Result<(), CredError>;
    pub async fn get(&self, name: &SecretName) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, CredError>;
    pub async fn delete(&self, name: &SecretName) -> Result<(), CredError>;
}
```

Secret name: **`telegram/bot-token`** (the Identity spec's own example). On the server it is in the server's secret store (Secret Service or the encrypted `0600` file, C-50); on the desktop in the OS keychain (ADR-077).

### Constants (2D-4 and *chosen here*)

| Constant | Value | Source |
|---|---|---|
| Pairing code validity | 10 minutes | 2D-4, 02:606 |
| Wrong codes that stop pairing | 5 within 10 minutes | 2D-4, ADR-073 |
| Pairing code form | 8 characters from `23456789ABCDEFGHJKMNPQRSTUVWXYZ`, shown as `ABCD-EFGH` | *chosen here*: no 0/O, 1/I/L; ~39.6 bits against 5 tries |
| Long-poll timeout | 50 s (`timeout` parameter); HTTP timeout 65 s | *chosen here* |
| Reconnect backoff | 1 s to 5 min, with jitter, never giving up | 2D-4, 03:965 |
| Limits | send files ≤ 50 MB; receive ≤ 20 MB; button data ≤ 64 bytes; about 1 message a second per chat; 20 a minute in a group; text ≤ 4,096; caption ≤ 1,024 | 2D-4, C-52 |
| Photo sent as a photo | ≤ 10 MB; larger images are sent as documents | *assumed here*: Bot API's photo limit, not in the HLD |
| Buttons per message | 8, one per row | *chosen here*: readable on a phone |
| Request timeout (other methods) | 30 s; file upload and download 120 s | *chosen here* |
| Base address | `https://api.telegram.org` | 08:82 |

### Configuration

| Source | Key | Use |
|---|---|---|
| Organisation profile (read by the Channel Gateway) | `channels.allowed[]` contains `telegram` | the server program starts this adapter only when `gateway.is_allowed(Telegram)` or the Telegram Service needs it |
| Setup command | bot token; pairing | `set_bot_token`, `begin_pairing` |
| Environment | `HTTPS_PROXY`, `NO_PROXY` (server) / the computer's proxy settings (desktop) | applied by `reqwest` (A-39, 2D-1) |

### Telegram Bot API methods used (08:82, 08:90)

`getMe`, `getUpdates`, `sendMessage`, `sendPhoto`, `sendDocument`, `editMessageText`, `answerCallbackQuery`, `getFile` (and the file download address `https://api.telegram.org/file/bot<token>/<file_path>`), `getChatMemberCount`. Requests are `POST` with a JSON body, except uploads (`multipart/form-data`). Every answer is `{"ok": bool, "result": …, "error_code": int, "description": str, "parameters": {"retry_after": int}}`.

## Public Interface

```rust
// crate ordin-telegram-adapter

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelegramMode {
    /// ordin server, personal profile: chat channel (with a gateway) and the Telegram Service's bot.
    Server,
    /// Desktop: the Telegram Service's bot only; answers no private chat (ADR-077).
    Desktop,
}

pub struct TelegramDeps {
    pub mode: TelegramMode,
    pub secrets: Arc<ordin_identity::SecretStore>,
    /// The tables of this crate live in the Journal's `Datastore` (`journal.datastore().sqlite()`;
    /// decision D4, SA-CLOSE-01); the program hands no pool.
    pub journal: Arc<ordin_journal::Journal>,
    /// `Some` in `Server` mode when Telegram is an allowed chat channel; `None` otherwise.
    pub gateway: Option<Arc<ordin_channel_gateway::ChannelGateway>>,
}

/// Shown by the setup command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode { pub pairing_id: ChannelPairingId, pub code: String /* "ABCD-EFGH" */, pub expires_at: Timestamp }

pub struct TelegramAdapter { /* private: Arc<Inner> */ }

impl TelegramAdapter {
    /// `ModuleMigrations { module: "telegram_adapter", sqlite: &[(1, "telegram adapter tables", <the Data Model SQL>)],
    /// postgres: &[] }`; the program registers it with the Journal (R3).
    pub fn migrations() -> ordin_journal::ModuleMigrations;

    pub fn new(deps: TelegramDeps) -> Result<Arc<Self>, TelegramAdapterError>;

    /// Reads the token, calls `getMe`, registers with the gateway (Server mode with a gateway)
    /// and starts the poll loop. With no token it stays `NotStarted` and returns `Ok`.
    pub async fn start(self: &Arc<Self>, shutdown: CancellationToken) -> Result<(), TelegramAdapterError>;
    /// Desktop: stop polling on sleep; resume on wake (ADR-011, ADR-077).
    pub fn pause(&self);
    pub fn resume(&self);

    // ---- setup command (server) ----
    /// Checks the token with `getMe` and stores it as `telegram/bot-token`. The running server
    /// reads it at its next start (the setup command says to restart it).
    pub async fn set_bot_token(&self, token: &str) -> Result<BotInfo, TelegramAdapterError>;
    /// Creates a pending pairing through the gateway and stores the code's hash; returns the code.
    pub async fn begin_pairing(&self, paired_by: &str) -> Result<PairingCode, TelegramAdapterError>;
}

#[async_trait::async_trait]
impl ordin_channel_gateway::ChannelAdapter for TelegramAdapter { /* step F */ }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct TelegramAdapterError { pub code: &'static str, pub retryable: bool }

// ======== The Telegram Service's boundary (owned here; shapes as the Telegram Service spec requires) ========

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

impl TelegramBot for TelegramAdapter { /* step G */ }
```

**Called by.**

| Item | Caller |
|---|---|
| `new`, `start` | Ordin Server program (`mode: Server`, the Journal, `gateway: Some(..)` only when `ChannelGateway::is_allowed(Telegram)` or the Telegram Service needs the bot); Ordin Desktop program (`mode: Desktop`, `gateway: None`) |
| `pause`, `resume` | Ordin Desktop program, on sleep and wake |
| `set_bot_token` | the setup command `ordin-server setup secret telegram-bot-token` (rows 491, 492): it validates and stores the token; a running server reads it at its next start, so the setup command prints the instruction to restart the server |
| `begin_pairing` | the setup command `ordin-server setup pair-chat --channel telegram`; ending a pairing is `ChannelGateway::end_pairing(Telegram)` (this adapter defines none, row 490) |
| `ChannelAdapter` methods | Channel Gateway |
| `TelegramBot` methods | Telegram Service |

### Example

Pairing (setup command, then the person's phone):

```text
$ ordin-server setup pair-chat telegram
Send this code to your bot in a private chat within 10 minutes: K7QM-29XD

getUpdates → {"update_id": 901244, "message": {"message_id": 1, "from": {"id": 48213377, "first_name": "Asha"},
              "chat": {"id": 48213377, "type": "private"}, "text": "/start K7QM29XD"}}
→ code matches → gateway.complete_pairing(<pairing id>, "48213377", "48213377") → the gateway sends the welcome
```

A card sent by the gateway, then edited:

```json
POST /bot<token>/sendMessage
{"chat_id": "48213377", "text": "Send the supplier summary to Priya Shah?\nWhere: Email, from asha.patel@acme.co.uk\nAffects: Priya Shah\nChanges: One email with the summary attached",
 "parse_mode": "HTML", "link_preview_options": {"is_disabled": true},
 "reply_markup": {"inline_keyboard": [[{"text": "Send", "callback_data": "c:q8Zt3VxN0pLk2mRb7sYw1A"}],
                                      [{"text": "Not now", "callback_data": "c:Lm4Qp0Rt7Vx2Zb5Nc8Df3g"}],
                                      [{"text": "Show details", "callback_data": "c:Hh2Kd9Ws1Qe6Zr3Tb8Nm4v"}]]}}
→ {"ok": true, "result": {"message_id": 1042, …}}  → SentMessage { chat_id: "48213377", message_id: "1042" }

POST /bot<token>/editMessageText
{"chat_id": "48213377", "message_id": 1042, "text": "…\nDecided in Slack, 10:44", "parse_mode": "HTML", "reply_markup": {"inline_keyboard": []}}
```

A group message handed to the Telegram Service:

```text
{"update_id": 901300, "message": {"message_id": 5812, "from": {"id": 7001, "first_name": "Ahmed", "last_name": "Khan"},
 "chat": {"id": -1001234, "type": "supergroup", "title": "Suppliers — Northwind"}, "date": 1729412040, "text": "Revised quote attached"}}
→ sink.on_group_update(Message(TelegramIncomingMessage { chat: { chat_id: -1001234, title: "Suppliers — Northwind",
     chat_type: Supergroup, is_forum: false }, topic_id: None, message_id: 5812, sender_name: "Ahmed Khan",
     sent_at: 2026-01-10T08:14:00.000Z, text: "Revised quote attached", reply_to_message_id: None, files: [] }))
```

## Internal Logic

**A. Start**

1. `new`: build `reqwest::Client` with rustls and `rustls-platform-verifier` (SA-HTTP-02), the system proxy, `connect_timeout(10 s)`, no redirects. No network call.
2. `start`: read `telegram/bot-token`. `None` → status `NotStarted`, log `telegram.token.missing`, return `Ok`. A `CredError` → `TGA_SECRET_STORE_FAILED` (retryable): retry every 30 s until shutdown.
3. `getMe` → `BotInfo` (`username`, `first_name` + `last_name` as `display_name`, `can_read_all_group_messages`), and the bot's numeric id. 401 → status `CredentialsRefused`, report to the gateway, retry step A.2–A.3 every 5 minutes.
4. Server mode with a gateway: `gateway.register_adapter(self)`.
5. Start the poll loop (step B).

**B. Poll loop**

1. Read `next_offset` from `telegram_poll_state` for this bot id (0 when absent; a different bot id starts at 0).
2. `getUpdates { offset, timeout: 50, allowed_updates: ["message", "channel_post", "callback_query", "my_chat_member"] }` with a 65-second request timeout. While `pause()` is in force (desktop), the loop waits on resume and makes no request.
3. On success: report `Connected` once (after any other status); reset the backoff; for each update in `update_id` order run step C, then write `next_offset = update_id + 1` (one statement per update).
4. On failure, by kind:
   - transport error, timeout, HTTP 5xx, `ok: false` with `error_code` ≥ 500 → `Reconnecting { since: <first failure> }` reported once; wait `delay(n) = clamp(min(300, 2^(n−1)) × U(0.8, 1.2), 1, 300)` seconds (2D-4: 1 s to 5 min with jitter) and retry, never giving up.
   - 401 → `CredentialsRefused`; re-read the token (A.2) every 5 minutes; a changed token restarts from A.3.
   - 409 (a webhook is set, or another receiver polls the same bot) → log `telegram.poll.conflict` (`TGA_POLL_CONFLICT`), report `Reconnecting`, back off as above. The adapter never calls `setWebhook` or `deleteWebhook` (ADR-073).
   - 429 → wait `parameters.retry_after` seconds.
5. `shutdown` → finish the update in hand, write its offset, return.

**C. Dispatch of one update**

1. `callback_query` (button press): sender `from.id`, chat `message.chat.id`. Private chat → C.3 with body `Press { data: callback_query.data, ack: callback_query.id }`. Any other chat → ignored (no answer; counted as `NotPrivate` in server mode).
2. `message` / `channel_post` / `my_chat_member` in a group, supergroup or channel → step D.
3. `message` in a private chat:
   - **Desktop mode**: dropped; nothing is sent (ADR-077 "the desktop's bot answers no one in a private chat").
   - **Server mode, no gateway** (Telegram not an allowed channel): dropped.
   - **Server mode with a gateway**: `p = gateway.active_pairing(Telegram)` (cached for 30 s; the cache is cleared after `complete_pairing`).
     - `p` paired and `from.id == p.chat_user_id` and `chat.id == p.private_chat_id` → C.4.
     - otherwise, when a pairing code row exists → step E (pairing attempt).
     - otherwise → `gateway.count_ignored(Telegram, if p is None { NotPaired } else { OtherUser })`; no reply.
4. **Building an `InboundItem`** (`channel: Telegram`, `sender_id`/`chat_id` as decimal strings, `is_private: true`, `message_id`, `received_at` = `date`):
   - `text` beginning with `/`: the first word without `@<bot username>` → `/stop` `Stop`, `/status` `Status`, `/run` `Run`, `/triggers` `Triggers` → `Command { command, rest: <the remaining text, trimmed> }`; `/start` with no other text → dropped (the paired person re-opened the chat); any other `/word` → `Text`.
   - other `text` → `Text { text }`.
   - `voice` or `video_note` → `Voice`.
   - `document` → `File { file: document.file_id, file_name: document.file_name or "file", media_type: document.mime_type, size_bytes: document.file_size, caption }`.
   - `photo` → the largest `PhotoSize` → `File { file_name: "photo-<message_id>.jpg", media_type: "image/jpeg", … }`.
   - `video`, `audio`, `animation` → `File` with their `file_name` (or `"<kind>-<message_id>"`) and `mime_type`.
   - anything else (sticker, location, contact, poll, dice, venue) → `Unsupported`.
   - `gateway.receive(item)`; an error is logged (`telegram.receive.failed {code}`); the offset still advances.

**D. Groups and channels (both modes)**

1. With no sink set (`set_group_sink` not called), the update is dropped.
2. `message` or `channel_post` with `new_chat_title` → `ChatTitle { chat_id, title }`.
3. Other `message` / `channel_post` → `Message(TelegramIncomingMessage)`: `chat` from `chat` (`type` → `TelegramChatType`; `is_forum`), `topic_id` = `message_thread_id` when `is_topic_message`, `sender_name` = `from.first_name + " " + from.last_name` trimmed, or `author_signature`, or the chat title for a channel post, `sent_at` = `date`, `text` = `text` or `caption` or `""`, `reply_to_message_id`, `files` = the document, the largest photo, video, audio or voice as `TelegramFile { file_id, file_name, size_bytes }` (unknown size → 0).
4. `my_chat_member` → `BotMembership { chat, bot_state }` with `new_chat_member.status`: `member` → `Member`, `administrator` → `Administrator`, `left`/`kicked` → `Removed`; `restricted` → `Member`.
5. Nothing from a group or channel ever reaches the gateway, and no group text is treated as a command (ADR-075, 07:67).

**E. Pairing**

1. `begin_pairing(paired_by)` (setup command; server mode only, else `TGA_NOT_SERVER_MODE`): `gateway.is_allowed(Telegram)` false → `TGA_NOT_ALLOWED`; `gateway.begin_pairing(Telegram, paired_by)` → `ChannelPairing`; draw 8 characters uniformly from the alphabet with `rand::rngs::OsRng`; store `(pairing_id, sha256(code), expires_at = pairing.code_expires_at, window_start = now, wrong = 0)` in `telegram_pairing_codes`, replacing any earlier row; return `PairingCode` with the code shown as `XXXX-XXXX`.
2. A private message from an unpaired account while a code row exists: take `text`; strip a leading `/start`; remove spaces and `-`; upper-case.
   - `now > expires_at` → delete the row, `gateway.expire_pairing(id)`, reply "That code has expired. Ask whoever runs your ordin server for a new code." (*chosen here*; 03:960 "told to ask for a new code").
   - `sha256(candidate)` equals the stored hash (constant-time compare) → delete the row; `gateway.complete_pairing(id, from.id, chat.id)`; the gateway sends the welcome. A gateway error (`CHAN_CODE_EXPIRED`, `CHAN_PAIRING_NOT_PENDING`) → reply as the expired case.
   - otherwise: when `now − window_start > 10 min` reset `window_start = now`, `wrong = 0`; `wrong += 1`; reply "That code isn't right. Check it, or ask whoever runs your ordin server for a new code." (*chosen here*); when `wrong` reaches 5: delete the row, `gateway.expire_pairing(id)`, and reply instead "Pairing has stopped after too many wrong codes. Ask whoever runs your ordin server to start it again." (*chosen here*; ADR-073).
3. Replies in this step are sent with `sendMessage` to the sender's private chat directly (not through the gateway's outbox, which serves only paired chats).

**F. `ChannelAdapter` (server mode)**

1. `kind()` → `Telegram`. `limits()` → `{ max_text_chars: 4096, max_caption_chars: 1024, max_buttons: 8, max_button_label_chars: 64, max_send_file_bytes: 52_428_800, max_receive_file_bytes: 20_971_520, min_send_interval_ms: 1000 }`.
2. **Text conversion** (both `send` and `edit`): escape `&`, `<`, `>` as HTML entities; then `**x**` → `<b>x</b>`, `*x*` → `<i>x</i>` (non-greedy, within one line); `parse_mode: "HTML"`; `link_preview_options: { is_disabled: true }` so Telegram fetches no link the text names.
3. `send(to, msg)`:
   - with `file`: `is_image` and ≤ 10 MB → `sendPhoto`; otherwise `sendDocument` (≤ 50 MB, else `Permanent TGA_FILE_TOO_LARGE`); multipart with `chat_id`, `caption` (= text), `parse_mode`, `reply_markup`.
   - without: `sendMessage { chat_id, text, parse_mode, link_preview_options, reply_markup }`.
   - `reply_markup = { inline_keyboard: [[{ text: label, callback_data: data }], …] }`, one button per row; `data` longer than 64 bytes → `Permanent TGA_BUTTON_DATA_TOO_LONG` (cannot happen with the gateway's 24-byte data).
   - result `message_id` → `SentMessage { chat_id, message_id }`.
4. `edit(sent, text, buttons)`: `editMessageText { chat_id, message_id, text, parse_mode, link_preview_options, reply_markup: { inline_keyboard: <buttons or []> } }`. An answer "message is not modified" → `Ok`; "message to edit not found" → `Permanent TGA_MESSAGE_GONE`.
5. `fetch_file(file, max_bytes)`: `getFile { file_id }` → `file_path`, `file_size`; `file_size > max_bytes` → `Permanent TGA_FILE_TOO_LARGE`; download from the file address with a 120 s timeout, streaming, stopping with `TGA_FILE_TOO_LARGE` beyond `max_bytes`.
6. `ack_press(ack)`: `answerCallbackQuery { callback_query_id }`, no text; errors are logged and ignored by the gateway.
7. Error mapping for every call: 429 → `Retryable { TGA_RATE_LIMITED, retry_after_ms: retry_after × 1000 }`; transport, timeout, 5xx → `Retryable { TGA_UNREACHABLE }`; 401 → `Retryable { TGA_TOKEN_REFUSED }` and status `CredentialsRefused`; 403 (bot blocked by the person, or removed from a chat) → `Permanent TGA_BLOCKED`; 400 with "message is too long" → `Permanent TGA_TEXT_TOO_LONG`; other 4xx → `Permanent TGA_REFUSED`; an answer without the fields read → `Permanent TGA_BAD_RESPONSE`.
8. **Rate limiter** (shared by `send`, `edit` and `post`): per chat, at least 1,000 ms between two calls to a private chat and 3,000 ms to a group or channel (20 a minute, C-52); calls wait for their turn in arrival order.
9. `status()` → the last status of steps A–B.

**G. `TelegramBot` (Telegram Service; both modes)**

1. `set_group_sink(sink)`: stores it; later calls replace it.
2. `bot_info()`: the `getMe` result of step A.3, refreshed when older than 1 hour; no token → `NotConfigured`; 401 → `KeyInvalid`.
3. `member_count(chat_id)`: `getChatMemberCount`; transport → `Unreachable`; 4xx → `Refused { description }` (Telegram's short description; no content of a message).
4. `download_file(file_id, max_bytes)`: as F.5 with `min(max_bytes, 20 MB)`; too large → `FileTooLarge`.
5. `post(post)`: validate text 1–4,096 chars and ≤ 10 files of ≤ 50 MB each (else `Refused { description: "post exceeds Telegram limits" }`); `sendMessage { chat_id, message_thread_id: topic_id, reply_parameters: { message_id: reply_to_message_id, allow_sending_without_reply: true }, text }` without `parse_mode` (agent-drafted text is sent as plain text, never interpreted as markup); then one `sendDocument` per file replying to the same message; through the rate limiter. Returns the identifiers. A failure after the text was sent returns `Refused` with the identifiers sent so far logged, so the Telegram Service records an unknown outcome rather than posting twice.
6. `connect_desktop_bot(key)` (desktop mode only, else `Refused { description: "desktop only" }`): `key` must match `^[0-9]{5,16}:[A-Za-z0-9_-]{30,64}$` (*assumed here*: Telegram's bot token form) else `KeyInvalid` with no network call; `getMe` with the key (401 → `KeyInvalid`; transport → `Unreachable`); `SecretStore::put(telegram/bot-token, key)`; reset `telegram_poll_state`; start the poll loop; return `BotInfo`.
7. `remove_desktop_bot()`: stop the poll loop, `SecretStore::delete(telegram/bot-token)`, delete `telegram_poll_state` rows.

**H. Never logged:** the token (requests are built with it in the path, so `reqwest` errors are logged only after `without_url()`), message text, captions, file names, sender names, chat titles, callback data. Identifiers of the person are logged as the first 12 hex of their SHA-256.

## Data Model

**SA-CLOSE-01** (*chosen here*; closing decision K7): this crate depends on the Journal (phase 1, allowed for a phase-10 crate under D4; 2B-3 adds the edge) and keeps its tables in the Journal's `Datastore`, reached as `journal.datastore().sqlite()`. It exports `pub fn migrations() -> ModuleMigrations` built from `MigrationStep` tuples (module `telegram_adapter`, version 1 = the SQL below, empty `postgres`), which the program registers; the programs no longer hand it a pool. It writes no journal entry or event. A `None` from `sqlite()` (a PostgreSQL datastore, which no program that builds this crate opens) is refused at `new` with `TGA_DATASTORE_FAILED`. SQLite only (desktop and personal server).

```sql
-- telegram_adapter 1
CREATE TABLE telegram_poll_state (
    bot_id       INTEGER PRIMARY KEY,        -- Telegram's numeric bot id from getMe
    next_offset  INTEGER NOT NULL CHECK (next_offset >= 0),
    updated_at   TEXT NOT NULL
);

-- Server mode only: the code's hash while it is valid (02:610 "Owns ... the pairing code while it is valid").
CREATE TABLE telegram_pairing_codes (
    pairing_id    TEXT PRIMARY KEY CHECK (length(pairing_id) = 36),
    code_sha256   TEXT NOT NULL CHECK (length(code_sha256) = 64),
    expires_at    TEXT NOT NULL,
    window_start  TEXT NOT NULL,
    wrong         INTEGER NOT NULL DEFAULT 0 CHECK (wrong BETWEEN 0 AND 5)
);
```

The code itself is never stored; a row is deleted on success, expiry or the fifth wrong code. Rollback: the migration adds tables only; restoring the pre-migration backup removes them.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No token in the secret store | status `NotStarted`; `NotConfigured` for `TelegramBot` | `TGA_TOKEN_MISSING` | `TelegramAdapterError { code, retryable: false }` |
| Secret store unreadable | `Err(retryable)` | `TGA_SECRET_STORE_FAILED` | as above |
| Token refused by Telegram (401) | `Err(retryable)`; status `CredentialsRefused` | `TGA_TOKEN_REFUSED` | `ChannelSendError::Retryable` / `TelegramBotError::KeyInvalid` |
| Token not in Telegram's form (desktop key, setup command) | `Err(permanent)` | `TGA_TOKEN_INVALID` | `TelegramBotError::KeyInvalid` |
| `getUpdates` conflict (409) | retried with backoff | `TGA_POLL_CONFLICT` (log) | — |
| Telegram says too many requests (429) | `Err(retryable)` with `retry_after_ms` | `TGA_RATE_LIMITED` | `ChannelSendError::Retryable` |
| Transport error, timeout or 5xx | `Err(retryable)` | `TGA_UNREACHABLE` | `ChannelSendError::Retryable` / `TelegramBotError::Unreachable` |
| Bot blocked by the person, or not in the chat (403) | `Err(permanent)` | `TGA_BLOCKED` | `ChannelSendError::Permanent` / `TelegramBotError::Refused` |
| Text over 4,096 chars reaches Telegram | `Err(permanent)` | `TGA_TEXT_TOO_LONG` | as above |
| Message to edit no longer exists | `Err(permanent)` | `TGA_MESSAGE_GONE` | as above |
| File over the send or receive limit | `Err(permanent)` | `TGA_FILE_TOO_LARGE` | `ChannelSendError::Permanent` / `TelegramBotError::FileTooLarge` |
| Button data over 64 bytes | `Err(permanent)` | `TGA_BUTTON_DATA_TOO_LONG` | as above |
| Other 4xx | `Err(permanent)` | `TGA_REFUSED` | as above |
| Answer missing a field this crate reads | `Err(permanent)` | `TGA_BAD_RESPONSE` | as above |
| `begin_pairing` outside server mode | `Err(permanent)` | `TGA_NOT_SERVER_MODE` | as above |
| `begin_pairing` while Telegram is not an allowed channel | `Err(permanent)` | `TGA_NOT_ALLOWED` | as above |
| Pairing code wrong | reply to the sender | `TGA_CODE_WRONG` (log) | — |
| Pairing code expired | reply to the sender; pairing ended | `TGA_CODE_EXPIRED` (log) | — |
| Fifth wrong code in 10 minutes | reply; pairing ended | `TGA_PAIRING_STOPPED` (log) | — |
| Offset or code table read/write fails | `Err(retryable)`; the loop retries the update | `TGA_DATASTORE_FAILED` | as above |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Telegram Adapter

  Background:
    Given server mode with a gateway that allows Telegram
    And the secret "telegram/bot-token" holds a valid token

  Scenario: Happy path — pairing with the code
    Given the setup command called begin_pairing and showed "K7QM-29XD"
    When user 48213377 sends "/start K7QM29XD" in a private chat within 10 minutes
    Then the gateway completes the pairing with chat user "48213377" and private chat "48213377"
    And the code row is deleted

  Scenario: Error — fifth wrong code stops pairing
    Given a pending code
    When an unpaired user sends 5 wrong codes within 10 minutes
    Then the fifth reply reads "Pairing has stopped after too many wrong codes. Ask whoever runs your ordin server to start it again."
    And the gateway expires the pairing and a correct code sent afterwards pairs nothing

  Scenario: Error — expired code
    Given a code shown 11 minutes ago
    When the user sends it
    Then the reply reads "That code has expired. Ask whoever runs your ordin server for a new code."

  Scenario: Messages from strangers after pairing get no reply
    Given Telegram is paired with user 48213377
    When user 7001 sends "hi" in a private chat
    Then nothing is sent and the gateway counts it as other_user

  Scenario: The paired person's message and commands reach the gateway
    Given Telegram is paired with user 48213377
    When user 48213377 sends "/status@PriyaOrdinBot"
    Then the gateway receives a Command Status with rest ""
    When user 48213377 sends a voice note
    Then the gateway receives Voice

  Scenario: Group messages go only to the Telegram Service
    Given the Telegram Service set its group sink
    When a message "/stop" arrives in supergroup -1001234
    Then the sink receives it as a Message with text "/stop"
    And the gateway receives nothing

  Scenario: Long polling survives an outage
    Given getUpdates fails with a transport error 4 times
    Then the waits are within [0.8, 1.2] × 1, 2, 4 and 8 seconds
    And the gateway is told Reconnecting once and Connected after the next success

  Scenario: Offset is written after each update
    Given updates 10, 11 and 12 arrive in one answer
    When the program stops after update 11 is dispatched
    Then the next start asks getUpdates with offset 12

  Scenario: Rate limit answer
    When sendMessage answers 429 with retry_after 7
    Then send returns Retryable TGA_RATE_LIMITED with retry_after_ms 7000

  Scenario: Card edit removes the buttons
    When the gateway edits message 1042 with no buttons
    Then editMessageText is called with an empty inline keyboard

  Scenario: Desktop bot answers no private chat
    Given desktop mode and a connected desktop bot
    When a user sends "hello" in a private chat with the bot
    Then nothing is sent and nothing is passed on

  Scenario: Desktop key in the wrong form
    When connect_desktop_bot is called with "not-a-token"
    Then KeyInvalid is returned without any request to Telegram

  Scenario: A webhook set elsewhere
    When getUpdates answers 409
    Then TGA_POLL_CONFLICT is logged, the adapter backs off, and it never calls deleteWebhook

  Scenario: Tables live in the Journal's datastore
    Given the program registered TelegramAdapter::migrations() with the Journal
    When the adapter is built with the Journal and pairing starts
    Then the pairing code row is written through journal.datastore().sqlite()
    And no separate pool is handed to the adapter
```

## Performance, Security, Observability

- **Performance targets:** an update dispatched within 50 ms of its arrival in the `getUpdates` answer (excluding the gateway and the sink); one offset write per update; memory < 32 MiB plus files in transit (≤ 50 MB out, ≤ 20 MB in); message pace never above 1 a second to a private chat or 20 a minute to a group.
- **Security:** outbound HTTPS only, the computer's trust store and proxy (A-39); no webhook, no port; the token only from the secret store or the keychain (ADR-073, ADR-077), never from the chat; pairing codes are random (OS RNG), stored only as SHA-256 while valid, compared in constant time, limited to 5 wrong tries in 10 minutes; only the paired account in its private chat reaches the gateway; group and channel content goes only to the Telegram Service's sink and is never a command; HTML mode escapes all text before adding the two allowed marks; link previews are off; agent-drafted posts are sent without any markup mode. **Never logged:** the token, message text, captions, file names, sender names, chat titles, callback data.
- **Observability (logs, `tracing`, target `ordin_telegram_adapter`):** `telegram.started {mode, bot_username}`, `telegram.token.missing`, `telegram.token.refused`, `telegram.poll.ok {updates}` (debug), `telegram.poll.failed {kind, attempt, delay_ms}`, `telegram.poll.conflict`, `telegram.update {kind, chat_type}`, `telegram.pairing.code_shown {pairing_id}`, `telegram.pairing.wrong {pairing_id, wrong}`, `telegram.pairing.stopped {pairing_id}`, `telegram.pairing.done {pairing_id}`, `telegram.send {method, outcome, code?}`, `telegram.receive.failed {code}`. No metrics or spans (desktop and personal server, SA-XC-04/05).

## Gaps

None.
