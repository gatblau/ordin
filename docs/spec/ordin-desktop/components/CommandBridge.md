# SPEC: Command Bridge

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-command-bridge` · **Build phase:** 3 · **Programs:** D
> **Dependencies:** Service API, Server Connector, Journal

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
- [Gaps](#gaps)
- [Amendment January 2026 — Releases and updates (ADR-114)](#amendment-january-2026--releases-and-updates-adr-114)

## Purpose

The Command Bridge is the Service API's transport on Tauri's IPC, inside the desktop program (`02-architecture.md:417-421`, ADR-096 item 2). It receives every command the screens send, refuses anything that is not a well-formed command of the catalogue, and routes it: a command for the desktop's own backend goes to the desktop program's `ServiceApi` implementation; a command for a paired server's backend goes to the Server Connector, which calls that server's Remote API. It also delivers every event the screens need, in order per backend: the desktop's own events from the Journal's `EventSink`, and each server's events from the channel the Bridge gives the Server Connector (2B-2 D1, D6). The WebView makes no network call and holds no key (01 — trust boundaries 1 and 6); the Bridge is the only door from the WebView into the core.

## Approach

One Tauri command per catalogue row is generated from `ordin_service_api::for_each_command!`, so Tauri capabilities can name each command (07 — Authorisation, layer 1), and every one of them calls the same routing function, which hands local commands to `ordin_service_api::handle` unchanged. Events reach the WebView over one `tauri::ipc::Channel` per window, opened by a transport command outside the catalogue (`service_events`, the IPC counterpart of the Remote API's `GET /v1/events`), in batches flushed every 16 ms. The Bridge owns no table and keeps no state that outlives the window except an in-memory replay buffer of recent server events. Error prefix `BRG_` for the Bridge's own failures; command refusals keep the Service API's `SVC_` codes. Rejected: *one generic `service_command` Tauri command* — capabilities could then not allow or deny individual commands; *Tauri global events (`emit`) for the event stream* — unordered across emit calls under load and slower than a channel for 1,000 events a second (C-73).

### Reconciliation assumptions

- **SA-RECON-01** Unjournalled streams to the screens — dictation updates (DictationService, reconciliation row 157, N39, O36) and the Project Service's terminal and language-server bytes (O172) — travel on one Bridge-owned transport command, `service_streams`, outside the catalogue like `service_events`. The Bridge cannot depend on the Dictation Service or the Project Service (same or higher phase, 2B-3), so it carries opaque JSON payloads tagged with a stream id and a `StreamKind`; the desktop program adapts each producer to `ScreenStreams` (ruling R6: the Bridge owns the transport, consumers conform). `dictation.start` stays a catalogue command whose reply carries the `session_id` the screens match against `StreamMessage.stream`.
- **SA-RECON-02** The `ordin` plugin's default permission set `ordin:default` allows every command of `IPC_COMMANDS`; the desktop program's capability grants `ordin:default` to window `main` (reconciliation row 156).

## Shared Context

### Service API types used (from `ordin-service-api`; 2C wire §1)

```rust
pub struct ServiceApiVersion { pub major: u16, pub minor: u16 }          // CURRENT = 1.0
pub struct CommandEnvelope<I> { pub api_version: ServiceApiVersion, pub request_id: CommandRequestId,
    pub command: CommandName, pub backend: BackendId, pub input: I }       // deny_unknown_fields
