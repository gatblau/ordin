# SPEC: Server Connector

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-server-connector` · **Build phase:** 2 · **Programs:** D
> **Dependencies:** Service API, Identity, Journal
> **Dependency correction proposed here:** + Journal (see Approach)

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

## Purpose

The Server Connector is the desktop's side of the link to the person's own ordin servers (`02-architecture.md:588-596`, ADR-096). It pairs the desktop with a server from **Settings › Servers › Connect to a server…**, keeps each desktop key in the keychain, sends the commands the screens address to a server's backend to that server's Remote API, holds one Server-Sent Events stream per server and hands its events to the Command Bridge in order, marks a server **Not connected** after 45 seconds of silence and reconnects with backoff while ordin is open, catches up with `Last-Event-ID`, refuses a server whose Service API version it cannot use, and ends or forgets a pairing when the person asks. It owns the desktop's `server_connections` records.

## Approach

One `reqwest::Client` per server carries both the event stream and the commands over one HTTP/2 connection (ADR-096 item 3), with the computer's trusted certificates through `rustls-platform-verifier`, or a pinned leaf-certificate fingerprint when the person pasted a pairing line (OQ-61). A supervisor task per server runs the connection state machine of 2C's `ServerConnection` (connected, not connected, disconnected, refused for version). While a server is not connected, read commands are answered from the last replies received and change commands are refused, which is how "keeps the last state received, read-only" is met without merging anything into the desktop's own workspaces (OQ-39). The crate stores its rows in the desktop's datastore, which the Journal crate owns (2B-1 rule 5), so it needs `ordin-journal` for the `Datastore` handle, its migrations and `Journal::append_event`, through which it writes `server_state_changed` (2B-3 lists the edge since the January 2026 amendment). Error prefix `CONN_` *(chosen here)*; the 2C field rules of `ServerConnection` use it too (`CONN_ADDRESS_INVALID`, `CONN_PORT_OUT_OF_RANGE`, `CONN_DUPLICATE`; moved from `SRV_`, which belongs to wrightrun's carried codes and the Remote API's routing refusals, ErrorHandling EH-D7). Rejected: *a WebSocket for events* — ADR-096 chose SSE; *persisting the read cache to disk* — the HLD keeps the last state "while ordin is open" (`02-architecture.md:589`), and a disk copy of a server's documents on the desktop is what OQ-39 rules out.

### Reconciliation assumptions

- **SA-RECON-01** The desktop program starts the Connector once with `start(events, cancel)` and stops it by cancelling that token; the Connector offers no `attach_events`, `connect_all` or `close` (reconciliation rows 155, 167, 168; N22, N23; ruling R6). It adds `on_wake`, which the program calls on the operating system's wake notice so that every server supervisor ends its backoff wait and reconnects at once (Internal Logic step 11).
- **SA-RECON-02** `ServerView` gains `older_side: Option<OlderSide>`, filled by this crate from the stored server version (row 170, N36), so the screens pick the right 11 §5.31 sentence after a restart as well as at connect time.
- **SA-RECON-03** Desktop keys are stored through Identity's `DesktopKeyStore` (implemented by `IdentityService`, errors `CredError`) under `keychain_ref` = `server-connector/desktop-key/<server_connection_id>`, the secret-name form Identity fixes (rows 171, N29, N142).
- **SA-RECON-04** The Remote API's `DESK_TOO_MANY` and `DESK_NAME_INVALID` refusals map to their own `CONN_` codes instead of `BadAnswer` (row 165, O40).

## Shared Context

### Entity owned (2C part 1, `ServerConnection`, copied)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServerConnection {
    pub id: ServerConnectionId,
    pub display_name: String,
    pub address: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_api_version: Option<String>,
    pub last_sequence_received: u64,
    /// Keychain item holding the desktop key; the key never enters the database.
    pub keychain_ref: String,
    pub state: ServerConnectionState,
    pub paired_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerConnectionState { Connected, NotConnected, Disconnected, RefusedForVersion }
```

| Field | Rule | Error code |
|---|---|---|
| address | host name ≤ 253 chars or IP literal | `CONN_ADDRESS_INVALID` |
| port | 1025–65535 | `CONN_PORT_OUT_OF_RANGE` |
| (address, port) | unique | `CONN_DUPLICATE` |
| display_name | 1–120 chars; taken from `PairReply.server_name`, so a name out of range is the server's bad answer (step 3.5) | `CONN_BAD_ANSWER` |

| From | Event | To |
|---|---|---|
| — | pairing succeeds | connected |
| connected | 45 seconds without event or heartbeat | not_connected |
| not_connected | stream resumes | connected |
| connected, not_connected | server reports a Service API version this desktop cannot use | refused_for_version |
| connected, not_connected | person ends it, or the server refuses the key | disconnected |
| any | **Remove this server** | (row deleted) |

### Service API types used (from `ordin-service-api`)

