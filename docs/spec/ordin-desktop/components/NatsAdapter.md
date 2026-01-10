# SPEC: NATS Adapter

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-nats` · **Build phase:** 2 · **Programs:** P A
> **Dependencies:** Identity, Organisation Profile

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

The NATS Adapter is the ordin server's only connection to the company's NATS servers (02:642–647, ADR-074). It connects outbound over TLS or secure WebSocket with credentials read from the server's secret store, reconnects for ever with exponential backoff and jitter from 1 to 60 seconds, and for each NATS event trigger opens one *feed*: a JetStream durable pull consumer with explicit acknowledgement when the trigger's subject is held in a stream, a plain core-NATS subscription otherwise. It delivers each received message, unparsed, to the Trigger Service together with a handle that acknowledges it, pulls no more messages than the trigger has free places, refuses any subject outside the organisation profile's list, enforces the payload ceiling (the smaller of 1 MiB and the server's maximum) and publishes nothing. It exists so that every NATS concern — connection, TLS, credentials, consumers, acknowledgement — sits behind one small interface and the Trigger Service never touches `async-nats`.

## Approach

**Reconciliation assumptions** (decision 80): **SA-RECON-01** — the `nats` section of the organisation profile is never absent (OrganisationProfile.md); empty `servers` turns event triggers off. **SA-RECON-02** — the program calls `reload` after a NATS secret is stored on a running server and on each profile change (row 495).

The adapter wraps `async-nats` 0.50.0 (05:25, C-53) and owns no business rule: it does not parse JSON, apply filters or decide duplicates — the Trigger Service does (03:970–1011). Flow control is pull-based: the Trigger Service gives each feed a shared `FeedCapacity` counter and the adapter fetches at most that many messages, so with JetStream the surplus stays in the stream (ADR-074 *Limits*). Messages waiting in the Trigger Service's queue are kept alive with `+WPI` (in-progress) acknowledgements, so the consumer's acknowledgement wait never redelivers a message that is merely queued. Rejected: a push consumer (it cannot bound delivery by the trigger's free places); acknowledging on receipt (a crash before the journal records the event would lose it — ADR-074 *Alternatives*).

## Shared Context

### Shared types used (from Phase 2C, copied; not redefined)

```rust
// ordin-types — conventions (2C domain §Conventions)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TriggerId(pub uuid::Uuid); // UUIDv7

// ordin-types — Trigger (2C domain §Trigger), the parts this adapter reads
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NatsDelivery { JetStream { consumer_name: String }, CoreNats }
```

| 2C rule used here | Value | Code |
|---|---|---|
| `consumer_name` | 1–255 chars `[A-Za-z0-9_-]` (LD-53) | `TRIG_CONSUMER_INVALID` (raised by Trigger Service; this adapter only generates names that satisfy it) |
| `subject` | within the profile's list (04:281) | `TRIG_SUBJECT_NOT_ALLOWED` (Trigger Service); this adapter re-checks with `NATS_SUBJECT_NOT_ALLOWED` |

### Configuration (Phase 2D)

| Source | Key | Type | Default | Use here |
|---|---|---|---|---|
| Organisation profile | `nats.servers[]` | list of URLs (`tls://`, `wss://`) | empty → event triggers off | servers to connect to |
| Organisation profile | `nats.tls.ca_certificate` | PEM | none → system trust store only | extra trust anchor (C-53) |
| Organisation profile | `nats.subjects_allowed[]` | list of subjects with `*`, `>` | empty | subjects a feed may open |
| Setup command → secret store | NATS credentials (credentials file, user and password, or token) | secret | none | connection credentials (2D-3, ADR-074 *Connection*) |
| Environment | `ORDIN_DATA_DIR` | path | 2D-1a | location of the written CA file (`<ORDIN_DATA_DIR>/nats/ca.pem`) |
| Fixed constant (2D-4) | NATS reconnect backoff | 1 to 60 s, exponential with jitter, never giving up; retry on first connect | — | step C |
| Fixed constant (2D-4) | Event payload size | the smaller of 1 MB and the NATS server's maximum | — | step F.3 |

