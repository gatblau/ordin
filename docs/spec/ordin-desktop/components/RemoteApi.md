# SPEC: Remote API

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-remote-api` · **Build phase:** 3 · **Programs:** P
> **Dependencies:** Service API, Approval Service, Journal, Identity, Organisation Profile

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
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)

## Purpose

The Remote API is the personal-profile ordin server's second way in, beside the Channel Gateway, serving the Service API to the person's paired ordin desktops (`02-architecture.md:580-586`, ADR-096 items 3 to 5). It listens on one private address and port chosen at setup, over HTTP/2 with TLS 1.3 and the server owner's certificate; pairs desktops through the setup command's one-time code and keeps each desktop key only as a hash; answers each Service API command at `POST /v1/<module>/<verb>` after checking the key; streams the server's events at `GET /v1/events` with the journal sequence number as the SSE `id:`, resumes from `Last-Event-ID` and sends `reset` when that number is older than the journal keeps; sends a heartbeat comment every 20 seconds; refuses an address for 10 minutes after 10 failed key checks in a minute; and ends a pairing at once when asked. It owns the desktop pairings and the open event streams. The agent profile does not build it (2B-3: programs P only), so an agent-profile server opens no Remote API and pairs no desktop (ADR-097).

## Approach

An `axum` 0.8.9 router served by `axum-server` 0.8.0 over `rustls` 0.23.46 (SA-HTTP-01). Commands are not routed one by one: a single handler takes every `POST /v1/{module}/{verb}`, authenticates the bearer key, and passes the body to `ordin_service_api::handle` with the path as the routed name, so the catalogue in `ordin-service-api` is the only route list (SA-API-01). The one exception is `approval.decide`, which the Remote API sends to the Approval Service's desktop-press entry itself, so a press from a desktop is always checked as "open card, single use, from a paired desktop" regardless of how the program wires its `ServiceApi` (ADR-096 item 7). The pairing and key tables are this crate's (SQLite only: the personal profile has no scaled shape). Error prefixes: the 2C `DESK_` codes for pairing and keys, the Service API's `SVC_` codes for command refusals, the carried `SRV_` codes `SRV_REFUSED`, `SRV_NO_ROUTE`, `SRV_METHOD_NOT_ALLOWED` for routing (SA-ENV-02, SA-XC-11), and `RAPI_` *(chosen here)* for start-up failures. Rejected: *one axum route per command* — a second route list beside the catalogue; *storing desktop keys encrypted rather than hashed* — ADR-096 item 5 keeps only a hash, and a key is never needed back.

### Reconciliation assumptions

- **SA-RECON-01** The Remote API owns its 2D-3 listening settings at run time (reconciliation row 159, N27, N139; ruling R6): `configure_listening` validates and stores them (address, port and certificate chain in its own table, key in the secret store) and re-binds a running `serve`. `RemoteApiConfig` therefore no longer carries `listen` or `cert_chain_path`.
- **SA-RECON-02** `end_pairing` keeps its `DesktopPairingId` argument (row 163): computer names are not unique, so the setup command's `--end COMPUTER_NAME` resolves the name through `list_pairings` and refuses an ambiguous name itself.
- **SA-RECON-03** The TLS key is read and written through Identity's `ServerSecrets` (implemented by `IdentityService`, errors `CredError`), and the profile through `OrganisationProfileService::current`/`subscribe` (reconciliation rows 166, N30, N143, N157).
- **SA-RECON-04** `approval.decide` returns the Approval Service's `ApprovalOutcome` unchanged; the catalogue row names that type (ruling R6).
- **SA-RECON-05** Ended pairings and old wrong-code attempts leave through the Journal's `PruneParticipant` mechanism (Journal O3), not a module-specific call.

## Shared Context

### Entity owned (2C part 1, `DesktopPairing`, copied)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DesktopPairing {
    pub id: DesktopPairingId,
    pub computer_name: String,
    pub desktop_key_hash: Sha256Hex,
    pub code_expires_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<Timestamp>,
    pub last_sequence_sent: u64,
    pub state: PairingState,
}

/// Shared by ChannelPairing and DesktopPairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingState { Pending, Paired, Ended }
```

| Field | Rule | Error code |
|---|---|---|
| computer_name | 1–63 chars | `DESK_NAME_INVALID` |
| desktop_key_hash | the key is never stored, only its hash | `DESK_KEY_STORED` |
| code_expires_at | creation + 10 minutes | `DESK_CODE_EXPIRED` |
| key check | after 10 failures in a minute the address is refused for 10 minutes | `DESK_ADDRESS_LOCKED` |

| From | Event | To |
|---|---|---|
| — | setup command shows a code | pending |
| pending | code used within 10 minutes | paired |
| pending | 10 minutes pass | ended |
| paired | **Disconnect this computer**, or setup command ends it (event stream closed at once) | ended |

Before a code is used, `desktop_key_hash` holds 64 zeros (`"000…0"`) *(assumed here: the 2C field is not optional and a pending pairing has no key yet; the all-zero value cannot equal the SHA-256 of any key in practice and is never compared, because only `paired` rows are checked)*.

### Service API used (from `ordin-service-api`)

- `ServiceApi` (trait, implemented by the ordin server program), `ErrorSentences`, `CallOrigin::PairedDesktop { desktop_pairing_id, computer_name }`, `CATALOGUE`, `command(name)`, `Incoming { body, routed_name, origin, this_backend }`, `handle(api, sentences, incoming, deadline, cancel) -> HandledReply { json, code, http_status }`, `Budget::{Interactive (10 s deadline), Search (10 s), Transfer (60 s)}`, `MAX_BODY_BYTES` 1 MiB, `MAX_TEXT_SAVE_BODY_BYTES` 9 MiB.
- `ServiceApiVersion { major, minor }`, `CURRENT = 1.0`, `desktop_accepts(desktop, server)`.
- `CommandReply`, `CommandOutcome::{Ok, Error}`, `ServiceError { code, message, action }`.
- Events: `EventEnvelope::new(backend, seq, at, kind: &str, data) -> Result<EventEnvelope, EventError>`, `encode_for_transport(&EventEnvelope) -> Result<TransportEvent { seq, kind, json }, EventError>`, `ServiceEvent::Reset`. `server_state_changed` is a desktop-local kind and never occurs on a server's journal.
- Pairing types: `PairRequest { api_version, code, computer_name }` (`deny_unknown_fields`), `PairReply { desktop_pairing_id, desktop_key, backend, server_name, api_version }`, `PairingLine { host, port, fingerprint, code }` with `format()`, `normalise_code`, `display_code` (`ABCD-EFGH`); alphabet `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`, 8 symbols.
- `approval.decide` input `ApprovalPress { card_ref: CardRef, decision: PressDecision::{Approve, Reject} }`, result `ApprovalOutcome { approval_id, state, decided_on }` (the catalogue row's result is the Approval Service's own type).
- Status mapping (2C wire §1): `ok` and domain refusals → 200; `SVC_INPUT_INVALID` and `SRV_BAD_CURSOR` → 422; `SVC_UNAUTHENTICATED` → 401; `SVC_VERSION_UNSUPPORTED` → 409; `SVC_RATE_LIMITED` → 429.