- `ServiceApiVersion { major, minor }`, `CURRENT = 1.0`, `desktop_accepts(desktop, server)`: same major and the server's minor equal to or one below the desktop's.
- `EventEnvelope { api_version, backend, seq, at, kind…flattened ServiceEvent }`; `ServiceEvent::Reset`; `ServiceEvent::ServerStateChanged { server_connection_id, state, as_of: Option<Timestamp> }`; `ServiceEvent::to_journal_data()`; `EventKind::ServerStateChanged`.
- `CommandMeta { name, rest_path, effect: Effect::{Read, Change}, … }`.
- Pairing types (`ordin_service_api::pairing`): `PairRequest { api_version, code, computer_name }`, `PairReply { desktop_pairing_id, desktop_key, backend, server_name, api_version }`, `PairingLine { host, port, fingerprint: Sha256Hex, code }` with `parse`/`format`, `normalise_code`. Code alphabet `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`, 8 symbols.
- Command catalogue rows this crate implements (scope `Local`): `servers.list` (`Done` → `Vec<ServerView>`), `servers.connect` (`ConnectServer` → `ServerView`), `servers.disconnect` (`ServerRef` → `Done`), `servers.remove` (`ServerRef` → `Done`), where

```rust
pub struct ServerView {
    pub server_connection_id: ServerConnectionId,
    pub display_name: String,
    pub state: ServerConnectionState,
    #[serde(skip_serializing_if = "Option::is_none")] pub as_of: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")] pub backend: Option<BackendId>,
    /// Present only when `state` is `refused_for_version`: which side must be updated
    /// (picks one of the two 11 §5.31 sentences). Service API type field (ServiceApi.md `ServerView`, `OlderSide`).
    #[serde(skip_serializing_if = "Option::is_none")] pub older_side: Option<OlderSide>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OlderSide { Desktop, Server }
#[serde(tag = "kind")]
pub enum ConnectServer { AddressAndCode { address: String, code: String }, PairingLine { line: String } }
pub struct ServerRef { pub server_connection_id: ServerConnectionId }
```

### Remote API routes called (from the Remote API spec)

| Route | Auth | Body → reply |
|---|---|---|
| `POST /v1/pairing` | none | `PairRequest` → `{"kind":"ok","result":PairReply}` or `{"kind":"error","error":ServiceError}`; status 200, 403 (`DESK_CODE_INVALID`, `DESK_CODE_EXPIRED`, `DESK_PAIRING_STOPPED`, `DESK_TOO_MANY`), 409 (`SVC_VERSION_UNSUPPORTED`), 422 (`SVC_INPUT_INVALID`, `DESK_NAME_INVALID`), 429 (`DESK_ADDRESS_LOCKED`) |
| `POST /v1/pairing/end` | bearer | `{}` → `{"kind":"ok","result":{}}` |
| `GET /v1/events` | bearer | `Last-Event-ID` → `text/event-stream`: `id:` seq, `event:` kind, `data:` envelope JSON; `: heartbeat` every 20 s |
| `POST /v1/<module>/<verb>` | bearer | `CommandEnvelope` → `CommandReply`; 200, 401 `SVC_UNAUTHENTICATED`, 404, 409, 422, 429 `SVC_RATE_LIMITED` / `DESK_ADDRESS_LOCKED`, 500, 503 |

Every response carries `Ordin-Api-Version: <major>.<minor>`.

### Identity and Credentials interface used (from the Identity spec; implemented by `IdentityService`)

```rust
#[async_trait::async_trait]
pub trait DesktopKeyStore: Send + Sync {
    /// Stores the 32-byte desktop key in the keychain under `keychain_ref`; replaces any value.
    async fn put_desktop_key(&self, keychain_ref: &str, key: &[u8; 32]) -> Result<(), CredError>;
    async fn get_desktop_key(&self, keychain_ref: &str) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, CredError>;
    async fn delete_desktop_key(&self, keychain_ref: &str) -> Result<(), CredError>;
    /// The client certificate named by the organisation profile, from the computer's store,
    /// as a rustls signing identity; `None` when the platform does not expose its key (SA-HTTP-02).
    async fn client_certificate(&self, name: &str) -> Result<Option<std::sync::Arc<rustls::sign::CertifiedKey>>, CredError>;
}
```

### Journal interface used (required from the Journal spec)

(From the Journal and Artefact Store spec.) `Datastore` (`Datastore::sqlite() -> Option<&sqlx::SqlitePool>` on the desktop), `ModuleMigrations { module, sqlite: &[(i64, &str, &str)], postgres: &[] }` registered by the program with `MigrationRegistry::register`, and `Journal::append_event(kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>` (assigns the seq; refuses `data` over 256 KiB).

### Constants (2D-4, ADR-096, SA-NET-01)

| Constant | Value |
|---|---|
| Not connected after | 45 s without an event or heartbeat |
| Reconnect backoff | 1 s, doubling, at most 60 s, never giving up while ordin is open |
| Remote API transport | HTTP/2 over TLS 1.3, prefix `/v1/` |
| Default port when the person types none | 8443 |
| Desktop key | 32 random bytes |
| Organisation profile keys (read by the desktop program and passed in) | `server.client_certificate_required` (bool), `server.client_certificate_name` (string) |