*Agent profile:* 2D *Former gaps, now settled* item 1 states that the agent profile reads no organisation profile; `nats.servers[]` is therefore empty there and the adapter stays `Off` (assumed here: follows 2D's reading; no other source of NATS settings exists for that profile).

`HTTPS_PROXY`/`HTTP_PROXY` are **not** applied: `async-nats` has no HTTP proxy support and NATS is reached directly (A-41, 2D-1, ADR-074 *Status*).

### Secret names in the server's secret store (*chosen here*)

| Secret name | Content |
|---|---|
| `nats/credentials` (follows the Identity spec's `<module>/<purpose>` convention) | UTF-8 JSON, one of: `{"kind":"creds_file","contents":"<the .creds file text: user JWT and NKey seed>"}`, `{"kind":"user_password","user":"<u>","password":"<p>"}`, `{"kind":"token","token":"<t>"}` |

### Calls this component makes on its dependencies

| Callee | Call as required here | Defined in |
|---|---|---|
| Identity and Credentials | `ordin_identity::SecretStore::get(&self, name: &SecretName) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, CredError>`; `SecretName::new("nats/credentials")` | Identity and Credentials spec (Secret store) |
| Organisation Profile | `OrganisationProfileService::current(&self) -> Arc<ProfileSnapshot>`; `subscribe(&self) -> watch::Receiver<Arc<ProfileSnapshot>>`; this spec reads `ProfileSnapshot.document.nats: NatsSection` with `NatsSection { servers: Vec<String>, tls: NatsTls { ca_certificate: Option<String> }, subjects_allowed: Vec<String> }` (2D-2 keys `nats.servers[]`, `nats.tls.ca_certificate`, `nats.subjects_allowed[]`) | OrganisationProfile.md |

### Libraries

| Library | Version | Use |
|---|---|---|
| `async-nats` | 0.50.0, features `websockets` (C-53 lab used `wss://`) | client, JetStream |
| `tokio` | workspace | tasks, `watch`, `Notify` |
| `tokio-util` | 0.7.18 | `CancellationToken` (added to 05 under decision D2) |
| `rand` | 0.9 | backoff jitter (added to 05 under decision D2) |
| `zeroize` | as used by Identity | credential bytes wiped after use |
| `async-trait` | workspace | `NatsEventSink` as a trait object (added to 05 under decision D2) |

## Public Interface

Error prefix: **`NATS_`** (*chosen here*).

```rust
// crate ordin-nats, module root

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::{watch, Notify};
use tokio_util::sync::CancellationToken;
use ordin_types::{Timestamp, TriggerId};

/// Settings read from the organisation profile at start and on each profile change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NatsSettings {
    /// Each `tls://host:port` or `wss://host[:port][/path]`; 0–16 entries (assumed here: a cluster seed list rarely exceeds 16).
    pub servers: Vec<String>,
    /// PEM text of one or more CA certificates, at most 64 KiB (assumed here).
    pub ca_certificate_pem: Option<String>,
    /// Subjects with `*` and `>` wildcards; 0–200 entries, each 1–255 bytes (assumed here).
    pub subjects_allowed: Vec<String>,
}

impl NatsSettings {
    /// Builds and validates settings from the profile view; `Err(NATS_SERVER_URL_INVALID)`
    /// or `Err(NATS_SUBJECT_INVALID)` names the first bad entry.
    /// The `nats` section is never absent (OrganisationProfile.md); empty `servers` → event triggers off.
    pub fn from_profile(snapshot: &ordin_organisation_profile::ProfileSnapshot) -> Result<Self, NatsError>;
    /// True when `subject` (no wildcards) or `pattern` (with wildcards) lies within one entry of
    /// `subjects_allowed` by NATS token rules (step B.2).
    pub fn subject_allowed(&self, subject: &str) -> bool;
}

/// Connection state, observable by the Trigger Service (for "Not connected" and the 10-minute notice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// No servers configured (event triggers off).
    Off,
    /// No credentials in the secret store; nothing is attempted until `reload` is called.
    NoCredentials,
    Connecting { since: Timestamp },
    Connected { server: String, since: Timestamp, max_payload: u32 },
    Reconnecting { since: Timestamp, attempts: u32 },
    /// The server answered with an authorisation violation or expired credentials.
    CredentialsRefused { since: Timestamp },
}

/// Free places of one trigger: the Trigger Service increments and decrements it;
/// the adapter fetches no more than `free()` messages.
#[derive(Debug, Default)]
pub struct FeedCapacity { free: AtomicU32, changed: Notify }