### Approval Service interface used (from the Approval Service spec)

```rust
pub enum PressOrigin { LocalScreen, PairedDesktop { computer_name: String }, PairedChat { channel: ChatChannel }, Approver { .. } }
pub struct ApprovalOutcome { pub approval_id: ApprovalId, pub state: ApprovalState, pub decided_on: String }
impl ApprovalService {
    /// Checks the card is open and the reference single-use, records `decided_on`
    /// ("Decided on <computer_name>" in the chats, ADR-096 item 7).
    pub async fn decide(&self, press: ApprovalPress, origin: PressOrigin) -> Result<ApprovalOutcome, ApprovalError>;
}
```

The Remote API returns the `ApprovalOutcome` unchanged as the command's result (reconciliation R6: the owner's type).

### Journal interface used (from the Journal and Artefact Store spec)

```rust
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {          // implemented by the Remote API, name "remote_api"
    fn name(&self) -> &'static str;
    async fn deliver(&self, event: &JournalEvent);   // once per event, in seq order
    async fn lagged(&self, next_seq: u64);           // queue overflowed from next_seq on
}
pub struct JournalEvent { pub backend: uuid::Uuid, pub seq: u64, pub at: Timestamp, pub kind: String, pub data: serde_json::Value }
impl Journal {
    pub fn datastore(&self) -> &Datastore;                      // Datastore::sqlite() on the personal server
    pub fn backend_uuid(&self) -> uuid::Uuid;                   // wrapped as BackendId
    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId;
    pub async fn events_since(&self, after_seq: u64, limit: u32) -> Result<Vec<JournalEvent>, JournalError>; // JRNL_RESUME_TOO_OLD, JRNL_SEQ_AHEAD
    pub fn last_seq(&self) -> u64;
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
}
pub struct NewJournalEntry { pub kind: String, pub component: String, pub run_id: Option<RunId>,
    pub crew_run_id: Option<CrewRunId>, pub workspace_id: Option<WorkspaceId>, pub summary: String, pub data: serde_json::Value }
```

Migrations: `ModuleMigrations { module: "remote_api", sqlite: &[(1, "desktop pairings", <0001 of Data Model>), (2, "listening settings", <0002 of Data Model>)], postgres: &[] }`, registered by the program. Security records (`remote_api_address_locked`, `remote_api_desktop_paired`, `remote_api_desktop_unpaired`) are journal entries with `component: "remote_api"`, never Service API events. Below, `backend` means `BackendId(journal.backend_uuid())`.

### Identity and Organisation Profile used (from their specs)

```rust
// ordin-identity (IdentityAndCredentials.md); implemented by `IdentityService`.
/// The Remote API's TLS private key on the server (secret name `remote-api/tls-key`).
#[async_trait::async_trait]
pub trait ServerSecrets: Send + Sync {
    async fn remote_api_tls_key(&self) -> Result<Option<zeroize::Zeroizing<String>>, CredError>;
    async fn put_remote_api_tls_key(&self, pem: zeroize::Zeroizing<String>) -> Result<(), CredError>;
    /// Succeeds when already absent.
    async fn delete_remote_api_tls_key(&self) -> Result<(), CredError>;
}

// ordin-organisation-profile (OrganisationProfile.md)
impl OrganisationProfileService {
    pub fn current(&self) -> std::sync::Arc<ProfileSnapshot>;
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>>;
}
```

Organisation profile keys (2D-2), read as `profile.current().document.server.<key>` at start and on each `subscribe()` change: `listening_allowed` (bool, default `true`; `false` switches the Remote API off), `client_certificate_required` (bool, default `false`).

### Journal history pruning (from the Journal and Artefact Store spec)

```rust
#[async_trait::async_trait]
pub trait PruneParticipant: Send + Sync {
    fn module(&self) -> &'static str;
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure>;
}
pub struct PruneRequest { pub cutoff: Timestamp, pub runs_in_progress: Vec<RunId> }
pub struct PruneFailure { pub code: &'static str }
```

`RemoteApi` implements it (module `remote_api`); the program registers it with `Journal::register_prune_participant`.

### Server settings (2D-3, written by the setup command; passed in by the program)

Remote API listening address and port (a private or loopback IP; port 1025–65535; none → Remote API off), the TLS certificate chain and the key. The Remote API is the 2D-3 owner of these settings: the setup command hands them to `configure_listening`, which keeps the address, port and certificate chain in this crate's `remote_api_listening` table and the key in the secret store (SA-RECON-01). Default port proposed by the setup command: 8443 (SA-NET-01).

### Constants (2D-4, ADR-096)

| Constant | Value |
|---|---|
| Heartbeat | an SSE comment every 20 s |
| Pairing code | valid 10 minutes; after 5 wrong codes in 10 minutes pairing stops until the setup command starts it again |
| Failed key checks | 10 a minute from one address → that address refused for 10 minutes |
| Desktop key | 32 random bytes, stored as SHA-256 |
| Transport | HTTP/2 (HTTP/1.1 also accepted, SA-HTTP-01), TLS 1.3 only, prefix `/v1/` |
| Service API compatibility | same major, same or previous minor |
| Allowed listen addresses | loopback; RFC 1918 IPv4 (`10/8`, `172.16/12`, `192.168/16`); RFC 4193 IPv6 (`fc00::/7`) |

Libraries: `axum` 0.8.9, `axum-server` 0.8.0, `rustls` 0.23.46 (SA-HTTP-01), `sqlx` 0.9.0 (SA-DB-01), `tokio`, `tracing`, `sha2` 0.10, `rand` 0.8 (`OsRng`), `subtle` 2 (constant-time compare), `base64` 0.22 *(`sha2`, `rand`, `base64` and `subtle` are named in HLD 05, row Utility and cryptography crates; `sha2`, `rand` and `base64` are also in `memory/Cargo.toml`)*, `rustls-pemfile` through `axum-server`'s `tls-rustls` feature (Phase 1A SA-RT-01).