pub struct CommandReply<O> { pub request_id: CommandRequestId, #[serde(flatten)] pub outcome: CommandOutcome<O> }
#[serde(tag = "kind")] pub enum CommandOutcome<O> { Ok { result: O }, Error { error: ServiceError } }
pub struct ServiceError { pub code: String, pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub action: Option<RepairAction>,
    #[serde(skip_serializing_if = "Option::is_none")] pub reference: Option<String> }   // Diagnostics line id
pub struct EventEnvelope { pub api_version: ServiceApiVersion, pub backend: BackendId, pub seq: u64,
    pub at: Timestamp, #[serde(flatten)] pub event: ServiceEvent }
```

From the Service API spec (Public Interface): `ServiceApi` (trait, implemented by the desktop program), `ErrorSentences`, `CallOrigin::LocalScreens`, `CommandMeta { name, ipc_name, scope: Scope::{Any, Local}, effect, budget: Budget::{Interactive, Search, Transfer}, … }`, `CATALOGUE`, `command(name)`, `for_each_command!`, `Incoming { body, routed_name, origin, this_backend }`, `handle(api, sentences, incoming, deadline, cancel) -> HandledReply { json, code, http_status }`, `EventEnvelope::new(backend, seq, at, kind: &str, data) -> Result<EventEnvelope, EventError>`, `encode_for_transport(&EventEnvelope) -> Result<TransportEvent { seq, kind, json }, EventError>`, `ServiceEvent::{Reset, ServerStateChanged, …}`, constants `MAX_BODY_BYTES` (1 MiB) and `MAX_TEXT_SAVE_BODY_BYTES` (9 MiB, for `files.save_edit`, `knowledge.save_edit` and `projects.save_file`). The Service API's *Streams outside the catalogue* section names this crate's `service_streams` as the carrier of `dictation.start`, `projects.terminal_*` and `projects.editor_open`/`lsp_send` values. Deadlines: `Interactive` and `Search` 10 s, `Transfer` 60 s.

Event kinds (2C wire `EventKind`, 19; amended January 2026, ADR-109): `run_state_changed`, `run_progress`, `plan_changed`, `step_marked`, `start_up_report_ready`, `approval_requested`, `approval_decided`, `crew_round_changed`, `block_shown`, `reset`, `server_state_changed`, `workspace_changed`, `needs_you_changed`, `block_available`, `locale_changed`, `brand_changed`, `setup_progress`, `sign_in_changed`, `language_changed`. The Bridge forwards every kind unchanged; `locale_changed`, `brand_changed`, `setup_progress` and `sign_in_changed` are desktop-local and never arrive from a server; `language_changed` arrives from the desktop's own core and from a paired server.

### Server Connector interface used (from the Server Connector spec)

```rust
impl ServerConnector {
    /// The connection holding `backend`, if this desktop is paired with that server.
    pub fn connection_for(&self, backend: BackendId) -> Option<ServerConnectionId>;
    /// Sends one command envelope to that server's Remote API and returns the reply JSON.
    pub async fn send_command(&self, backend: BackendId, meta: &'static CommandMeta,
        envelope_json: bytes::Bytes, deadline: std::time::Instant) -> Result<bytes::Bytes, ConnectorError>;
}
/// One item the Server Connector hands to the Bridge, per server, in the server's order.
pub enum ServerEvent {
    /// One event, already checked by the Connector (seq strictly increasing per backend).
    Event { backend: BackendId, envelope: std::sync::Arc<EventEnvelope> },
    /// The server sent `reset`, or the Connector lost events: drop buffered events of `backend`.
    Reset { backend: BackendId },
}
pub type ServerEventSender = tokio::sync::mpsc::Sender<ServerEvent>;
impl CodedError for ConnectorError { /* code(&self) -> &'static str */ }   // CONN_… and SRV_… codes
```

### Journal interface used (from the Journal and Artefact Store spec)

```rust
/// Implemented by the Bridge (ADR-100 item 1); registered with `Journal::subscribe`.
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    fn name(&self) -> &'static str;                          // "command_bridge"
    /// Called once per event, in seq order, never concurrently for one sink.
    async fn deliver(&self, event: &JournalEvent);
    /// The sink's queue overflowed; events from `next_seq` on were not delivered.
    async fn lagged(&self, next_seq: u64);
}
pub struct JournalEvent { pub backend: uuid::Uuid, pub seq: u64, pub at: Timestamp, pub kind: String, pub data: serde_json::Value }
impl Journal {
    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId;
    pub fn unsubscribe(&self, id: SubscriptionId);
    /// Events with seq > `after_seq`, at most `limit` (1–1,000); `JRNL_RESUME_TOO_OLD` when older
    /// than kept, `JRNL_SEQ_AHEAD` when beyond the last.
    pub async fn events_since(&self, after_seq: u64, limit: u32) -> Result<Vec<JournalEvent>, JournalError>;
    pub fn last_seq(&self) -> u64;
}
```

### Constants and rules

- Tauri 2 (05); WebView side `@tauri-apps/api` 2.12.4 (SA-UI-01). Tauri's `Channel` delivers messages to the WebView in send order.
- A-03: 150 ms p95 for interface actions that call no model. A-46: a server's event reaches the screens within 500 ms of its journal recording it.
- C-73 (bound assertion): *Tauri's IPC can carry the Service API's commands and events at the desktop's targets with the same JSON types the Remote API uses* — spike: 1,000 events a second for one minute, command round trips at p95. This spec's design meets it with batched channel messages; if the spike still misses the targets, the HLD's fallback is ADR-096's "a more compact encoding is added only if A-46's targets are missed with JSON", which is an HLD change, not a Bridge setting.
- The desktop opens no port (ADR-096 item 2); the WebView loads bundled assets only (SA-XC-11).

## Public Interface

```rust
/// Built once by the desktop program and stored with `app.manage`.
pub struct CommandBridge { /* private */ }

pub struct BridgeConfig {
    /// The desktop core's own backend id (minted at first start, held by the Journal).
    pub local_backend: BackendId,
    /// Events kept per paired server for replay to a reloaded window; default 1,000.
    pub server_replay_events: usize,
}

impl CommandBridge {
    pub fn new(
        cfg: BridgeConfig,
        api: std::sync::Arc<dyn ServiceApi>,
        sentences: std::sync::Arc<dyn ErrorSentences>,
        connector: std::sync::Arc<ServerConnector>,
        journal: ordin_journal::Journal,
    ) -> (CommandBridge, ServerEventSender);
    /// Subscribes the Bridge's sink to the Journal and starts the task that reads the server channel.
    pub fn start(&self, cancel: tokio_util::sync::CancellationToken);
    /// Every event the Bridge delivers (local and server), each backend in seq order, for the
    /// desktop program's notifications (UX-A4, D6). Capacity 1,024; a slow receiver gets
    /// `RecvError::Lagged` and misses events, never delays the screens.
    pub fn subscribe_all(&self) -> tokio::sync::broadcast::Receiver<std::sync::Arc<EventEnvelope>>;
    /// Quit: new commands are refused with `BRG_CLOSING`; waits up to 2 s for commands in flight,
    /// closes every window stream and unsubscribes from the Journal.
    pub async fn close(&self);
    /// The Tauri plugin carrying every generated command, `service_events` and `service_streams`.
    pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R>;
    /// The sender through which the desktop program passes unjournalled stream messages
    /// (dictation updates, terminal bytes, language-server messages) to the main window.
    pub fn streams(&self) -> ScreenStreams;
}

/// The IPC names of every catalogue command plus `service_events` and `service_streams`, for the
/// desktop program's build script:
/// `tauri_build::Attributes::new().plugin("ordin", InlinedPlugin::new().commands(IPC_COMMANDS)
///     .default_permission(DefaultPermissionRule::AllowAllCommands))`.
pub const IPC_COMMANDS: &[&str];

/// Cloneable sender into the main window's unjournalled stream (`service_streams`).
#[derive(Clone)]
pub struct ScreenStreams { /* private */ }

impl ScreenStreams {
    /// Queues one message for the main window. Never awaits the WebView.
    /// `Err(StreamSendError::NoWindow)` when no window has opened `service_streams`;
    /// `Err(StreamSendError::Lagging)` when the window's stream queue (4,096 messages) is full.
    pub fn send(&self, stream: uuid::Uuid, kind: StreamKind, payload: serde_json::Value)
        -> Result<(), StreamSendError>;
    /// Queues the closing message of `stream` (`kind` unchanged, payload `{"closed": reason}`).
    pub fn close(&self, stream: uuid::Uuid, kind: StreamKind, reason: &str);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind { Dictation, Terminal, Lsp }

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StreamSendError {
    #[error("BRG_STREAM_NO_WINDOW")] NoWindow,
    #[error("BRG_STREAM_LAGGING")] Lagging,
}
```

Called by (*Interactions*, owner side):

| Item | Caller | Use |
|---|---|---|
| `CommandBridge::new`, `start`, `plugin`, `IPC_COMMANDS`, `close` | ordin desktop program (`apps/desktop`) | construction (wiring row 3.49), plugin registration, build script, quit |
| `subscribe_all` | ordin desktop program | notifications from events (UX-A4, D6) |
| `ServerEventSender` (returned by `new`) | ordin desktop program, handed to `ServerConnector::start(events, cancel)` | the Server Connector sends `ServerEvent` values on it |
| `EventSink` (implemented by the Bridge) | Journal and Artefact Store, after `Journal::subscribe` | local events |
| `streams()` → `ScreenStreams` | ordin desktop program | implements the Project Service's `StreamSink` (terminal bytes, LSP messages) and forwards the Dictation Service's `DictationSession.updates` |
| generated `<module>_<verb>` commands, `service_events`, `service_streams` | screens module (`apps/desktop/ui/`) | every command, the event stream, the unjournalled stream |

Generated Tauri commands, one per catalogue row (Tauri plugin `ordin`; invoked as `plugin:ordin|<module>_<verb>`):

```rust
#[tauri::command]
async fn approval_decide(
    bridge: tauri::State<'_, CommandBridge>,
    window: tauri::WebviewWindow,
    envelope: Box<serde_json::value::RawValue>,
) -> Result<Box<serde_json::value::RawValue>, ()>;   // always Ok: the reply carries errors
```

Transport command outside the catalogue *(chosen here)*:

```rust
/// Opens the event stream for the calling window. `after` maps a backend id to the last seq
/// the window has drawn; absent backends start live. Replaces any earlier stream of that window.
#[tauri::command]
async fn service_events(
    bridge: tauri::State<'_, CommandBridge>,
    window: tauri::WebviewWindow,
    after: std::collections::BTreeMap<BackendId, u64>,
    on_events: tauri::ipc::Channel<EventBatch>,
) -> Result<EventsOpened, BridgeError>;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EventBatch {
    /// Each element is one serialised `EventEnvelope`, in delivery order.
    pub events: Vec<Box<serde_json::value::RawValue>>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EventsOpened {
    pub local_backend: BackendId,
    /// Backends for which `after` could not be honoured; the window reloads them by command.
    pub reset: Vec<BackendId>,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
#[error("{code}")]
pub struct BridgeError { pub code: &'static str }
```

Transport command for unjournalled streams, outside the catalogue *(chosen here, SA-RECON-01)*:

```rust
/// Opens the unjournalled stream for the calling window: dictation updates (stream id = the
/// `session_id` that `dictation.start` returned), terminal bytes and language-server messages
/// (stream id = the Project Service's `StreamId`). Replaces any earlier stream of that window.
/// Nothing on this channel is journalled or replayed (ADR-022, ADR-043).
#[tauri::command]
async fn service_streams(
    bridge: tauri::State<'_, CommandBridge>,
    window: tauri::WebviewWindow,
    on_stream: tauri::ipc::Channel<StreamMessage>,
) -> Result<(), BridgeError>;

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StreamMessage {
    pub stream: uuid::Uuid,
    pub kind: StreamKind,
    /// Dictation: one `DictationUpdate` as JSON. Terminal: `{"bytes_b64": "<base64url, no padding>"}`.
    /// Lsp: the language server's JSON-RPC message. Closing: `{"closed": "<reason>"}`.
    pub payload: serde_json::Value,
}
```

Permission set *(chosen here)*: the inlined plugin `ordin` declares its default permission set `ordin:default` with `DefaultPermissionRule::AllowAllCommands`, so `ordin:default` equals `ordin:allow-<ipc_name>` for every entry of `IPC_COMMANDS`. Capability file the desktop program ships *(the program owns the file; this is the content the Bridge requires)*: `apps/desktop/capabilities/main.json` grants window `main` the permission `ordin:default` and nothing else from the `ordin` plugin; no other window is granted any `ordin` permission.

##### Example

WebView (generated TypeScript wrapper):

```ts
const reply = await invoke("plugin:ordin|approval_decide", { envelope: {
  api_version: { major: 1, minor: 0 },
  request_id: "0192a1c0-0d1e-7f20-8a31-425364758697",
  command: "approval.decide",
  backend: "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b",   // a paired server's backend
  input: { card_ref: "q8Zt3VxN0pLk2mRb7sYw1A", decision: "approve" },
}});
// reply (from the server, unchanged):
// {"request_id":"0192a1c0-…","kind":"ok","result":{"approval_id":"0192a1c3-…","state":"approved"}}

const ch = new Channel<EventBatch>();
ch.onmessage = (b) => b.events.forEach(drawEvent);
await invoke("plugin:ordin|service_events", { after: { "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b": 48212 }, onEvents: ch });
// → {"local_backend":"0192a0aa-…","reset":[]}, then batches such as
// {"events":[{"api_version":{"major":1,"minor":0},"backend":"0192a1b4-…","seq":48213,"at":"2026-01-10T09:41:07.312Z","kind":"approval_decided","approval_id":"0192a1c3-…","state":"approved","decided_on":"Asha's laptop"}]}
```

## Internal Logic

1. **Construction.** `new` creates the server event channel (`tokio::sync::mpsc::channel(1024)`) and returns its sender for the program to give the Server Connector (`ServerConnector::start(events: sender, cancel)`), so the Bridge never constructs the Connector and the Connector never sees the Bridge (D6).
2. **A command arrives** (`<module>_<verb>`):
   1. Take the raw envelope text; if longer than the command's body limit (`MAX_TEXT_SAVE_BODY_BYTES` for `files_save_edit`, `knowledge_save_edit` and `projects_save_file`, else `MAX_BODY_BYTES`) reply `SVC_BODY_TOO_LARGE` without parsing further.
   2. Read only the `backend` and `request_id` keys (a `#[derive(Deserialize)] struct Peek { backend: BackendId, request_id: CommandRequestId }` that ignores other keys). Unreadable → pass the body to `handle` with `this_backend = local_backend`, which produces the exact `SVC_` refusal.
   3. `backend == local_backend` → `ordin_service_api::handle(api, sentences, Incoming { body, routed_name: meta.name, origin: CallOrigin::LocalScreens, this_backend: local_backend }, now + budget deadline, window_cancel)` and return `HandledReply.json`.
   4. `connector.connection_for(backend)` is `Some` → if `meta.scope == Scope::Local` reply `SVC_COMMAND_LOCAL_ONLY` built with `sentences` (the server would refuse it as well; refusing here saves a round trip and keeps desktop-only features off the link). Otherwise `connector.send_command(backend, meta, body, deadline)`; `Ok(bytes)` is returned unchanged; `Err(e)` is turned into a reply `{request_id, kind: "error", error: {code: e.code(), message: sentences.sentence(e.code()), action: sentences.action(e.code()), reference: None}}`.
   5. Neither → reply `SVC_BACKEND_UNKNOWN`.
   6. Log `brg.command` once (step 7).
   The Bridge never alters a reply that came from the local `handle` or from a server.
3. **Window cancellation.** Each `WebviewWindow` label gets a `CancellationToken` created on its first command and cancelled on `WindowEvent::Destroyed`; in-flight local commands see `ctx.cancel` fire and their reply is dropped.
4. **Local events.** `start` registers the Bridge's `EventSink` with `journal.subscribe`. `deliver(event)`: `EventEnvelope::new(local_backend, event.seq, event.at, &event.kind, event.data.clone())` then `encode_for_transport`; an `EventError` is logged as `brg.event_dropped` and the record skipped; otherwise the `TransportEvent` goes to every open window stream (step 6) and the envelope to `subscribe_all`'s channel. `deliver` only enqueues and never awaits a window, so the Journal's sink queue is drained at once. `lagged(next_seq)`: read the missed events with `journal.events_since(next_seq - 1, 1000)` in pages until caught up and deliver them as above, logging `BRG_JOURNAL_LAGGED` once; `JRNL_RESUME_TOO_OLD` → send a `reset` envelope for the local backend to every window instead.
5. **Server events.** `start` spawns one task reading the server channel. `ServerEvent::Event` → `encode_for_transport` → append to that backend's replay buffer (a `VecDeque` capped at `server_replay_events`) → send to every open window stream. `ServerEvent::Reset` → clear that backend's buffer and send a `reset` envelope for that backend (seq = the last seq seen, so the screens' order rule does not mark a gap).
6. **Window streams (`service_events`).**
   1. A second call from the same window closes the earlier stream first.
   2. For the local backend, when `after` has an entry: hold live delivery to this window, read `journal.events_since(after, 1000)` in pages up to the seq of the first held live event, send them, then release the held events, dropping any whose seq was already sent. `JRNL_RESUME_TOO_OLD` or `JRNL_SEQ_AHEAD` → add the local backend to `reset`. Without an entry: live only.
   3. For each server backend in `after`: if the replay buffer holds every seq after `after`, send them; else add it to `reset`.
   4. A backend id in `after` that is neither local nor paired → `BRG_BACKEND_UNKNOWN` for the whole call (the window holds a stale backend list and must call `service.hello`/`servers.list` again).
   5. Live delivery: events for one window are queued in one bounded queue (capacity 8,192). A flusher task sends an `EventBatch` every 16 ms or when 50 events wait, whichever comes first. Per backend, events stay in seq order; across backends, arrival order.
   6. Queue full (the WebView stopped reading): drop the queue, send one `reset` envelope per backend instead, log `BRG_WINDOW_LAGGING`. The screens reload by command.
   7. `Channel::send` failing (window gone) closes the stream and drops its queue.
7. **Logging.** `brg.command` at `info` with `request_id`, `command`, `backend_kind` (`local` or `server`), `outcome` (`ok` or code), `elapsed_ms`. `brg.event_dropped` at `warn` with `backend`, `seq`, `kind`, `code`. `brg.stream` at `info` on open and close with `window`, `reset_count`. `brg.stream_lagging` at `warn` with `stream`, `kind`. Never the envelope body, input, result or event data.
8. **Unjournalled streams (`service_streams`).** One stream per window label; only window `main` holds the permission, so in practice one. `streams()` returns a `ScreenStreams` that writes into the current `main` stream's bounded queue (capacity 4,096). A sender task drains the queue and calls `Channel::send` once per message, without batching (dictation partials and terminal echo are latency-sensitive). `send` with no open stream → `StreamSendError::NoWindow` (the caller drops the message; a dictation session with no window is cancelled by the program). Queue full → `StreamSendError::Lagging`, `brg.stream_lagging` logged once per stream id per second; the Bridge never drops a message it has queued. A second `service_streams` call from the window closes the earlier stream and drops its queue. `Channel::send` failing closes the stream. The Bridge never reads `payload` and never logs it.
9. **No business decision.** The Bridge never interprets a command's input beyond `backend` and `request_id`, never answers from a cache, and never raises a notification (the desktop program does that from events, D6).

## Data Model

N/A — the Bridge owns no table (`02-architecture.md:419` *Owns* "the command and event catalogue", which is the `ordin-service-api` crate's data). The replay buffers and window queues are memory only and end with the program.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Envelope over the body limit | reply `kind: error`, `Err(permanent)` | `SVC_BODY_TOO_LARGE` | `{"request_id":null,"kind":"error","error":{"code":"SVC_BODY_TOO_LARGE","message":"…"}}` |
| Envelope unreadable, input invalid, version or name refused (local backend) | reply from `handle`, `Err(permanent)` | the `SVC_` code `handle` gives | the `CommandReply` `handle` built |
| Local-only command addressed to a server | reply, `Err(permanent)` | `SVC_COMMAND_LOCAL_ONLY` | `{"request_id":…,"kind":"error","error":{"code":"SVC_COMMAND_LOCAL_ONLY",…}}` |
| Backend neither local nor paired | reply, `Err(permanent)` | `SVC_BACKEND_UNKNOWN` | as above |
| Server not connected | reply, `Err(retryable)` | `CONN_NOT_CONNECTED` | as above; sentence "Waiting for [server]" (11 §5.31) |
| Server unreachable while sending | reply, `Err(retryable)` | `CONN_UNREACHABLE` | as above, action `try_again` |
| Server refused the desktop key | reply, `Err(permanent)` | `CONN_KEY_REFUSED` | as above; sentence "This computer is no longer connected to [server]." |
| Server version unusable | reply, `Err(permanent)` | `CONN_REFUSED_FOR_VERSION` | as above; one of the two 11 §5.31 sentences |
| Server did not answer by the deadline | reply, `Err(retryable)` | `CONN_DEADLINE` | as above, action `try_again` |
| `service_events` names an unknown backend | `Err(BridgeError)` | `BRG_BACKEND_UNKNOWN` | `{"code":"BRG_BACKEND_UNKNOWN"}` |
| The Bridge's Journal queue overflowed | logged; missed events re-read; `reset` when too old | `BRG_JOURNAL_LAGGED` | — |
| Command after `close` began | reply, `Err(permanent)` | `BRG_CLOSING` | `{"request_id":…,"kind":"error","error":{"code":"BRG_CLOSING",…}}` |
| Window not reading events | `reset` per backend sent | `BRG_WINDOW_LAGGING` | — |
| `ScreenStreams::send` with no open `service_streams` channel | `Err(StreamSendError::NoWindow)` to the program | `BRG_STREAM_NO_WINDOW` | — |
| `ScreenStreams::send` with the window's stream queue full | `Err(StreamSendError::Lagging)` to the program; logged | `BRG_STREAM_LAGGING` | — |
| Event record not decodable or too large | logged, event skipped | `SVC_EVENT_INVALID`, `SVC_EVENT_TOO_LARGE` | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Command Bridge

  Scenario: Happy path — a local command reaches the desktop core
    Given the desktop core with local backend 0192a0aa-0000-7000-8000-000000000001
    When the main window invokes plugin:ordin|runs_stop for run 0192a1c2-3344-7556-8677-8899aabbccdd on the local backend
    Then the desktop program's ServiceApi runs_stop is called once with origin LocalScreens
    And the window receives a reply with kind "ok"

  Scenario: Happy path — a server's command goes to the Server Connector
    Given a paired server whose backend is 0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b and which is connected
    When the window invokes plugin:ordin|approval_decide for that backend
    Then ServerConnector.send_command is called with the envelope bytes unchanged
    And the window receives the server's reply bytes unchanged

  Scenario: Error path — a local-only command addressed to a server
    Given a paired server backend
    When the window invokes plugin:ordin|servers_connect for that backend
    Then the reply has code SVC_COMMAND_LOCAL_ONLY
    And the Server Connector is not called

  Scenario: Error path — the server is not connected
    Given a paired server in state not_connected
    When the window invokes plugin:ordin|approval_decide for that backend
    Then the reply has code CONN_NOT_CONNECTED and action absent

  Scenario: Error path — an unknown backend
    Given no server with backend 0192ffff-0000-7000-8000-000000000000
    When the window sends any command for that backend
    Then the reply has code SVC_BACKEND_UNKNOWN

  Scenario: Happy path — a window reload catches up from the replay buffer
    Given the server backend's replay buffer holds seq 48200 to 48213
    When the reloaded window calls service_events with after {server: 48210}
    Then the first batch holds seq 48211, 48212 and 48213 in order
    And EventsOpened.reset is empty

  Scenario: Edge case — the window asks for events older than the buffer
    Given the server backend's replay buffer starts at seq 48200
    When the window calls service_events with after {server: 47000}
    Then EventsOpened.reset contains that backend
    And live events follow

  Scenario: Edge case — the server sends reset
    Given an open window stream
    When the Server Connector sends Reset for a backend
    Then the window receives one reset envelope for that backend
    And that backend's replay buffer is empty

  Scenario: Error path — a window that stops reading
    Given an open window stream whose queue holds 8,192 events
    When one more event arrives
    Then the queue is dropped and one reset envelope per backend is sent
    And BRG_WINDOW_LAGGING is logged

  Scenario: Error path — an oversized envelope
    Given an approval.decide envelope of 1,048,577 bytes
    When the window invokes plugin:ordin|approval_decide
    Then the reply has code SVC_BODY_TOO_LARGE and no module is called

  Scenario: Happy path — a dictation update reaches the main window unjournalled
    Given the main window has opened service_streams
    And the desktop program forwards dictation session 0192b3c4-1111-7222-8333-444455556666
    When ScreenStreams.send is called with kind dictation and payload {"kind":"partial","text":"Please summarise"}
    Then the window's channel receives one StreamMessage with stream 0192b3c4-1111-7222-8333-444455556666 and kind "dictation"
    And the Journal records nothing for it

  Scenario: Error path — a stream message with no window
    Given no window has opened service_streams
    When ScreenStreams.send is called
    Then it returns BRG_STREAM_NO_WINDOW

  Scenario: Happy path — events are batched in order
    Given 120 local events recorded within 10 ms
    When they are delivered
    Then the window receives three batches of 50, 50 and 20 events
    And seq values are strictly increasing across the batches
```

## Performance, Security, Observability

- **Performance targets:** routing overhead (step 2 excluding `handle` and the Connector) ≤ 1 ms p95; a local `Interactive` command's IPC round trip ≤ 150 ms p95 end to end (A-03), of which the Bridge and IPC take ≤ 10 ms p95 for a 4 KiB envelope; a local event reaches the WebView within 50 ms p95 of the Journal recording it; sustained 1,000 events a second for 60 s with no `BRG_WINDOW_LAGGING` on the reference machine (C-73); memory: at most 8,192 queued events per window plus 1,000 buffered events per paired server.
- **Security:** commands are reachable only through the Tauri capability granting `ordin:default` (every `ordin:allow-<ipc_name>`) to window `main`; no command takes a file path, URL or key from the WebView except the inputs the catalogue types define and check; `CallOrigin::LocalScreens` is set by the Bridge, never read from input; the Bridge holds no server key (the Server Connector reads it from the keychain per call); a server's reply is passed through, never evaluated. Never logged: envelope bodies, inputs, results, event data.
- **Observability:** log events `brg.command`, `brg.event_dropped`, `brg.stream`, `brg.stream_lagging` with the fields of Internal Logic step 7. No metrics or spans (desktop, SA-XC-04, SA-XC-05).

## Gaps

None.

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 item 2; `ServiceApi.md` amendment January 2026 (SVC-RL-2, SA-SVC-RL-03).
>
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

- Every `Incoming` this crate builds — step 3 of the local path — sets `groups: api.groups()`, from the `ServiceApi` it serves on the desktop. This crate decides no group itself; `accept` refuses a command whose group is absent with `SVC_NOT_IN_THIS_VERSION`, which reaches the screens as any other `ServiceError`.
- **Acceptance Criteria (Gherkin) — added scenario**

```gherkin
  Scenario: A command of a group this build does not answer
    Given the ServiceApi served here answers groups that do not include "mail"
    When a mail command arrives
    Then the reply is an error with code SVC_NOT_IN_THIS_VERSION
    And no module is called

  Scenario: A command of a group this build answers
    Given the ServiceApi served here answers groups that include "approval"
    When an approval.list_pending command arrives
    Then the ServiceApi is called with it
    And the reply is the module's result
```

### Gaps

None.