impl FeedCapacity {
    pub fn new(initial_free: u32) -> Arc<Self>;
    pub fn free(&self) -> u32;
    /// Sets the free places and wakes the feed's fetch loop.
    pub fn set_free(&self, free: u32);
}

/// What a feed listens to.
#[derive(Debug, Clone)]
pub struct FeedSpec {
    pub trigger_id: TriggerId,
    /// The trigger's subject, within `subjects_allowed`; may contain wildcards.
    pub subject: String,
    pub capacity: Arc<FeedCapacity>,
    /// The delivery the Trigger Service stored for this trigger (`Trigger.delivery`), if any;
    /// used to report the feed's kind while disconnected.
    pub known_delivery: Option<ordin_types::NatsDelivery>,
}

/// How the feed was opened; the Trigger Service stores it as `Trigger.delivery` (2C `NatsDelivery`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedKind {
    JetStream { stream: String, consumer_name: String },
    CoreNats,
}

impl FeedKind {
    pub fn to_delivery(&self) -> ordin_types::NatsDelivery;
}

/// One received message. The payload is not parsed here.
#[derive(Debug)]
pub struct NatsEvent {
    pub trigger_id: TriggerId,
    pub subject: String,
    /// Value of the `Nats-Msg-Id` header, if present (1–256 chars; longer values are cut to none).
    pub msg_id: Option<String>,
    /// Up to 64 headers, each name ≤ 128 bytes and value ≤ 1024 bytes (assumed here); others dropped.
    pub headers: Vec<(String, String)>,
    /// Empty when `payload_check` is `TooLarge`.
    pub payload: bytes::Bytes,
    pub payload_check: PayloadCheck,
    pub received_at: Timestamp,
    /// JetStream delivery count > 1.
    pub redelivered: bool,
    pub ack: AckHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadCheck { WithinLimit, TooLarge { bytes: u64, limit: u32 } }

/// Acknowledges one message. For core NATS every call is a no-op returning `Ok(())`.
#[derive(Debug)]
pub struct AckHandle { /* private: Option<async_nats::jetstream::Message> */ }

impl AckHandle {
    pub fn is_jetstream(&self) -> bool;
    /// Final, explicit acknowledgement (`+ACK`). Consumes the handle.
    pub async fn ack(self) -> Result<(), NatsError>;
    /// Keeps a queued message from redelivery (`+WPI`); called every 20 s while it waits.
    pub async fn in_progress(&self) -> Result<(), NatsError>;
}

/// Implemented by the Trigger Service.
#[async_trait::async_trait]
pub trait NatsEventSink: Send + Sync + 'static {
    /// Called once per received message, in arrival order per feed. Must not block for longer
    /// than it takes to record the event; queuing happens inside the sink.
    async fn deliver(&self, event: NatsEvent);
}

/// An open feed; dropping it stops its fetch loop without deleting the consumer.
#[derive(Debug)]
pub struct FeedHandle { pub trigger_id: TriggerId, pub kind: FeedKind, /* private: stop token */ }

impl FeedHandle {
    pub fn close(self);
}

#[derive(Debug, Clone)]
pub struct NatsAdapter { /* private: Arc<Inner> */ }

impl NatsAdapter {
    /// Builds the adapter; makes no network call.
    pub fn new(
        settings: NatsSettings,
        secrets: Arc<ordin_identity::SecretStore>,
        data_dir: std::path::PathBuf,
    ) -> Self;

    /// Starts the connection task. Returns at once; the state moves through `watch_state`.
    /// With empty `servers` the state is `Off` and nothing is started.
    pub async fn start(&self, shutdown: CancellationToken) -> Result<(), NatsError>;

    /// Replaces settings (profile changed) or re-reads credentials (setup command changed them):
    /// closes the connection, keeps feed specifications, reconnects and reopens every feed.
    pub async fn reload(&self, settings: NatsSettings) -> Result<(), NatsError>;

    pub fn state(&self) -> ConnectionState;
    pub fn watch_state(&self) -> watch::Receiver<ConnectionState>;

    /// The payload ceiling in force: min(1_048_576, server's `max_payload`); 1_048_576 before
    /// the first connection.
    pub fn payload_limit(&self) -> u32;

    pub fn subject_allowed(&self, subject: &str) -> bool;