## Public Interface

```rust
pub struct RemoteApiConfig {
    /// The server's display name sent to desktops ("Northwind server").
    pub server_name: String,
    /// Commands a pairing may send per second (token bucket); default 50, burst 100.
    pub commands_per_second: u32,
    /// Most desktops paired at once; default 10.
    pub max_paired_desktops: u32,
}

pub struct RemoteApi { /* private */ }

impl RemoteApi {
    /// This crate's migrations; the program registers them.
    pub fn migrations() -> ModuleMigrations;

    pub fn new(
        cfg: RemoteApiConfig,
        api: std::sync::Arc<dyn ServiceApi>,
        sentences: std::sync::Arc<dyn ErrorSentences>,
        approvals: std::sync::Arc<ordin_approval::ApprovalService>,
        journal: ordin_journal::Journal,
        secrets: std::sync::Arc<dyn ServerSecrets>,
        profile: std::sync::Arc<OrganisationProfileService>,
    ) -> RemoteApi;

    /// Serves until `cancel`. Returns at once with `Ok(Serving::Off)` when the profile forbids
    /// listening. With no listening settings stored it opens no port and waits for
    /// `configure_listening` or `cancel`; `cancel` then returns `Ok(Serving::Stopped)`.
    pub async fn serve(&self, cancel: tokio_util::sync::CancellationToken) -> Result<Serving, RemoteApiError>;

    // ---- used by the server's setup command (in the running server through its control
    //      socket, or in-process when no server runs) ----
    /// Stores the listening settings (`Some`) or switches listening off (`None`). A running
    /// `serve` re-binds within 1 s and this call waits up to 5 s for the outcome.
    pub async fn configure_listening(&self, s: Option<ListenSettings>) -> Result<(), RemoteApiError>;
    /// The stored listening settings, without the key; `None` when listening is not set.
    pub async fn listening(&self) -> Result<Option<ListenSummary>, RemoteApiError>;

    /// Starts a pairing: a new pending row and a code valid 10 minutes.
    pub async fn start_pairing(&self) -> Result<PairingOffer, RemoteApiError>;
    pub async fn list_pairings(&self) -> Result<Vec<PairingSummary>, RemoteApiError>;
    /// Ends a paired desktop's pairing; its stream closes within 2 s.
    pub async fn end_pairing(&self, id: DesktopPairingId) -> Result<(), RemoteApiError>;
}

pub enum Serving { Off, Stopped }

pub struct ListenSettings {
    /// Loopback or private (RFC 1918, RFC 4193) address.
    pub address: std::net::IpAddr,
    /// 1025–65535.
    pub port: u16,
    /// PEM of the server certificate followed by its chain, read once by the setup command.
    pub certificate_chain_pem: String,
    /// PEM of the private key, read once by the setup command; stored only in the secret store.
    pub key_pem: zeroize::Zeroizing<String>,
}

pub struct ListenSummary {
    pub address: std::net::IpAddr,
    pub port: u16,
    /// SHA-256 of the leaf certificate's DER, lower-case hex (the pairing line's fingerprint).
    pub fingerprint: Sha256Hex,
    pub changed_at: Timestamp,
}

pub struct PairingOffer {
    pub desktop_pairing_id: DesktopPairingId,
    /// Display form `ABCD-EFGH`.
    pub code: String,
    pub expires_at: Timestamp,
    /// Present when an address is configured: `ordin-pair:1:<host>:<port>:<fingerprint>:<code>`.
    pub pairing_line: Option<String>,
}

pub struct PairingSummary {
    pub desktop_pairing_id: DesktopPairingId,
    pub computer_name: String,
    pub state: PairingState,
    pub paired_at: Option<Timestamp>,
    pub last_connected_at: Option<Timestamp>,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteApiError {
    #[error("listening address is not private")] AddressNotPrivate,         // RAPI_ADDRESS_NOT_PRIVATE
    #[error("port outside 1025-65535")] PortInvalid,                         // RAPI_PORT_INVALID
    #[error("the organisation profile forbids listening")] ListeningForbidden, // RAPI_LISTENING_FORBIDDEN
    #[error("secret store")] SecretStore,                                   // RAPI_SECRET_STORE
    #[error("certificate or key unreadable")] TlsMaterialInvalid,           // RAPI_TLS_MATERIAL_INVALID
    #[error("cannot bind")] BindFailed,                                     // RAPI_BIND_FAILED
    #[error("too many paired desktops")] TooManyDesktops,                   // DESK_TOO_MANY
    #[error("unknown pairing")] UnknownPairing,                             // DESK_UNKNOWN
    #[error("datastore")] Datastore,                                        // RAPI_DATASTORE
}
impl CodedError for RemoteApiError { /* see Error Table */ }
```

Called by (*Interactions*, owner side):

| Item | Caller | Use |
|---|---|---|
| `migrations()` | ordin server program (`apps/server`) | registered with the Journal before the datastore opens |
| `PruneParticipant` (implemented by `RemoteApi`) | ordin server program registers it; the Journal calls `prune` during history pruning | history retention (A-08) |
| `RemoteApi::new`, `serve` | ordin server program, personal profile | construction (wiring row 5.41) and start (step 6) |
| `configure_listening`, `listening` | ordin server program, `ordin-server setup listen` and `setup pair-desktop` (listening not set → `SERVER_LISTENING_NOT_SET`) | setup steps |
| `start_pairing`, `list_pairings`, `end_pairing` | ordin server program, `ordin-server setup pair-desktop [--list \| --end COMPUTER_NAME]` (the program resolves a computer name to its id through `list_pairings`) | setup steps |
| `EventSink` (implemented by the Remote API) | Journal and Artefact Store, after `Journal::subscribe` | the event fan-out |
| Routes `POST /v1/pairing`, `POST /v1/pairing/end`, `GET /v1/events`, `POST /v1/{module}/{verb}` | Server Connector (desktop) | pairing, commands, event stream |

### Routes

| Method and path | Auth | Request | Response |
|---|---|---|---|
| `POST /v1/pairing` | none | `PairRequest` JSON, ≤ 4 KiB | 200 `{"kind":"ok","result":PairReply}`; error statuses below with `{"kind":"error","error":ServiceError}` |
| `POST /v1/pairing/end` | bearer | `{}` | 200 `{"kind":"ok","result":{}}` |
| `GET /v1/events` | bearer | header `Last-Event-ID` (optional) | 200 `text/event-stream` |
| `POST /v1/{module}/{verb}` | bearer | `CommandEnvelope` JSON | the `CommandReply` with the status `handle` returns |
| any other path | — | — | 404 `SRV_NO_ROUTE` |
| any other method on a known path | — | — | 405 `SRV_METHOD_NOT_ALLOWED` |

