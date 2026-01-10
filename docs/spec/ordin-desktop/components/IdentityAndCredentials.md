# SPEC: Identity and Credentials

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-identity` · **Build phase:** 1 · **Programs:** D P A
> **Dependencies:** Shared types

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
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)

## Purpose

Identity and Credentials signs the person in to the external systems ordin reaches with the person's own account — GitHub, the organisation's OIDC provider, Slack (user token), Confluence (Atlassian OAuth), Microsoft (SharePoint and Outlook), Google (Gmail) and the company's own mail server (single sign-on, or an app password or mail password) — keeps every token and password in the platform secret store, refreshes tokens before they expire, and hands a usable token only to the core module that asked for it (02:684–693; ADR-070, ADR-071; 07 — Authentication). It also owns the **secret store** every other module uses for named secrets: the OS keychain on the desktop, and on the ordin server the session's Secret Service or an encrypted `0600` file whose key the server owner supplies at start (02:574–578; ADR-073 *Secrets on the server*; C-50). It never signs anyone in to a model vendor and never reads a harness's credentials (ADR-044). No credential reaches the WebView, a log line or the journal (02:691).

## Approach

One crate, one `IdentityService` that runs the OAuth 2.0 flows itself over `reqwest` (SA-HTTP-02) — authorisation code with PKCE and a loopback redirect on the desktop, the device authorisation grant (RFC 8628) on the server (ADR-073 *Other sign-ins*) — and one `SecretStore` with three back ends behind one enum (OS keychain, Secret Service, encrypted file). Each sign-in is one `Credential` row (2C) whose secret material lives only in the secret store under `Credential.keychain_ref`. Profile values (client identifiers, tenants, issuers) are passed in by the caller in a `ProviderConfig`, because this crate has no Organisation Profile edge (2B-3). Rejected: the `oauth2` crate (not named in 1A/05; the four grant shapes needed are each under 100 lines over `reqwest`); one keychain item per token field (doubles keychain prompts on macOS; one JSON blob per credential instead).

## Shared Context

### Types copied from Phase 2C (domain part, `ordin-types`)

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// RFC 3339, UTC, millisecond precision, `Z` suffix (`2026-01-10T08:15:30.123Z`).
/// Serialisation refuses anything else with TS_NOT_RFC3339_MS_UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub DateTime<Utc>);

/// UUIDv7 newtype; a value that is not version 7 is refused with ID_NOT_UUIDV7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CredentialId(pub Uuid);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Credential {
    pub id: CredentialId,
    pub kind: CredentialKind,
    pub account_label: String,
    /// Key of the OS keychain item that holds the secret; the secret never enters the database.
    pub keychain_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
    pub state: CredentialState,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Github, Gitlab, Oidc, Slack, Confluence, Microsoft, Google,
    MailServerSingleSignOn, MailServerPassword, MailServerAppPassword,
    /// Jira Cloud (amended January 2026, ADR-106): a pasted API token or Atlassian OAuth; desktop only.
    Jira,
}
// `Gitlab` (decision D9): this component holds the GitLab sign-in so the Project Service can
// push over HTTPS with a keychain token.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState { Active, Revoked }
```

| Field (2C) | Rule | Error code |
|---|---|---|
| account_label | 1–254 chars (LD-10) | `CRED_LABEL_INVALID` |
| keychain_ref | 1–255 chars (LD-10); refers to an existing secret-store item while `active` | `CRED_KEYCHAIN_MISSING` |
| (kind, account_label) | unique among `active` rows | `CRED_DUPLICATE` |
| any field | never holds secret material | `CRED_SECRET_IN_RECORD` |

Lifecycle (2C): `— → active` on sign-in; `active → active` on refresh (`refreshed_at` set); `active → revoked` on sign-out.

### Service API error envelope (SA-ENV-01; used by the programs, not by this crate)

`ServiceError` (2C wire) and `RepairAction { TryAgain, SignInAgain, SendToIt }` (2C, `ordin_types`) are imported, never restated as local types (reconciliation row 51). This crate uses `RepairAction` and `CodedError` only.
This crate returns `CredError`; the program maps each variant to `ServiceError` through the error registry using the *Status* column of the Error Table, and Localisation builds `message`.

### Configuration this component reads (all passed in by the program; the crate calls no `std::env` function)

| Source | Name | Use here | Trace |
|---|---|---|---|
| Env (server, read by `apps/server` at start) | `ORDIN_SECRETS_KEY_FILE` | Path of the file holding the 32-byte key of the encrypted secrets file (container secret). Owner: this component (2D-1a). | C-50, ADR-073 |
| Env (server) | `ORDIN_SECRETS_KEY_CREDENTIAL` | Path of the key encrypted with `systemd-creds encrypt --user`; decrypted here with `systemd-creds decrypt --user`. Owner: this component (2D-1a). | C-50, OQ-44 |
| Env (server) | `DBUS_SESSION_BUS_ADDRESS` | Presence selects the Secret Service back end (2D-1b). | C-50 |
| Env (server) | `ORDIN_DATA_DIR` | Application data root; the encrypted file lives under it. Owner: ordin server. | ADR-072 |
| Profile (2D-2) | `microsoft.tenant_id`, `microsoft.client_id` | Microsoft sign-in (public client). | C-14 |
| Profile | `google.client_id`, `google.client_secret` | Google sign-in; the secret is non-confidential (Desktop OAuth client) and sent in the token exchange. | C-44; 08:82 |
| Profile | `identity_provider.issuer`, `.client_id`, `.scopes[]` | OIDC sign-in (`https` issuer only). | 02:697 |
| Profile | `confluence.oauth_client_id` | Atlassian OAuth, for Confluence and, from revision 0.54.0, for Jira (one Atlassian app, a separate Jira grant; amendment January 2026 — Jira). Absent → Jira is connected with a pasted API token only. | OQ-23, C-12; ADR-106 |
| Profile, or the person's own Slack app | `slack.client_id`; when absent, the client id the person pasted under **Settings › Slack › Connect your own Slack app** (kept by the Slack Service, `slack_connection.own_client_id`) | Slack sign-in (PKCE-enabled app). The program builds `ProviderConfig::Slack { workspace, client_id }` from whichever is present, the profile's first. Neither present → the program does not build the variant and `identity.sign_in` for `slack` returns `CRED_PROVIDER_NOT_CONFIGURED`. | 08:82; decision 82 |
| Profile | `email.sign_in_methods[]` (`single_sign_on`, `app_password`, `mail_password`) | Which mail sign-in to offer; the Mail Service passes the first method the server accepts. | ADR-071 |
| Profile | `gitlab.host` (default `gitlab.com`), `gitlab.client_id` (optional) | GitLab sign-in (SA-CLOSE-01): with a client id, OAuth device flow on that host; without one, a pasted personal access token. | D9; 2D-2 (Phase 3 closing round) |
| Profile | `server.client_certificate_name` | The name under which `import_client_certificate` keeps the person's client certificate and under which `client_certificate` reads it (SA-CLOSE-02). | `08-interfaces.md:85` |
| Build constant | `GITHUB_APP_CLIENT_ID` *(chosen here)* | Client identifier of ordin's GitHub App, compiled in by the branded build (`option_env!`); absent means GitHub sign-in is not offered. | OQ-6 (recommended GitHub App), ADR-041 build layer |

The program builds `ProviderConfig` values from the active `ProfileSnapshot` (`identity_provider: Option<IdentityProviderSection { issuer, client_id, scopes }>`, `microsoft: Option<MicrosoftSection { tenant_id, client_id }>`, `google: Option<GoogleSection { client_id, client_secret }>`, `confluence: Option<ConfluenceSection { site, oauth_client_id }>`, `slack: Option<SlackSection { workspace, dedicated_channel, client_id }>` (`client_id` absent → `CRED_PROVIDER_NOT_CONFIGURED`), `email: Option<EmailSection>` with `sign_in_methods: Vec<MailSignInMethod>`).

### Fixed constants (chosen here unless traced)

| Constant | Value |
|---|---|
| Refresh margin: refresh an access token when it expires within | 5 minutes |
| Browser sign-in wait (loopback listener open) | 10 minutes |
| Loopback redirect ports, tried in order *(chosen here)* | `47821`, `47822`, `47823` on `127.0.0.1` only; redirect URI `http://127.0.0.1:<port>/callback` |
| HTTP timeout per token-endpoint call | 15 seconds |
| `systemd-creds decrypt` timeout | 10 seconds |
| PKCE | S256, verifier 64 characters from `[A-Za-z0-9-._~]` |
| `state` parameter | 32 random bytes, base64url without padding |
| Mail password or app password length | 1–1024 bytes UTF-8 |
| Secret name pattern | `^[a-z0-9][a-z0-9_./-]{0,199}$` |
| Secret value size | ≤ 64 KiB |
| Encrypted secrets file | `<ORDIN_DATA_DIR>/secrets/secrets.enc`, directory `0700`, file `0600`; XChaCha20-Poly1305 |
| Slack user scopes (SA-SLACK-01) | `chat:write`, `channels:history`, `groups:history`, `channels:read`, `groups:read` — the scopes of every Slack sign-in |
| Slack user scopes asked later (SA-SLACK-02) | `users:read` (first channel linked), `users:read.email` and `im:write` (first direct message or receiver look-up); requested by the Slack Service through `begin_sign_in` with these scopes, never at upgrade; scope names *assumed here* from Slack's published Web API (C-84) |

### Confirmation items this spec carries as conditional (phase 1 table)

| Item | Assertion here | Fallback the HLD gives |
|---|---|---|
| C-06 | The GitHub App user token from this component reads the agent repositories. | A token with private-repository read scope from an OAuth App (ADR-014 text); the GitHub token is never handed to the Copilot SDK either way (ADR-044). |
| C-10 | The Project Service may give `gh` and git this component's GitHub token (`access_token(Github)`). | `gh` keeps its own sign-in, started by the Project Service; this component is not involved. |
| C-12 | Atlassian 3LO issues tokens to a public client using PKCE with no stored secret, with refresh tokens. | A company-hosted token exchange (C-12 text); no profile key names its address, so until C-12 passes, a refused Confluence token request ends in `CRED_PROVIDER_REFUSED` and Confluence stays off (HLD-impact candidate B2-F23). |
| C-14 | Microsoft Entra public client with PKCE serves Graph delegated permissions. | SharePoint and Outlook are not offered (`CRED_PROVIDER_REFUSED`). |
| C-49 | GitHub and Microsoft device sign-in work on the server under company rules. | The source is not offered on the server (`CRED_FLOW_NOT_OFFERED`). Atlassian has no device grant: Confluence is never offered on the server. |
| C-50 | The encrypted `0600` file with a systemd or container key keeps the server's secrets readable only by its user. | None stated; the lab result (systemd 257) is taken as the design. |

### Reconciliation assumptions