    /// Opens (or reopens after restart) the feed of one trigger. Looks up the stream holding
    /// the subject; creates or binds the durable consumer `ordin_<trigger id, 32 hex>`; or
    /// subscribes on core NATS. Works while disconnected: the feed is recorded and opened on connect.
    pub async fn open_feed(
        &self,
        spec: FeedSpec,
        sink: Arc<dyn NatsEventSink>,
    ) -> Result<FeedHandle, NatsError>;

    /// Deletes the durable consumer of a removed trigger (no-op for core NATS).
    pub async fn delete_feed(&self, trigger_id: TriggerId, subject: &str) -> Result<(), NatsError>;

    /// The consumer name this adapter uses for a trigger: `ordin_` + the UUID's 32 hex digits.
    pub fn consumer_name_for(trigger_id: TriggerId) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NatsError {
    #[error("NATS_SERVER_URL_INVALID")] ServerUrlInvalid { index: usize },
    #[error("NATS_SUBJECT_INVALID")] SubjectInvalid { index: usize },
    #[error("NATS_CA_INVALID")] CaInvalid,
    #[error("NATS_NOT_CONFIGURED")] NotConfigured,
    #[error("NATS_NO_CREDENTIALS")] NoCredentials,
    #[error("NATS_CREDENTIALS_MALFORMED")] CredentialsMalformed,
    #[error("NATS_SECRET_STORE_UNAVAILABLE")] SecretStoreUnavailable,
    #[error("NATS_SUBJECT_NOT_ALLOWED")] SubjectNotAllowed,
    #[error("NATS_NOT_CONNECTED")] NotConnected,
    #[error("NATS_STREAM_LOOKUP_FAILED")] StreamLookupFailed,
    #[error("NATS_CONSUMER_FAILED")] ConsumerFailed,
    #[error("NATS_SUBSCRIBE_FAILED")] SubscribeFailed,
    #[error("NATS_ACK_FAILED")] AckFailed,
}

impl NatsError {
    pub fn code(&self) -> &'static str;
    pub fn retryable(&self) -> bool;
}
```

**Called by.**

| Item | Caller |
|---|---|
| `NatsSettings::from_profile`, `new`, `start` | Ordin Server program, personal profile, at start |
| `reload` | Ordin Server program: after the setup command stores a NATS secret on a running server (`setup secret nats-credentials`, `nats-user-password` or `nats-token`, each written as the one secret `nats/credentials` with the matching `kind`), and on each Organisation Profile change (rows 494, 495) |
| `state`, `watch_state`, `subject_allowed`, `open_feed`, `delete_feed`, `FeedHandle::close`, `FeedCapacity`, `AckHandle` | Trigger Service |
| `payload_limit`, `consumer_name_for` | this crate only (Internal Logic D.3, D.6 and F.3); no other component calls them |
| `NatsEventSink` (implemented, not called) | implemented by the Trigger Service; called by this crate |


### Example

Settings from the profile and one feed:

```rust
let settings = NatsSettings {
    servers: vec!["tls://nats-1.northwind.example:4222".into(), "wss://nats.northwind.example/ws".into()],
    ca_certificate_pem: None,
    subjects_allowed: vec!["finance.invoice.*".into(), "hr.>".into()],
};
assert!(settings.subject_allowed("finance.invoice.created"));
assert!(!settings.subject_allowed("finance.payment.created"));

let cap = FeedCapacity::new(11); // 1 run + 10 waiting, all free
let handle = adapter.open_feed(FeedSpec {
    trigger_id: TriggerId("0192a1d0-1234-7abc-8def-0123456789ab".parse()?),
    subject: "finance.invoice.created".into(),
    capacity: cap.clone(),
}, sink).await?;
assert_eq!(handle.kind, FeedKind::JetStream {
    stream: "FINANCE".into(),
    consumer_name: "ordin_0192a1d012347abc8def0123456789ab".into(),
});
```

Delivered event (as the sink receives it):

```text
NatsEvent {
  trigger_id: 0192a1d0-1234-7abc-8def-0123456789ab,
  subject: "finance.invoice.created",
  msg_id: Some("inv-2291-created"),
  headers: [("Nats-Msg-Id","inv-2291-created"), ("Content-Type","application/cloudevents+json")],
  payload: b"{\"specversion\":\"1.0\",\"id\":\"9f1c…\",\"type\":\"invoice.created\",\"source\":\"/ledger\",\"data\":{\"number\":\"INV-2291\",\"customer\":\"Northwind Group\"}}",
  payload_check: WithinLimit,
  received_at: 2026-01-10T08:14:03.120Z,
  redelivered: false,
  ack: AckHandle(jetstream),
}
```

## Internal Logic

**A. Settings**

1. `NatsSettings::from_profile` reads `snapshot.document.nats` (never absent; `servers` empty means off). Each server URL must parse with `url::Url`, have scheme `tls` or `wss`, a host, and for `tls` a port (default `4222` added when absent); any other scheme (including `nats://` and `ws://`) gives `NATS_SERVER_URL_INVALID { index }` and the whole settings value is refused; the caller (program start) logs `nats.settings.refused` with the code and index, and the adapter runs with `servers` empty (state `Off`). (assumed here: an unencrypted server breaks ADR-074's "connects outbound over TLS".)
2. Each subject in `subjects_allowed` must be 1–255 bytes of tokens separated by `.`, each token non-empty and free of whitespace; `>` only as the last token; else `NATS_SUBJECT_INVALID { index }` with the same refusal as step 1.
3. A CA PEM that does not parse as at least one certificate gives `NATS_CA_INVALID`; the settings are refused as in step 1.

**B. Subject check**

1. `subject_allowed(s)`: split `s` and each allowed pattern on `.`. `s` is allowed when some pattern matches it by NATS rules: `*` matches exactly one token, `>` matches one or more remaining tokens, any other token matches only itself. When `s` itself contains wildcards (a trigger subject such as `finance.invoice.*`), it is allowed only when every subject it can match is matched by one pattern: token by token, a `*` in `s` needs `*` or `>` in the pattern, a `>` in `s` needs `>` in the pattern at the same position.
2. Used by `open_feed` (step D.1) as a second line of defence; the Trigger Service applies the same check first.

**C. Connection**

1. `start`: when `servers` is empty set `Off` and return. Read `SecretStore::get(&SecretName::new("nats/credentials"))`. `Ok(None)` → state `NoCredentials`, log `nats.credentials.missing` (no value), return `Ok(())`. `Err` → `NATS_SECRET_STORE_UNAVAILABLE` (retryable): log, retry the read after 30 s up to the shutdown. Parse the bytes as UTF-8 JSON; any other shape → `NATS_CREDENTIALS_MALFORMED`, state `NoCredentials`.
2. Build `async_nats::ConnectOptions`:
   - credentials: `creds_file` → `ConnectOptions::credentials(contents)`; `user_password` → `ConnectOptions::with_user_and_password(user, password)`; `token` → `ConnectOptions::with_token(token)`. The secret is exposed only for this call and dropped afterwards.
   - `.require_tls(true)` when every server is `tls://`.
   - when `ca_certificate_pem` is set: write it to `<ORDIN_DATA_DIR>/nats/ca.pem` with mode `0600` (directory `0700`), then `.add_root_certificates(path)`; the operating system's roots stay in use (C-53).
   - `.retry_on_initial_connect()`; `.max_reconnects(None)` (unlimited).
   - `.reconnect_delay_callback(|attempts| delay(attempts))` with `delay(n) = clamp(min(60, 2^(n−1)) × U(0.8, 1.2), 1, 60)` seconds, `U` uniform random (2D-4: 1 to 60 s, exponential with jitter).
   - `.connection_timeout(10 s)`, `.ping_interval(60 s)` (assumed here: `async-nats` defaults, stated so a change is deliberate).
   - `.event_callback(on_event)` (step C.3).
   - `.name("ordin-server")`.
3. `on_event` maps client events to state, each with one log line:
   - `Connected` → `Connected { server, since: now, max_payload: client.server_info().max_payload }`; log `nats.connected` with `server`; then step D.5 (reopen feeds).
   - `Disconnected` → `Reconnecting { since: <first disconnect time>, attempts }`; log `nats.disconnected`.
   - `ServerError(AuthorizationViolation)` and any server error whose text contains `Authorization Violation` or `expired` → `CredentialsRefused { since }`; log `nats.credentials.refused` (no credential material). Reconnection continues with the same backoff; the state returns to `Connected` if a later attempt succeeds.
   - `LameDuckMode` → log `nats.lame_duck` only; the client moves to another server.
   - `SlowConsumer(sid)` → log `nats.slow_consumer` with the subscription id.
4. `async_nats::connect_with_options` is awaited in a task; with `retry_on_initial_connect` it returns at once and the client connects in the background. The task holds the `Client` in `Inner` and exits on `shutdown`, calling `client.drain()` with a 5 s limit.

**D. Feeds**

1. `open_feed(spec, sink)`: `subject_allowed(spec.subject)` false → `Err(NATS_SUBJECT_NOT_ALLOWED)`. Record `spec` and `sink` in `Inner.feeds` (keyed by `trigger_id`). When the state is not `Connected`, return a `FeedHandle` whose `kind` is derived from `spec.known_delivery` (`JetStream { consumer_name }` with `stream` empty until connected; `CoreNats` when `None` or `CoreNats`) and open it on connect (step D.5).
2. Stream lookup: `jetstream::new(client).stream_by_subject(&spec.subject)`.
   - `Ok(stream_name)` → JetStream feed (step D.3).
   - error of kind "no stream matches subject" → core NATS feed (step D.4).
   - any other error → `Err(NATS_STREAM_LOOKUP_FAILED)` (retryable); the feed stays recorded and the open is retried on the next `Connected` event and every 60 s.
3. JetStream: `stream.get_or_create_consumer(name, pull::Config { durable_name: Some(name), filter_subject: spec.subject, ack_policy: AckPolicy::Explicit, ack_wait: 60 s, max_ack_pending: 11, deliver_policy: DeliverPolicy::New, max_deliver: -1, ..Default })` with `name = consumer_name_for(trigger_id)`. (assumed here: `DeliverPolicy::New` — a newly turned-on trigger starts from the events published after **Turn on**, not from the stream's history; `max_ack_pending: 11` = one starting plus ten waiting, ADR-074 *Limits*.) On error → `NATS_CONSUMER_FAILED` (retryable). Start the fetch loop (step E). The returned `FeedKind` is what the Trigger Service stores as `Trigger.delivery`.
4. Core NATS: `client.subscribe(spec.subject)`; error → `NATS_SUBSCRIBE_FAILED` (retryable). Start the subscription loop (step E.4). A core-NATS feed does not read `capacity`: every message is delivered, and the Trigger Service refuses the surplus (ADR-074: "with core NATS the rest are refused and counted").
5. On each `Connected` event, every recorded feed whose loop is not running is opened again by steps D.2 to D.4; JetStream consumers resume from their stored position, so events published while the server was down are read after it restarts (ADR-074 *Delivery*).
6. `delete_feed(trigger_id, subject)`: stop its loop; look up the stream for `subject` (step D.2); when one exists call `stream.delete_consumer(consumer_name_for(trigger_id))` (a "not found" answer counts as success); with no stream nothing is deleted. While disconnected → `Err(NATS_NOT_CONNECTED)` (retryable); the Trigger Service retries on its next start.

**E. Fetch loops**

1. JetStream loop, per feed, until the feed is closed or shutdown: `n = capacity.free()`. When `n == 0`, await `capacity.changed` (or 30 s). Otherwise `consumer.fetch().max_messages(n).expires(30 s).messages().await` and iterate.
2. For each message: build `NatsEvent` (step F) and `sink.deliver(event).await`. The loop does not acknowledge; the Trigger Service does.
3. A fetch error is logged `nats.fetch.failed` with the error kind; the loop waits 1 s, then 2 s, doubling to 60 s across consecutive failures, and resets after a successful fetch.
4. Core loop: `while let Some(msg) = subscriber.next().await` → step F → `sink.deliver`. When the subscription ends because the connection dropped, the loop exits and step D.5 reopens it.

**F. Building an event**

1. `msg_id`: header `Nats-Msg-Id`, kept when 1–256 chars; otherwise `None`.
2. `headers`: the first 64 headers within the size bounds in the Public Interface, in received order.
3. Payload ceiling: `limit = payload_limit()`. When `payload.len() > limit`, set `payload_check = TooLarge { bytes, limit }` and `payload = Bytes::new()`; log `nats.payload.too_large` with `trigger_id`, `bytes`, `limit`.
4. `redelivered`: JetStream `info().delivered > 1`; core NATS `false`.
5. `received_at`: now (UTC, ms).

**G. Acknowledgement**

1. `ack(self)`: JetStream → `message.ack_with(AckKind::Ack).await`; core → `Ok(())`. Failure → `NATS_ACK_FAILED` (retryable): the caller logs it and does nothing else — the event has already been recorded, so the redelivery is caught as a duplicate by the Trigger Service (ADR-074 *Duplicates*).
2. `in_progress(&self)`: JetStream → `ack_with(AckKind::Progress)`; core → `Ok(())`.

**H. Nothing is published.** The crate exposes no publish method, and the program never hands its `Client` to another crate (ADR-074 *Not in this step*, OQ-41).

## Data Model

N/A — this crate owns no table. Its dependencies (Identity, Organisation Profile) do not include the Journal, which owns the datastore (2B-1 rule 5), so it cannot register migrations. The durable consumer's name is derived from the trigger identifier (`consumer_name_for`) and stored by the Trigger Service in `Trigger.delivery`; the connection state is held in memory and rebuilt at each start (02:646 *Owns*: "consumer names and connection state" — the naming rule and the live state).

## Error Table

In-process API; Status column gives the Rust error variant and whether the caller may retry.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A profile server URL is not `tls://` or `wss://`, or unparsable | `Err(permanent)` | `NATS_SERVER_URL_INVALID` | `NatsError::ServerUrlInvalid { index }` |
| A profile subject is malformed | `Err(permanent)` | `NATS_SUBJECT_INVALID` | `NatsError::SubjectInvalid { index }` |
| Profile CA PEM does not parse | `Err(permanent)` | `NATS_CA_INVALID` | `NatsError::CaInvalid` |
| `open_feed` called while `servers` is empty | `Err(permanent)` | `NATS_NOT_CONFIGURED` | `NatsError::NotConfigured` |
| No `nats.credentials` secret | state `NoCredentials`; `Err(permanent)` from `open_feed` only after connect is attempted | `NATS_NO_CREDENTIALS` | `NatsError::NoCredentials` |
| Credentials secret not in one of the three shapes | `Err(permanent)` | `NATS_CREDENTIALS_MALFORMED` | `NatsError::CredentialsMalformed` |
| Secret store unreadable | `Err(retryable)` | `NATS_SECRET_STORE_UNAVAILABLE` | `NatsError::SecretStoreUnavailable` |
| Trigger subject outside `subjects_allowed` | `Err(permanent)` | `NATS_SUBJECT_NOT_ALLOWED` | `NatsError::SubjectNotAllowed` |
| `delete_feed` while not connected | `Err(retryable)` | `NATS_NOT_CONNECTED` | `NatsError::NotConnected` |
| Stream lookup fails for a reason other than "no stream" | `Err(retryable)` | `NATS_STREAM_LOOKUP_FAILED` | `NatsError::StreamLookupFailed` |
| Creating or binding the durable consumer fails | `Err(retryable)` | `NATS_CONSUMER_FAILED` | `NatsError::ConsumerFailed` |
| Core subscription fails | `Err(retryable)` | `NATS_SUBSCRIBE_FAILED` | `NatsError::SubscribeFailed` |
| `+ACK` or `+WPI` not accepted | `Err(retryable)` | `NATS_ACK_FAILED` | `NatsError::AckFailed` |
| Server refuses credentials | state `CredentialsRefused` (no error returned; reconnection continues) | `NATS_CREDENTIALS_REFUSED` (log code) | — |
| Payload above the ceiling | event delivered with `PayloadCheck::TooLarge` | `NATS_PAYLOAD_TOO_LARGE` (log code) | — |

The person never sees these codes. The words shown for a disconnected or refused connection ("Not connected", the 10-minute notice) belong to the Trigger Service (11 §5.24).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: NATS Adapter

  Background:
    Given the profile lists servers "tls://nats-1.test:4222"
    And the profile allows subjects "finance.invoice.*"
    And the secret "nats/credentials" holds a credentials file

  Scenario: Happy path — a JetStream event is delivered and acknowledged after recording
    Given stream "FINANCE" holds subject "finance.invoice.created"
    And a feed is opened for trigger T with 11 free places
    When an event with header "Nats-Msg-Id: inv-2291-created" is published on "finance.invoice.created"
    Then the sink receives one NatsEvent with msg_id "inv-2291-created" and payload_check WithinLimit
    And the message is not acknowledged until the sink calls ack
    When the sink calls ack
    Then the consumer's pending count for T is 0

  Scenario: Edge — no free places means nothing is pulled
    Given a feed for trigger T with 0 free places
    When 5 events are published on "finance.invoice.created"
    Then the sink receives no event
    And the stream still holds 5 unacknowledged messages for consumer "ordin_<T hex>"
    When the capacity is set to 2
    Then the sink receives exactly 2 events

  Scenario: Edge — core NATS subject with no stream
    Given no stream holds subject "finance.invoice.voided"
    And the profile allows "finance.invoice.*"
    When a feed is opened for "finance.invoice.voided"
    Then the feed kind is CoreNats
    And every published event is delivered regardless of free places
    And ack returns Ok without contacting the server

  Scenario: Edge — a queued message is kept alive with in-progress acknowledgements
    Given a JetStream feed with ack_wait 60 seconds
    And the sink holds an event unacknowledged for 150 seconds, calling in_progress every 20 seconds
    Then the event is not redelivered

  Scenario: Edge — crash before acknowledgement redelivers
    Given the sink received an event and the program stopped before ack
    When the program starts and the feed is reopened
    Then the same event is delivered again with redelivered true

  Scenario: Edge — payload above the ceiling
    Given the server's max_payload is 8388608
    When a 2 MiB event is published on "finance.invoice.created"
    Then the sink receives an event with payload_check TooLarge { bytes: 2097152, limit: 1048576 } and an empty payload

  Scenario: Error — subject outside the profile's list
    When a feed is opened for "hr.payroll.run"
    Then open_feed returns NATS_SUBJECT_NOT_ALLOWED
    And no subscription or consumer is created

  Scenario: Error — credentials refused
    Given the server rejects the credentials with an authorization violation
    When the adapter starts
    Then the state becomes CredentialsRefused
    And reconnection attempts continue with delays between 1 and 60 seconds

  Scenario: Error — server unreachable then back
    Given the server is unreachable
    When the adapter starts
    Then the state is Reconnecting with increasing attempts
    And no attempt waits less than 0.8 seconds or more than 60 seconds
    When the server becomes reachable
    Then the state becomes Connected and every recorded feed is reopened

  Scenario: Error — insecure server URL in the profile
    Given the profile lists servers "nats://nats-1.test:4222"
    When settings are built from the profile
    Then NATS_SERVER_URL_INVALID with index 0 is returned
    And the adapter state is Off

  Scenario: Removed trigger deletes its consumer
    Given a JetStream feed for trigger T
    When delete_feed(T, "finance.invoice.created") is called while connected
    Then consumer "ordin_<T hex>" no longer exists on stream "FINANCE"
```

## Performance, Security, Observability

- **Performance targets:** delivery from fetch to `sink.deliver` call p95 < 20 ms (excluding the sink); memory per feed < 64 KiB plus in-flight payloads (at most 11 × 1 MiB per JetStream feed); up to 100 feeds per installation (assumed here: the daily limit of 200 runs makes more triggers pointless); reconnect attempts never closer than 0.8 s.
- **Security:** outbound only, TLS (`tls://`) or TLS WebSocket (`wss://`), no plain transport; trust = operating-system roots plus the profile's CA; credentials only from the server's secret store, never from the chat, the environment or a file the person writes (ADR-074 *Connection*); the CA file is `0600`; credential values, payloads and header values are never logged (`msg_id` is logged only as its SHA-256 first 12 hex chars); subjects are checked against the profile's list twice (Trigger Service, then here); nothing is published.
- **Observability (logs, `tracing`, target `ordin_nats`):** `nats.connected {server}`, `nats.disconnected {attempts}`, `nats.credentials.missing`, `nats.credentials.refused`, `nats.lame_duck {server}`, `nats.slow_consumer {sid}`, `nats.settings.refused {code, index}`, `nats.feed.opened {trigger_id, kind, stream?, consumer_name?}`, `nats.feed.failed {trigger_id, code}`, `nats.feed.deleted {trigger_id}`, `nats.fetch.failed {trigger_id, error_kind}`, `nats.payload.too_large {trigger_id, bytes, limit}`, `nats.ack.failed {trigger_id, msg_id_digest}`. No metrics or spans on the personal server (SA-XC-04/05). Agent profile: the adapter is `Off` there; no metric is defined.

## Gaps

None.