Every response carries `Ordin-Api-Version: 1.0` (`<CURRENT.major>.<CURRENT.minor>`), `Cache-Control: no-store`, `Content-Type: application/json` (or `text/event-stream`), and no CORS header. Bearer form: `Authorization: Bearer <43 characters base64url>`.

SSE frame format:

```
id: 48213
event: approval_requested
data: {"api_version":{"major":1,"minor":0},"backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b","seq":48213,"at":"2026-01-10T09:41:07.312Z","kind":"approval_requested","card":{…}}

: heartbeat

```

##### Example

Setup command (`ordin-server setup pair-desktop`) output built from `start_pairing()`:

```
Code: K7QM-2XPA (valid until 10:12)
Or paste this line on the desktop: ordin-pair:1:10.20.4.17:8443:9f2c5b0e7a1d4c3b8e6f2a9d0c1b7e5f4a3d2c1b0e9f8a7d6c5b4a3f2e1d0c9b:K7QM2XPA
```

Pairing:

```
POST /v1/pairing
{"api_version":{"major":1,"minor":0},"code":"K7QM2XPA","computer_name":"Asha's laptop"}

200 Ordin-Api-Version: 1.0
{"kind":"ok","result":{"desktop_pairing_id":"0192a1d1-0000-7000-8000-000000000042","desktop_key":"t9V2c0aQe8pXr1Lm4Nk7Hs3Dw6Fy5Gj0Bz2Ux8Ci9Ko","backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b","server_name":"Northwind server","api_version":{"major":1,"minor":0}}}
```

Command:

```
POST /v1/approval/decide
Authorization: Bearer t9V2c0aQe8pXr1Lm4Nk7Hs3Dw6Fy5Gj0Bz2Ux8Ci9Ko
{"api_version":{"major":1,"minor":0},"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","command":"approval.decide","backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b","input":{"card_ref":"q8Zt3VxN0pLk2mRb7sYw1A","decision":"approve"}}

200 Ordin-Api-Version: 1.0
{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"ok","result":{"approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f","state":"approved","decided_on":"Asha's laptop"}}
```

## Internal Logic