Libraries: `reqwest` 0.13.5 with rustls and `rustls-platform-verifier` 0.7.1 (SA-HTTP-02), `sse-stream` 0.3.0 (SA-HTTP-02), `rustls` 0.23.46, `sha2` 0.10 (already in `memory/Cargo.toml` and `sandbox/sandbox-host/Cargo.toml`), `zeroize` (transitive through rustls) *(both named in HLD 05, row Utility and cryptography crates)*, `sqlx` 0.9.0 (SA-DB-01), `tokio`, `tracing`.

### Words shown to the person (11 §5.31, §6; this crate owns their codes)

- Wrong or expired code: "That code didn't work. Ask for a new one from whoever runs your ordin server."
- Unreachable: "Can't reach [server] right now. ordin will keep trying while it's open."
- Not connected header: "Not connected · as of 10:42"; disabled buttons: "Waiting for [server]".
- Key refused: "This computer is no longer connected to [server]." with **Connect again…**.
- Version: "[server] runs a different version of ordin. Ask whoever runs it to update it." or "Update ordin on this computer to use [server]."
- Never shown: port numbers, keys, certificates, the words *API*, *stream*, *tunnel*, *pairing*.

## Public Interface

```rust
pub struct ConnectorConfig {
    /// This computer's name, sent when pairing (1–63 characters).
    pub computer_name: String,
    /// From the organisation profile (`server.client_certificate_required`, `…_name`).
    pub client_certificate: Option<String>,
    /// Upper bound of cached read replies per server; default 32 MiB.
    pub read_cache_bytes: usize,
}

pub struct ServerConnector { /* private */ }

impl ServerConnector {
    /// This crate's migrations (module `server_connector`); the program registers them.
    pub fn migrations() -> ModuleMigrations;

    pub fn new(
        cfg: ConnectorConfig,
        journal: ordin_journal::Journal,          // for `datastore()` and `append_event`
        keys: std::sync::Arc<dyn DesktopKeyStore>,
    ) -> Result<ServerConnector, ConnectorError>;

    /// Loads every connection and starts one supervisor per server not `disconnected` or
    /// `refused_for_version`. Server events go to `events` (the Command Bridge's channel).
    /// Called once; a second call returns `AlreadyStarted`. Cancelling `cancel` is the quit path:
    /// every stream closes and the states are persisted (step 11); there is no separate `close`.
    pub async fn start(&self, events: ServerEventSender, cancel: tokio_util::sync::CancellationToken)
        -> Result<(), ConnectorError>;

    /// The computer woke from sleep (or its network came back): every supervisor in
    /// `not_connected`, or waiting in backoff, skips the rest of its wait and reconnects now.
    /// Supervisors in `connected` drop a stream silent for more than 20 s and reconnect.
    /// Does nothing before `start` or after `cancel`. Never fails.
    pub fn on_wake(&self);

    // ---- the servers.* commands ----
    pub async fn list(&self) -> Result<Vec<ServerView>, ConnectorError>;
    pub async fn connect(&self, input: ConnectServer) -> Result<ServerView, ConnectorError>;
    pub async fn disconnect(&self, id: ServerConnectionId) -> Result<(), ConnectorError>;
    pub async fn remove(&self, id: ServerConnectionId) -> Result<(), ConnectorError>;

    // ---- used by the Command Bridge ----
    pub fn connection_for(&self, backend: BackendId) -> Option<ServerConnectionId>;
    /// The server's display name, for "On [server]" in notifications (11 §7).
    pub fn display_name(&self, backend: BackendId) -> Option<String>;
    pub async fn send_command(
        &self,
        backend: BackendId,
        meta: &'static CommandMeta,
        envelope_json: bytes::Bytes,
        deadline: std::time::Instant,
    ) -> Result<bytes::Bytes, ConnectorError>;
}

pub enum ServerEvent {
    Event { backend: BackendId, envelope: std::sync::Arc<EventEnvelope> },
    Reset { backend: BackendId },
}
pub type ServerEventSender = tokio::sync::mpsc::Sender<ServerEvent>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectorError {
    #[error("server address invalid")] AddressInvalid,                 // CONN_ADDRESS_INVALID
    #[error("port out of range")] PortOutOfRange,                      // CONN_PORT_OUT_OF_RANGE
    #[error("already paired with this server")] Duplicate,            // CONN_DUPLICATE
    #[error("pairing line invalid")] PairingLineInvalid,               // CONN_PAIRING_LINE_INVALID
    #[error("code refused")] CodeRefused,                              // CONN_CODE_REFUSED
    #[error("pairing stopped on the server")] PairingStopped,          // CONN_PAIRING_STOPPED
    #[error("server has its most paired desktops")] TooManyDesktops,   // CONN_TOO_MANY_DESKTOPS
    #[error("computer name refused")] ComputerNameInvalid,             // CONN_COMPUTER_NAME_INVALID
    #[error("address locked by the server")] AddressLocked,            // CONN_ADDRESS_LOCKED
    #[error("server unreachable")] Unreachable,                        // CONN_UNREACHABLE
    #[error("certificate not trusted")] CertificateUntrusted,          // CONN_CERT_UNTRUSTED
    #[error("certificate fingerprint changed")] FingerprintMismatch,   // CONN_FINGERPRINT_MISMATCH
    #[error("client certificate unavailable")] ClientCertUnavailable,  // CONN_CLIENT_CERT_UNAVAILABLE
    #[error("server version unusable")] RefusedForVersion { server_older: bool }, // CONN_REFUSED_FOR_VERSION
    #[error("not connected")] NotConnected,                            // CONN_NOT_CONNECTED
    #[error("desktop key refused")] KeyRefused,                        // CONN_KEY_REFUSED
    #[error("server deadline")] Deadline,                              // CONN_DEADLINE
    #[error("server answer unreadable")] BadAnswer,                    // CONN_BAD_ANSWER
    #[error("unknown server")] UnknownServer,                          // CONN_UNKNOWN_SERVER
    #[error("keychain")] Keychain,                                     // CONN_KEYCHAIN
    #[error("datastore")] Datastore,                                   // CONN_DATASTORE
    #[error("already started")] AlreadyStarted,                        // CONN_ALREADY_STARTED
}
impl CodedError for ConnectorError { /* see Error Table */ }
```

