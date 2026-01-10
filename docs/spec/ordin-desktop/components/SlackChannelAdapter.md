# SPEC: Slack Channel Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-slack-channel-adapter` · **Build phase:** 10 · **Programs:** P A
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

The Slack Channel Adapter is the ordin server's connection to a Slack app created for it (02:613–619, ADR-076, ADR-097, ADR-098, 08:83). It connects by **Socket Mode** — an outbound WebSocket opened with the app-level token (`xapp-`) — acknowledges every envelope on receipt, reconnects with backoff from 1 second to 5 minutes and never gives up, and calls the Slack Web API with the bot token (`xoxb-`).

- **Personal profile:** the Channel Gateway's second adapter. It pairs the person's Slack account with a one-time code sent in the direct message with the app, passes only that user's direct-message text, files and button presses to the gateway, reads the words **stop**, **status**, **run** and **triggers** as commands, and sends cards as Block Kit messages whose button values are single-use references, updating them once decided.
- **Agent profile:** the hosted agent's chat link, one per `chat[]` connection of the agent configuration, run in instances started in the chat role. It turns mentions, thread mentions, direct messages, slash commands, button presses, form submissions and the agent account's channel joins and leaves into the Channel Gateway's `HostedEvent`s, and implements the gateway's `HostedChatLink` (post, post to one person only, update, reactions, thread and file reads, channel documents, the thread-member check, the hourly list of the account's channels, forms).

It is separate from the Slack Thread Adapter, which uses the person's own Slack user token to reach agents (ADR-008, 07:68).

*Amended January 2026 (ADR-107; HLD 0.55.0):* for a hosted agent answering in team channels the link also reads thread replies for the chat-history ingest (`replies`, `HistoryMessage.reply_count`), keeps the content of messages whose substance is in Block Kit blocks or attachments (step F.11), reads every canvas of a channel including one shown only as a tab and turns canvas HTML into text (F.5, F.6), renders workflow buttons (`HostedButtonKind::WorkflowLink`), and passes a slash command's or button press's `response_url` so the gateway can replace an ephemeral message (`replace_ephemeral`, F.12).

## Approach

Two layers in one crate: a **Socket Mode link** (WebSocket loop, envelope parsing, acknowledgement, reconnection) and a **Web API client** (one `reqwest` client, one method per Slack call, one error mapping). Each mode is a thin front over them: `PersonalSlack` implements the gateway's `ChannelAdapter`, `HostedSlackLink` implements `HostedChatLink`. The WebSocket uses `tokio-tungstenite` 0.28.0 with rustls, added to 05 under decision D2 (licence to be confirmed by `cargo-deny`). Rejected: Slack's Events API over HTTPS (needs an inbound port — ADR-076 *Alternatives*); Slack's own slash commands in the personal profile (each workspace would need one registered — ADR-076).

Error prefix: **`SCA_`** (*chosen here*; `STA_` is the Slack Thread Adapter's).

## Shared Context

### Shared types (Phase 2C and the Channel Gateway spec, copied)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
uuid_id!(ChannelPairingId);
pub enum ChannelKind { Telegram, Slack }
// ChannelPairing { id, channel_kind, chat_user_id, private_chat_id, paired_by, code_expires_at, paired_at, ended_at, state }
// — as in the Channel Gateway spec (2C domain).

// ---- ordin-channel-gateway (personal profile) ----
pub struct ChannelLimits { pub max_text_chars: u32, pub max_caption_chars: u32, pub max_buttons: u8,
    pub max_button_label_chars: u8, pub max_send_file_bytes: u64, pub max_receive_file_bytes: u64, pub min_send_interval_ms: u32 }
pub struct ChannelAddress { pub chat_id: String }
pub struct OutboundMessage { pub text: String, pub buttons: Vec<OutboundButton>, pub file: Option<OutboundFile> }
pub struct OutboundButton { pub label: String, pub data: String }
pub struct OutboundFile { pub file_name: String, pub media_type: String, pub bytes: bytes::Bytes, pub is_image: bool }
pub struct SentMessage { pub chat_id: String, pub message_id: String }
pub enum ChannelSendError { Retryable { code: &'static str, retry_after_ms: Option<u64> }, Permanent { code: &'static str } }
pub enum ChannelStatus { NotStarted, Connected, Reconnecting { since: Timestamp }, CredentialsRefused { since: Timestamp } }
#[async_trait] pub trait ChannelAdapter { /* kind, limits, send, edit, fetch_file, ack_press, status — Channel Gateway spec */ }
pub struct InboundItem { pub channel: ChannelKind, pub sender_id: String, pub chat_id: String, pub is_private: bool,
    pub message_id: String, pub received_at: Timestamp, pub body: InboundBody }
pub enum InboundBody { Text { text: String }, Command { command: ChatCommand, rest: String }, Voice, File(InboundFile), Unsupported, Press { data: String, ack: PressAck } }
pub enum ChatCommand { Stop, Status, Run, Triggers }
pub struct InboundFile { pub file: InboundFileRef, pub file_name: String, pub media_type: Option<String>, pub size_bytes: Option<u64>, pub caption: Option<String> }
pub struct InboundFileRef(pub String);
pub struct PressAck(pub String);
pub enum IgnoredReason { NotPaired, OtherUser, NotPrivate, ChannelNotAllowed }

// ---- ordin-channel-gateway (agent profile) ----
#[async_trait] pub trait HostedChatLink { /* connection, agent_user_id, post, post_direct, update, react, unreact,
    thread, file, channel_documents, may_read_thread, member_channels, open_form, replace_ephemeral — Channel Gateway spec */ }
pub struct HostedPost { pub channel: String, pub thread: Option<String>, pub only_to: Option<String>, pub text: String, pub buttons: Vec<HostedButton> }
pub struct HostedButton { pub action: String, pub label: String, pub value: String, pub url: Option<String>,
    pub kind: HostedButtonKind /* read only when `url` is set; default Url */ }
pub enum HostedButtonKind { Url, WorkflowLink }
pub struct HostedMessage { pub id: String, pub sender: String, pub from_program: bool, pub text: String, pub files: Vec<HostedFile> }
pub struct HostedFile { pub id: String, pub name: String, pub media_type: String, pub size_bytes: Option<u64> }
pub enum ContextItem { Piece { piece: String, label: String, media_type: String, content: String, size_bytes: u64, cut: bool,
                               step: Option<u16>, items: u32, observed_bytes: Option<u64> },      // the last three: ADR-107
                       LeftOut { piece: String, label: String, reason: String,
                                 step: Option<u16>, observed_bytes: Option<u64> } }              // the last two: ADR-107
pub struct MemberChannels { pub channels: Vec<String>, pub complete: bool }
pub struct HostedForm { pub form: String, pub title: String, pub fields: Vec<HostedFormField>, pub metadata: String }
pub struct HostedFormField { pub name: String, pub label: String, pub kind: HostedFieldKind, pub initial: Option<String>, pub optional: bool }
pub enum HostedFieldKind { Flag, Number { min: Option<f64>, max: Option<f64>, decimal: bool }, Text { max_chars: u32, multiline: bool },
                           Selection { options: Vec<String>, selected: Vec<String> }, Lines { max_chars: u32 } }
pub enum HostedEvent { Message { .. }, Action { .. }, FormSubmitted { .. }, Joined { .. }, Left { .. } } // fields: Channel Gateway spec;
// `Message` and `Action` carry `response_url: Option<String>` (ADR-107)
pub enum HostedMessageKind { Mention, ThreadMention, DirectMessage, Command }
pub enum HostedAck { Plain, FormErrors(BTreeMap<String, String>) }
pub struct LinkError { pub code: &'static str }
impl ChannelGateway {
    pub fn register_adapter(&self, a: Arc<dyn ChannelAdapter>) -> Result<(), GatewayError>;
    pub fn register_link(&self, l: Arc<dyn HostedChatLink>) -> Result<(), GatewayError>;
    pub async fn receive(&self, item: InboundItem) -> Result<(), GatewayError>;
    pub async fn receive_hosted(&self, event: HostedEvent) -> Result<HostedAck, GatewayError>;
    pub fn count_ignored(&self, channel: ChannelKind, reason: IgnoredReason);
    pub async fn report_status(&self, channel: ChannelKind, status: ChannelStatus);
    pub fn is_allowed(&self, channel: ChannelKind) -> bool;
    pub async fn begin_pairing(&self, channel: ChannelKind, paired_by: &str) -> Result<ChannelPairing, GatewayError>;
    pub async fn complete_pairing(&self, id: ChannelPairingId, chat_user_id: &str, private_chat_id: &str) -> Result<ChannelPairing, GatewayError>;
    pub async fn expire_pairing(&self, id: ChannelPairingId) -> Result<(), GatewayError>;
    pub async fn active_pairing(&self, channel: ChannelKind) -> Result<Option<ChannelPairing>, GatewayError>;
}
```

`ChatConnection` (agent configuration, `ordin_types::agent_profile`) is read for `name`, `bot_token_env`, `app_token_env`; its other keys are the gateway's.

### Secrets

| Profile | Where | Name |
|---|---|---|
| Personal | Identity's `SecretStore` (`put`, `get`, `delete`) | `slack/bot-token`, `slack/app-token` (*chosen here*, Identity spec convention; set by `ordin-server setup secret slack-bot-token` / `slack-app-token`, OrdinServer spec) |
| Agent | resolved by the server program from the variable names `chat[].bot-token-env` and `chat[].app-token-env` — the server's secret store in the single shape, `ORDIN_SECRETS_DIR/<NAME>` in the scaled shape (2D-1d) — and passed in as `SlackTokens` | — |

`ORDIN_IMAGE` (2D-1a) is read by the program for the version command, not by this crate.

### Constants

| Constant | Value | Source |
|---|---|---|
| Reconnect backoff | 1 s to 5 min, jitter ×U(0.8, 1.2), never giving up | 2D-4, 02:615 |
| Pairing code | 8 characters from `23456789ABCDEFGHJKMNPQRSTUVWXYZ`, valid 10 minutes, 5 wrong codes in 10 minutes stop pairing | 2D-4 (10 minutes); the 5-code rule *assumed here*: ADR-076 "As for Telegram" |
| Message text | ≤ 4,000 characters; ≤ 50 blocks; section text ≤ 3,000; button value ≤ 2,000; label ≤ 75 | 2D-4, ADR-076 *Status*, C-56 |
| Buttons per message | 10, in one actions block per 5 | *chosen here* |
| Files sent and received | ≤ 100 MB | *assumed here*: Slack allows 1 GB (C-56); 100 MB bounds memory on a small server |
| Channel list check | once an hour; at most 50 pages of 200 | 2D-4; wrightrun `ChatClient.md` |
| Web API timeout | 15 s per call, 120 s for file transfer; at most 2 waits on a 429 | wrightrun `ChatClient.md` (`CALL_TIMEOUT_SECONDS`, `RATE_LIMIT_RETRIES`) |
| Silence before reconnect | no frame (including Slack's pings) for 60 s | *chosen here* |
| Action identifier prefix | `ordin:` | *chosen here* (wrightrun used `wrightrun:`) |

### Bot-token scopes the Slack app needs (*chosen here*; listed in the setup instructions)

Personal: `chat:write`, `im:history`, `im:read`, `im:write`, `files:read`, `files:write`. Agent profile adds `app_mentions:read`, `channels:history`, `groups:history`, `channels:read`, `groups:read`, `reactions:write`, `commands`, `pins:read`. App-level token: `connections:write` (C-56). Events subscribed: `message.im`, `app_mention`, `member_joined_channel`, `channel_left`, `group_left`; interactivity on. *Amended January 2026 (ADR-107):* no scope is added — `conversations.replies` is covered by `channels:history` and `groups:history`, a channel's tabs by `channels:read` and `groups:read`, canvas downloads by `files:read`, and a `response_url` needs no token.

### Libraries

`reqwest` 0.13.5 + `rustls-platform-verifier` 0.7.1 (SA-HTTP-02); `tokio-tungstenite` 0.28.0 with `rustls-tls-native-roots` off and a caller-supplied `rustls::ClientConfig` (platform verifier) — B9-F5; `serde_json`; `rand` 0.9; `sha2`; `tokio-util`; `async-trait` (B9-F7).

## Public Interface

```rust
// crate ordin-slack-channel-adapter

pub struct SlackTokens { pub bot: zeroize::Zeroizing<String>, pub app: zeroize::Zeroizing<String> }

pub struct PersonalSlackDeps {
    pub secrets: Arc<ordin_identity::SecretStore>,
    /// Tables live in the Journal's `Datastore` (`journal.datastore().sqlite()`; D4, SA-CLOSE-01).
    pub journal: Arc<ordin_journal::Journal>,
    pub gateway: Arc<ordin_channel_gateway::ChannelGateway>,
}

pub struct PersonalSlack { /* private */ }
impl PersonalSlack {
    /// `ModuleMigrations { module: "slack_channel_adapter", sqlite: &[(1, "slack channel adapter tables", <the Data Model SQL>)],
    /// postgres: &[] }`; the program registers it with the Journal (R3).
    pub fn migrations() -> ordin_journal::ModuleMigrations;
    pub fn new(deps: PersonalSlackDeps) -> Result<Arc<Self>, SlackAdapterError>;
    /// Reads both tokens, `auth.test`, registers with the gateway, opens Socket Mode. With a token
    /// missing it stays `NotStarted` and returns Ok.
    pub async fn start(self: &Arc<Self>, shutdown: CancellationToken) -> Result<(), SlackAdapterError>;
    /// Setup command: checks each token (`auth.test`; `apps.connections.open`) and stores it.
    pub async fn set_tokens(&self, bot: &str, app: &str) -> Result<SlackIdentity, SlackAdapterError>;
    /// Setup command: pending pairing through the gateway, code hash stored; returns the code.
    pub async fn begin_pairing(&self, paired_by: &str) -> Result<SlackPairingCode, SlackAdapterError>;
}
#[async_trait::async_trait]
impl ordin_channel_gateway::ChannelAdapter for PersonalSlack { /* step D */ }

pub struct HostedSlackLink { /* private */ }
impl HostedSlackLink {
    /// One link per chat connection; makes no request.
    pub fn new(connection: &ordin_types::agent_profile::ChatConnection, tokens: SlackTokens,
               gateway: Arc<ordin_channel_gateway::ChannelGateway>,
               metrics: Arc<dyn ordin_channel_gateway::MetricsSink>) -> Result<Arc<Self>, SlackAdapterError>;
    /// `auth.test`, registers with the gateway, opens Socket Mode; readiness is `is_open()`.
    pub async fn start(self: &Arc<Self>, shutdown: CancellationToken) -> Result<(), SlackAdapterError>;
    /// For the instance's readiness route (chat role).
    pub fn is_open(&self) -> bool;
    /// Channel history, for the Knowledge Service's chat-history ingest through the program.
    pub async fn history(&self, channel: &str, oldest: Option<&str>, cursor: Option<&str>) -> Result<HistoryPage, LinkError>;
    /// The replies of one thread, without its parent, for the same ingest (ADR-107; step F.10a).
    pub async fn replies(&self, channel: &str, thread_ts: &str, cursor: Option<&str>) -> Result<HistoryPage, LinkError>;
}
#[async_trait::async_trait]
impl ordin_channel_gateway::HostedChatLink for HostedSlackLink { /* step F */ }

pub struct SlackIdentity { pub bot_user_id: String, pub team_id: String, pub bot_name: String }
pub struct SlackPairingCode { pub pairing_id: ChannelPairingId, pub code: String, pub expires_at: Timestamp }
/// One page of channel history in the terms of the Knowledge Service's `IngestItem` (the program maps
/// `ts` → `origin_id`, `time` → `origin_time`, `thread_ts` → `thread`; `newest_ts` → `ChatHistoryPage.newest_position`;
/// the `ts` of each message with `reply_count > 0` → `ChatHistoryPage.threads`, in page order).
pub struct HistoryPage { pub messages: Vec<HistoryMessage>, pub newest_ts: Option<String>, pub next_cursor: Option<String> }
pub struct HistoryMessage {
    pub ts: String,                    // Slack `ts`, the message's id in its channel
    pub time: Timestamp,               // `ts` read as seconds since the epoch
    pub thread_ts: Option<String>,     // set when the message is in a thread
    pub sender: String,                // `user`, or `bot_id` for a program
    pub sender_name: Option<String>,   // the sender's display name; None when Slack does not give one
    pub from_program: bool,            // `bot_id` present
    pub own: bool,                     // `sender` is this link's `agent_user_id`
    pub text: String,                  // `text` plus the text of blocks and attachments (step F.11)
    pub reply_count: u32,              // Slack `reply_count`; 0 when absent (a reply, or a message with no thread)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct SlackAdapterError { pub code: &'static str, pub retryable: bool }
```

**Called by.**

| Item | Caller |
|---|---|
| `PersonalSlack::new`, `start` | Ordin Server program, personal profile, with the secret store, the Journal and the Channel Gateway |
| `set_tokens` | the setup command: it collects `slack-bot-token` and `slack-app-token`, then calls `set_tokens` once so both are checked and stored as `slack/bot-token` and `slack/app-token` (rows 483, 484) |
| `begin_pairing` | the setup command `ordin-server setup pair-chat --channel slack`; ending a pairing is `ChannelGateway::end_pairing(Slack)` (this adapter defines none, row 482) |
| `HostedSlackLink::new`, `start`, `is_open` | Ordin Server program, agent profile `chat` role, one link per `chat[]` connection, with `SlackTokens` read from the variables named by `bot-token-env` and `app-token-env` and the program's `MetricsSink` |
| `history`, `replies` | the agent-profile program, as the chat-history source of the Knowledge Service's ingest (`ChatHistory::page` and `ChatHistory::replies`), which the Job Runner schedules (R14) |
| `ChannelAdapter`, `HostedChatLink` methods | Channel Gateway; the agent-profile program also wraps `HostedChatLink::channel_documents` as the Connection Binder's `ChannelDocuments` (this crate has no edge to the Connection Binder) |

### Example

Socket Mode envelope (a direct message from the paired person) and its acknowledgement:

```json
← {"envelope_id":"57d6a792-4d35-4d0f-b8d6-9a2a3c0e1c44","type":"events_api","accepts_response_payload":false,
   "payload":{"event_id":"Ev07P2QKJ7","event":{"type":"message","channel_type":"im","channel":"D07ASHA","user":"U07ASHA",
   "text":"status","ts":"1729412640.000200"}}}
→ {"envelope_id":"57d6a792-4d35-4d0f-b8d6-9a2a3c0e1c44"}
→ gateway.receive(InboundItem { channel: Slack, sender_id: "U07ASHA", chat_id: "D07ASHA", is_private: true,
    message_id: "1729412640.000200", body: Command { command: Status, rest: "" } })
```

A card (gateway `send`) as Block Kit:

```json
POST https://slack.com/api/chat.postMessage
{"channel":"D07ASHA","text":"Send the supplier summary to Priya Shah?",
 "blocks":[{"type":"section","text":{"type":"mrkdwn","text":"Send the supplier summary to Priya Shah?\nWhere: Email, from asha.patel@acme.co.uk\nAffects: Priya Shah\nChanges: One email with the summary attached"}},
           {"type":"actions","elements":[
             {"type":"button","action_id":"ordin:0","text":{"type":"plain_text","text":"Send"},"value":"c:q8Zt3VxN0pLk2mRb7sYw1A"},
             {"type":"button","action_id":"ordin:1","text":{"type":"plain_text","text":"Not now"},"value":"c:Lm4Qp0Rt7Vx2Zb5Nc8Df3g"},
             {"type":"button","action_id":"ordin:2","text":{"type":"plain_text","text":"Show details"},"value":"c:Hh2Kd9Ws1Qe6Zr3Tb8Nm4v"}]}]}
→ {"ok":true,"channel":"D07ASHA","ts":"1729412650.000300"} → SentMessage { chat_id: "D07ASHA", message_id: "1729412650.000300" }
```

Agent profile — a mention becomes a `HostedEvent`:

```text
← events_api app_mention { channel: "C0HELPDESK", user: "U07ASKER", text: "<@U07AGENT> please email the supplier the corrected invoice", ts: "1729412345.000100" }
→ ack; HostedEvent::Message { connection: "support", envelope_id, kind: Mention, channel: "C0HELPDESK", thread: None,
    message: Some("1729412345.000100"), user: "U07ASKER", from_program: false,
    text: "please email the supplier the corrected invoice", files: [], trigger: None }
```

## Internal Logic

**A. Web API client**

1. `call(method, token, body)`: `POST https://slack.com/api/<method>` with `Authorization: Bearer <token>`, JSON body (form-encoded for `apps.connections.open`), timeout 15 s; the computer's proxy and trust store (A-39).
2. HTTP 429 or `error = "ratelimited"`: wait `Retry-After` seconds and repeat, at most 2 waits; then `SCA_RATE_LIMITED` (retryable, with `retry_after_ms`).
3. `ok: false`: `invalid_auth`, `not_authed`, `token_revoked`, `account_inactive` → `SCA_TOKEN_REFUSED` (status `CredentialsRefused`); `missing_scope` → `SCA_MISSING_SCOPE` (permanent; log names the `needed` scope); `channel_not_found`, `not_in_channel`, `message_not_found`, `cant_update_message` → `SCA_NOT_FOUND` (permanent); `msg_too_long`, `invalid_blocks` → `SCA_MESSAGE_INVALID` (permanent); any other → `SCA_CALL_FAILED` (permanent) with Slack's short error word.
4. Transport error, timeout, 5xx → `SCA_UNREACHABLE` (retryable). An answer lacking a field read → `SCA_BAD_RESPONSE`.
5. One log line per call failure: `slack.call.failed {connection, method, code}`; never arguments, answers or tokens.

**B. Socket Mode link**

1. **Open**: `apps.connections.open` with the app-level token → `url` (`ok: false` with `invalid_auth` → `CredentialsRefused`). Connect the WebSocket to `url` with rustls using the platform verifier. When the system proxy applies to `wss-primary.slack.com` (or the URL's host) and the host is not in `NO_PROXY`, open TCP to the proxy, send `CONNECT <host>:443 HTTP/1.1`, require `200`, then run TLS and the WebSocket handshake over that stream (*chosen here*: A-39 applies to every outbound HTTPS call). Wait for `{"type":"hello"}` (10 s) → status `Connected`, reset the backoff.
2. **Read loop**: each text frame is one JSON envelope:
   - `hello` → as B.1.
   - `disconnect` (`reason`: `warning`, `refresh_requested`) → open a new link at once (no backoff) and close this one; `link_disabled` → status `CredentialsRefused` (Socket Mode was turned off for the app) and retry every 5 minutes.
   - `events_api`, `slash_commands`, `interactive` → step B.3.
   - unknown `type` → ignored, logged `slack.envelope.unknown {type}`.
3. **Acknowledge on receipt** (C-56): send `{"envelope_id": …}` before anything else, except an `interactive` envelope whose payload `type` is `view_submission`, which is acknowledged after the gateway answers (≤ 2 s budget), with `{"envelope_id": …, "payload": {"response_action": "errors", "errors": {…}}}` when the gateway returns `FormErrors`. Then pass the envelope to the mode's dispatcher (C or E) on a task; one task per envelope, at most 64 at once per link (further envelopes wait).
4. **Liveness**: tungstenite answers Slack's pings; no frame for 60 s, a closed socket or a read error → status `Reconnecting { since }` and reopen after `delay(n) = clamp(min(300, 2^(n−1)) × U(0.8, 1.2), 1, 300)` s, never giving up (2D-4).
5. `shutdown` → send a close frame and stop; envelopes already acknowledged finish their task.

**C. Personal profile — inbound (dispatcher)**

1. `events_api` with `event.type = "message"` and `channel_type = "im"`:
   - drop `subtype` ∈ {`bot_message`, `message_changed`, `message_deleted`, `channel_join`}, and messages whose `user` is the bot's own user id.
   - `p = gateway.active_pairing(Slack)` (cached 30 s). Paired and `user = p.chat_user_id` and `channel = p.private_chat_id` → C.2. Otherwise, with a code row present → step G (pairing). Otherwise → `count_ignored(Slack, NotPaired | OtherUser)`; no reply.
2. Build `InboundItem { channel: Slack, sender_id: user, chat_id: channel, is_private: true, message_id: ts, received_at: ts as time }`:
   - Commands (ADR-076: the words, no slash commands): trimmed text, lower-cased, trailing `.`/`!` removed, equal to `stop`, `status` or `triggers` → `Command { …, rest: "" }`; equal to `run` or beginning with `run ` → `Command { Run, rest: <the rest, original case> }`. (*Reading taken*: a word counts only when it is the whole message, or `run` followed by a name, so "stop the email to Priya" stays a request to Ordin.)
   - `files`: each file → one item `File { file: id, file_name: name, media_type: mimetype, size_bytes: size, caption: text for the first file only }`; a file with `subtype = "slack_audio"` (a recorded voice clip) → `Voice`.
   - other text → `Text`.
   - `gateway.receive(item)`.
3. `interactive` `block_actions` from the paired user in the paired channel: for each action whose `action_id` begins with `ordin:` → `Press { data: value, ack: PressAck(envelope_id) }`. From anyone else → `count_ignored(OtherUser)`.
4. `app_mention`, channel messages, `slash_commands` and anything else in the personal profile → `count_ignored(NotPrivate)`; no reply.

**D. Personal profile — `ChannelAdapter`**

1. `limits()` → `{ max_text_chars: 4000, max_caption_chars: 4000, max_buttons: 10, max_button_label_chars: 75, max_send_file_bytes: 104_857_600, max_receive_file_bytes: 104_857_600, min_send_interval_ms: 1000 }`.
2. **Text to mrkdwn**: escape `&`, `<`, `>`; a left-to-right scanner turns `**x**` into `*x*` (bold) and a single `*x*` into `_x_` (italic); nothing else is converted.
3. **Blocks**: the text split into `section` blocks of ≤ 3,000 characters at line ends; buttons in `actions` blocks of ≤ 5 elements, `action_id = "ordin:<index>"`, `value = data`, label as `plain_text`; ≤ 50 blocks in all. Top-level `text` = the first 4,000 characters (notification fallback).
4. `send(to, msg)`: without a file → `chat.postMessage { channel: to.chat_id, text, blocks }` → `SentMessage { chat_id: channel, message_id: ts }`. With a file: `files.getUploadURLExternal { filename, length }` → `upload_url`, `file_id`; `POST` the bytes to `upload_url` (120 s); `files.completeUploadExternal { files: [{id: file_id, title: file_name}], channel_id, initial_comment: text }`; then, when there are buttons, a separate `chat.postMessage` with the buttons only; the returned message is the last one sent. Bytes over 100 MB → `Permanent SCA_FILE_TOO_LARGE`.
5. `edit(sent, text, buttons)` → `chat.update { channel, ts, text, blocks }` (no `actions` block when `buttons` is empty).
6. `fetch_file(file, max)`: `files.info { file }` → `size`, `url_private_download`; `size > max` → `SCA_FILE_TOO_LARGE`; `GET` the `https` address with the bot token, no redirect followed, stop beyond `max`.
7. `ack_press` → `Ok(())` (the envelope was acknowledged in B.3).
8. `status()` → the link's status.

**E. Agent profile — inbound (dispatcher)**

1. `events_api`:
   - `app_mention` → `Message { kind: ThreadMention when thread_ts is set and differs from ts, else Mention, channel, thread: thread_ts, message: ts, user, from_program: bot_id present, text: <the text with every `<@agent_user_id>` removed and trimmed>, files, response_url: None }`.
   - `message` with `channel_type = "im"` (no `subtype` other than `file_share`; not from the agent) → `Message { kind: DirectMessage, channel, thread: thread_ts, message: ts, … }`.
   - `member_joined_channel` with `user = agent_user_id` → `Joined { channel }`; `channel_left` / `group_left` → `Left { channel }`.
   - anything else → ignored.
2. `slash_commands` → `Message { kind: Command, channel: channel_id, thread: None, message: None, user: user_id, text: payload.text, trigger: Some(trigger_id), response_url: payload.response_url }` (*assumed here*: the Slack administrators register one slash command for the app, and the configuration's command texts are the words that follow it).
3. `interactive`: `block_actions` → one `Action { action: action_id without "ordin:", value, user, channel, thread: message.thread_ts, response_url: payload.response_url }` per action (a `workflow_button` press starts the Slack workflow and arrives as an action like a link button's); `view_submission` → `FormSubmitted { form: view.callback_id, metadata: view.private_metadata, values: <block_id → selected or typed values>, user }` and its acknowledgement carries the gateway's `HostedAck` (B.3).
4. Every event → `gateway.receive_hosted(event)`; the gateway's record of `(connection, envelope_id)` drops Slack's retries.

**F. Agent profile — `HostedChatLink`** (bot token; errors as step A, mapped to `LinkError { code }`)

1. `post(p)`: `only_to` set → `chat.postEphemeral { channel, user: only_to, thread_ts, text, blocks }` (returns no persistent id; the result is `message_ts`); else `chat.postMessage { channel, thread_ts, text, blocks }` → `ts`. Buttons as D.3 with `action_id = "ordin:" + action`; a button with `url` and `kind = Url` as a link button (`"url": url`); a button with `url` and `kind = WorkflowLink` as `{"type":"workflow_button","action_id":"ordin:" + action,"text":{"type":"plain_text","text":label},"workflow":{"trigger":{"url":url}}}` (ADR-107; the address is a Slack workflow link trigger, `https://slack.com/shortcuts/…`, SA-HOST-19); workflow buttons are placed first in the actions blocks. Text over `MAX` (6 sections × 3,000) is cut and ends with "…".
2. `post_direct(user, text, buttons)`: `conversations.open { users: user }` → channel; `chat.postMessage` → `(channel, ts)`.
3. `update` → `chat.update`. `react`/`unreact` → `reactions.add`/`reactions.remove { channel, timestamp, name }`; `already_reacted` and `no_reaction` count as success.
4. `thread(channel, thread, limit)`: `conversations.replies` page by page (200, at most 25 pages); sorted by `ts`; the last `limit` messages, each `HostedMessage { id: ts, sender: user or bot_id, from_program, text: <step F.11>, files }`.
5. `file(id, max)`: `files.info`; image types (`image/png`, `image/jpeg`, `image/gif`, `image/webp`) → `Piece { piece: "image", content: base64 }`; text types (`text/*`, `application/json`, `text/html` converted to text, `application/vnd.slack-docs`) → `Piece { piece: "document", media_type: "text/plain" }`; other → `LeftOut { reason: "wrong-type" }`; over `max` → `LeftOut { "too-large" }`; any read failure → `LeftOut { "read-failed" }` (never an error). *Amended January 2026 (ADR-107):* a canvas (`application/vnd.slack-docs`, Slack's HTML) is converted to text by these rules, in order (SA-HOST-20): `h1`, `h2`, `h3` become lines starting `# `, `## `, `### `; each `p` and each `li` becomes one line (`- ` before a list item); a link element (`a`) becomes its text followed by its `href` in brackets, `text (https://…)`, the address kept even when Slack marks the element `lnk`; every other tag is removed with its attributes, so no element identifier such as `temp:C:…` remains; character entities are decoded; runs of blank lines become one. The piece's `label` is `canvas <file id>` and `size_bytes` the UTF-8 byte size of the text supplied.
6. `channel_documents(channel, max)` — *amended January 2026 (ADR-107)*. Canvases first, then files pinned in the channel (`pins.list`, items of type `file`), in that order, no file id twice (*assumed here*: wrightrun left the field to read open, its Gap 2). The canvases are, in this order (SA-HOST-21): the channel canvas (`conversations.info` → `channel.properties.canvas.file_id`), then every entry of `channel.properties.tabs` whose `type` is `canvas`, by its `data.file_id`, in the order Slack lists them. A tab is chosen by its `type` and never by its position; a tab of any other type (`files`, `bookmarks`, `workflows`) is ignored. Every canvas is returned while the total stays within `max`; before a canvas is downloaded, its `files.info` `size` is compared with the remaining budget, and one larger than the budget is not downloaded and becomes `LeftOut { reason: "over-total" }`. Each item is read as F.5 against the remaining byte budget; a converted text longer than the budget is cut to it at a character boundary with `cut: true`. The gateway reads the bytes supplied and the number of canvases returned (zero included) from these items (`size_bytes`, pieces labelled `canvas …`). Every item this adapter builds has `step: None` (the Channel Gateway sets the step, K.3.7) and, for a `Piece`, `items: 1`; `observed_bytes` is the UTF-8 size of the converted text before a cut on a `Piece` with `cut: true`, and the `files.info` `size` on a `LeftOut` for `too-large` or `over-total`, otherwise `None`.
7. `may_read_thread(user, channel)`: `conversations.members` pages (200, at most 50) until `user` is found (`true`), the last page (`false`) or the ceiling (`false`, logged); errors are returned, so the gateway starts nothing (fail closed).
8. `member_channels()`: `users.conversations { types: "public_channel,private_channel", exclude_archived: true, limit: 200 }` page by page, at most 50 pages; identifiers sorted, unique; `complete = false` when the ceiling ended the reading; any page error is returned and no partial list.
9. `open_form(trigger, form)`: `views.open { trigger_id, view: { type: "modal", callback_id: form.form, title (≤ 24 chars), submit: "Save", close: "Cancel", private_metadata: form.metadata (≤ 3,000), blocks: one `input` block per field } }`; field kinds → `checkboxes` (flag), `number_input`, `plain_text_input` (multiline for `Lines`), `multi_static_select` (selection, ≤ 100 options of ≤ 75 chars); more than 100 fields or limits exceeded → `SCA_FORM_TOO_LARGE`.
10. `history(channel, oldest, cursor)` → `conversations.history { channel, oldest, cursor, limit: 200 }` → `HistoryPage`: one `HistoryMessage` per message, in `ts` order; `text` by step F.11; `sender_name` from `users.info` (`profile.display_name`, else `real_name`), each user read once and kept for the link's life (*chosen here*), a failed read → `None`; `own` = `sender == agent_user_id`; `reply_count` = the message's `reply_count`, 0 when absent (*amended January 2026, ADR-107*); `newest_ts` = the largest `ts` on the page, `None` for an empty page; `next_cursor` = `response_metadata.next_cursor` when non-empty. `conversations.history` returns the parent of a thread and not its replies; the replies are read with `replies`.
    a. `replies(channel, thread_ts, cursor)` (*added January 2026, ADR-107*) → `conversations.replies { channel, ts: thread_ts, cursor, limit: 200 }` → `HistoryPage` built as step 10, without the message whose `ts` equals `thread_ts` (the parent, already read by `history`); every message carries `thread_ts`; `newest_ts` is the largest `ts` of the replies returned. Errors as step A; the caller decides what one failed thread means.
11. **Message text** (*added January 2026, ADR-107*; SA-HOST-18) — the one rule for `history`, `replies` and `thread`. Start from the message's `text`. When the message has a `blocks` array, append the text of its blocks, one line per piece, in block order: a `section` block's `text` and each of its `fields`; a `header` block's text; each text element of a `context` block; a `rich_text` block's plain runs joined, a link run as its text followed by its `url` in brackets (its `url` alone when it has no text), a user mention as `<@U…>`, a channel mention as `<#C…>`. Then, for each entry of `attachments`, its `title` and its `text`, else its `fallback`. A line equal (after trimming) to one already present is not appended again, so a message whose `text` repeats its blocks is stored once. `mrkdwn` marks are kept as they are.
12. `replace_ephemeral(response_url, text, buttons)` (*added January 2026, ADR-107*; SA-HOST-22): the address must begin `https://hooks.slack.com/` (else `SCA_RESPONSE_URL_INVALID`, nothing sent). `POST` to it, without the bot token, `{"replace_original": true, "response_type": "ephemeral", "text", "blocks"}` built as F.1, timeout 15 s. A `200` answer is success; `404` or a body of `expired_url` or `used_url` (Slack keeps an address for 30 minutes and 5 uses) → `SCA_NOT_FOUND`; transport errors and `5xx` → `SCA_UNREACHABLE`. The address is never logged.

**G. Pairing (personal profile)**

1. `begin_pairing(paired_by)`: `gateway.is_allowed(Slack)` false → `SCA_NOT_ALLOWED`; `gateway.begin_pairing(Slack, paired_by)`; 8 random characters (OS RNG); store `(pairing_id, sha256(code), expires_at, window_start, wrong = 0)` in `slack_pairing_codes`; return the code as `XXXX-XXXX`.
2. A direct message from an unpaired user while a code row exists: normalise (remove spaces and `-`, upper-case). Expired → delete, `gateway.expire_pairing`, reply in that direct message "That code has expired. Ask whoever runs your ordin server for a new code."; match (constant-time) → delete, `gateway.complete_pairing(id, user, channel)`; wrong → count within a 10-minute window, reply "That code isn't right. Check it, or ask whoever runs your ordin server for a new code.", and at the fifth: end the pairing and reply "Pairing has stopped after too many wrong codes. Ask whoever runs your ordin server to start it again." (same words as the Telegram Adapter; *chosen here*).

**H. `set_tokens(bot, app)`** (setup command): `bot` must begin `xoxb-`, `app` `xapp-` (else `SCA_TOKEN_INVALID`); `auth.test` with `bot` (→ `SlackIdentity`), `apps.connections.open` with `app` (the URL is discarded); both `ok` → `SecretStore::put` for both names. The running server reads them at its next start.

## Data Model

**SA-CLOSE-01** (*chosen here*; closing decision K7): this crate depends on the Journal (2B-3 adds the edge; D4) and keeps its table in the Journal's `Datastore`, reached as `journal.datastore().sqlite()`; it exports `pub fn migrations() -> ModuleMigrations` (module `slack_channel_adapter`, version 1 = the SQL below, empty `postgres`), which the personal-server program registers; no pool is handed over. It writes no journal entry or event. A `None` from `sqlite()` is refused at `PersonalSlack::new` with `SCA_DATASTORE_FAILED`. Personal profile only (SQLite). The agent profile keeps no table here: its state (scopes, pending questions, replies, events seen) is the Channel Gateway's, so any chat instance can carry on another's work; the connection state is in memory (02:618 *Owns* "the connection state").

```sql
-- slack_channel_adapter 1
CREATE TABLE slack_pairing_codes (
    pairing_id    TEXT PRIMARY KEY CHECK (length(pairing_id) = 36),
    code_sha256   TEXT NOT NULL CHECK (length(code_sha256) = 64),
    expires_at    TEXT NOT NULL,
    window_start  TEXT NOT NULL,
    wrong         INTEGER NOT NULL DEFAULT 0 CHECK (wrong BETWEEN 0 AND 5)
);
```

The Slack pairing itself is the gateway's `channel_pairings` row of kind `slack` (*reading taken*: 02:618 gives this adapter "the Slack pairing", 02:604 and 2C give pairings to the Channel Gateway; the adapter owns the code while valid, as the Telegram Adapter does — findings D-1). Rollback: adds one table; the pre-migration backup removes it.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A token missing at start | status `NotStarted` | `SCA_TOKEN_MISSING` | `SlackAdapterError { code, retryable: false }` |
| Token not of the right form at setup | `Err(permanent)` | `SCA_TOKEN_INVALID` | as above |
| Token refused (`invalid_auth`, `token_revoked`) or Socket Mode disabled (`link_disabled`) | `Err(retryable)`; status `CredentialsRefused` | `SCA_TOKEN_REFUSED` | `ChannelSendError::Retryable` / `LinkError` |
| Scope missing from the app | `Err(permanent)` | `SCA_MISSING_SCOPE` | as above (log names the scope) |
| Rate limited after 2 waits | `Err(retryable)` | `SCA_RATE_LIMITED` | `ChannelSendError::Retryable { retry_after_ms }` |
| Transport error, timeout, 5xx | `Err(retryable)` | `SCA_UNREACHABLE` | as above |
| Channel, message or file not found / not in channel | `Err(permanent)` | `SCA_NOT_FOUND` | as above |
| Message or blocks refused | `Err(permanent)` | `SCA_MESSAGE_INVALID` | as above |
| File over 100 MB or over the caller's limit | `Err(permanent)` | `SCA_FILE_TOO_LARGE` | as above |
| Form over Slack's limits | `Err(permanent)` | `SCA_FORM_TOO_LARGE` | `LinkError` |
| `replace_ephemeral` with an address that is not `https://hooks.slack.com/…` (ADR-107) | `Err(permanent)`; nothing sent | `SCA_RESPONSE_URL_INVALID` | `LinkError` |
| `replace_ephemeral` address expired or used up (`404`, `expired_url`, `used_url`) | `Err(permanent)` | `SCA_NOT_FOUND` | `LinkError` |
| Answer missing a field read | `Err(permanent)` | `SCA_BAD_RESPONSE` | as above |
| Other Slack error word | `Err(permanent)` | `SCA_CALL_FAILED` | as above |
| WebSocket closed, silent 60 s or proxy `CONNECT` refused | reconnect with backoff | `SCA_LINK_LOST` (log) | — |
| Pairing outside an allowed channel | `Err(permanent)` | `SCA_NOT_ALLOWED` | as above |
| Pairing code wrong / expired / fifth wrong | reply in the direct message | `SCA_CODE_WRONG`, `SCA_CODE_EXPIRED`, `SCA_PAIRING_STOPPED` (log) | — |
| Code table read or write fails | `Err(retryable)` | `SCA_DATASTORE_FAILED` | as above |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Slack Channel Adapter

  Scenario: Happy path — every envelope is acknowledged on receipt
    Given a personal-profile link that received hello
    When an events_api envelope with id E1 arrives
    Then {"envelope_id":"E1"} is sent before the gateway is called

  Scenario: Pairing in the direct message
    Given begin_pairing showed "K7QM-29XD"
    When U07ASHA sends "k7qm 29xd" in direct message D07ASHA within 10 minutes
    Then the gateway completes the pairing with chat user "U07ASHA" and private chat "D07ASHA"

  Scenario: Command words only as the whole message
    Given Slack is paired with U07ASHA in D07ASHA
    When U07ASHA sends "Stop."
    Then the gateway receives Command Stop
    When U07ASHA sends "stop the email to Priya"
    Then the gateway receives Text "stop the email to Priya"
    When U07ASHA sends "run Invoice check"
    Then the gateway receives Command Run with rest "Invoice check"

  Scenario: Anyone else gets nothing
    Given Slack is paired with U07ASHA
    When U07OTHER sends "hello" in their direct message with the app
    Then nothing is sent and the gateway counts it as other_user

  Scenario: Card update removes the buttons
    When the gateway edits a sent card with no buttons
    Then chat.update is called with blocks holding no actions block

  Scenario: Refresh requested
    When Slack sends disconnect with reason refresh_requested
    Then a new link is opened at once without backoff

  Scenario: Error — link lost
    Given no frame arrives for 60 seconds
    Then the status becomes Reconnecting and reopening waits between 0.8 and 1.2 seconds, then doubles up to 5 minutes

  Scenario: Error — rate limited
    When chat.postMessage answers 429 with Retry-After 3 three times
    Then the call waits twice and returns SCA_RATE_LIMITED

  Scenario: Agent profile — mention in a thread
    When app_mention arrives with ts 2.0 and thread_ts 1.0 in C0HELPDESK
    Then the gateway receives Message with kind ThreadMention, thread "1.0" and the agent's mention removed from the text

  Scenario: Agent profile — joins and leaves
    When member_joined_channel arrives for the agent's own user in C0NEW
    Then the gateway receives Joined C0NEW

  Scenario: Agent profile — settings form errors are returned on the acknowledgement
    Given the gateway answers FormErrors {"limit": "Must be at most 10."}
    When a view_submission envelope E9 arrives
    Then the acknowledgement for E9 carries response_action "errors" with that map

  Scenario: Agent profile — incomplete channel list
    Given users.conversations has more than 50 pages
    When member_channels is called
    Then complete is false

  Scenario: The pairing code table lives in the Journal's datastore
    Given the personal-server program registered PersonalSlack::migrations() with the Journal
    When PersonalSlack is built with the Journal and a pairing code is issued
    Then the code row is written through journal.datastore().sqlite()

  Scenario: Agent profile — history carries what the Knowledge ingest needs
    Given channel C has a message by U1 (display name "Asha") at ts "1729414800.000100" in thread "1729414700.000200" and one by the agent's own user
    When history is called for C
    Then the first HistoryMessage has ts "1729414800.000100", thread_ts "1729414700.000200", sender_name "Asha" and own false
    And the agent's message has own true
    And newest_ts is the larger of the two ts values

  Scenario: Agent profile — a thread parent reports its replies
    Given channel C has a parent message at ts "1.0" with reply_count 3
    When history is called for C
    Then its HistoryMessage has reply_count 3 and the replies are not on the page

  Scenario: Agent profile — replies are read without their parent
    Given the thread "1.0" in channel C has 3 replies
    When replies is called for C and "1.0"
    Then conversations.replies is called with ts "1.0" and limit 200
    And the page holds the 3 replies, each with thread_ts "1.0", and not the parent

  Scenario: Agent profile — a bot message held only in blocks keeps its content
    Given a message with text "Alert" and a section block "Disk 91% on host-4"
    When history is called
    Then the HistoryMessage text holds the line "Alert" and the line "Disk 91% on host-4"

  Scenario: Agent profile — a line repeated in text and blocks is kept once
    Given a message with text "Disk 91% on host-4" and a section block "Disk 91% on host-4"
    When thread is called
    Then the HostedMessage text holds "Disk 91% on host-4" once

  Scenario: Agent profile — an attachment without text gives its fallback
    Given a message with no text and an attachment with title "Build failed" and fallback "Build 412 failed"
    When history is called
    Then the HistoryMessage text holds the lines "Build failed" and "Build 412 failed"

  Scenario: Agent profile — a workflow button is rendered as a workflow_button element
    Given a post with a button of kind WorkflowLink, label "Reset access" and url "https://slack.com/shortcuts/Ft07RESET/abc"
    When post is called
    Then the actions block holds a workflow_button with that label and the trigger url, before any other button

  Scenario: Agent profile — a canvas that exists only as a tab is read
    Given conversations.info gives no channel canvas and tabs [{type "files"}, {type "canvas", data.file_id "F07CANVAS"}]
    When channel_documents is called with 100000 bytes
    Then the canvas F07CANVAS is returned with label "canvas F07CANVAS" and the files tab is ignored

  Scenario: Agent profile — a canvas larger than the remaining budget is not downloaded
    Given a channel canvas of 60000 bytes is read and a tab canvas reports size 50000
    When channel_documents is called with 100000 bytes
    Then the tab canvas is LeftOut "over-total" and its file is not downloaded

  Scenario: Agent profile — canvas HTML becomes text
    Given a canvas holding <h2>Paging</h2>, <p id="temp:C:abc">Call <a class="lnk" href="https://ops.example.org/rota">the rota</a></p>
    When channel_documents is called
    Then the text is "## Paging" and "Call the rota (https://ops.example.org/rota)" and holds no "temp:C:"

  Scenario: Agent profile — a slash command passes its response_url
    When a slash_commands envelope arrives with response_url "https://hooks.slack.com/commands/T1/2/abc"
    Then the gateway receives Message with kind Command and that response_url

  Scenario: Agent profile — an ephemeral message is replaced through its response_url
    Given an action event carried response_url "https://hooks.slack.com/actions/T1/2/abc"
    When replace_ephemeral is called with that address and the text "Shared in the thread."
    Then the address receives replace_original true and response_type ephemeral, and no bot token

  Scenario: Error — a response_url on another host is refused
    When replace_ephemeral is called with "https://example.org/hook"
    Then it returns SCA_RESPONSE_URL_INVALID and nothing is sent
```

## Performance, Security, Observability

- **Performance targets (amended January 2026, ADR-107):** `replies` reads one page of 200 per call, as `history`; `replace_ephemeral` one `POST` within 15 s. Envelope acknowledgement within 100 ms of receipt p95 (Slack's limit is 3 s); event to gateway call < 20 ms (excluding the gateway); ≤ 64 envelopes in flight per link; reconnect after `refresh_requested` within 2 s; memory < 64 MiB plus files in transit (≤ 100 MB).
- **Security:** outbound only (Socket Mode), so no port and no public address (ADR-076); tokens only from the secret store (personal) or the program's secret resolution (agent), never from the chat or the environment, and kept apart from the person's own Slack user token (07:68); personal profile answers only the paired user in their direct message; pairing codes as the Telegram Adapter's (random, hashed, 5 tries); button values are opaque references; file downloads only over `https` with no redirect; all text escaped before the two allowed marks. **Never logged:** tokens, message text, file names and content, user names; Slack identifiers only in the agent profile's existing wrightrun log lines (no content).
- **Observability (logs, `tracing`, target `ordin_slack_channel_adapter`):** `slack.link.open {connection?, num_connections}`, `slack.link.lost {connection?, reason, attempt, delay_ms}`, `slack.link.refresh`, `slack.token.refused`, `slack.envelope {type, event_type}` (debug), `slack.envelope.unknown {type}`, `slack.call.failed {connection?, method, code}`, `slack.pairing.*` as the Telegram Adapter's, `chat.channels.ceiling {connection}`, `chat.members.ceiling {connection}`, `slack.canvas.skipped {connection, reason}` (ADR-107; never a file name or text). Agent profile: the program's `MetricsSink` receives `wrightrun_chat_link_closed_total{connection}` on each lost link; the other chat counters are the gateway's. No metrics or spans on the personal server.

## Gaps

None.