1. **`serve` — start.**
   1. `profile.current().document.server.listening_allowed` is `false` → log `rapi.off` with the reason and return `Serving::Off` (07: the profile can switch the Remote API off). Read the `remote_api_listening` row; none → log `rapi.off` (`reason` `not_configured`), open no port and wait for a `configure_listening` notice (step 13) or `cancel` (→ `Serving::Stopped`).
   2. The listen IP is not loopback, RFC 1918 or RFC 4193 → `AddressNotPrivate` (the setup command already refuses such an address; this re-checks a hand-edited settings file). Port ≤ 1024 → `AddressNotPrivate`.
   3. Read the chain PEM (the row's `certificate_chain_pem`) and the key (`secrets.remote_api_tls_key()`); either missing or unparseable → `TlsMaterialInvalid`. Compute the leaf fingerprint (SHA-256 of the leaf DER) for pairing lines.
   4. Build the rustls `ServerConfig`: protocol versions TLS 1.3 only; ALPN `h2`, `http/1.1`; client authentication: when `server.client_certificate_required` is `true`, `WebPkiClientVerifier` with roots = the CA certificates found in the chain file after the leaf *(assumed here: the company's own authority issues both the server's certificate and its desktops' certificates; no other CA input exists in 2D)*; otherwise no client authentication. A chain file with no CA certificate while client certificates are required → `TlsMaterialInvalid`.
   5. Bind with `axum_server::bind_rustls(listen, config)`; failure → `BindFailed`. Load the pairing cache (step 3.1). Log `rapi.listening` with address and port.
   6. On `cancel`: stop accepting, send each open stream its end, wait up to 5 s for in-flight commands, return `Serving::Stopped`.
   7. On an organisation-profile change that sets `listening_allowed` to `false`: behave as `cancel`, and log `rapi.off`.
2. **Request pre-checks (every route).** A request with an `Origin` header → 403 `SRV_REFUSED` (SA-XC-11). Body larger than the route's limit (pairing 4 KiB; commands `MAX_BODY_BYTES`, or `MAX_TEXT_SAVE_BODY_BYTES` for `files/save_edit`, `knowledge/save_edit` and `projects/save_file`) → 422 `SVC_BODY_TOO_LARGE` without reading further. The client IP is taken from the TCP peer address only (no forwarding header is trusted: there is no proxy in front of the Remote API).
3. **Key check (bearer routes).**
   1. Pairing cache: the `paired` rows (`id`, `computer_name`, `desktop_key_hash`), reloaded at start, after each pairing change made by this process, and every 2 s from the datastore so that a pairing ended by the setup command (another process) is seen within 2 s.
   2. If the client IP is locked (step 4) → 429 `DESK_ADDRESS_LOCKED`, without reading the key.
   3. Header missing or not `Bearer` + 43 characters of base64url → failure. Otherwise decode to 32 bytes, hash with SHA-256, and compare the hash with every cached row's hash using `subtle::ConstantTimeEq`, visiting every row whatever the result (SA-XC-01). No match → failure.
   4. On failure: count it for the IP (step 4) and reply 401 `SVC_UNAUTHENTICATED`. On success: `CallOrigin::PairedDesktop { desktop_pairing_id, computer_name }`.
4. **Failed-check lockout.** Per client IP, a list of failure times within the last 60 s (in memory). The tenth failure inside 60 s locks the IP until now + 10 minutes, clears its list and appends the journal entry `remote_api_address_locked` (`summary` "An address was refused for 10 minutes after 10 failed key checks.", `data` `{"address": ip, "until": ts}`) without any key material. Locks are memory only and end with the process *(assumed here: a restart ends a 10-minute lock early, which the setup command's own SSH access makes no weaker)*.
5. **`POST /v1/pairing`.**
   1. Parse `PairRequest` (`deny_unknown_fields`) → 422 `SVC_INPUT_INVALID`. `desktop_accepts(req.api_version, CURRENT)` false → 409 `SVC_VERSION_UNSUPPORTED`. `computer_name` 1–63 characters after NFC and trim → else 422 `DESK_NAME_INVALID`. `normalise_code(req.code)` → else 403 `DESK_CODE_INVALID`.
   2. In one `BEGIN IMMEDIATE` transaction: if `desktop_pairing_attempts` holds 5 or more failures with `at` within the last 10 minutes and `stopped = 1` → 403 `DESK_PAIRING_STOPPED`. Find the `pending` pairing whose `code_hash` equals `sha256(code)`; none → insert a failure row, and when it is the fifth in 10 minutes set `stopped = 1` on every pending code and mark those pairings `ended` → 403 `DESK_CODE_INVALID`. Found but `code_expires_at < now` → mark it `ended`, 403 `DESK_CODE_EXPIRED`. Paired desktops already at `max_paired_desktops` → 403 `DESK_TOO_MANY`.
   3. Generate 32 bytes from `OsRng`; set the row to `paired`, `desktop_key_hash = sha256(key)`, `computer_name`, `paired_at = now`; delete its `desktop_pairing_codes` row; commit. Reload the pairing cache.
   4. Reply 200 with `PairReply { desktop_pairing_id, desktop_key: b64url(key), backend, server_name, api_version: CURRENT }`. The key is zeroised after the reply is written; it is never logged or stored.
   5. Append the journal entry `remote_api_desktop_paired` (`data` `{"desktop_pairing_id": id, "computer_name": name}`).
   6. Wrong or expired codes reply with the same sentence ("That code didn't work. Ask for a new one from whoever runs your ordin server.") so a caller cannot tell which; only the code differs, for Diagnostics.
6. **`start_pairing()` (setup command).** `BEGIN IMMEDIATE`: clear `desktop_pairing_attempts` and its `stopped` flag (a new start by the setup command lifts a stop, ADR-096 item 5); generate a code of 8 symbols from the alphabet with `OsRng` (rejection sampling, no modulo bias); insert a `desktop_pairings` row (`pending`, `code_expires_at = now + 10 min`, all-zero hash, `last_sequence_sent = 0`, `computer_name` = `"pending"`) and a `desktop_pairing_codes` row (`sha256(code)`); commit. When a `remote_api_listening` row exists, build the pairing line with `PairingLine { host: listen IP (IPv6 in brackets), port, fingerprint, code }.format()`. Return `PairingOffer` with `display_code(code)`. The code is printed by the setup command and never stored in clear.
7. **`POST /v1/pairing/end` and `end_pairing(id)`.** Set the row `ended`; reload the cache; close that pairing's stream (step 9.6); append the journal entry `remote_api_desktop_unpaired` (`data` `{"desktop_pairing_id": id}`). `end_pairing` with an unknown or already ended id → `UnknownPairing`.
8. **`POST /v1/{module}/{verb}`.**
   1. Key check (step 3). Rate limit: a token bucket per pairing (`commands_per_second`, burst twice that); empty → 429 `SVC_RATE_LIMITED`.
   2. `routed_name = "{module}.{verb}"`. When `routed_name == "approval.decide"`: run `ordin_service_api::accept` (so every envelope rule applies), deserialise the input as `ApprovalPress`, call `approvals.decide(press, PressOrigin::PairedDesktop { computer_name })` within the `Interactive` deadline, return the `ApprovalOutcome` as the result, and build the `CommandReply` (an `ApprovalError` becomes a `ServiceError` through `ErrorSentences`; status 200 as a domain refusal).
   3. Otherwise: request-id replay (step 8.4) for `Effect::Change` commands, then `ordin_service_api::handle(api, sentences, Incoming { body, routed_name, origin, this_backend: backend }, deadline by budget, cancel tied to the connection)`. `handle` refuses unknown names (404 `SVC_COMMAND_UNKNOWN`), `Local` commands (404 `SVC_COMMAND_LOCAL_ONLY`), a path that does not match the envelope, input and version errors.
   4. Replay cache: per pairing, `request_id → reply JSON and status`, kept 10 minutes (LW-3), at most 10,000 entries per pairing (oldest evicted). A request whose id is present returns the stored reply without dispatching; one still in flight waits for the first and returns the same reply.
   5. Write the reply with `HandledReply.http_status` and log `rapi.command` (step 12).
9. **`GET /v1/events`.**
   1. Key check (step 3). One stream per pairing: a new stream for a pairing that has one closes the older one first (ADR-096 item 3).
   2. `Last-Event-ID`: absent → `after = journal.last_seq()`, and the stream begins with one `reset` frame (`id:` = `after`) so the desktop loads its state by commands — the sequence diagram's "the server's workspaces and Needs you" (03 *Connect the desktop to an ordin server*). Present and not an integer ≥ 1 → treat as absent. Present → `after` = that value.
   3. Catch-up: register the stream with the Remote API's fan-out (step 9.8) holding live events, then read `journal.events_since(after, 1000)` in pages up to the first held live seq, write them, then release the held events, skipping any seq already written. `JRNL_RESUME_TOO_OLD` or `JRNL_SEQ_AHEAD` → write one `reset` frame with `id:` = `journal.last_seq()` and continue live; log `SVC_RESUME_TOO_OLD` at `info`.
   4. Each `JournalEvent` → `EventEnvelope::new(backend, seq, at, &kind, data)` → `encode_for_transport` → frame `id: <seq>`, `event: <kind>`, `data: <json>`; an `EventError` skips the record with `rapi.event_dropped`. Desktop-local kinds (`server_state_changed`, `brand_changed`, `setup_progress`, `sign_in_changed`) are never sent. Write each frame and flush.
   5. Heartbeat: a `: heartbeat` comment when 20 s pass without a frame.
   6. The stream ends when the pairing leaves `paired` (checked against the cache on each frame and each heartbeat, so within 2 s of `end_pairing` and at once for `POST /v1/pairing/end`), when the subscription fails (after a `rapi.stream_lost` log; the desktop reconnects), on `cancel`, or when the client disconnects.
   7. On open: `last_connected_at = now`. While open: `last_sequence_sent` = the last seq written, persisted every 5 s while it changed and on close.
   8. Fan-out: the Remote API registers one `EventSink` with `journal.subscribe` when `serve` starts. `deliver` encodes the event once and pushes the frame to every open stream's bounded queue (1,024 frames); a full queue ends that stream with `rapi.stream_lagging` (the desktop reconnects with `Last-Event-ID` and catches up by step 9.3). `lagged(next_seq)` ends every open stream the same way, since each can catch up on reconnect.
10. **Agent profile.** The crate is not linked into an agent-profile build (2B-3 programs: P); the ordin server program does not construct it when the server profile is `agent`.
11. **Version header.** A middleware adds `Ordin-Api-Version: 1.0` to every response, error responses included.
12. **Logging.** `rapi.command` at `info`: `request_id`, `command`, `desktop_pairing_id`, `status`, `code` (when an error), `elapsed_ms`, `replayed` (bool). `rapi.key_failed` at `warn`: `client_ip`, `failures_in_minute`. `rapi.stream` at `info` on open and close: `desktop_pairing_id`, `after`, `reset` (bool), `frames`. Never the key, the code, bodies or event data.
13. **`configure_listening(s)`.**
   1. `Some(s)`: the address is not loopback, RFC 1918 or RFC 4193 → `AddressNotPrivate`; port outside 1025–65535 → `PortInvalid`; `profile.current().document.server.listening_allowed` is `false` → `ListeningForbidden`. Parse the chain (first certificate is the leaf) and the key and build the rustls `ServerConfig` of step 1.4 with them; any failure → `TlsMaterialInvalid`. Compute the leaf fingerprint.
   2. Store the key with `secrets.put_remote_api_tls_key(key_pem)` (failure → `SecretStore`), then upsert the single `remote_api_listening` row (`address`, `port`, `certificate_chain_pem`, `fingerprint`, `changed_at = now`; failure → `Datastore`). The key is stored before the row, so a row never names a key that is not there.
   3. `None`: delete the row, then `secrets.delete_remote_api_tls_key()`.
   4. Append the journal entry `remote_api_listening_changed` (`data` `{"address": ip, "port": n, "fingerprint": hex}` or `{"off": true}`; never key material).
   5. When `serve` is running in this process, send the change on its `tokio::sync::watch` channel with a `oneshot` for the outcome. `serve` stops accepting on the old listener, ends every open event stream (desktops reconnect with `Last-Event-ID`), and binds the new address (step 1.2–1.5); for `None` it stays without a port. A bind failure is returned as `BindFailed` (the settings stay stored; the next start retries) and logged `rapi.bind_failed`. No answer within 5 s → `BindFailed`. When `serve` is not running, the call returns after step 4 and the next start binds.
14. **`listening()`.** Reads the row and returns `ListenSummary`, or `None` when there is no row; failure → `Datastore`.

## Data Model

SQLite, in the server's datastore (owned by the Journal crate; tables owned by this crate; migrations registered under module `remote_api`). The personal profile has no PostgreSQL shape, and the agent profile does not build this crate, so no PostgreSQL variant is defined.

```sql
-- 0001_desktop_pairings.sql
CREATE TABLE desktop_pairings (
    id                    TEXT PRIMARY KEY CHECK (length(id) = 36),
    computer_name         TEXT NOT NULL CHECK (length(computer_name) BETWEEN 1 AND 63),
    desktop_key_hash      TEXT NOT NULL CHECK (length(desktop_key_hash) = 64),
    code_expires_at       TEXT NOT NULL,
    paired_at             TEXT,
    last_connected_at     TEXT,
    last_sequence_sent    INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence_sent >= 0),
    state                 TEXT NOT NULL CHECK (state IN ('pending','paired','ended')),
    CHECK ((state = 'paired') = (paired_at IS NOT NULL) OR state = 'ended')
);
CREATE INDEX desktop_pairings_state ON desktop_pairings (state);

-- Private to this crate: the hash of a pending code (2C keeps no code on the entity).
CREATE TABLE desktop_pairing_codes (
    desktop_pairing_id    TEXT PRIMARY KEY REFERENCES desktop_pairings(id) ON DELETE CASCADE,
    code_hash             TEXT NOT NULL UNIQUE CHECK (length(code_hash) = 64)
);

-- Private to this crate: wrong codes, for the 5-in-10-minutes stop.
CREATE TABLE desktop_pairing_attempts (
    at                    TEXT NOT NULL,
    stopped               INTEGER NOT NULL DEFAULT 0 CHECK (stopped IN (0, 1))
);
CREATE INDEX desktop_pairing_attempts_at ON desktop_pairing_attempts (at);

-- 0002_listening_settings.sql
-- 2D-3 "Remote API listening address and port" and "TLS certificate"; one row at most.
CREATE TABLE remote_api_listening (
    singleton             INTEGER PRIMARY KEY CHECK (singleton = 1),
    address               TEXT NOT NULL,
    port                  INTEGER NOT NULL CHECK (port BETWEEN 1025 AND 65535),
    certificate_chain_pem TEXT NOT NULL CHECK (length(certificate_chain_pem) BETWEEN 1 AND 65536),
    fingerprint           TEXT NOT NULL CHECK (length(fingerprint) = 64),
    changed_at            TEXT NOT NULL
);
```

Rows in `ended` are kept for the history retention period (2D-3, default 90 days) and then deleted through this crate's `PruneParticipant`: `prune` deletes `desktop_pairings` rows with `state = 'ended'` whose `coalesce(last_connected_at, paired_at, code_expires_at)` is before `request.cutoff`, and `desktop_pairing_attempts` rows with `at` before it, in one transaction, and returns the number of rows deleted; a datastore failure returns `PruneFailure { code: "RAPI_DATASTORE" }`. `paired` and `pending` rows are never pruned. Rollback: the migration only creates tables; its rollback is the pre-migration backup copy of SA-XC-07.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Request carries an `Origin` header | 403 | `SRV_REFUSED` | `{"kind":"error","error":{"code":"SRV_REFUSED","message":"…"}}` |
| Unknown path | 404 | `SRV_NO_ROUTE` | as above |
| Wrong method on a known path | 405 | `SRV_METHOD_NOT_ALLOWED` | as above |
| Body over the route's limit | 422 | `SVC_BODY_TOO_LARGE` | as above |
| Pairing body malformed | 422 | `SVC_INPUT_INVALID` | as above |
| Pairing from an unusable Service API version | 409 | `SVC_VERSION_UNSUPPORTED` | as above; sentence names the side to update |
| Computer name empty or over 63 characters | 422 | `DESK_NAME_INVALID` | as above |
| Pairing code malformed or not a pending code | 403 | `DESK_CODE_INVALID` | as above; "That code didn't work. Ask for a new one from whoever runs your ordin server." |
| Pairing code expired | 403 | `DESK_CODE_EXPIRED` | as above; same sentence |
| Five wrong codes within 10 minutes | 403 | `DESK_PAIRING_STOPPED` | as above; same sentence |
| Ten desktops already paired | 403 | `DESK_TOO_MANY` | as above |
| Missing, malformed or unknown desktop key | 401 | `SVC_UNAUTHENTICATED` | `{"request_id":null,"kind":"error","error":{"code":"SVC_UNAUTHENTICATED",…}}` |
| Address locked after 10 failed key checks | 429 | `DESK_ADDRESS_LOCKED` | as above |
| Command rate above the pairing's bucket | 429 | `SVC_RATE_LIMITED` | as above, action `try_again` |
| Envelope, command, scope or input refused | 404 / 409 / 422 | the `SVC_` code `handle` returns, or `SRV_BAD_CURSOR` (422) | the `CommandReply` `handle` built |
| Approval press refused by the Approval Service | 200 | the `APPR_` code (e.g. `APPR_CARD_ALREADY_DECIDED`) | `{"request_id":…,"kind":"error","error":{"code":"APPR_CARD_ALREADY_DECIDED","message":"This was already decided on Asha's laptop."}}` |
| Implementation deadline or panic | 503 / 500 | `SVC_DEADLINE` / `SVC_INTERNAL` | the `CommandReply` `handle` built |
| Listen address not loopback or private, or port ≤ 1024 | start refused | `RAPI_ADDRESS_NOT_PRIVATE` | `RemoteApiError::AddressNotPrivate` (setup command and log) |
| Certificate chain or key missing or unreadable | start refused | `RAPI_TLS_MATERIAL_INVALID` | `RemoteApiError::TlsMaterialInvalid` |
| Port already in use | start refused, `Err(retryable)` | `RAPI_BIND_FAILED` | `RemoteApiError::BindFailed` |
| Datastore failure during pairing or key check | 503, `Err(retryable)` | `RAPI_DATASTORE` | `{"kind":"error","error":{"code":"RAPI_DATASTORE",…,"action":"try_again"}}` |
| `configure_listening` with a port outside 1025–65535 | `Err(permanent)` | `RAPI_PORT_INVALID` | `RemoteApiError::PortInvalid` (setup command shows `SERVER_PORT_INVALID`) |
| `configure_listening` while the profile forbids listening | `Err(permanent)` | `RAPI_LISTENING_FORBIDDEN` | `RemoteApiError::ListeningForbidden` (setup command shows `SERVER_LISTENING_FORBIDDEN`) |
| `configure_listening` cannot store or delete the key | `Err(retryable)` | `RAPI_SECRET_STORE` | `RemoteApiError::SecretStore` |
| `configure_listening`: the running server cannot bind the new address within 5 s | `Err(retryable)` | `RAPI_BIND_FAILED` | `RemoteApiError::BindFailed`; settings stay stored |
| `end_pairing` of an unknown or ended id | `Err(permanent)` | `DESK_UNKNOWN` | `RemoteApiError::UnknownPairing` |
| `Last-Event-ID` older than the journal keeps | 200, `reset` frame | `SVC_RESUME_TOO_OLD` (log only) | `event: reset` |
| Journal record not decodable or too large | frame skipped | `SVC_EVENT_INVALID` / `SVC_EVENT_TOO_LARGE` (log only) | — |

`DESK_CODE_INVALID`, `DESK_PAIRING_STOPPED`, `DESK_TOO_MANY` and `DESK_UNKNOWN` are *chosen here* under 2C's `DESK_` prefix.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Remote API

  Scenario: Happy path — pairing a desktop with a code
    Given the setup command started a pairing with code K7QM-2XPA at 10:02
    When a desktop posts /v1/pairing with code "K7QM2XPA" and computer_name "Asha's laptop" at 10:05
    Then the reply is 200 with a 43-character desktop_key and the server's backend id
    And the desktop_pairings row is paired and holds only the key's SHA-256
    And the desktop_pairing_codes row is gone

  Scenario: Error path — an expired code
    Given a pairing started at 10:02
    When a desktop posts /v1/pairing with its code at 10:13
    Then the reply is 403 with code DESK_CODE_EXPIRED
    And the message is "That code didn't work. Ask for a new one from whoever runs your ordin server."

  Scenario: Error path — five wrong codes stop pairing
    Given a pending pairing
    When five wrong codes are posted within 10 minutes
    Then the fifth reply is 403 with code DESK_CODE_INVALID
    And the next attempt, even with the right code, is 403 with code DESK_PAIRING_STOPPED
    And a new start_pairing from the setup command lifts the stop

  Scenario: Happy path — a command from a paired desktop
    Given a paired desktop "Asha's laptop"
    When it posts /v1/runs/stop with a valid envelope and its key
    Then the server's ServiceApi runs_stop is called with origin PairedDesktop and computer_name "Asha's laptop"
    And the response carries Ordin-Api-Version: 1.0

  Scenario: Happy path — an approval press is checked by the Approval Service
    Given an open approval card on the server
    When a paired desktop posts /v1/approval/decide with the approve card_ref
    Then ApprovalService.decide is called with origin PairedDesktop and that desktop's computer name
    And the reply is 200 with state "approved"

  Scenario: Error path — the card was decided in Telegram first
    Given a card already approved from Telegram
    When a paired desktop posts /v1/approval/decide for it
    Then the reply is 200 with kind "error" and code APPR_CARD_ALREADY_DECIDED

  Scenario: Error path — a wrong key
    When a request to /v1/runs/list carries a key that matches no paired desktop
    Then the reply is 401 with code SVC_UNAUTHENTICATED

  Scenario: Error path — ten failed key checks lock the address
    Given nine failed key checks from 10.20.4.30 within the last minute
    When a tenth request from 10.20.4.30 carries a wrong key
    Then that address is refused with 429 DESK_ADDRESS_LOCKED for 10 minutes, even with a right key
    And the journal entry remote_api_address_locked is appended without any key

  Scenario: Error path — a local-only command
    When a paired desktop posts /v1/servers/connect
    Then the reply is 404 with code SVC_COMMAND_LOCAL_ONLY

  Scenario: Error path — a browser request
    When a request to /v1/runs/list carries an Origin header
    Then the reply is 403 with code SRV_REFUSED

  Scenario: Happy path — resuming the event stream
    Given a paired desktop whose last event was 48212
    When it opens GET /v1/events with Last-Event-ID 48212
    Then the first frame has id 48213 and its event line equals its kind
    And a ": heartbeat" comment follows any 20 seconds without a frame

  Scenario: Edge case — resuming from before the last compaction
    Given the journal keeps events from 50000 onwards
    When a desktop opens GET /v1/events with Last-Event-ID 48212
    Then the first frame is event reset with id equal to the head sequence number

  Scenario: Edge case — a first stream with no Last-Event-ID
    When a newly paired desktop opens GET /v1/events without Last-Event-ID
    Then the first frame is event reset
    And later events follow live

  Scenario: Happy path — ending a pairing closes the stream
    Given a paired desktop with an open stream
    When the setup command ends its pairing
    Then within 2 seconds the stream closes
    And the next request with that key is 401 SVC_UNAUTHENTICATED

  Scenario: Edge case — the profile forbids listening
    Given server.listening_allowed is false
    When serve is called
    Then it returns Serving::Off and no port is opened

  Scenario: Error path — a public listen address
    Given the settings name 203.0.113.7 as the listen address
    When serve is called
    Then it fails with RAPI_ADDRESS_NOT_PRIVATE

  Scenario: Happy path — setup listen re-binds a running server
    Given a running personal server listening on 10.20.4.17:8443
    When configure_listening is called with 10.20.4.17, port 9443 and a valid certificate and key
    Then the key is stored under remote-api/tls-key and the remote_api_listening row holds port 9443
    And the server accepts connections on port 9443 within 1 second and no longer on 8443
    And open event streams are ended so desktops reconnect
    And the journal entry remote_api_listening_changed carries no key material

  Scenario: Error path — setup listen with a public address
    When configure_listening is called with 203.0.113.7 and port 8443
    Then it fails with RAPI_ADDRESS_NOT_PRIVATE and nothing is stored

  Scenario: Error path — setup listen while the profile forbids it
    Given server.listening_allowed is false
    When configure_listening is called with 10.20.4.17 and port 8443
    Then it fails with RAPI_LISTENING_FORBIDDEN

  Scenario: Edge case — switching listening off
    Given a running server listening on 10.20.4.17:8443
    When configure_listening is called with None
    Then no port is open, the row and the key are removed
    And listening() returns None

  Scenario: Edge case — history pruning removes old ended pairings only
    Given an ended pairing last connected 120 days ago and a paired desktop paired 200 days ago
    When the Journal prunes with a cutoff 90 days ago
    Then the ended pairing is deleted and the paired desktop's row is kept

  Scenario: Edge case — a resent change command
    Given approval.decide with request_id R was answered 3 minutes ago
    When the same desktop posts approval.decide again with request_id R
    Then the stored reply is returned and ApprovalService.decide is not called a second time
```

## Performance, Security, Observability

- **Performance targets:** on the company network a command that does not wait for a model answers within 150 ms p95 (A-46, 07 — Performance), of which this crate (TLS session reuse, key check, replay lookup, routing) takes ≤ 2 ms p95; an event reaches the desktop's screens within 500 ms of the journal recording it (A-46), of which this crate takes ≤ 50 ms p95 from the subscription yielding it to the frame being flushed; up to 10 paired desktops, each with one stream and 50 commands a second; memory ≤ 10,000 replay entries per pairing (bounded by reply size: entries over 256 KiB are not cached, and the request then dispatches again on a resend).
- **Security:** TLS 1.3 only, with the server owner's certificate, and client certificates when the profile requires them; listens only on loopback or a private address, never on the internet (ADR-096 item 4); every route except pairing requires a desktop key, checked in constant time against hashes before the body is read; 10 failed checks a minute lock an address for 10 minutes; pairing codes are single-use, valid 10 minutes, stored only as a hash, and stop after 5 wrong attempts; desktop keys are 32 bytes from the operating system's random source, sent once and stored only as SHA-256; `Origin` requests are refused and no CORS header is sent (SA-XC-11); `Local` commands are unreachable; approval presses go to the Approval Service's desktop-press check (R-85: a desktop key is as binding as the paired chat account). Never logged: keys, codes, bearer headers, bodies, replies, event data. Logged: client IP, pairing id, computer name, command name, status, code.
- **Observability:** log events `rapi.listening`, `rapi.off` (`reason`), `rapi.bind_failed`, `rapi.command`, `rapi.key_failed`, `rapi.stream`, `rapi.event_dropped` (`seq`, `kind`, `code`), `rapi.stream_lost`, with the fields of Internal Logic step 12; journal entries `remote_api_desktop_paired`, `remote_api_desktop_unpaired`, `remote_api_address_locked`, `remote_api_listening_changed`. No metrics or spans (personal server, SA-XC-04, SA-XC-05).
- C-71 (bound assertion): *SSE over HTTP/2 passes company proxies with 20 s comments*. The 20 s heartbeat is the HLD's measure; until C-71 is confirmed, the desktop's 45 s silence rule and `Last-Event-ID` resume are the fallback (ADR-096 item 3).

## Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111. Owner of the commands: `components/MemoryService.md` (amendment January 2026: the `memory.*` commands added). The agent profile's owners reach a hosted agent's memory through the Caller API (CallerApi.md amendment January 2026, route 33), not through this crate: the Remote API is built in the personal profile only.
> Nothing above changes.

### Purpose (addition)

A paired desktop that shows a personal server's workspace sees that workspace's things, links, fact history, fading facts and **What Ordin used** exactly as on its own workspaces, because the new `memory.*` commands reach the server through this crate's one command handler.

### Approach (addition)

Commands are not routed one by one (*Approach* above): the thirteen new commands are rows of the Service API catalogue (ServiceApi.md amendment January 2026) and are answered at `POST /v1/memory/<verb>` by the existing handler with no change here. A command sent with `scope_id` reaches the personal server's Memory Service, which refuses it with `MEM_NOT_SUPPORTED`; this crate passes that error object through unchanged. A non-owner's refusal for a hosted agent's memory is not this crate's: it is the Caller API's `AUTH_FORBIDDEN` (403) on route 33. This crate's own refusals stay as above (an unpaired or wrong key is refused before any command, *Error Table*), and the personal profile has one person, so it has no owner check.

### Shared Context, Public Interface, Internal Logic, Data Model, Error Table

N/A — no type, route, table or code changes.

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: A new memory command passes through the command handler
    Given a paired desktop key
    When POST /v1/memory/things is sent with {"workspace_id": W}
    Then ordin_service_api::handle is called with routed name "memory.things" and that body
    And the answer is the handler's result

  Scenario: Error — a scope id on the personal profile
    When POST /v1/memory/things is sent with {"scope_id": S}
    Then the answer carries the Service API error object with code MEM_NOT_SUPPORTED
```

### Performance, Security, Observability (additions)

N/A — no change.

### Gaps

None.

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 item 2; `ServiceApi.md` amendment January 2026 (SVC-RL-2, SA-SVC-RL-03).
>
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

- Every `Incoming` this crate builds — the `handle` call of step 3 and the direct `accept` call for `approval.decide` — sets `groups: api.groups()`, from the `ServiceApi` it serves on a personal server. This crate decides no group itself; `accept` refuses a command whose group is absent with `SVC_NOT_IN_THIS_VERSION`, which reaches the screens as any other `ServiceError`.
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