Called by (*Interactions*, owner side):

| Item | Caller | Use |
|---|---|---|
| `migrations()`, `ServerConnector::new` | ordin desktop program (`apps/desktop`) | registered with the Journal; construction (wiring row 3.13) with `ConnectorConfig`, the Journal and a `DesktopKeyStore` |
| `start(events, cancel)` | ordin desktop program, once, with the sender `CommandBridge::new` returned (wiring row 3.49); quit cancels the token | starts every supervisor |
| `on_wake` | ordin desktop program, on the operating system's wake notice | reconnect at once |
| `list`, `connect`, `disconnect`, `remove` | ordin desktop program's `ServiceApi` implementation, for `servers.list`, `servers.connect`, `servers.disconnect`, `servers.remove` | the Servers settings screen |
| `connection_for`, `send_command` | Command Bridge | routing a command to a server's backend |
| `display_name` | ordin desktop program (notifications "On [server]", 11 §7) | |
| `ServerEvent`, `ServerEventSender` | defined here; the Command Bridge creates the channel and reads it | server events in order |

##### Example

`servers.connect` with a pairing line:

```json
{"kind":"pairing_line","line":"ordin-pair:1:10.20.4.17:8443:9f2c5b0e7a1d4c3b8e6f2a9d0c1b7e5f4a3d2c1b0e9f8a7d6c5b4a3f2e1d0c9b:K7QM2XPA"}
```

Request sent to `https://10.20.4.17:8443/v1/pairing` (certificate accepted only if its SHA-256 equals the fingerprint):

```json
{"api_version":{"major":1,"minor":0},"code":"K7QM2XPA","computer_name":"Asha's laptop"}
```

Result returned to the screens:

```json
{"server_connection_id":"0192a1d0-1111-7222-8333-444455556666","display_name":"Northwind server","state":"connected","backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b"}
```

Row written: `server_connections(id='0192a1d0-…', display_name='Northwind server', address='10.20.4.17', port=8443, service_api_version='1.0', last_sequence_received=0, keychain_ref='server-connector/desktop-key/0192a1d0-1111-7222-8333-444455556666', state='connected', paired_at='2026-01-10T10:02:11.004Z')`.

## Internal Logic