- **SA-RECON-01** This crate is handed the SQLite pool of the Journal's `Datastore` by the program and exports `MODULE_NAME` and `SQLITE_MIGRATIONS`; it has no Journal edge (phase 1; D4, R20).
- **SA-RECON-02** GitLab sign-in belongs here as `CredentialKind::Gitlab` (decision D9).
- **SA-RECON-03** Every caller receives `CredError` and matches on `CodedError::code()`; no module defines its own identity error type (rows 508, 511, 512).
- **SA-RECON-04** The `identity.*` commands (eight after SA-CLOSE-01 and SA-CLOSE-02; nine with `identity.complete_relayed_sign_in`, amended January 2026, ADR-102) and the `sign_in_changed` payload are the catalogue's (R1, R6); harness sign-ins are the Setup Assistant's (row 519).
- **SA-RECON-05** The server's secret back end follows `server_secret_backend`: a key variable selects the encrypted file, otherwise the session's Secret Service (row 503; the server program's order is the same).
- **SA-RECON-06** A company token for a hosted agent is refreshed within this component's 5-minute margin, not 60 seconds (row 514).
- **SA-RECON-07** The device-code challenge of a server sign-in is shown on the paired desktop's screen; the chat never takes a sign-in (O116).
- **SA-CLOSE-01** *(chosen here, closes ID-G3)* GitLab's host and client id come from the profile keys `gitlab.host` (default `gitlab.com`) and `gitlab.client_id` (2D-2). With a client id, `begin_sign_in` for `Gitlab` uses the OAuth device flow on that host, on the desktop as on the server. Without one, the person pastes a personal access token with the scopes `read_repository`, `write_repository` and `api` (`save_gitlab_token`); it is kept in the secret store as a `CredentialKind::Gitlab` credential with no refresh token.
- **SA-CLOSE-02** *(chosen here, closes ID-G4)* ordin reads no operating-system certificate store. The person imports a client certificate once, in Settings › Sign-ins, by picking a PEM certificate file and a PEM private-key file (`identity.import_client_certificate`); 05 names no PKCS#12 reader, so PKCS#12 is not accepted. Both are parsed with the PEM support of `rustls` (`rustls::pki_types::pem::PemObject`) and kept together in the secret store under `client-certificate/<SHA-256 of the name, 64 hex characters>`; `client_certificate(name)` reads them back. No library beyond `rustls`, already in 05.
- **SA-CLOSE-03** `SignInChanged` names its kind field `credential_kind`, because `kind` is the tag of the Service API event that carries it (ServiceApi.md SA-CLOSE-02).
- **SA-RECON-08** The Slack client id and the Google Desktop client's non-confidential secret come from the profile keys `slack.client_id` and `google.client_secret` (HLD 08:82, revision 0.49.0); this closes the two earlier client-registration gaps.

## Public Interface

All public items are in crate `ordin_identity`. Every `async fn` is cancel-safe at its `.await` points except where stated. Keychain calls are blocking and run on `tokio::task::spawn_blocking`.

### Construction

```rust
pub const MODULE_NAME: &str = "identity";
/// (version, description, SQL) — registered by the program with the Journal's MigrationRegistry.
pub const SQLITE_MIGRATIONS: &[(i64, &str, &str)] = &[(1, "create credentials and credential_meta", MIGRATION_1_SQL),
    (2, "jira credential kind", MIGRATION_2_SQL)];   // step 2: amended January 2026, ADR-106

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program { Desktop, PersonalServer, AgentSingleShape, AgentScaledShape }

pub struct IdentityConfig {
    pub program: Program,
    /// Application data root (desktop: the platform app-data folder; server: ORDIN_DATA_DIR).
    pub data_root: std::path::PathBuf,
    /// Keychain service name: the branded build's bundle identifier (ADR-041), e.g. "io.gatblau.ordin".
    pub keychain_service: String,
    pub secret_backend: SecretBackendConfig,
    /// Compiled-in GitHub App client id (`option_env!("GITHUB_APP_CLIENT_ID")`); None disables GitHub.
    pub github_app_client_id: Option<String>,
    /// Shared client built by the program (rustls + platform verifier, system proxy; SA-HTTP-02).
    pub http: reqwest::Client,
    /// Opens a URL in the system browser. Implemented by the desktop program (`apps/desktop`) over
    /// `tauri-plugin-opener` (OrdinDesktop.md, row 3.3); None on the server, which has no system browser
    /// and does not wire it (OrdinServer.md, not-wired list).
    pub browser: Option<std::sync::Arc<dyn BrowserOpener>>,
}

pub enum SecretBackendConfig {
    /// macOS login keychain, Windows Credential Manager (desktop).
    OsKeychain,
    /// The user session's Secret Service over D-Bus (server with a session bus).
    SecretService,
    /// `0600` file under the data root, key from a container secret file or a systemd credential.
    EncryptedFile { key_source: KeySource },
    /// Agent profile, scaled shape: no secret store (secrets come from ORDIN_SECRETS_DIR files read elsewhere).
    None,
}

pub enum KeySource { KeyFile(std::path::PathBuf), SystemdCredential(std::path::PathBuf) }

/// Pure rule that turns the server's environment values into a back end. The server program reads
/// `DBUS_SESSION_BUS_ADDRESS`, `ORDIN_SECRETS_KEY_FILE` and `ORDIN_SECRETS_KEY_CREDENTIAL` once at start
/// and passes their values here; the desktop program passes `SecretBackendConfig::OsKeychain` directly.
pub fn server_secret_backend(
    dbus_session_bus_address: Option<&str>,
    secrets_key_file: Option<&std::path::Path>,
    secrets_key_credential: Option<&std::path::Path>,
) -> Result<SecretBackendConfig, CredError>;

/// Implemented by the desktop program (OrdinDesktop.md, row 3.3); the server program does not implement it.
pub trait BrowserOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), String>;
}

pub struct IdentityService { /* private */ }

impl IdentityService {
    /// `pool` is the SQLite pool of the Journal's `Datastore`, handed over by the program because this
    /// phase-1 crate has no Journal edge (D4, R20); None only for `AgentScaledShape`.
    pub async fn new(config: IdentityConfig, pool: Option<sqlx::SqlitePool>) -> Result<Self, CredError>;
    /// A shared handle on the one secret store this service holds; each call returns a clone of the same `Arc`.
    pub fn secrets(&self) -> std::sync::Arc<SecretStore>;
    /// Server only (agent single shape and personal server under systemd): makes a new 32-byte
    /// random key, encrypts it with `systemd-creds encrypt --user` into `credential_path` (mode
    /// 0600) and returns. The file is what `ORDIN_SECRETS_KEY_CREDENTIAL` then names (2D-1a; this
    /// component owns the key variables). Refuses an existing file (CRED_SECRET_KEY_EXISTS).
    pub async fn create_secrets_key_credential(credential_path: &std::path::Path) -> Result<(), CredError>;
}
```

### Sign-in

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SignInChallengeId(pub Uuid); // UUIDv7, chosen here

#[derive(Debug, Clone)]
pub enum ProviderConfig {
    /// host: "github.com" or a GitHub Enterprise host; client id from IdentityConfig.
    Github { host: String },
    /// host: profile `gitlab.host` (default "gitlab.com"); client_id: profile `gitlab.client_id`
    /// (D9, SA-CLOSE-01). Signs in by the device flow only; without a client id the program
    /// offers `save_gitlab_token` instead and never builds this variant.
    Gitlab { host: String, client_id: String },
    Oidc { issuer: String, client_id: String, scopes: Vec<String> },
    Microsoft { tenant_id: String, client_id: String },
    /// `client_secret` is the non-confidential secret of a Desktop OAuth client (profile `google.client_secret`).
    Google { client_id: String, client_secret: Option<String> },
    Slack { workspace: String, client_id: String },
    Confluence { site: String, client_id: String },
    /// Jira Cloud through Atlassian OAuth (amended January 2026, ADR-106): `site` is the person's Jira site
    /// as `https://{name}.atlassian.net`; `client_id` is the profile's `confluence.oauth_client_id`.
    Jira { site: String, client_id: String },
    /// Company mail server single sign-on: tokens from the organisation's OIDC provider (assumed here, SA-CRED-12).
    MailServerSso { issuer: String, client_id: String, scopes: Vec<String>, mail_account: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInMethod {
    Browser,
    DeviceCode,
    /// Personal server only (decision 82): the server builds the authorisation URL and keeps the
    /// PKCE verifier and `state`; the paired desktop opens the URL in the person's browser with its
    /// own loopback listener and forwards the code. Slack only *(chosen here)*.
    RelayedBrowser,
}

#[derive(Debug, Clone)]
pub struct SignInRequest {
    pub provider: ProviderConfig,
    /// Scopes the caller needs (e.g. Graph `Files.ReadWrite.All`); `offline_access` is added for OIDC-style providers.
    pub scopes: Vec<String>,
    /// Desktop: Browser. Server: DeviceCode, or RelayedBrowser for `Slack`. Chosen by the program.
    pub method: SignInMethod,
    /// RelayedBrowser only: the loopback port the paired desktop has bound, one of 47821–47823.
    pub relay_port: Option<u16>,
    /// Words naming who needs it, shown by the caller ("Release planner"); never parsed here.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignInChallenge {
    /// The system browser was opened at the provider's page; wait for `wait_for_sign_in`.
    BrowserOpened { challenge_id: SignInChallengeId, expires_at: Timestamp },
    /// Show `verification_uri` and `user_code` to the person (chat or screen).
    DeviceCode { challenge_id: SignInChallengeId, verification_uri: String, user_code: String, expires_at: Timestamp },
    /// An active credential already covers the requested scopes: nothing is asked.
    AlreadySignedIn { credential: Credential },
    /// Personal server, `RelayedBrowser`: the paired desktop opens `authorisation_url` in the
    /// person's browser, receives the redirect on its own listener at `relay_port`, and calls
    /// `complete_relayed_sign_in` with the code and state. Then `wait_for_sign_in` as usual.
    RelayedBrowser { challenge_id: SignInChallengeId, authorisation_url: String, expires_at: Timestamp },
}

impl IdentityService {
    pub async fn begin_sign_in(&self, request: SignInRequest) -> Result<SignInChallenge, CredError>;
    /// Resolves when the provider issues tokens, the person cancels, or the challenge expires.
    pub async fn wait_for_sign_in(&self, challenge_id: SignInChallengeId) -> Result<Credential, CredError>;
    pub fn cancel_sign_in(&self, challenge_id: SignInChallengeId) -> Result<(), CredError>;
    /// Personal server only. Exchanges the code a paired desktop received for the
    /// challenge's tokens. `state` must equal the challenge's; the challenge must be open and of
    /// method RelayedBrowser. Resolves the pending `wait_for_sign_in` of that challenge.
    pub async fn complete_relayed_sign_in(&self, challenge_id: SignInChallengeId, code: SecretString, state: String)
        -> Result<(), CredError>;
}
```

### Mail passwords

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailPasswordKind { MailPassword, AppPassword }

/// A secret that never prints: Debug and Display write "[redacted]"; zeroised on drop.
pub struct SecretString(/* private: zeroize::Zeroizing<String> */);
impl SecretString { pub fn new(s: String) -> Self; pub fn expose(&self) -> &str; }

impl IdentityService {
    /// Stores the password the person typed once (Settings › Email or the add-email screen).
    /// Replaces the password of an existing active credential for the same account and clears its refusal mark.
    pub async fn save_mail_password(&self, account_label: &str, kind: MailPasswordKind, password: SecretString)
        -> Result<Credential, CredError>;
    /// GitLab without an OAuth application (profile `gitlab.client_id` absent, SA-CLOSE-01): stores
    /// the personal access token the person pasted, after checking it on `host` (step H.11).
    /// Replaces the token of an existing active GitLab credential for the same account.
    pub async fn save_gitlab_token(&self, host: &str, token: SecretString) -> Result<Credential, CredError>;
    /// Only the Mail Service's IMAP and SMTP adapter calls this.
    pub async fn mail_password(&self, credential_id: CredentialId) -> Result<SecretString, CredError>;
    /// The server refused this credential (IMAP/SMTP auth failure, provider `invalid_grant`, HTTP 401 after refresh).
    /// Marks it `needs_sign_in`; every later `mail_password`/`access_token` returns CRED_SIGN_IN_REQUIRED
    /// until the person signs in again. Never retried here (ADR-071).
    pub async fn report_refused(&self, credential_id: CredentialId, reason_code: &str) -> Result<(), CredError>;
}
```

### Tokens and state

```rust
pub struct AccessToken { pub token: SecretString, pub expires_at: Option<Timestamp>, pub credential_id: CredentialId }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInState { SignedOut, SignedIn, NeedsSignIn }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CredentialView {
    pub id: CredentialId,
    pub kind: CredentialKind,
    pub account_label: String,
    pub state: SignInState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RefreshReport { pub refreshed: u32, pub needs_sign_in: Vec<CredentialId>, pub unreachable: Vec<CredentialId> }

impl IdentityService {
    /// A valid access token, refreshed first when it expires within 5 minutes.
    /// `account_label` None selects the single active credential of that kind (CRED_AMBIGUOUS if several).
    pub async fn access_token(&self, kind: CredentialKind, account_label: Option<&str>) -> Result<AccessToken, CredError>;
    pub async fn sign_in_state(&self, kind: CredentialKind) -> Result<SignInState, CredError>;
    pub async fn list(&self) -> Result<Vec<CredentialView>, CredError>;
    /// Refreshes every active credential that expires within 5 minutes (Setup Assistant's background repair).
    pub async fn refresh_due(&self) -> Result<RefreshReport, CredError>;
    /// Deletes the secret, marks the row `revoked`. Idempotent on an already revoked row.
    pub async fn sign_out(&self, credential_id: CredentialId) -> Result<(), CredError>;
    /// Notified on every change of a credential's SignInState.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SignInChanged>;

    // ---- added in reconciliation (rows 508–517; R6: the provider adds what callers need)
    /// As `access_token(kind, None)`, and the credential's granted scopes must contain every
    /// entry of `scopes`; otherwise CRED_SCOPE_MISSING (the caller starts a sign-in with those
    /// scopes; incremental consent, step C.3).
    pub async fn access_token_for(&self, kind: CredentialKind, scopes: &[&str]) -> Result<AccessToken, CredError>;
    /// The token of one credential by id (step E), for callers that stored the id.
    pub async fn access_token_by_id(&self, credential_id: CredentialId) -> Result<AccessToken, CredError>;
    /// The person's company (OIDC) access token for one audience — a hosted agent's API
    /// (step H.2). `issuer` and `client_id` must equal those of the active `Oidc` credential.
    /// Refreshed when it expires within 5 minutes (this component's refresh margin).
    pub async fn access_token_for_audience(&self, issuer: &str, client_id: &str, audience: &str) -> Result<AccessToken, CredError>;
    /// Drops the cached token of that audience, so the next call refreshes it.
    pub async fn invalidate_audience_token(&self, issuer: &str, client_id: &str, audience: &str);
    /// Refresh with the stored refresh token only, never a prompt (step H.3). `Ok(true)` when a
    /// new access token was obtained, `Ok(false)` when there is no refresh token or the
    /// provider refused it (the credential is then marked needs sign-in).
    pub async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError>;
    /// The active mail credential (`MailServerSingleSignOn`, `MailServerPassword` or
    /// `MailServerAppPassword`); `None` when the person has not set up mail.
    pub async fn mail_account(&self) -> Result<Option<CredentialView>, CredError>;
    /// Who the person is, from the active credentials' labels (step H.4).
    pub async fn person(&self) -> Result<PersonProfile, CredError>;
    /// The client certificate the person imported under `name` (the profile's
    /// `server.client_certificate_name`), read from ordin's secret store; `Ok(None)` when none was
    /// imported under that name (step H.5, SA-CLOSE-02).
    pub async fn client_certificate(&self, name: &str) -> Result<Option<std::sync::Arc<rustls::sign::CertifiedKey>>, CredError>;
    /// Imports a client certificate once (Settings › Sign-ins): PEM certificate chain and PEM
    /// private key, kept in the secret store under `name` (step H.12). Replaces an earlier import.
    pub async fn import_client_certificate(&self, name: &str, certificate_pem: &[u8], key_pem: zeroize::Zeroizing<Vec<u8>>)
        -> Result<ClientCertificateView, CredError>;
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientCertificateView {
    pub name: String,
    /// The leaf certificate's subject common name, ≤ 254 characters.
    pub subject: String,
    pub not_after: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PersonProfile {
    /// The ID token's `name` claim, else the GitHub profile name, else the email's local part.
    pub name: String,
    /// The account label of the active `Oidc`, `Microsoft` or `Google` credential, in that order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SignInChanged { pub credential_id: CredentialId, pub credential_kind: CredentialKind, pub state: SignInState }
```

### Secret store (named secrets for other modules)

```rust
/// Validated name, pattern `^[a-z0-9][a-z0-9_./-]{0,199}$`. Convention: `<module>/<purpose>/<id>`,
/// e.g. `server-connector/desktop-key/0192…`, `telegram/bot-token`, `credential/github/0192…`.
pub struct SecretName(/* private */ String);
impl SecretName { pub fn new(s: &str) -> Result<Self, CredError>; }

pub struct SecretStore { /* private */ }
impl SecretStore {
    pub async fn put(&self, name: &SecretName, value: &[u8]) -> Result<(), CredError>;
    /// Ok(None) when absent.
    pub async fn get(&self, name: &SecretName) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, CredError>;
    pub async fn delete(&self, name: &SecretName) -> Result<(), CredError>;
    pub fn backend(&self) -> SecretBackendKind;
    /// The value under `name`, or `len` (16–64) random bytes written there first when absent
    /// (an HMAC key a module needs once per installation). One writer at a time per name.
    pub async fn get_or_create_random(&self, name: &SecretName, len: usize) -> Result<zeroize::Zeroizing<Vec<u8>>, CredError>;
    /// The key the person entered for `key_name` of an agent instance (SA-CRED-01), stored under
    /// `agent-mcp/<agent instance id>/<key_name>`. `Ok(None)` when not entered.
    pub async fn agent_key(&self, agent_instance_id: AgentInstanceId, key_name: &str) -> Result<Option<zeroize::Zeroizing<Vec<u8>>>, CredError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretBackendKind { OsKeychain, SecretService, EncryptedFile, None }
```
Fixed names *(chosen here)*: `remote-api/tls-key` (the Remote API's TLS private key, PEM; written by the server program's `configure_listening`, read by the Remote API); `server-connector/desktop-key/<server id>` (one per paired server). Three narrow interfaces over these names, defined here and implemented by `IdentityService`, serve the Remote API, the Server Connector and the server program; every other module uses `SecretStore::{put, get, delete}` directly. Errors are `CredError` throughout (there is no `IdentityError`, `SecretText`, `SecretStoreError` or `CredentialError`; `ServerSecretStore` and `SandboxSecrets` are replaced by `SecretStore` and the methods above).

```rust
/// The Remote API's TLS private key on the server (secret name `remote-api/tls-key`).
#[async_trait::async_trait]
pub trait ServerSecrets: Send + Sync {
    /// The PEM the setup command stored; `Ok(None)` when absent.
    async fn remote_api_tls_key(&self) -> Result<Option<zeroize::Zeroizing<String>>, CredError>;
    /// Stores (replaces) that key.
    async fn put_remote_api_tls_key(&self, pem: zeroize::Zeroizing<String>) -> Result<(), CredError>;
    /// Removes it; succeeds when already absent.
    async fn delete_remote_api_tls_key(&self) -> Result<(), CredError>;
}

/// The desktop's per-server keys (secret names `server-connector/desktop-key/<id>`, passed in
/// whole as `keychain_ref`) and the client certificate the profile names.
#[async_trait::async_trait]
pub trait DesktopKeyStore: Send + Sync {
    async fn put_desktop_key(&self, keychain_ref: &str, key: &[u8; 32]) -> Result<(), CredError>;
    async fn get_desktop_key(&self, keychain_ref: &str) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, CredError>;
    async fn delete_desktop_key(&self, keychain_ref: &str) -> Result<(), CredError>;
    /// As `IdentityService::client_certificate` (step H.5).
    async fn client_certificate(&self, name: &str) -> Result<Option<std::sync::Arc<rustls::sign::CertifiedKey>>, CredError>;
}

impl ServerSecrets for IdentityService { /* step H.10 */ }
impl DesktopKeyStore for IdentityService { /* step H.10 */ }

impl IdentityService {
    /// The server setup command's `secret NAME` step: stores `value` (UTF-8) under `name`.
    /// `name` must pass `SecretName::new`; empty `value` → CRED_PASSWORD_INVALID.
    pub async fn put_server_secret(&self, name: &str, value: &str) -> Result<(), CredError>;
}
```

Users named in the HLD: Server Connector (desktop keys), Remote API (TLS key), Telegram Adapter, Slack Channel Adapter, NATS Adapter (bot, app-level and NATS credentials), Model Policy and Connection Binder (single shape), Sandbox (`PYS_AUDIT_HMAC_KEY_REF`, 2D-1f), and SA-CRED-01 (an agent's MCP server credential, stored under `agent-mcp/<agent instance id>/<server name>`).

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum CredError {
    #[error("{code}")] Retryable { code: &'static str },
    #[error("{code}")] Permanent { code: &'static str },
}
impl ordin_types::CodedError for CredError {
    fn code(&self) -> &'static str;
    fn retryable(&self) -> bool;
    /// `SignInAgain` for CRED_SIGN_IN_REQUIRED, CRED_SCOPE_MISSING, CRED_KEYCHAIN_MISSING, CRED_RELAY_STATE_MISMATCH,
    /// CRED_SIGN_IN_CANCELLED, CRED_SIGN_IN_TIMEOUT; `TryAgain` for every other retryable code;
    /// `SendToIt` for CRED_URL_INVALID, CRED_PROVIDER_NOT_CONFIGURED, CRED_PROVIDER_REFUSED,
    /// CRED_LABEL_INVALID; `None` otherwise — the *Response Body* column of the Error Table.
    fn repair(&self) -> Option<ordin_types::RepairAction>;
}
```
Codes are those of the Error Table; `Display` writes the code only, never a provider message, token or path.

### Service API commands (implemented by the programs; names *chosen here*, SA-API-01)

| Command | Input | Result | Calls |
|---|---|---|---|
| `identity.list` | `{}` | `{"items": [CredentialView]}` | `list()` |
| `identity.sign_in` | `{"kind": CredentialKind, "reason": string?, "relay_port": u16?, "site": string?}` | `SignInChallenge` (without credential secrets) | program builds `ProviderConfig` and scopes for that kind, then `begin_sign_in`; `relay_port` read only on the personal server for `slack` (*Amended January 2026 (ADR-102).*); `site` read only for `jira`, the person's Jira site address (*amended January 2026, ADR-106*) |
| `identity.sign_in_wait` | `{"challenge_id": SignInChallengeId}` | `CredentialView` | `wait_for_sign_in` |
| `identity.sign_in_cancel` | `{"challenge_id": SignInChallengeId}` | `{}` | `cancel_sign_in` |
| `identity.complete_relayed_sign_in` | `{"challenge_id": SignInChallengeId, "code": string, "state": string}` | `{}` | `complete_relayed_sign_in`; personal server only (scope `Any`, reached from the paired desktop through the Remote API, ADR-096); carries a secret in |
| `identity.sign_out` | `{"credential_id": CredentialId}` | `{}` | `sign_out` |
| `identity.save_mail_password` | `{"account_label": string, "kind": "mail_password" \| "app_password", "password": string}` | `CredentialView` | `save_mail_password` |
| `identity.save_gitlab_token` | `{"token": string}` | `CredentialView` | program passes the profile's `gitlab.host` (default `gitlab.com`); refused with `CRED_FLOW_NOT_OFFERED` when the profile has `gitlab.client_id` (SA-CLOSE-01) |
| `identity.import_client_certificate` | `{}` | `{"certificate": ClientCertificateView?}` | desktop only (scope `Local`): the desktop program opens the operating system's file picker twice (the certificate file, then the key file; the paths never reach the screens), reads each file (≤ 64 KiB, else `CRED_CERTIFICATE_INVALID`) and passes the profile's `server.client_certificate_name` (absent → `CRED_PROVIDER_NOT_CONFIGURED`); either picker cancelled → `certificate` absent, nothing stored (SA-CLOSE-02) |

Event `sign_in_changed {credential_id, credential_kind, state}` (2C `EventKind::SignInChanged`), emitted by the program from `subscribe()` through `Journal::append_event`. These eight commands and this payload are the definitive ones; the Service API catalogue takes them as written (R1) — there are no `signins.*` commands and no `Account` enumeration here. A harness sign-in (Claude Code, Copilot CLI) is not a credential of this component: it is the Setup Assistant's `setup.sign_in_harness`.

### Who calls each public item (reconciliation §3; R6)

| Item | Called by |
|---|---|
| `MODULE_NAME`, `SQLITE_MIGRATIONS` | the programs, registering them with the Journal's `MigrationRegistry`; the program also hands `IdentityService::new` the SQLite pool of the Journal's `Datastore` (R20) |
| `server_secret_backend`, `IdentityService::new`, `create_secrets_key_credential` | server program (`server_secret_backend` then `new`; `create_secrets_key_credential` in its setup command); desktop program (`new` with `OsKeychain`) |
| `ServerSecrets` | Remote API (read, store and remove its TLS key); server program's `configure_listening` |
| `DesktopKeyStore` | Server Connector (desktop keys and client certificate) |
| `put_server_secret` | server program, setup command `secret NAME` |
| `secrets()` → `SecretStore::{put, get, delete}` | Telegram Adapter, Slack Channel Adapter, NATS Adapter, Model Policy; the server program, for the Connection Binder's `SecretSource` |
| `SecretStore::get_or_create_random`, `SecretStore::agent_key` | Sandbox (`PYS_AUDIT_HMAC_KEY_REF` and agent keys) |
| `begin_sign_in`, `wait_for_sign_in`, `cancel_sign_in`, `list`, `sign_out`, `save_mail_password`, `save_gitlab_token`, `import_client_certificate` | the programs, for the nine `identity.*` commands; Slack Service (SA-SLACK-02 scopes, through the same `identity.sign_in` path the screens use — the Slack Service never opens a browser itself) |
| `access_token` | Project Service (`Github`, `Gitlab`), Slack Thread Adapter through the program's `SlackUserTokenSource` (`access_token(Slack, None)`), Slack Service (`access_token(Slack, None)` for reading and channel posts), Mail Service (`MailServerSingleSignOn`), Setup Assistant (help request to a ticket address with `access_token(Oidc, None)` or to a Slack channel with `access_token(Slack, None)`) |
| `access_token_for` | Confluence Adapter, SharePoint Adapter (scopes they need), Slack Service (SA-SLACK-02 scopes before `users.info`, `users.lookupByEmail`, `conversations.open`) |
| `access_token_by_id` | Sandbox |
| `access_token_for_audience`, `invalidate_audience_token` | Hosted Agent Adapter, through the program's `CompanyTokenSource` |
| `refresh_silently` | Copilot Local Adapter (`Github`), SharePoint Adapter (`Microsoft`, once after a Graph 401) |
| `mail_account`, `mail_password`, `report_refused` | Mail Service (`report_refused` also from the Slack Thread Adapter's token source and from the Slack Service on `invalid_auth`, `token_revoked`, `not_authed`, `account_inactive`) |
| `person` | Handover Service, through the program's `PersonIdentity` |
| `client_certificate` | Server Connector (client certificate required by the profile) |
| `sign_in_state`, `refresh_due` | Setup Assistant (checks and background repair) |
| `subscribe` | the programs (forwarding `sign_in_changed`) |

### Example

```rust
// SharePoint Adapter asks for a Graph token; no Microsoft sign-in yet on the desktop.
let req = SignInRequest {
    provider: ProviderConfig::Microsoft { tenant_id: "4a1c…-tenant".into(), client_id: "9f0e…-app".into() },
    scopes: vec!["https://graph.microsoft.com/Files.ReadWrite.All".into(), "https://graph.microsoft.com/Sites.Read.All".into()],
    method: SignInMethod::Browser,
    reason: Some("SharePoint".into()),
};
let ch = identity.begin_sign_in(req).await?;
// → BrowserOpened { challenge_id: 0192a3…, expires_at: "2026-01-10T09:40:00.000Z" }
let cred = identity.wait_for_sign_in(ch_id).await?;
// → Credential { kind: Microsoft, account_label: "asha.rao@acme.com", keychain_ref: "credential/microsoft/0192a3f0-…", state: Active, expires_at: Some("2026-01-10T10:31:12.000Z"), … }
let tok = identity.access_token(CredentialKind::Microsoft, None).await?; // AccessToken { token: [redacted], … }
```
Server (device code) reply returned by `identity.sign_in` over the Remote API and shown on the paired desktop's screen (the chat never takes a sign-in):
```json
{"kind":"device_code","challenge_id":"0192a3f1-7c2e-7d10-9a11-2b3c4d5e6f70","verification_uri":"https://github.com/login/device","user_code":"WDJB-MJHT","expires_at":"2026-01-10T09:45:00.000Z"}
```

## Internal Logic

### A. Start-up (`IdentityService::new`)

1. Validate `config`: `program` and `secret_backend` agree (`OsKeychain` only with `Desktop`; `None` only with `AgentScaledShape`; `pool` is `Some` except for `AgentScaledShape`); else `CRED_CONFIG_INVALID`.
2. Open the secret store (section D). Failure stops the program start with the code (the program logs it and exits; the instance never reports ready, 2D-1a).
3. Load every `active` credential row with its `credential_meta` row into an in-memory map keyed by id. Secrets are not read at start (no keychain prompt before use).
4. For each active row whose secret item is missing in the store, set `credential_meta.needs_sign_in = 1`, `last_error_code = 'CRED_KEYCHAIN_MISSING'`; log `cred.secret_missing` (warn) with `credential_id`, `kind`.

### B. `server_secret_backend` rule

1. Both key paths given → `CRED_SECRET_KEY_CONFLICT` (2D-1a: "Setting both … stops the start with a named error").
2. `secrets_key_file` given → `EncryptedFile { KeyFile }`.
3. `secrets_key_credential` given → `EncryptedFile { SystemdCredential }`.
4. Neither, and `dbus_session_bus_address` non-empty → `SecretService`.
5. Neither and no session bus → `CRED_SECRET_KEY_MISSING`.

### C. Sign-in (`begin_sign_in`, `wait_for_sign_in`)

1. Validate the request: `issuer`/`site` are `https` URLs ≤ 2048 bytes (`CRED_URL_INVALID`); client ids 1–256 chars (`CRED_PROVIDER_NOT_CONFIGURED` when empty); `Github` requires `github_app_client_id` (`CRED_PROVIDER_NOT_CONFIGURED`).
2. Refuse a flow the program cannot offer (`CRED_FLOW_NOT_OFFERED`): `Browser` without `config.browser`; `DeviceCode` with `Slack`, `Confluence` or `Google` (no device grant usable for their scopes; ADR-073, C-49); `RelayedBrowser` unless `program` is the personal server and the provider is `Slack`, or with `relay_port` absent or outside 47821–47823 (*amended January 2026, ADR-102*); any sign-in when `program` is `AgentSingleShape` or `AgentScaledShape` (no person signs in, ADR-097).
3. Find an active credential of the same kind (and, for `MailServerSso`, the same `mail_account`). If its `credential_meta.scopes` contain every requested scope and it is not `needs_sign_in`, return `AlreadySignedIn`. Otherwise the new sign-in requests the union of stored and requested scopes (incremental consent: mail permissions added to the SharePoint sign-in, C-43) and, on success, updates that row instead of adding one.
4. Resolve endpoints:
   - `Github`: device `https://<host>/login/device/code`; token `https://<host>/login/oauth/access_token`; on the desktop the device grant is used too (no client secret; the browser is opened at `https://<host>/login/device` and the code shown) *(assumed here, SA-CRED-11)*.
   - `Oidc`, `MailServerSso`: `GET <issuer>/.well-known/openid-configuration` (timeout 15 s), read `authorization_endpoint`, `token_endpoint`, `device_authorization_endpoint`; the discovered `issuer` must equal the configured one (`CRED_PROVIDER_REFUSED`). Missing `device_authorization_endpoint` with `DeviceCode` → `CRED_FLOW_NOT_OFFERED`.
   - `Microsoft`: `https://login.microsoftonline.com/<tenant_id>/oauth2/v2.0/{authorize,token,devicecode}`.
   - `Google`: `https://accounts.google.com/o/oauth2/v2/auth`, `https://oauth2.googleapis.com/token`.
   - `Gitlab` (D9, SA-CLOSE-01): device `https://<host>/oauth/authorize_device`, token `https://<host>/oauth/token`; scopes `read_repository`, `write_repository`, `api` (`api` is needed to open merge requests and to read the user); public client, no client secret. The device flow is used whatever `method` says: on the desktop the browser is opened at the returned `verification_uri` and the code is shown, as for GitHub.
   - `Slack`: `https://slack.com/oauth/v2/authorize` with `user_scope=` the scopes step 3 settled (the SA-SLACK-01 scopes always, plus any SA-SLACK-02 scope requested now or already held — the union rule of step 3 unchanged) and `team` from the workspace; token `https://slack.com/api/oauth.v2.access`. *Amended January 2026 (ADR-102).*
   - `Confluence`: `https://auth.atlassian.com/authorize` with `audience=api.atlassian.com&prompt=consent`; token `https://auth.atlassian.com/oauth/token`.
   Discovery failure (DNS, TLS, timeout, non-2xx) → `CRED_PROVIDER_UNREACHABLE` (retryable).
5. **Browser method:** bind the first free port of 47821–47823 on `127.0.0.1` (all busy → `CRED_LOOPBACK_BUSY`); build PKCE verifier and S256 challenge, a `state`, and the authorisation URL with `response_type=code`, `redirect_uri`, `scope` (plus `offline_access` for OIDC, Microsoft and Confluence; `access_type=offline` for Google), `code_challenge`; call `browser.open(url)` (failure → `CRED_BROWSER_FAILED`); return `BrowserOpened` with `expires_at = now + 10 min`. The listener answers exactly one `GET /callback`: wrong or missing `state` → HTTP 400 page and the flow stays open; `error=access_denied` → `CRED_SIGN_IN_CANCELLED`; other `error` → `CRED_PROVIDER_REFUSED`; a `code` → token exchange (step 7). The listener returns a static page "You're signed in. You can close this tab." in the build's locale *(text chosen here, 11 † row)* and closes.
5a. **Relayed browser method (personal server, Slack; decision 82).** Bind no port. Build the PKCE verifier (S256), a `state` of 32 random bytes in base64url, and the authorisation URL exactly as step 5 does, with `redirect_uri = http://127.0.0.1:<relay_port>/callback`. Keep `{verifier, state, redirect_uri, provider, scopes, row to update}` in the in-memory pending challenge (the 4-challenge cap of step 12 applies); return `RelayedBrowser { challenge_id, authorisation_url, expires_at = now + 10 min }`. The verifier never leaves the server. `complete_relayed_sign_in(id, code, state)`: unknown or ended challenge → `CRED_CHALLENGE_UNKNOWN`; method not `RelayedBrowser` → `CRED_FLOW_NOT_OFFERED`; `state` not equal by constant-time comparison → `CRED_RELAY_STATE_MISMATCH` (the challenge stays open until it expires; three mismatches end it with the same code, *chosen here*); after `expires_at` → `CRED_SIGN_IN_TIMEOUT`. Otherwise run step 7 (token exchange with the kept verifier and redirect URI) and steps 8–11; the pending `wait_for_sign_in` returns the credential. The code is held only for the exchange and dropped (zeroised) after it. Log `cred.sign_in.relayed` with `challenge_id`, `kind` (never the code or the state).
6. **Device method:** POST the device authorisation endpoint with `client_id` and `scope`; return `DeviceCode` with the provider's `verification_uri` (or `verification_uri_complete` when given), `user_code`, `expires_at = now + expires_in`. `wait_for_sign_in` polls the token endpoint every `interval` seconds (default 5), adding 5 s on `slow_down`, continuing on `authorization_pending`, ending with `CRED_SIGN_IN_CANCELLED` on `access_denied` and `CRED_SIGN_IN_TIMEOUT` on `expired_token` or at `expires_at`.
7. Token exchange: POST `grant_type=authorization_code` (or the device grant type) with `code_verifier`, `redirect_uri`, `client_id`; no client secret is sent by any flow except Google's, which sends the profile's non-confidential `google.client_secret` when present (a Desktop OAuth client; 08:82). Non-2xx with `invalid_client` or `unauthorized_client` → `CRED_PROVIDER_REFUSED` (this is how C-12 or C-14 failing shows); network failure → `CRED_PROVIDER_UNREACHABLE`. Read `access_token`, `refresh_token`, `expires_in`, `scope`, `id_token`.
8. Account label: OIDC, Microsoft, Google, MailServerSso → `preferred_username`, else `email`, else `sub` of the ID token's payload (decoded only to read the label; trust comes from the TLS token endpoint); GitHub → `GET https://api.<host>/user` (`api.github.com` for `github.com`) `login`, and its `name` kept as `display_name`; GitLab → `GET https://<host>/api/v4/user` `username`, and its `name` kept as `display_name`; Slack → `authed_user.id` and `team.name` joined as `<team>/<user id>`; Confluence → `GET https://api.atlassian.com/oauth/token/accessible-resources`, the resource whose `url` equals `site`, label `<site host>` (no matching resource → `CRED_PROVIDER_REFUSED`); `MailServerSso` → `mail_account`. Label checked 1–254 chars (`CRED_LABEL_INVALID`).
9. For OIDC, Microsoft and Google also keep the ID token's `name` claim (≤ 254 characters, else cut) as `credential_meta.display_name`.
10. Persist (one transaction): write the token blob `{"access_token","refresh_token","expires_at","scopes","token_type"}` to the secret store under `credential/<kind>/<credential id>` first; then insert or update the `credentials` row (`state='active'`, `expires_at`, `refreshed_at` on update) and `credential_meta` (`provider_host`, `scopes`, `needs_sign_in=0`, `refused_at=NULL`). A unique-index violation on `(kind, account_label)` among active rows → `CRED_DUPLICATE`. If the row write fails after the secret write, delete the secret item (no orphan) and return `CRED_STORE_FAILED`.
11. Publish `SignInChanged { state: SignedIn }`; log `cred.sign_in.completed`.
12. `cancel_sign_in` closes the listener, stops polling, or ends a relayed challenge and makes the pending `wait_for_sign_in` return `CRED_SIGN_IN_CANCELLED`; an unknown id → `CRED_CHALLENGE_UNKNOWN`. Pending challenges are held in memory only, at most 4 at once (`CRED_TOO_MANY_SIGN_INS` beyond).

### D. Secret store

1. **OsKeychain:** `keyring::Entry::new(keychain_service, name)`; `get_secret`/`set_secret`/`delete_credential`. `keyring::Error::NoEntry` → `Ok(None)` on get, success on delete. `PlatformFailure` or `NoStorageAccess` (locked keychain, refused prompt) → `CRED_KEYCHAIN_UNAVAILABLE` (retryable). Library: `keyring` 3 with `apple-native`, `windows-native` (as `sandbox/sandbox-host/Cargo.toml:61`; HLD 05, row *OS keychain and Secret Service*).
2. **SecretService:** the same `keyring` crate with `sync-secret-service`; a D-Bus error → `CRED_KEYCHAIN_UNAVAILABLE`.
3. **EncryptedFile:**
   1. Key: `KeyFile(p)` → read `p`; `SystemdCredential(p)` → run `systemd-creds decrypt --user <p> -` with no shell, a cleared environment except `PATH` and `XDG_RUNTIME_DIR`, timeout 10 s; non-zero exit or timeout → `CRED_SYSTEMD_CREDS_FAILED`; binary absent → same code. The bytes, trimmed of ASCII whitespace, must be exactly 32 raw bytes or the standard base64 of 32 bytes; otherwise `CRED_SECRET_KEY_INVALID`. The key is kept in memory only (`Zeroizing<[u8; 32]>`).
   2. Create `<data_root>/secrets/` with mode `0700` if absent. If `secrets.enc` exists with a mode other than `0600`, set it to `0600` and log `cred.secret_file_mode_fixed` (warn) (the systemd lab created `0644`, C-50).
   3. File format (JSON): `{"format":1,"nonce":"<base64, 24 bytes>","ciphertext":"<base64>"}`; plaintext is a JSON object name → base64 value; AEAD XChaCha20-Poly1305 with associated data `ordin-secrets-v1`. Decryption failure → `CRED_SECRET_STORE_CORRUPT` (wrong key or tampered file; permanent; the start stops and the file is left untouched). `format` other than 1 → same code.
   4. Each `put`/`delete` holds a `tokio::sync::Mutex`, rewrites the whole file with a fresh nonce to `secrets.enc.tmp` (mode `0600`), `fsync`, renames over `secrets.enc`, `fsync`s the directory. I/O failure → `CRED_SECRET_STORE_WRITE_FAILED` (retryable); the previous file stays intact.
   5. AEAD library: `chacha20poly1305` 0.10 (`Apache-2.0 OR MIT`) — HLD 05, row *Server secrets file*.
4. **None:** every call returns `CRED_SECRET_STORE_ABSENT`.
5. Value size > 64 KiB → `CRED_SECRET_TOO_LARGE`. `SecretName::new` failing the pattern → `CRED_SECRET_NAME_INVALID`.

### E. `access_token`

1. Select the credential: by `(kind, account_label)` among active rows, or the single active row of `kind`; none → `CRED_SIGN_IN_REQUIRED` (action Sign in again); several with `None` → `CRED_AMBIGUOUS`. Kinds `MailServerPassword`, `MailServerAppPassword` → `CRED_KIND_HAS_NO_TOKEN`.
2. `needs_sign_in = 1` → `CRED_SIGN_IN_REQUIRED`.
3. Read the token blob (cached in memory after first read, cache cleared on sign-out and refresh). Missing → set `needs_sign_in`, `CRED_KEYCHAIN_MISSING`.
4. If `expires_at` is within 5 minutes: refresh, one refresh per credential at a time (per-credential `Mutex`; concurrent callers wait for the same result). POST `grant_type=refresh_token`. Success → write the new blob (rotate the refresh token when the provider returns one), update `expires_at`, `refreshed_at`; log `cred.refreshed`. `invalid_grant` or HTTP 400/401 → `needs_sign_in=1`, `last_error_code='CRED_REFRESH_REFUSED'`, publish `NeedsSignIn`, return `CRED_SIGN_IN_REQUIRED` (the caller pauses its run, 07). Network failure → `CRED_PROVIDER_UNREACHABLE` (retryable; `needs_sign_in` unchanged). No refresh token and expired → `CRED_SIGN_IN_REQUIRED`.
5. Return `AccessToken`.

### F. Mail passwords

1. `save_mail_password`: label 1–254 (`CRED_LABEL_INVALID`), password 1–1024 bytes (`CRED_PASSWORD_INVALID`); kind maps to `MailServerPassword` or `MailServerAppPassword`. Write the secret `credential/<kind>/<id>` (raw UTF-8), then the row as in C.9; an existing active row for the same account (either password kind) is updated in place and its kind changed if the person switched. Clear `needs_sign_in`. Never logged; never echoed in any result.
2. `mail_password`: row must be an active password kind (`CRED_NOT_FOUND`, `CRED_KIND_MISMATCH`); `needs_sign_in = 1` → `CRED_SIGN_IN_REQUIRED`; read the secret (missing → `CRED_KEYCHAIN_MISSING`).
3. `report_refused`: set `needs_sign_in=1`, `refused_at=now`, `last_error_code=reason_code` (≤ 64 chars `[A-Z0-9_]`, else stored as `UNSPECIFIED`); publish `NeedsSignIn`; log `cred.refused`. The secret is kept until the person signs in again or out, so nothing is asked twice and nothing is retried.

### G. `refresh_due`, `sign_out`, `list`

1. `refresh_due`: for each active, not `needs_sign_in`, token-kind credential with `expires_at` within 5 minutes, run E.4; collect counts. A single credential's failure does not stop the others.
2. `sign_out`: unknown id → `CRED_NOT_FOUND`. For token kinds, a best-effort revocation call is made where the provider has one (Google `https://oauth2.googleapis.com/revoke`, Slack `auth.revoke`, GitHub none, Microsoft none) with a 5 s timeout, failures logged only. Delete the secret (keychain errors → `CRED_KEYCHAIN_UNAVAILABLE`, row unchanged); set `state='revoked'`; publish `SignedOut`; log `cred.signed_out`.
3. `list`: active rows; `state` = `NeedsSignIn` when `needs_sign_in = 1`, else `SignedIn`.

### H. Methods added in reconciliation

1. **`access_token_for(kind, scopes)`**: select as E.1–E.2; when `credential_meta.scopes` lacks any entry of `scopes` → `CRED_SCOPE_MISSING` (permanent, action Sign in again; the caller starts `begin_sign_in` with those scopes). Otherwise E.3–E.5. **`access_token_by_id(id)`**: row must exist and be active (`CRED_NOT_FOUND`); a password kind → `CRED_KIND_HAS_NO_TOKEN`; then E.2–E.5.
2. **`access_token_for_audience(issuer, client_id, audience)`**: `audience` 1–512 characters (`CRED_URL_INVALID` beyond). The active `Oidc` credential must have this `issuer` and `client_id` (stored in `credential_meta.provider_host` and the token blob) → else `CRED_SIGN_IN_REQUIRED`. A cached audience token (memory only, keyed by `(issuer, client_id, audience)`) that does not expire within 5 minutes is returned. Otherwise POST the token endpoint with `grant_type=refresh_token`, the stored refresh token, `client_id`, and `audience=<audience>` *(assumed here: the company provider issues audience-bound tokens on refresh, as Okta and Entra do; Entra reads `scope=<audience>/.default` instead, sent when the issuer host is `login.microsoftonline.com`)*. Errors as E.4. The refresh margin is this component's 5 minutes (reconciliation row 514 settled to the provider). `invalidate_audience_token` removes the cache entry; it never fails.
3. **`refresh_silently(kind)`**: select as E.1 (the single active row of `kind`; none → `Ok(false)`). No refresh token stored → `Ok(false)`. Run E.4 regardless of `expires_at`: success → `Ok(true)`; `invalid_grant` or 400/401 → mark needs sign-in as E.4, publish, `Ok(false)`; network failure → `CRED_PROVIDER_UNREACHABLE`. Never opens a browser, never starts a device flow.
4. **`person()`**: `github_login` = account label of the active `Github` credential; `email` = account label of the active `Oidc`, else `Microsoft`, else `Google` credential; `name` = the first `display_name` found in that same order and then `Github`, else the part of `email` before `@`, else `github_login`, else `CRED_SIGN_IN_REQUIRED` (nothing identifies the person). No network call.
5. **`client_certificate(name)`** (SA-CLOSE-02): `name` 1–255 characters (`CRED_SECRET_NAME_INVALID` otherwise). Desktop only (`program = Desktop`; else `CRED_FLOW_NOT_OFFERED`). Read the secret `client-certificate/<hex SHA-256 of name>`; absent → `Ok(None)`. The value is the JSON `{"format":1,"certificate_pem":"…","key_pem":"…"}`; a value that does not parse → `CRED_SECRET_STORE_CORRUPT`. Parse the chain with `CertificateDer::pem_slice_iter` and the key with `PrivateKeyDer::from_pem_slice`, load the key with the process's default `rustls` crypto provider (`key_provider.load_private_key`) and return `CertifiedKey::new(chain, key)`, cached in memory until the next import. A keychain error → `CRED_KEYCHAIN_UNAVAILABLE` (retryable). The key bytes are zeroised after loading and never logged.
6. **`mail_account()`**: active rows of the three mail kinds; none → `Ok(None)`; one → its `CredentialView`; several → `CRED_AMBIGUOUS`.
7. **`SecretStore::get_or_create_random(name, len)`**: `len` outside 16–64 → `CRED_SECRET_TOO_LARGE`. Under a per-name `tokio::sync::Mutex`: `get`; present → return it; absent → fill `len` bytes from the operating system's random source, `put`, return them.
8. **`SecretStore::agent_key(agent_instance_id, key_name)`**: `SecretName::new("agent-mcp/<agent instance id>/<key_name>")` (pattern failure → `CRED_SECRET_NAME_INVALID`), then `get`.
9. **`create_secrets_key_credential(path)`**: the file exists → `CRED_SECRET_KEY_EXISTS`. Fill 32 random bytes; run `systemd-creds encrypt --user --name=ordin-secrets-key - <path>` with no shell, the cleared environment of D.3.1 and a 10-second timeout, writing the key on standard input; non-zero exit, timeout or binary absent → `CRED_SYSTEMD_CREDS_FAILED`. Set the file mode to `0600`. The key bytes are zeroised after the call and never logged.
10. **`ServerSecrets`, `DesktopKeyStore`, `put_server_secret`**: each is one `SecretStore` call on the fixed name. `remote_api_tls_key` reads `remote-api/tls-key` and refuses bytes that are not UTF-8 (`CRED_SECRET_STORE_CORRUPT`); `put_remote_api_tls_key` refuses a value without a `-----BEGIN` line (`CRED_PASSWORD_INVALID`). The desktop-key methods pass `keychain_ref` to `SecretName::new` (pattern failure → `CRED_SECRET_NAME_INVALID`); `get_desktop_key` refuses a stored value that is not exactly 32 bytes (`CRED_SECRET_STORE_CORRUPT`). `put_server_secret`: `SecretName::new(name)`, empty value → `CRED_PASSWORD_INVALID`, value > 64 KiB → `CRED_SECRET_TOO_LARGE`, then `put`. None of them logs the name's value.
11. **`save_gitlab_token(host, token)`** (SA-CLOSE-01): `host` 1–255 characters, a host name with no scheme, path or port (`CRED_URL_INVALID` otherwise); `token` 1–1024 bytes (`CRED_PASSWORD_INVALID`). `GET https://<host>/api/v4/personal_access_tokens/self` with header `PRIVATE-TOKEN`, timeout 15 s: 401 → `CRED_PROVIDER_REFUSED`; network failure or 5xx → `CRED_PROVIDER_UNREACHABLE`; `revoked: true` or `active: false` → `CRED_PROVIDER_REFUSED`; `scopes` lacking any of `read_repository`, `write_repository`, `api` → `CRED_SCOPE_MISSING`. Label from `GET https://<host>/api/v4/user` `username` (its `name` kept as `display_name`), checked as C.8. Persist as C.10 with token blob `{"access_token":<token>,"refresh_token":null,"expires_at":<the response's expires_at at 00:00 UTC, or null>,"scopes":[…],"token_type":"private_token"}`, `credential_meta.provider_host = host`; an existing active `gitlab` row for the same label is updated in place. Publish `SignInChanged { state: SignedIn }`; log `cred.sign_in.completed` with `kind = gitlab`, `method = pasted_token`. `access_token(Gitlab)` then returns it; with no refresh token, an expired token yields `CRED_SIGN_IN_REQUIRED` (E.4), and the person pastes a new one. The token is never logged or echoed.
12. **`import_client_certificate(name, certificate_pem, key_pem)`** (SA-CLOSE-02): desktop only (`CRED_FLOW_NOT_OFFERED`). `name` 1–255 characters (`CRED_SECRET_NAME_INVALID`). Parse as H.5: no certificate, more than 10 certificates, no private key, a key the crypto provider cannot load, or a leaf whose public key does not match the key → `CRED_CERTIFICATE_INVALID` (permanent); a leaf whose `notAfter` is past → `CRED_CERTIFICATE_EXPIRED` (permanent). Store the JSON of H.5 under `client-certificate/<hex SHA-256 of name>` (size > 64 KiB → `CRED_SECRET_TOO_LARGE`), replacing an earlier import; clear the H.5 cache; return `ClientCertificateView` (subject common name, else the whole subject in RFC 4514 form, cut to 254 characters). Log `cred.client_certificate.imported` (`name_hash`, `not_after`).

## Data Model

SQLite, in the journal database (registered by the program as `ModuleMigrations { module: MODULE_NAME, sqlite: SQLITE_MIGRATIONS, postgres: &[] }`, Journal spec *Migration registry*); `MIGRATION_1_SQL`:

```sql
CREATE TABLE credentials (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    kind          TEXT NOT NULL CHECK (kind IN ('github','oidc','slack','confluence','microsoft','google',
                     'mail_server_single_sign_on','mail_server_password','mail_server_app_password','gitlab')),
    account_label TEXT NOT NULL CHECK (length(account_label) BETWEEN 1 AND 254),
    keychain_ref  TEXT NOT NULL CHECK (length(keychain_ref) BETWEEN 1 AND 255),
    expires_at    TEXT,
    state         TEXT NOT NULL CHECK (state IN ('active','revoked')),
    created_at    TEXT NOT NULL,
    refreshed_at  TEXT
);
CREATE UNIQUE INDEX credentials_active_kind_label ON credentials(kind, account_label) WHERE state = 'active';

-- chosen here: non-secret sign-in health and granted scopes
CREATE TABLE credential_meta (
    credential_id   TEXT PRIMARY KEY REFERENCES credentials(id) ON DELETE CASCADE,
    provider_host   TEXT NOT NULL CHECK (length(provider_host) BETWEEN 1 AND 255),
    scopes          TEXT NOT NULL DEFAULT '[]',          -- JSON array of strings
    needs_sign_in   INTEGER NOT NULL DEFAULT 0 CHECK (needs_sign_in IN (0,1)),
    refused_at      TEXT,
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) <= 64),
    display_name    TEXT CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 254)
);
```
No PostgreSQL variant: this component holds no tables in the agent profile's scaled shape. Rollback: the migration only creates tables; restoring the pre-migration backup (SA-XC-07) removes them. Revoked rows are kept for the journal's retention period and then deleted by `DELETE FROM credentials WHERE state='revoked' AND created_at < ?` called from the program's retention pass *(assumed here, SA-CRED-14)*.

## Error Table

Status for in-process calls: `Err(retryable)` or `Err(permanent)`; the Service API action follows in the body.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Config: back end and program disagree, pool missing | Err(permanent) | `CRED_CONFIG_INVALID` | start stops; logged |
| Both server key variables set | Err(permanent) | `CRED_SECRET_KEY_CONFLICT` | start stops: "Set ORDIN_SECRETS_KEY_FILE or ORDIN_SECRETS_KEY_CREDENTIAL, not both" (log line for the server owner) |
| No session bus and no key variable | Err(permanent) | `CRED_SECRET_KEY_MISSING` | start stops; log names both variables |
| Key bytes not 32 bytes / base64 of 32 | Err(permanent) | `CRED_SECRET_KEY_INVALID` | start stops |
| `systemd-creds decrypt --user` failed or timed out | Err(permanent) | `CRED_SYSTEMD_CREDS_FAILED` | start stops; exit status logged, no output |
| Secrets file cannot be decrypted or unknown format | Err(permanent) | `CRED_SECRET_STORE_CORRUPT` | start stops; file untouched |
| Secrets file write failed | Err(retryable) | `CRED_SECRET_STORE_WRITE_FAILED` | `{"code":"CRED_SECRET_STORE_WRITE_FAILED","action":"try_again"}` |
| Scaled shape, secret store called | Err(permanent) | `CRED_SECRET_STORE_ABSENT` | — (programming error, logged) |
| Secret name fails pattern | Err(permanent) | `CRED_SECRET_NAME_INVALID` | — (caller defect) |
| Secret value > 64 KiB | Err(permanent) | `CRED_SECRET_TOO_LARGE` | — (caller defect) |
| Keychain or Secret Service locked, refused or unreachable | Err(retryable) | `CRED_KEYCHAIN_UNAVAILABLE` | `{"code":…,"action":"try_again"}` |
| Secret item missing for an active credential | Err(permanent) | `CRED_KEYCHAIN_MISSING` | `{"code":…,"action":"sign_in_again"}` |
| Issuer or site not `https` / too long | Err(permanent) | `CRED_URL_INVALID` | `{"code":…,"action":"send_to_it"}` |
| Client id or GitHub App id absent | Err(permanent) | `CRED_PROVIDER_NOT_CONFIGURED` | `{"code":…,"action":"send_to_it"}` |
| Flow not offered here (Confluence or Google on the server; Slack on the server by device code; agent profile; no browser) *Amended January 2026 (ADR-102): Slack on the server signs in by the relayed browser method.* | Err(permanent) | `CRED_FLOW_NOT_OFFERED` | `{"code":…,"action":null}` |
| All loopback ports busy | Err(retryable) | `CRED_LOOPBACK_BUSY` | `{"code":…,"action":"try_again"}` |
| System browser could not be opened | Err(retryable) | `CRED_BROWSER_FAILED` | `{"code":…,"action":"try_again"}` |
| More than 4 sign-ins pending | Err(retryable) | `CRED_TOO_MANY_SIGN_INS` | `{"code":…,"action":"try_again"}` |
| Unknown challenge id | Err(permanent) | `CRED_CHALLENGE_UNKNOWN` | `{"code":…,"action":null}` |
| Person cancelled or denied at the provider | Err(permanent) | `CRED_SIGN_IN_CANCELLED` | `{"code":…,"action":"sign_in_again"}` |
| Sign-in not finished in time (10 min / device expiry) | Err(retryable) | `CRED_SIGN_IN_TIMEOUT` | `{"code":…,"action":"sign_in_again"}` |
| Relayed sign-in's `state` differs from the challenge's | Err(permanent) | `CRED_RELAY_STATE_MISMATCH` | `{"code":"CRED_RELAY_STATE_MISMATCH","action":"sign_in_again"}`; the paired desktop shows "That sign-in didn't match. Try signing in to Slack again." |
| Provider refused the client or the grant (incl. C-12/C-14 failing) | Err(permanent) | `CRED_PROVIDER_REFUSED` | `{"code":…,"action":"send_to_it"}` |
| Provider unreachable (DNS, TLS, timeout, 5xx) | Err(retryable) | `CRED_PROVIDER_UNREACHABLE` | `{"code":…,"action":"try_again"}` |
| Account label out of range | Err(permanent) | `CRED_LABEL_INVALID` | `{"code":…,"action":"send_to_it"}` |
| Active credential with same kind and label exists | Err(permanent) | `CRED_DUPLICATE` | `{"code":…,"action":null}` |
| Database write failed | Err(retryable) | `CRED_STORE_FAILED` | `{"code":…,"action":"try_again"}` |
| No usable credential / marked needs sign-in / refresh refused | Err(permanent) | `CRED_SIGN_IN_REQUIRED` | `{"code":…,"action":"sign_in_again"}` |
| Provider refused the refresh (`invalid_grant`, HTTP 400 or 401; step 4) | recorded, not returned: `needs_sign_in = 1`; the caller receives `CRED_SIGN_IN_REQUIRED` (row above) | `CRED_REFRESH_REFUSED` | stored as the credential's `last_error_code`; `NeedsSignIn` published |
| Several active credentials, none named | Err(permanent) | `CRED_AMBIGUOUS` | — (caller defect) |
| `access_token` on a password kind | Err(permanent) | `CRED_KIND_HAS_NO_TOKEN` | — (caller defect) |
| `mail_password` on a token kind | Err(permanent) | `CRED_KIND_MISMATCH` | — (caller defect) |
| Unknown credential id | Err(permanent) | `CRED_NOT_FOUND` | `{"code":…,"action":null}` |
| Password empty or > 1024 bytes | Err(permanent) | `CRED_PASSWORD_INVALID` | `{"code":…,"action":null}` |
| A secret found in a record field (test-time guard) | Err(permanent) | `CRED_SECRET_IN_RECORD` | — (emitter defect, caught by test) |
| Active credential lacks a scope the caller needs; a pasted GitLab token lacks `read_repository`, `write_repository` or `api` | Err(permanent) | `CRED_SCOPE_MISSING` | `{"code":…,"action":"sign_in_again"}` |
| Imported client certificate or key unreadable, missing, unloadable or not a pair | Err(permanent) | `CRED_CERTIFICATE_INVALID` | `{"code":…,"action":null}` |
| Imported client certificate already expired | Err(permanent) | `CRED_CERTIFICATE_EXPIRED` | `{"code":…,"action":"send_to_it"}` |
| `create_secrets_key_credential` target file already exists | Err(permanent) | `CRED_SECRET_KEY_EXISTS` | setup command prints "A secrets key already exists at this path; nothing was changed." (log line for the server owner) † |

Words this component owns: the loopback page "You're signed in. You can close this tab." *(chosen here)*. Every other sentence the person reads is built by Localisation from the code (11 §6: "Sign in again").

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Identity and Credentials

  Scenario: Happy path — Microsoft sign-in on the desktop
    Given the profile names microsoft tenant "4a1c-tenant" and client "9f0e-app"
    And no Microsoft credential exists
    When the SharePoint Adapter calls begin_sign_in with method Browser and Graph file scopes
    Then the system browser opens the Microsoft authorise page with a S256 code challenge and redirect "http://127.0.0.1:47821/callback"
    When the provider redirects with a code and the matching state
    Then a credential of kind "microsoft" with state "active" is stored
    And the token blob is in the keychain under "credential/microsoft/<id>"
    And no table column holds the access or refresh token

  Scenario: Edge — scopes already granted
    Given an active Microsoft credential whose scopes include the requested scopes
    When begin_sign_in is called for Microsoft
    Then the result is AlreadySignedIn and the browser is not opened

  Scenario: Edge — incremental consent adds mail permissions
    Given an active Microsoft credential with file scopes only
    When begin_sign_in requests "Mail.ReadWrite" and the person consents
    Then the same credential row is updated with the union of scopes and refreshed_at set

  Scenario: Happy path — device sign-in on the server
    Given the program is PersonalServer and the GitHub App client id is compiled in
    When begin_sign_in is called for Github with method DeviceCode
    Then the result carries verification_uri "https://github.com/login/device" and a user code
    And wait_for_sign_in polls every interval seconds and returns the credential once approved

  Scenario: Error — Confluence on the server
    Given the program is PersonalServer
    When begin_sign_in is called for Confluence with method DeviceCode
    Then it fails with CRED_FLOW_NOT_OFFERED

  Scenario: Edge — token refreshed before expiry
    Given an active GitHub credential that expires in 3 minutes
    When two callers ask access_token at the same time
    Then exactly one refresh request is sent and both receive the new token

  Scenario: Error — refresh refused
    Given an active Slack credential whose refresh token the provider rejects with invalid_grant
    When access_token is called
    Then it fails with CRED_SIGN_IN_REQUIRED and SignInChanged NeedsSignIn is published

  Scenario: Error — refused mail password is never retried
    Given an active mail_server_app_password credential
    When the Mail Service calls report_refused
    And later calls mail_password
    Then mail_password fails with CRED_SIGN_IN_REQUIRED without reading the keychain
    When the person saves a new app password for the same account
    Then mail_password returns the new password

  Scenario: Error — person cancels at the provider
    Given a browser sign-in is pending
    When the provider redirects with error "access_denied"
    Then wait_for_sign_in fails with CRED_SIGN_IN_CANCELLED and no credential row is written

  Scenario: Error — provider unreachable
    Given the OIDC issuer does not answer within 15 seconds
    When begin_sign_in is called for Oidc
    Then it fails with CRED_PROVIDER_UNREACHABLE marked retryable

  Scenario: Happy path — encrypted secrets file on a server
    Given ORDIN_SECRETS_KEY_FILE points to a file holding base64 of 32 bytes and no session bus
    When the store puts "telegram/bot-token"
    Then "<data root>/secrets/secrets.enc" exists with mode 0600
    And restarting with the same key returns the value

  Scenario: Error — both key variables set
    Given ORDIN_SECRETS_KEY_FILE and ORDIN_SECRETS_KEY_CREDENTIAL are both set
    When server_secret_backend is called
    Then it fails with CRED_SECRET_KEY_CONFLICT

  Scenario: Error — wrong key
    Given secrets.enc was written with key A
    When the server starts with key B
    Then the start fails with CRED_SECRET_STORE_CORRUPT and the file is unchanged

  Scenario: Error — systemd credential cannot be decrypted
    Given ORDIN_SECRETS_KEY_CREDENTIAL points to a file another user encrypted
    When the server starts
    Then the start fails with CRED_SYSTEMD_CREDS_FAILED

  Scenario: Error — keychain locked
    Given the macOS login keychain refuses access
    When access_token is called for an active credential whose token is not cached
    Then it fails with CRED_KEYCHAIN_UNAVAILABLE marked retryable

  Scenario: Error — a token without the needed scope
    Given an active Microsoft credential with file scopes only
    When the SharePoint Adapter calls access_token_for(Microsoft, ["https://graph.microsoft.com/Sites.Read.All"])
    Then it fails with CRED_SCOPE_MISSING and no provider request is sent

  Scenario: Happy path — a company token for one hosted agent's audience
    Given an active Oidc credential for issuer "https://example.okta.com" and client "0oa1b2c3"
    When access_token_for_audience is called with audience "api://finance-agent"
    Then one refresh-token request carrying audience "api://finance-agent" is sent
    And a second call within the token's life sends no request
    When invalidate_audience_token is called and the token is asked again
    Then a new refresh-token request is sent

  Scenario: Edge — silent refresh never prompts
    Given an active Github credential whose refresh token the provider rejects
    When refresh_silently(Github) is called
    Then it returns false, no browser is opened, and SignInChanged NeedsSignIn is published

  Scenario: Happy path — the person's identity for a handover
    Given an active Oidc credential labelled "asha.rao@acme.com" with display name "Asha Rao" and an active Github credential "asharao"
    When person() is called
    Then it returns name "Asha Rao", email "asha.rao@acme.com" and github_login "asharao"

  Scenario: Happy path — an installation HMAC key is made once
    Given no secret named "sandbox/audit-hmac-key"
    When get_or_create_random is called twice for that name with length 32
    Then both calls return the same 32 bytes and one put was made

  Scenario: Happy path — the Remote API TLS key round-trips through ServerSecrets
    Given the server's encrypted secrets file is open
    When put_remote_api_tls_key is called with a PEM private key
    Then remote_api_tls_key returns the same PEM and the secret "remote-api/tls-key" exists
    When delete_remote_api_tls_key is called twice
    Then both calls succeed and remote_api_tls_key returns None

  Scenario: Error — a stored desktop key of the wrong length
    Given the secret "server-connector/desktop-key/0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f70" holds 16 bytes
    When get_desktop_key is called for it
    Then it fails with CRED_SECRET_STORE_CORRUPT

  Scenario: Error — the secrets key credential already exists
    Given a file exists at "/home/ordin/.config/ordin/secrets.key.cred"
    When create_secrets_key_credential is called with that path
    Then it fails with CRED_SECRET_KEY_EXISTS and the file is unchanged

  Scenario: Happy path — GitLab sign-in on the desktop by device flow (D9)
    Given the profile has gitlab.host "gitlab.example.com" and gitlab.client_id "a1b2c3"
    When begin_sign_in is called for Gitlab with method Browser
    Then a device code is requested from "https://gitlab.example.com/oauth/authorize_device" with scopes "read_repository write_repository api"
    And the browser opens the returned verification_uri and a DeviceCode challenge is returned
    When the person approves and wait_for_sign_in completes
    Then a credential of kind "gitlab" labelled with the GitLab username is stored

  Scenario: Happy path — a pasted GitLab token without an OAuth application
    Given the profile has no gitlab.client_id and gitlab.host is absent
    And gitlab.com reports the token active with scopes read_repository, write_repository and api
    When identity.save_gitlab_token is called with that token
    Then a credential of kind "gitlab" with provider_host "gitlab.com" is stored and SignInChanged SignedIn is published
    And no log line contains the token

  Scenario: Error path — a pasted GitLab token lacks a scope
    Given gitlab.com reports the token's scopes as read_repository only
    When save_gitlab_token is called
    Then it fails with CRED_SCOPE_MISSING and nothing is stored

  Scenario: Happy path — import a client certificate and read it back
    Given the profile's server.client_certificate_name is "ACME desktop"
    And a PEM certificate valid until 2027-06-30 and its matching PEM key
    When import_client_certificate is called on the desktop
    Then the secret store holds one item under "client-certificate/<sha-256 of 'ACME desktop'>"
    And client_certificate("ACME desktop") returns a CertifiedKey whose leaf is that certificate

  Scenario: Error path — a key that does not match the certificate
    Given a PEM certificate and a PEM key from a different pair
    When import_client_certificate is called
    Then it fails with CRED_CERTIFICATE_INVALID and nothing is stored

  Scenario: Edge case — no certificate imported under the profile's name
    When client_certificate("ACME desktop") is called with nothing imported
    Then it returns None

  Scenario: Happy path — sign out
    Given an active Google credential
    When sign_out is called
    Then the keychain item is deleted, the row state is "revoked" and SignInChanged SignedOut is published
```

## Performance, Security, Observability

- **Performance targets:** `access_token` with a cached valid token p95 ≤ 2 ms; with a keychain read p95 ≤ 50 ms (macOS, Windows); with a refresh, bounded by the 15 s HTTP timeout. `list` p95 ≤ 10 ms for ≤ 50 credentials. `SecretStore::put` on the encrypted file p95 ≤ 20 ms for a file ≤ 1 MiB. Memory: ≤ 2 MiB resident for the service and its caches.
- **Security:**
  - Entry points: in-process calls only; the Service API commands reach it through the Command Bridge (Tauri capabilities) or the Remote API (desktop key), never with a token in a result. `identity.save_mail_password` and `identity.save_gitlab_token` are the two commands carrying a secret into the core, and `identity.import_client_certificate` reads a private key from a file the person picks, never from the screens; the WebView never receives one back (02:691).
  - Validation: every URL `https` (the loopback redirect excepted), every name and label bounded as listed, `state` compared in constant time, PKCE on every code flow, no confidential client secret stored or sent.
  - Loopback listener on `127.0.0.1` only, one request, closed after use or 10 minutes (ADR-072).
  - Secrets in memory are `Zeroizing`; `SecretString` never prints.
  - Never logged, journalled or put in an error: access, refresh and ID tokens, authorisation codes, PKCE verifiers, `state`, device codes and user codes, passwords, the secrets key, keychain item contents, `secrets.enc` contents. Account labels are logged as a SHA-256 prefix (8 hex characters).
  - Revocation on sign-out where the provider offers it.
- **Observability:** desktop and personal server: `tracing` JSON lines only (no metrics, no spans; SA-XC-04/05). Events (target `ordin_identity`):
  - `cred.sign_in.started` (info): `challenge_id`, `kind`, `method`.
  - `cred.sign_in.completed` (info): `challenge_id`, `credential_id`, `kind`, `account_hash`.
  - `cred.sign_in.failed` (warn): `challenge_id`, `kind`, `code`.
  - `cred.refreshed` (debug): `credential_id`, `kind`, `expires_at`.
  - `cred.refresh_failed` (warn): `credential_id`, `kind`, `code`.
  - `cred.refused` (warn): `credential_id`, `kind`, `reason_code`.
  - `cred.signed_out` (info): `credential_id`, `kind`.
  - `cred.secret_missing` (warn), `cred.secret_file_mode_fixed` (warn), `cred.secret_store.opened` (info): `backend`.
  - `cred.client_certificate.imported` (info): `name_hash`, `not_after`.
  Agent profile: the same events; no Identity metrics (Telemetry owns counters).

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

The sections above carry these changes in place (each marked where it is amended):

### Slack scopes asked later (SA-SLACK-02)

The *Fixed constants* table has two Slack rows: the SA-SLACK-01 scopes of every Slack sign-in, and the SA-SLACK-02 scopes (`users:read`, `users:read.email`, `im:write`) the Slack Service asks for later through `begin_sign_in`, never at upgrade. Internal Logic §C step 4's Slack bullet requests the scopes step 3 settled — the union rule of step 3 is unchanged and is the incremental consent the Slack Service relies on: `access_token_for(CredentialKind::Slack, &["users:read.email", "im:write"])` returns `CRED_SCOPE_MISSING` until the person has allowed them, and the Slack Service then calls `begin_sign_in` with those scopes. The *Who calls each public item* table names the Slack Service beside `access_token`, `access_token_for`, `report_refused` and `begin_sign_in`.

### The Slack client id may be the person's own app

*Configuration this component reads* takes the client id from the profile's `slack.client_id` or, when absent, from the person's own Slack app (kept by the Slack Service); the program passes the value, so this crate reads no Slack Service table (no new edge). Neither present → `CRED_PROVIDER_NOT_CONFIGURED`.

### Relayed browser sign-in for the personal server

`SignInMethod::RelayedBrowser`, `SignInRequest.relay_port`, `SignInChallenge::RelayedBrowser` and `complete_relayed_sign_in` are in *Public Interface › Sign-in*; the Service API table has `identity.complete_relayed_sign_in` and the `relay_port` input of `identity.sign_in` (nine `identity.*` commands, SA-RECON-04). Internal Logic §C: step 2 refuses `RelayedBrowser` outside the personal server's Slack sign-in; step 5a builds and completes the relayed challenge; step 12 ends a relayed challenge on cancel. The Error Table has `CRED_RELAY_STATE_MISMATCH`; the existing rows cover the other cases: `CRED_FLOW_NOT_OFFERED` (relayed outside the personal server, or not Slack, or no port), `CRED_CHALLENGE_UNKNOWN`, `CRED_SIGN_IN_TIMEOUT`, `CRED_PROVIDER_REFUSED` (Slack refuses the app: app approval on, R-06; the Slack Service maps it to `SLK_APP_NOT_APPROVED`), `CRED_PROVIDER_NOT_CONFIGURED`. `CodedError::action` answers `CRED_RELAY_STATE_MISMATCH` with `SignInAgain`.

### Acceptance Criteria (Gherkin) — added scenarios


```gherkin
  Scenario: a Slack sign-in that holds only the first scopes is asked for the later ones once
    Given an active Slack credential whose scopes are the SA-SLACK-01 scopes
    When access_token_for(Slack, ["users:read.email", "im:write"]) is called
    Then it fails with CRED_SCOPE_MISSING
    And begin_sign_in for Slack with scopes ["users:read.email", "im:write"] builds an authorisation URL whose user_scope holds the five SA-SLACK-01 scopes and both new scopes
    And on success the same credentials row is updated, not a second one added

  Scenario: the person's own Slack app when the profile names none
    Given the organisation profile has no slack.client_id
    And the person pasted the client id "1234567890.9876543210" on Settings › Slack
    When identity.sign_in is called with kind slack
    Then the authorisation URL carries client_id=1234567890.9876543210

  Scenario: no Slack client id at all
    Given the profile has no slack.client_id and the person has connected no app of their own
    When identity.sign_in is called with kind slack
    Then it fails with CRED_PROVIDER_NOT_CONFIGURED and nothing is opened

  Scenario: relayed Slack sign-in on the personal server
    Given the program is the personal server
    When identity.sign_in is called with kind slack and relay_port 47821
    Then the challenge is RelayedBrowser with an authorisation URL whose redirect_uri is http://127.0.0.1:47821/callback
    And no port is bound on the server
    When identity.complete_relayed_sign_in is called with that challenge, a code and the matching state
    Then the code is exchanged with the verifier the server kept and identity.sign_in_wait returns the Slack credential

  Scenario: relayed sign-in with the wrong state
    Given an open RelayedBrowser challenge
    When complete_relayed_sign_in is called with a state that differs
    Then it fails with CRED_RELAY_STATE_MISMATCH and no token request is made

  Scenario: relayed sign-in refused on the desktop
    Given the program is the desktop
    When begin_sign_in is called with method RelayedBrowser
    Then it fails with CRED_FLOW_NOT_OFFERED
```

### Performance, Security, Observability — added lines

- **Security:** the relayed code crosses only the paired desktop's authenticated Remote API connection (ADR-096); the PKCE verifier and the `state` never leave the server, so a code read from the desktop's loopback cannot be exchanged anywhere else; the code is zeroised after the exchange and never logged or journalled.
- **Observability:** event `cred.sign_in.relayed {challenge_id, kind}`; `cred.sign_in.completed` unchanged.

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86 (the user's decisions of January 2026: Jira as a handover place and Jira issues as workspace links); HLD revision 0.54.0, ADR-106. Counterpart: `components/JiraAdapter.md`, the only caller of the two methods added here; it checks a pasted token itself before calling `save_jira_token`.

### Purpose (addition)

This component also holds the person's **Jira** sign-in: an Atlassian API token the person creates and pastes, with no company set-up (R-103 names the case where an organisation admin blocks such tokens), or, when the organisation profile names an Atlassian OAuth app (`confluence.oauth_client_id`), a sign-in through that app with a separate Jira grant. One Jira site per person. Jira is offered on the desktop only.

### Approach (addition)

The pasted-token path follows the GitLab precedent (SA-CLOSE-01) with one difference: the Jira Adapter checks the token with `GET /rest/api/3/myself` before calling `save_jira_token`, so this phase-1 crate needs no Jira knowledge beyond the token's storage. The OAuth path is the Confluence sign-in with Jira's scopes. Rejected: *one token shared by Confluence and Jira* — a grant change for one would ask the person to consent again for both, and signing out of one would sign out of the other.

### Shared Context (additions)

In place above: `CredentialKind::Jira` (stored `jira`), `ProviderConfig::Jira { site, client_id }`, the profile row for `confluence.oauth_client_id`, migration step 2 in `SQLITE_MIGRATIONS`.

| Constant | Value |
|---|---|
| `JIRA_OAUTH_SCOPES` | `read:jira-work`, `write:jira-work`, `read:jira-user`, `offline_access` (ADR-106) |
| Jira site host rule | lower case, `{name}.atlassian.net`, 1–255 characters, no scheme, path or port (A-53: Jira Cloud only) |

### Public Interface (additions)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JiraCredentialMethod {
    /// A pasted Atlassian API token, sent with Basic authentication as `email:token`.
    ApiToken { email: String },
    /// Atlassian 3LO through the gateway, scopes `JIRA_OAUTH_SCOPES`.
    Oauth,
}
pub struct JiraAccess {
    /// The person's Jira site host, `{name}.atlassian.net`: the credential's account label.
    pub site: String,
    pub method: JiraCredentialMethod,
    /// The API token, or the OAuth access token refreshed when it expires within 5 minutes.
    pub token: AccessToken,
}
impl IdentityService {
    /// The person's single active Jira credential. Ok(None) when no Jira credential exists.
    /// OAuth: granted scopes must contain `JIRA_OAUTH_SCOPES`, else CRED_SCOPE_MISSING.
    /// A credential marked needs-sign-in → CRED_SIGN_IN_REQUIRED.
    pub async fn jira_access(&self) -> Result<Option<JiraAccess>, CredError>;
    /// Keeps a pasted API token that the Jira Adapter has already checked with `GET /rest/api/3/myself`.
    /// Stored as a `CredentialKind::Jira` credential, label = `site`, method api_token, no refresh token.
    /// Replaces the token of an active Jira credential for the same site; a different site replaces
    /// nothing and returns CRED_DUPLICATE (one Jira site per person).
    pub async fn save_jira_token(&self, site: &str, email: &str, token: SecretString,
                                 account_id: &str, display_name: &str) -> Result<Credential, CredError>;
}
```

The existing `refresh_silently(CredentialKind::Jira)` (once after a Jira 401 on an OAuth token) and `report_refused(credential_id, "JIRA_TOKEN_REFUSED")` (a 401 on an API token) serve the Jira Adapter unchanged; *Who calls each public item* gains the Jira Adapter for `jira_access`, `save_jira_token`, `refresh_silently` and `report_refused`.

Service API: `identity.sign_in` gains the optional input `site: String?`, read for `kind = jira` only (the person's Jira site address, `https://{name}.atlassian.net`). The program builds `ProviderConfig::Jira { site, client_id: <profile confluence.oauth_client_id> }`; with no such profile key it returns `CRED_PROVIDER_NOT_CONFIGURED` and the screens offer the token form. The pasted token reaches the core through the Jira Adapter's `jira.connect_token` (sensitive input), not through an `identity.*` command, so the nine `identity.*` commands stay nine.

### Internal Logic (additions) — §J. Jira

1. **Where it is offered.** `begin_sign_in` with `ProviderConfig::Jira` and `save_jira_token` refuse with `CRED_FLOW_NOT_OFFERED` unless `program` is `Desktop` (no Atlassian device grant, C-49; and decision 86 keeps Jira on the desktop). Step C.2 also refuses `DeviceCode` and `RelayedBrowser` for `Jira`.
2. **OAuth sign-in.** As the `Confluence` bullet of step C.4: `https://auth.atlassian.com/authorize` with `audience=api.atlassian.com&prompt=consent`, token `https://auth.atlassian.com/oauth/token`, scopes `JIRA_OAUTH_SCOPES` (`offline_access` included, as for Confluence). `site` must match the site host rule after its `https://` scheme (`CRED_URL_INVALID`). Label (step C.8): `GET https://api.atlassian.com/oauth/token/accessible-resources`, the resource whose `url` equals `site` and whose `scopes` include `read:jira-work`; label = the site host; no matching resource → `CRED_PROVIDER_REFUSED`. Persist as C.10 with `credential_meta.provider_host` = the site host and the token blob's `"method":"oauth"`. An active Jira credential for another site → `CRED_DUPLICATE`, nothing stored.
3. **`save_jira_token(site, email, token, account_id, display_name)`.** `site` matches the site host rule (`CRED_URL_INVALID`); `email` 3–254 characters with one `@`, `token` 1–1024 bytes (`CRED_PASSWORD_INVALID`); `account_id` 1–128 characters and `display_name` 1–254 characters (`CRED_LABEL_INVALID`). No network call. An active Jira credential for another site → `CRED_DUPLICATE`. Persist as C.10: token blob `{"access_token":<token>,"refresh_token":null,"expires_at":null,"scopes":[],"token_type":"atlassian_api_token","method":"api_token","email":<email>,"account_id":<account_id>}` under `credential/jira/<credential id>`; `account_label` = `site`; `credential_meta.provider_host` = `site`, `display_name` = `display_name`, `needs_sign_in = 0`. An active row for the same site is updated in place (its secret replaced, its refusal mark cleared). Publish `SignInChanged { state: SignedIn }`; log `cred.sign_in.completed` with `kind = jira`, `method = pasted_token`. The token is never logged or echoed.
4. **`jira_access()`.** The single active `jira` row; none → `Ok(None)`. `needs_sign_in = 1` → `CRED_SIGN_IN_REQUIRED`. Read the blob (step E.3). `"method":"api_token"` → `JiraAccess { site, method: ApiToken { email }, token }` with `expires_at = None`. `"method":"oauth"`: the granted scopes must contain `JIRA_OAUTH_SCOPES` (`CRED_SCOPE_MISSING`), then step E.4 refreshes a token that expires within 5 minutes; `JiraAccess { site, method: Oauth, token }`. With no refresh token and an expired token → `CRED_SIGN_IN_REQUIRED`.
5. **Signing out.** `sign_out` on a Jira credential deletes the secret and marks the row revoked (step G.2); no revocation call is made (Atlassian offers none to a public client).

### Data Model (additive migration)

`MIGRATION_2_SQL` widens the kind `CHECK`. SQLite cannot change a `CHECK` in place, so the step builds new tables, copies every row and replaces the old ones inside the Journal's single migration transaction (DatabaseMigrations step 8). `credential_meta` is copied and dropped before `credentials`, so no `ON DELETE CASCADE` fires; the renames carry its reference to the new `credentials`.

```sql
-- step (2, "jira credential kind", …)
CREATE TABLE credentials_v2 (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    kind          TEXT NOT NULL CHECK (kind IN ('github','oidc','slack','confluence','microsoft','google',
                     'mail_server_single_sign_on','mail_server_password','mail_server_app_password','gitlab','jira')),
    account_label TEXT NOT NULL CHECK (length(account_label) BETWEEN 1 AND 254),
    keychain_ref  TEXT NOT NULL CHECK (length(keychain_ref) BETWEEN 1 AND 255),
    expires_at    TEXT,
    state         TEXT NOT NULL CHECK (state IN ('active','revoked')),
    created_at    TEXT NOT NULL,
    refreshed_at  TEXT
);
CREATE TABLE credential_meta_v2 (
    credential_id   TEXT PRIMARY KEY REFERENCES credentials_v2(id) ON DELETE CASCADE,
    provider_host   TEXT NOT NULL CHECK (length(provider_host) BETWEEN 1 AND 255),
    scopes          TEXT NOT NULL DEFAULT '[]',
    needs_sign_in   INTEGER NOT NULL DEFAULT 0 CHECK (needs_sign_in IN (0,1)),
    refused_at      TEXT,
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) <= 64),
    display_name    TEXT CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 254)
);
INSERT INTO credentials_v2 SELECT id, kind, account_label, keychain_ref, expires_at, state, created_at, refreshed_at FROM credentials;
INSERT INTO credential_meta_v2 SELECT credential_id, provider_host, scopes, needs_sign_in, refused_at, last_error_code, display_name FROM credential_meta;
DROP TABLE credential_meta;
DROP TABLE credentials;
ALTER TABLE credentials_v2 RENAME TO credentials;
ALTER TABLE credential_meta_v2 RENAME TO credential_meta;
CREATE UNIQUE INDEX credentials_active_kind_label ON credentials(kind, account_label) WHERE state = 'active';
```

Rollback: the step only widens a `CHECK` list; every row is kept. It drops and recreates two tables to do so, as a widening rebuild under DatabaseMigrations rule 12a: `credential_meta` is copied and dropped before `credentials`, and the scenario *the migration keeps every credential* is the rule's migration test (row counts unchanged, `PRAGMA foreign_key_check` empty). Rollback is the backup copy of DatabaseMigrations step 7, restored with the previous program, which refuses the migrated file with `JRNL_SCHEMA_NEWER`. Secrets in the platform store are untouched by the step.

### Error Table (rows changed)

No new code. `CRED_FLOW_NOT_OFFERED` also covers Jira anywhere but the desktop; `CRED_DUPLICATE` covers a second Jira site; `CRED_URL_INVALID` a site that is not `{name}.atlassian.net`; `CRED_SCOPE_MISSING` an OAuth Jira grant without `JIRA_OAUTH_SCOPES`; `CRED_PROVIDER_NOT_CONFIGURED` an OAuth Jira sign-in with no `confluence.oauth_client_id`; `CRED_PROVIDER_REFUSED` an OAuth grant with no matching Jira resource.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a pasted Jira token with no company set-up
    Given the organisation profile has no confluence section
    And the Jira Adapter has checked the token for site "acme.atlassian.net"
    When save_jira_token("acme.atlassian.net", "sam.lee@acme.co.uk", <token>, "5b10ac8d82e05b22cc7d4ef5", "Sam Lee") is called
    Then a credential of kind "jira" labelled "acme.atlassian.net" is stored and SignInChanged SignedIn is published
    And the token is in the secret store under "credential/jira/<id>"
    And no table column, log line, journal entry or Service API result holds the token
    And jira_access() returns site "acme.atlassian.net" with method ApiToken { email "sam.lee@acme.co.uk" }

  Scenario: Happy path — Jira through the company's Atlassian app
    Given the profile has confluence.oauth_client_id "atl-123"
    When identity.sign_in is called with kind jira and site "https://acme.atlassian.net"
    Then the browser opens https://auth.atlassian.com/authorize with scopes "read:jira-work write:jira-work read:jira-user offline_access"
    And on success jira_access() returns method Oauth

  Scenario: Edge case — no Jira credential
    When jira_access() is called with no Jira credential
    Then it returns None

  Scenario: Error path — a second Jira site
    Given an active Jira credential for "acme.atlassian.net"
    When save_jira_token is called for "other.atlassian.net"
    Then it fails with CRED_DUPLICATE and the first credential is unchanged

  Scenario: Error path — Jira on the server
    Given the program is the personal server
    When identity.sign_in is called with kind jira
    Then it fails with CRED_FLOW_NOT_OFFERED

  Scenario: Edge case — the migration keeps every credential
    Given a journal database at identity step 1 with a github and a confluence credential and their meta rows
    When step 2 runs
    Then both credentials and both meta rows are present unchanged and a row of kind jira can be inserted
    And PRAGMA foreign_key_check returns no row
```

### Performance, Security, Observability (additions)

- **Security.** The pasted API token enters the core once, through the Jira Adapter's sensitive-input command, and is kept only in the platform secret store; it never reaches the WebView again, a log line, the journal or a screen payload (02:691). `account_id` and the email are kept in the token blob, not in a table.
- **Observability.** `cred.sign_in.completed` with `kind = jira` and `method = pasted_token` or `browser`.

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (knowledge inherits its readers: a knowledge agent checks each person against the system that holds a document, at the time of the question, and keeps no copy of reader lists; a person's own ordin links their accounts from the sign-ins it already holds). Counterparts: `components/KnowledgeQueryClient.md` (amendment January 2026: linking and the delegation token), `components/OrdinDesktop.md` and `components/OrdinServer.md` (amendments January 2026: forwarding sign-in changes, the Microsoft scope union).

### Purpose of the change

A knowledge agent can only ask GitHub, GitLab or Confluence "may this person read this?" when it knows the person's account there, and can only ask SharePoint as the person when it holds a short-lived Microsoft token issued for it. Both come from the sign-ins this component already keeps. It adds **no sign-in kind, no stored credential and no command**: one method returns a Microsoft token for another resource, the Confluence scope list gains one scope, and the *Who calls* table names the Knowledge Query Client.

### Assertions

- **SA-ID-PD-01** *(chosen here)* **Tokens for linking are the existing ones.** The Knowledge Query Client links a person's GitHub and GitLab accounts by choosing the active credential from `list()` and presenting, once per link, the token `access_token_by_id(credential_id)` returns, and the Confluence account with the token `access_token_for(Confluence, &["read:me"])` returns (KnowledgeQueryClient.md amendment January 2026, PD-A). The client watches `subscribe()` itself for sign-in changes. Nothing is recorded here about where a token went; the knowledge agent asks the system who the token belongs to and drops it (SourceAccess.md).
- **SA-ID-PD-02** *(chosen here)* **`read:me` only on the linking path.** `read:me` (for Atlassian's `GET https://api.atlassian.com/me`) is asked for only by the Knowledge Query Client's linking call; it is **not** added to the Confluence Adapter's scopes, so the desktop's Confluence sign-in and every adapter call are unchanged. When the Confluence credential lacks it, `access_token_for` returns `CRED_SCOPE_MISSING` (step H.1, unchanged) and the client skips Confluence linking only: that system's restricted items are withheld silently, and the connection details show Confluence as not linked. The scope is granted when the person's next Confluence sign-in requests it; the program adds `read:me` to the Confluence scope set it builds for `identity.sign_in` only while a live connection's `about.accounts` lists Confluence, as it adds front doors' check scopes for Microsoft (OrdinDesktop.md amendment January 2026, SA-DSK-PD-01).
- **SA-ID-PD-03** *(chosen here)* **SharePoint is never linked; it is delegated.** No Microsoft Graph token is ever handed to a knowledge agent. For SharePoint the client asks for a token whose audience is the front door's own Microsoft Entra app — scope `api://<client-id>/knowledge.check`, the `client-id` taken from the front door's `about` — through `access_token_for_scope`. That token can be used by the front door only in an on-behalf-of exchange for checks (SourceAccess.md); it cannot be used to read Graph directly.
- **SA-ID-PD-04** *(chosen here)* **Incremental consent for the check scope.** The first `access_token_for_scope(Microsoft, "api://<client-id>/knowledge.check")` for a front door's app the person has not consented to returns `CRED_SCOPE_MISSING`. The program then requests that scope at the person's next Microsoft sign-in, by adding every front door's check scope to the Microsoft scope set it builds for `identity.sign_in` (OrdinDesktop.md amendment January 2026). The person signs in once; Microsoft shows its own consent page for the front door's app. Whether a tenant lets a person consent to another app's delegated scope without an administrator is C-104.

### Public Interface — addition

Add to `impl IdentityService`, after `invalidate_audience_token`:

```rust
    /// A Microsoft access token for another resource than Graph — a knowledge agent's own
    /// Entra app (ADR-115) — minted from the stored Microsoft refresh token (step H.13).
    /// `kind` must be `Microsoft` (else CRED_KIND_MISMATCH); `scope` 1–512 characters,
    /// `api://` or `https://` prefix (else CRED_URL_INVALID). Not consented → CRED_SCOPE_MISSING.
    /// Cached in memory only, keyed by (credential id, scope), until 5 minutes before expiry.
    pub async fn access_token_for_scope(&self, kind: CredentialKind, scope: &str) -> Result<AccessToken, CredError>;
```

### Internal Logic — step H.13 (new)

**`access_token_for_scope(kind, scope)`:**

1. `kind != Microsoft` → `CRED_KIND_MISMATCH`. `scope` empty, over 512 characters, or not starting `api://` or `https://` → `CRED_URL_INVALID`.
2. The active `Microsoft` credential (as `access_token(Microsoft, None)` selects it) → none → `CRED_SIGN_IN_REQUIRED`; `needs_sign_in` → `CRED_SIGN_IN_REQUIRED`.
3. A cached token for (credential id, scope) that does not expire within 5 minutes → return it.
4. `POST https://login.microsoftonline.com/<tenant_id>/oauth2/v2.0/token` with `grant_type=refresh_token`, the stored refresh token, `client_id`, `scope=<scope> offline_access`; timeout 10 s.
5. Answer `200` → cache the access token (memory only); a rotated refresh token in the answer replaces the stored one in the secret store, as step H.3 does; return it. `400` with `error=invalid_grant` and an `error_codes` entry `65001` (consent required) → `CRED_SCOPE_MISSING`; other `invalid_grant` → mark `needs_sign_in`, publish `SignInChanged`, `CRED_SIGN_IN_REQUIRED`; `5xx`, timeout or connection failure → `CRED_PROVIDER_UNREACHABLE` (retryable).
6. Log `cred.resource_token` (debug): `credential_id`, `outcome` (`cached`, `minted`, `consent_required`, `refused`, `unreachable`). Never the scope's client id, never the token.

`sign_out` of the Microsoft credential drops every cached resource token of it (step G.2 gains: "and every `access_token_for_scope` cache entry of that credential").

### Fixed constants — edit

No edit here and none in ConfluenceAdapter.md: the adapter's scope list is unchanged (SA-ID-PD-02); `read:me` reaches a sign-in only through the program's scope set for `identity.sign_in`.

### Who calls each public item — edits

Replace the `access_token_by_id`, `access_token_for` and `subscribe` rows (each keeps its existing callers and gains the Knowledge Query Client), and add the `access_token_for_scope` row:

```
| `access_token_by_id` | Sandbox; Knowledge Query Client (`Github`, `Gitlab` credentials chosen from `list()`, linking one account, SA-ID-PD-01) |
| `access_token_for` | Confluence Adapter, SharePoint Adapter (scopes they need), Slack Service (SA-SLACK-02 scopes before `users.info`, `users.lookupByEmail`, `conversations.open`), Knowledge Query Client (`Confluence`, `["read:me"]`, linking only, SA-ID-PD-02) |
| `access_token_for_scope` | Knowledge Query Client (`Microsoft`, a front door's `api://<client-id>/knowledge.check`, SA-ID-PD-03) |
| `subscribe` | the programs (forwarding `sign_in_changed`); Knowledge Query Client (relinking on sign-in changes, SA-ID-PD-01) |
```

`list` gains the Knowledge Query Client as a caller (choosing the credential to link).

### Error Table — no new code

The method raises existing codes only: `CRED_KIND_MISMATCH`, `CRED_URL_INVALID`, `CRED_SIGN_IN_REQUIRED`, `CRED_SCOPE_MISSING`, `CRED_PROVIDER_UNREACHABLE`. All five are in the current Error Table (checked January 2026).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Tokens for per-document read access (ADR-115)

  Scenario: Happy path — a resource token for a front door's app
    Given an active Microsoft credential that has consented to "api://7c1e-app/knowledge.check"
    When the Knowledge Query Client calls access_token_for_scope(Microsoft, "api://7c1e-app/knowledge.check")
    Then a token request is sent with scope "api://7c1e-app/knowledge.check offline_access"
    And the token is returned and cached in memory only
    And a second call within its life sends no request

  Scenario: Error — consent not yet given
    Given an active Microsoft credential that has not consented to "api://7c1e-app/knowledge.check"
    And the token endpoint answers 400 invalid_grant with error code 65001
    When access_token_for_scope is called for that scope
    Then the call fails with CRED_SCOPE_MISSING
    And the credential is not marked needs sign-in

  Scenario: Error — not a Microsoft credential
    When access_token_for_scope is called with kind Github
    Then the call fails with CRED_KIND_MISMATCH and no request is sent

  Scenario: Edge — a Confluence sign-in without read:me skips linking only
    Given an active Confluence credential whose scopes lack "read:me"
    When the Knowledge Query Client calls access_token_for(Confluence, ["read:me"])
    Then the call fails with CRED_SCOPE_MISSING
    And the Confluence Adapter's own calls with its own scopes still succeed

  Scenario: Security — signing out drops resource tokens
    Given a cached resource token for the active Microsoft credential
    When that credential is signed out
    Then the next access_token_for_scope call fails with CRED_SIGN_IN_REQUIRED and sends no token request
```

### Security

The resource token never leaves the process except to the front door that owns its audience (KnowledgeQueryClient.md); it is never written to the secret store, the journal or a log. `cred.resource_token` carries no scope and no token.