1. **Start.** Read every row of `server_connections` joined with `server_connection_pins` and `server_connection_backends`. For each row in `connected` or `not_connected`, set it to `not_connected` in memory (no event yet: the link is not up), fetch the key with `keys.get_desktop_key(keychain_ref)` (absent → state `disconnected`, journal `CONN_KEYCHAIN`), build the server's client (step 2) and spawn its supervisor (step 6). Rows in `disconnected` or `refused_for_version` are listed but not connected.
2. **Client per server.** `reqwest::Client::builder()` with `use_preconfigured_tls(config)`, `http2_prior_knowledge()` off (ALPN negotiates `h2`, HTTP/1.1 accepted), `connect_timeout(5 s)`, `pool_idle_timeout(None)`, `http2_keep_alive_interval(20 s)`, the system proxy (default `reqwest` behaviour, A-39), no redirects. The rustls `ClientConfig`: TLS 1.3 only; server verification by `rustls_platform_verifier::Verifier` when no pin exists, or by a pinning verifier that accepts the chain only if `sha256(end_entity_der)` equals the stored fingerprint (and still checks the handshake signature with rustls' default algorithms) when a pin exists; client authentication with `keys.client_certificate(name)` when `cfg.client_certificate` is set. A client certificate that is required but `None` → `ClientCertUnavailable`, the server stays `not_connected`, and the Connect screen says so (SA-HTTP-02).
3. **`connect(input)`.**
   1. Parse: `AddressAndCode` — split an optional `:port` (IPv6 in brackets); port absent → 8443; address rule → `AddressInvalid`; port outside 1025–65535 → `PortOutOfRange`; `normalise_code(code)` → `CodeRefused` on failure. `PairingLine` — `PairingLine::parse` → `PairingLineInvalid`.
   2. `(address, port)` already in `server_connections` with a state other than `disconnected` → `Duplicate`. A `disconnected` row for the same address and port is reused ("Connect again…"): its id is kept, its key replaced.
   3. Build a client (step 2) with the line's fingerprint as the pin when given.
   4. `POST /v1/pairing` with `PairRequest { api_version: CURRENT, code, computer_name }`, 15 s timeout. Map: TLS failure with no pin → `CertificateUntrusted`; pin mismatch → `FingerprintMismatch`; connection failure or timeout → `Unreachable`; 403 `DESK_CODE_INVALID` or `DESK_CODE_EXPIRED` → `CodeRefused`; 403 `DESK_PAIRING_STOPPED` → `PairingStopped`; 403 `DESK_TOO_MANY` → `TooManyDesktops`; 422 `DESK_NAME_INVALID` → `ComputerNameInvalid`; 429 → `AddressLocked`; 409 → `RefusedForVersion` with `server_older` computed as in step 9; any other status or an unreadable body → `BadAnswer`.
   5. Check `PairReply`: `desktop_key` decodes (`b64url_decode_exact(…, 32)`) → else `BadAnswer`; `desktop_accepts(CURRENT, reply.api_version)` → else `RefusedForVersion`; `server_name` 1–120 characters → else `BadAnswer`.
   6. In one transaction: insert or update the `server_connections` row (state `connected`, `last_sequence_received = 0`, `service_api_version = "<major>.<minor>"`, `paired_at = now`), the `server_connection_pins` row when a fingerprint was given, and the `server_connection_backends` row (`backend`, `desktop_pairing_id`). Then `keys.put_desktop_key(keychain_ref, key)`; on keychain failure delete the rows and return `Keychain`. The key is zeroised after the call.
   7. Spawn the supervisor; publish `server_state_changed` (step 8); log `conn.paired` with `server_connection_id` and `address`; return `ServerView`.
4. **`send_command(backend, meta, envelope, deadline)`.**
   1. `connection_for(backend)` → else `UnknownServer`.
   2. State `refused_for_version` → `RefusedForVersion`; `disconnected` → `KeyRefused`; `not_connected` → for `Effect::Read`, look up the read cache (step 5) by `(meta.name, sha256(input JSON))` and return the cached reply bytes if present, else `NotConnected`; for `Effect::Change` → `NotConnected`.
   3. `POST https://<address>:<port><meta.rest_path>` with `Authorization: Bearer <b64url key>`, `Content-Type: application/json`, the envelope bytes unchanged, timeout `deadline - now`.
   4. Retries: on a connection error before any response byte, a `Change` command is retried once after 200 ms with the same body (the server's request-id replay cache returns the first reply if the first attempt arrived); a `Read` command up to twice, after 200 ms and 400 ms. Each retry still ends by `deadline`.
   5. Response: check `Ordin-Api-Version` (step 9). Status 401 → mark `disconnected` (step 8), return `KeyRefused`. Status 429 with code `DESK_ADDRESS_LOCKED` → `AddressLocked`. Any other status with a JSON `CommandReply` body → return the body unchanged (the screens read `kind`). A body that is not a `CommandReply` → `BadAnswer`. Deadline passed → `Deadline`. Connection lost → `Unreachable` and the supervisor is told to probe.
   6. For a `Read` command whose reply `kind` is `ok`, store the reply bytes in the read cache.
5. **Read cache.** Per server, a map `(command name, sha256 of the normalised input JSON) → reply bytes` with least-recently-used eviction at `read_cache_bytes`. Cleared on `reset` from that server, on `disconnect` and on `remove`. Memory only.
6. **Supervisor per server** (state machine over `ServerConnectionState`):
   1. Open `GET /v1/events` with `Accept: text/event-stream`, `Authorization`, and `Last-Event-ID: <last_sequence_received>` when it is above 0. Parse frames with `sse-stream`.
   2. On HTTP 200: check `Ordin-Api-Version` (step 9); set `connected` if not already (step 8); reset the backoff to 1 s.
   3. Each frame: a comment (heartbeat) only refreshes the silence timer. An event frame: parse `data` as `EventEnvelope` → failure: log `conn.event_unreadable` with `seq` from `id:`, skip. `envelope.backend` must equal the connection's backend → else skip and log. `kind == reset` → clear the read cache, send `ServerEvent::Reset { backend }`, set `last_sequence_received` to the frame's `id`. Otherwise if `seq <= last_sequence_received` → ignore (late or duplicate); if `seq > last + 1` → log `SVC_EVENT_GAP` with both numbers and send `ServerEvent::Reset` first; then send `ServerEvent::Event`, set `last_sequence_received = seq`. `send` on a full channel waits (back-pressure on the stream, never dropping).
   4. Persist `last_sequence_received` every 5 s while it changed, and on stop.
   5. Silence: no byte for 45 s → drop the stream, set `not_connected` with `as_of` = the time of the last event received (step 8), and reconnect.
   6. Reconnect: wait the backoff (1, 2, 4, 8, 16, 32, 60, 60… s, each multiplied by a random factor in [0.8, 1.2]), then go to 6.1. Never stops while `cancel` is not fired and the state is `connected` or `not_connected`.
   7. HTTP 401 → `disconnected` (step 8), stop. HTTP 409 or an unusable `Ordin-Api-Version` → `refused_for_version` (step 8), stop. HTTP 429 `DESK_ADDRESS_LOCKED` → wait 10 minutes, then 6.1. Any other failure → 6.5.
7. **`disconnect(id)`.** Send `POST /v1/pairing/end` with the key (10 s timeout; a failure is logged as `conn.end_unconfirmed` and does not stop the disconnect: the server-side pairing then ends at the setup command); stop the supervisor; set `disconnected`; `keys.delete_desktop_key`; clear the read cache; publish `server_state_changed`. **`remove(id)`.** As `disconnect` if not already `disconnected`, then delete the rows of the three tables in one transaction. Unknown id → `UnknownServer`.
8. **State change.** Update `server_connections.state` (and `service_api_version` when it changed) in the datastore, then `journal.append_event(EventKind::ServerStateChanged.as_str(), ServiceEvent::ServerStateChanged { server_connection_id, state, as_of }.to_journal_data())`; the event reaches the screens through the Command Bridge's Journal subscription like any local event. A datastore failure is logged as `CONN_DATASTORE` and the in-memory state still changes, so the screens stay truthful for this session. The desktop program raises the computer's notifications from Service API events; this crate raises none (D6).
9. **Version check.** Parse `Ordin-Api-Version` as `<major>.<minor>`; absent or malformed → `BadAnswer`. `desktop_accepts(CURRENT, server)` false → `refused_for_version` with `server_older = server.major < CURRENT.major || (server.major == CURRENT.major && server.minor + 1 < CURRENT.minor)`; otherwise the desktop is the older side. The sentence chosen by Localisation follows `server_older` (11 §5.31).
10. **`list()`** returns every row as `ServerView`, with `backend` from `server_connection_backends`, `as_of` from memory, ordered by `display_name`. For a row in `refused_for_version`, `older_side` is computed from the stored `service_api_version` with the rule of step 9 (`Server` when `server_older`, else `Desktop`); a missing or malformed stored version gives `Desktop`. Every other state has `older_side = None`. The same value is set on the `ServerView` that `connect` returns when it fails over version (the screen's sentence) and is derivable by the screens from `server_state_changed` followed by `servers.list`.
11. **Wake.** `on_wake()` sends a wake notice on a `tokio::sync::watch` channel every supervisor selects on. A supervisor waiting in 6.6 ends its wait and goes to 6.1 with the backoff reset to 1 s; a supervisor in `connected` whose last byte is older than 20 s drops its stream and goes to 6.1; others ignore the notice. Logged `conn.wake` once with the number of supervisors woken.
12. **Stop.** On `cancel`: close every stream, persist every `last_sequence_received`, leave states as they are in the datastore except `connected`, which is written as `not_connected`.

## Data Model

SQLite, in the desktop's datastore (owned by the Journal crate, tables owned by this crate; `ModuleMigrations { module: "server_connector", sqlite: &[(1, "server connections", <the SQL below>)], postgres: &[] }`). The desktop runs no PostgreSQL shape.

```sql
-- 0001_server_connections.sql
CREATE TABLE server_connections (
    id                      TEXT PRIMARY KEY CHECK (length(id) = 36),
    display_name            TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    address                 TEXT NOT NULL CHECK (length(address) BETWEEN 1 AND 253),
    port                    INTEGER NOT NULL CHECK (port BETWEEN 1025 AND 65535),
    service_api_version     TEXT,
    last_sequence_received  INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence_received >= 0),
    keychain_ref            TEXT NOT NULL UNIQUE,
    state                   TEXT NOT NULL CHECK (state IN ('connected','not_connected','disconnected','refused_for_version')),
    paired_at               TEXT NOT NULL,
    UNIQUE (address, port)
);

-- Private to this crate: what 2C's ServerConnection does not carry.
CREATE TABLE server_connection_pins (
    server_connection_id    TEXT PRIMARY KEY REFERENCES server_connections(id) ON DELETE CASCADE,
    fingerprint_sha256      TEXT NOT NULL CHECK (length(fingerprint_sha256) = 64)
);

CREATE TABLE server_connection_backends (
    server_connection_id    TEXT PRIMARY KEY REFERENCES server_connections(id) ON DELETE CASCADE,
    backend_id              TEXT NOT NULL UNIQUE CHECK (length(backend_id) = 36),
    desktop_pairing_id      TEXT NOT NULL CHECK (length(desktop_pairing_id) = 36)
);
```

Rollback: the migration only creates tables; its rollback is restoring the backup copy SA-XC-07 takes before migrating. Removing a server deletes its rows (`ON DELETE CASCADE`); nothing else references them. `server_connection_pins` and `server_connection_backends` are *chosen here*: a pinned fingerprint (OQ-61) and the server's backend id are needed after a restart and 2C's entity has no field for them.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Address not a host name or IP literal | `Err(permanent)` | `CONN_ADDRESS_INVALID` | `ConnectorError::AddressInvalid` |
| Port outside 1025–65535 | `Err(permanent)` | `CONN_PORT_OUT_OF_RANGE` | `ConnectorError::PortOutOfRange` |
| Address and port already paired | `Err(permanent)` | `CONN_DUPLICATE` | `ConnectorError::Duplicate` |
| Pairing line malformed | `Err(permanent)` | `CONN_PAIRING_LINE_INVALID` | `ConnectorError::PairingLineInvalid` |
| Code malformed, wrong or expired | `Err(permanent)` | `CONN_CODE_REFUSED` | `ConnectorError::CodeRefused`; "That code didn't work. Ask for a new one from whoever runs your ordin server." |
| Server stopped pairing after five wrong codes | `Err(permanent)` | `CONN_PAIRING_STOPPED` | `ConnectorError::PairingStopped`; same sentence |
| Server already has its most paired desktops (`DESK_TOO_MANY`) | `Err(permanent)` | `CONN_TOO_MANY_DESKTOPS` | `ConnectorError::TooManyDesktops`; "[server] already has ten computers connected. Ask whoever runs it to disconnect one." *(chosen here)* |
| Server refused this computer's name (`DESK_NAME_INVALID`) | `Err(permanent)` | `CONN_COMPUTER_NAME_INVALID` | `ConnectorError::ComputerNameInvalid`; "This computer's name can't be used. Rename the computer and try again." *(chosen here)* |
| Server locked this address | `Err(retryable)` | `CONN_ADDRESS_LOCKED` | `ConnectorError::AddressLocked` |
| Server unreachable (connect, TLS transport, timeout) | `Err(retryable)`, `try_again` | `CONN_UNREACHABLE` | `ConnectorError::Unreachable`; "Can't reach [server] right now. ordin will keep trying while it's open." |
| Certificate not trusted by the computer and no pin | `Err(permanent)` | `CONN_CERT_UNTRUSTED` | `ConnectorError::CertificateUntrusted` |
| Pinned fingerprint does not match | `Err(permanent)`, `send_to_it` | `CONN_FINGERPRINT_MISMATCH` | `ConnectorError::FingerprintMismatch` |
| Required client certificate not available | `Err(permanent)`, `send_to_it` | `CONN_CLIENT_CERT_UNAVAILABLE` | `ConnectorError::ClientCertUnavailable` |
| Service API version unusable | `Err(permanent)` | `CONN_REFUSED_FOR_VERSION` | `ConnectorError::RefusedForVersion { server_older }`; one of the two 11 §5.31 sentences |
| Command while not connected (no cached read) | `Err(retryable)` | `CONN_NOT_CONNECTED` | `ConnectorError::NotConnected`; "Waiting for [server]" |
| Server refused the desktop key (401) | `Err(permanent)` | `CONN_KEY_REFUSED` | `ConnectorError::KeyRefused`; "This computer is no longer connected to [server]." |
| Server did not answer by the deadline | `Err(retryable)`, `try_again` | `CONN_DEADLINE` | `ConnectorError::Deadline` |
| Answer not a `CommandReply` / `PairReply`, or no version header | `Err(retryable)` | `CONN_BAD_ANSWER` | `ConnectorError::BadAnswer` |
| Unknown server id or backend | `Err(permanent)` | `CONN_UNKNOWN_SERVER` | `ConnectorError::UnknownServer` |
| Keychain read, write or delete failed | `Err(retryable)`, `try_again` | `CONN_KEYCHAIN` | `ConnectorError::Keychain` |
| Datastore read or write failed | `Err(retryable)` | `CONN_DATASTORE` | `ConnectorError::Datastore` |
| `start` called a second time | `Err(permanent)` | `CONN_ALREADY_STARTED` | `ConnectorError::AlreadyStarted` |
| Event frame unreadable or for another backend | logged, frame skipped | `CONN_EVENT_UNREADABLE` | — |
| Sequence jump in the stream | `Reset` sent to the Bridge, logged | `SVC_EVENT_GAP` | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Server Connector

  Scenario: Happy path — pairing with an address and a code
    Given a server at 10.20.4.17:8443 whose certificate the computer trusts
    And the server's setup command showed code "K7QM-2XPA"
    When the person connects with address "10.20.4.17" and code "k7qm-2xpa"
    Then POST /v1/pairing is sent with code "K7QM2XPA" and computer_name "Asha's laptop"
    And the desktop key is stored in the keychain under "server-connector/desktop-key/<id>"
    And a server_connections row with state connected and port 8443 exists
    And a server_state_changed event with state connected is published

  Scenario: Happy path — pairing with a pairing line pins the certificate
    Given a server whose certificate is signed by no authority the computer trusts
    When the person pastes its pairing line
    Then the TLS connection is accepted because the leaf certificate's SHA-256 equals the line's fingerprint
    And the fingerprint is stored in server_connection_pins

  Scenario: Error path — a wrong code
    Given the server answers 403 with code DESK_CODE_INVALID
    When the person connects
    Then connect fails with CONN_CODE_REFUSED
    And no row and no keychain item are written

  Scenario: Error path — the server already has ten desktops
    Given a server with ten paired desktops
    When the person connects with a valid code
    Then connect fails with CONN_TOO_MANY_DESKTOPS and no row or keychain item is written

  Scenario: Error path — the pinned certificate changes
    Given a paired server with a pinned fingerprint
    When the server presents a different certificate
    Then the connection is refused with CONN_FINGERPRINT_MISMATCH
    And the server stays not_connected

  Scenario: Edge case — 45 seconds of silence
    Given a connected server whose stream sends nothing for 45 seconds
    Then the server is marked not_connected with as_of the time of its last event
    And a reconnect is attempted after 1 second, then 2, 4, 8, 16, 32 and 60 seconds, each within ±20 %

  Scenario: Happy path — catching up after reconnecting
    Given a server whose last_sequence_received is 48212
    When the stream reconnects
    Then the request carries Last-Event-ID 48212
    And events 48213 onwards are handed to the Command Bridge in order

  Scenario: Edge case — the server sends reset
    Given a reconnect whose Last-Event-ID is older than the server keeps
    When the server sends a reset event with id 50001
    Then ServerEvent::Reset is handed to the Command Bridge
    And the read cache for that server is empty
    And last_sequence_received becomes 50001

  Scenario: Edge case — reading while not connected
    Given a not_connected server for which workspaces.list was answered earlier
    When the screens send workspaces.list with the same input
    Then the cached reply is returned
    And an approval.decide command is refused with CONN_NOT_CONNECTED

  Scenario: Error path — the server ended this computer's pairing
    Given a connected server
    When a command is answered with 401
    Then the server is marked disconnected and its stream stops
    And the command fails with CONN_KEY_REFUSED

  Scenario: Error path — a server two minor versions behind
    Given this desktop at Service API 1.2
    When the server's responses carry Ordin-Api-Version 1.0
    Then the server is marked refused_for_version with server_older true

  Scenario: Happy path — a change command survives one dropped connection
    Given a connected server whose connection drops before the first response byte of approval.decide
    When the command is sent
    Then it is sent a second time after 200 ms with the same request_id
    And the server's reply is returned once

  Scenario: Happy path — waking from sleep reconnects at once
    Given a server in not_connected whose supervisor is waiting 60 seconds of backoff
    When the desktop program calls on_wake
    Then the supervisor opens GET /v1/events within 1 second with Last-Event-ID set
    And conn.wake is logged with 1 supervisor woken

  Scenario: Edge case — the version screen names the side to update
    Given a server row in refused_for_version whose stored service_api_version is 1.0
    And this desktop at Service API 1.2
    When servers.list is called
    Then that ServerView has older_side "server"
    And every other ServerView has no older_side

  Scenario: Happy path — disconnecting
    Given a connected server
    When the person chooses Disconnect this computer
    Then POST /v1/pairing/end is sent with the key
    And the keychain item is deleted and the state is disconnected
```

## Performance, Security, Observability

- **Performance targets:** on the company network, a command to a connected server adds ≤ 5 ms p95 to the Remote API's own time, so that A-46's 150 ms p95 holds end to end; an event is handed to the Command Bridge within 20 ms p95 of its last byte arriving (A-46's 500 ms covers the whole path); one HTTP/2 connection per server; memory per server ≤ `read_cache_bytes` (32 MiB) plus 1 MiB of buffers.
- **Security:** the desktop key lives only in the keychain (`DesktopKeyStore`), is read per supervisor start and per command, held in `Zeroizing` memory and sent only as `Authorization: Bearer` over TLS 1.3 to the address it was paired with; certificates are verified by the operating system's trust store or by the pinned fingerprint, never skipped; no redirect is followed (a redirect could carry the key elsewhere); addresses come only from the person's input or the pairing line, never from a server reply. Never logged: the desktop key, the pairing code, the pairing line, command bodies, replies, event data. Logged: server connection id, address, port, state, status codes, seq numbers.
- **Observability:** log events `conn.paired` (`server_connection_id`, `address`), `conn.state` (`server_connection_id`, `from`, `to`, `reason_code`), `conn.reconnect` (`server_connection_id`, `attempt`, `delay_ms`), `conn.command` at `debug` (`server_connection_id`, `command`, `status`, `elapsed_ms`, `retries`), `conn.event_unreadable` (`server_connection_id`, `seq`), `conn.end_unconfirmed` (`server_connection_id`), `conn.wake` (`woken`). No metrics or spans (desktop, SA-XC-04, SA-XC-05).
- C-71 (bound assertion): *SSE over HTTP/2 passes company proxies with 20 s comments*. Until confirmed, the 45 s silence rule and the reconnect loop are the fallback the HLD gives (ADR-096 item 3): a proxy that cuts the stream costs one reconnect with `Last-Event-ID`, not lost events.

## Gaps

None.
