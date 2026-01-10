# SPEC: Organisation Profile

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (Phase 1 build corrections: `engineering.coding_tasks.max_attempts`, as Configuration.md 2D-2 lists it; approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-organisation-profile` · **Build phase:** 1 · **Programs:** D P A
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
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment January 2026 — Reading other teams' knowledge (ADR-110)](#amendment-january-2026--reading-other-teams-knowledge-adr-110)

---

## Purpose

The Organisation Profile reads the company's signed organisation profile from the address the installer carries (or, on the personal server, the address the setup command stored), verifies its Ed25519 detached signature, checks every value against the bounds of 2D-2, and serves the result as one typed, immutable snapshot to the rest of the core (02:695–708; ADR-013; 08:77; SA-SIGN-01). It refreshes the profile at start and every 60 minutes (2D-4); a profile that fails any check is ignored and the last verified one is kept (02:707). It owns the cached, verified profile and, from revision 0.48.0, each remote agent's status — listed, enabled, verified, withdrawn — kept in step with the profile's remote agent catalogue (02:708). When no profile exists, it serves the documented defaults, so every caller always has a complete set of settings. The agent profile reads no organisation profile (2D *Configuration sources*); there the component runs in a *not used* mode that serves defaults only.

## Approach

A refresh loop fetches two files with the company's own HTTPS (system proxy and trust store, A-39), verifies the signature before parsing anything, deserialises the document into strict Rust types that refuse unknown keys, validates the bounds in one table-driven pass, and only then activates it in one SQLite transaction and publishes it through a `tokio::sync::watch` channel. Readers never wait for the network: `current()` returns an `Arc` of the snapshot in force. Rejected: a JSON Schema validator crate — 1A names none, and the bounds include cross-field rules (`fallback ∈ offered`, `agents_at_once ≤ agents_at_once_max`) that are clearer in code; serving the raw `serde_json::Value` and letting each consumer read its keys — every consumer would repeat the defaults and the bounds.

## Shared Context

### Identifiers and timestamps (2C domain, *Conventions*)

```rust
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// RFC 3339, UTC, millisecond precision, `Z`; refused otherwise with TS_NOT_RFC3339_MS_UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub DateTime<Utc>);

/// UUIDv7 newtypes, serde(transparent): OrganisationProfileId, RemoteAgentId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrganisationProfileId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RemoteAgentId(pub Uuid);

/// 64 lowercase hexadecimal characters (HASH_NOT_SHA256_HEX).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String);
```

### Entities owned (2C domain, copied verbatim)

```rust
// OrganisationProfile — owner: Organisation Profile (02:707); 04:26–29; ADR-013; 04:412.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OrganisationProfile {
    pub id: OrganisationProfileId,
    pub profile_version: String,
    /// Detached signature as delivered, base64.
    pub signature: String,
    pub document_digest: Sha256Hex,
    pub cached_path: String,
    pub state: OrganisationProfileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal_reason: Option<String>,
    pub fetched_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrganisationProfileState { Fetched, Verified, Active, Replaced, FailedVerification }

// RemoteAgent — status owned by Organisation Profile from revision 0.48.0 (02:708); 04:163–166.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteAgent {
    pub id: RemoteAgentId,
    pub display_name: String,
    pub route: RemoteRoute,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_channel: Option<String>,
    /// Name of the identity-provider settings entry in the active organisation profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_provider_ref: Option<String>,
    pub status: RemoteAgentStatus,
    pub listed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteRoute { Http, Slack }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentStatus { Listed, Enabled, Verified, Withdrawn }
```

| Entity · field | Rule | Error code |
|---|---|---|
| OrganisationProfile.profile_version | 1–64 chars; strictly newer than the active version to become active | `PROF_VERSION_NOT_NEWER` |
| OrganisationProfile.signature | non-empty, ≤ 8192 bytes (LD-08); verifies against an installer key | `PROF_SIGNATURE_INVALID` |
| OrganisationProfile.state | at most one row `active` | `PROF_MULTIPLE_ACTIVE` |
| OrganisationProfile.state | `failed_verification` never becomes `active` (04:29) | `PROF_UNVERIFIED_ACTIVATION` |
| RemoteAgent.display_name | 1–120 chars; unique among non-withdrawn rows | `RAG_NAME_INVALID` |
| RemoteAgent route `http` | `deployment_address` (`https`, ≤ 2048 bytes) and `identity_provider_ref` present; Slack fields absent | `RAG_HTTP_FIELDS_INVALID` |
| RemoteAgent route `slack` | `slack_account` present, matching `^[UW][A-Z0-9]{8,20}$`; `slack_channel` present, matching `^[CG][A-Z0-9]{8,20}$` (SA-CLOSE-04; LD-15); `deployment_address` absent | `RAG_SLACK_FIELDS_INVALID` |

State machines (2C):

| Entity | From | Event | To |
|---|---|---|---|
| OrganisationProfile | — | fetched from the address | fetched |
| | fetched | signature, format and bounds pass | verified |
| | fetched | any check fails | failed_verification |
| | verified | becomes the served profile | active |
| | active | a newer version becomes active | replaced |
| RemoteAgent | — | listed by the profile | listed |
| | listed | the person first uses it and signs in to the route's account | enabled |
| | enabled | one successful read of the deployment or channel | verified |
| | listed, enabled, verified | the profile drops it | withdrawn |

`failed_verification` here covers every check before activation — signature, format version, schema and bounds — because 02:707 treats any of them as "fails verification".

### Shared types this component's document uses (2C domain)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTone { Friendly, Neutral, Warm, Direct }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRegister { Everyday, Formal }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind {
    SlackPost, ConnectionWrite, WrightrunRunStart, EmailSend, OriginPush, CopyPublish, TelegramPost,
    PullRequestOpened, HandoverWritten,   // 2C, decision 80: C-7
}
```

### Configuration (2D-2, 2D-2b, 2D-3, 2D-4)

| Item | Value | Source |
|---|---|---|
| Profile format | one JSON document, detached JWS (RFC 7515 Appendix F), `alg` `EdDSA` (Ed25519), verified with `ed25519-dalek` 3.0.0 | SA-SIGN-01; 08:77 |
| Verifying keys | `profile_signing_keys` of the installer file: at most two (rotation) | 2D-2b; 08:77 |
| Installer file | `<application data root>/installer.json` *(chosen here)*; the branded build may compile the same three values in as defaults | 2D-2b; ADR-041 build layer |
| Refresh | at start and every 60 minutes | 2D-4 |
| Retry while no profile is active | 5 s, doubling, at most 5 minutes between attempts, without end *(chosen here)* | 03:55 ("setup waits and retries in the background") |
| Cache | `<application data root>/profile/active.json` and `active.jws` | 04:412 ("cached under the application data root") |
| Format version this build reads | `1` *(chosen here)* | 2D-2 `format_version` |
| HTTP client | `reqwest` 0.13.5 with `rustls-platform-verifier` (system trust store, company inspection authority) and the system proxy | SA-HTTP-02; A-39 |
| Programs | desktop and personal server: active; agent profile: not used | 2D *Configuration sources* |

The crate reads **no environment variable**. The program passes the application data root and the HTTP client.

### Reconciliation assumptions

- **SA-RECON-01** This crate is handed the SQLite pool of the Journal's `Datastore` by the program and exports `MODULE_NAME` and `SQLITE_MIGRATIONS`; it has no Journal edge (phase 1; D4, R20).
- **SA-RECON-02** Consumers read `current()` and `subscribe()` and use this spec's section types; the profile publishes no per-consumer getter trait (rows 520–528, R6).
- **SA-RECON-03** A verified profile may redirect the update feed (decision D10).
- **SA-RECON-04** The Service API catalogue takes `profile.status`, `profile.refresh`, `profile.get_document` and `remote_agents.list` with these result types (R1, row 530).
- **SA-RECON-05** Telegram chat ids in `telegram.allowed_chats` are integers (row 526).
- **SA-RECON-06** With no `help.destination`, **Send to IT** goes through the person's own mail app (08:14; row 529).
- **SA-CLOSE-01** *(K9, closes OP-G1)* `craft.apps[]` entries are `{ app_id, version, allowed_tools?, platforms: { <platform>: { asset_url, sha256, file_name } } }` with `<platform>` one of `macos_arm64`, `macos_x86_64`, `windows_x86_64`, `linux_x86_64`; an absent `allowed_tools` allows every tool the app's MCP mode lists (2D-2 as amended in the closing round).
- **SA-CLOSE-02** *(K10)* The document carries an optional `gitlab` section: `host` (a host name, default `gitlab.com`) and `client_id` (optional). Identity and Credentials signs in to GitLab by device flow when `client_id` is present and takes a pasted personal access token otherwise.
- **SA-CLOSE-03** *(K12)* `language_servers` is a list of `{ language, name, version, platforms: { <platform>: { asset_url, sha256, file_name, binary_path } } }`, at most one entry per `language` (`go`, `rust`, `java`, `typescript`); without an entry for a language the Setup Assistant uses the table compiled into the build (`language-servers.json`, same shape). This replaces the earlier `language_servers.<language>.approved[]` / `.default` form.
- **SA-CLOSE-04** *(chosen here)* Slack identifiers are checked by form: `slack.dedicated_channel` and `RemoteAgent.slack_channel` are channel ids (`^[CG][A-Z0-9]{8,20}$`: public `C…` or private `G…`), and `RemoteAgent.slack_account` (the profile entry's `address`) is a member id (`^[UW][A-Z0-9]{8,20}$`); a profile breaking them is refused with `PROF_CONTENT_INVALID`.
- **SA-RECON-07** The document carries `slack.client_id`, `google.client_secret` and `confluence.token_exchange_url` as HLD 08:82 (revision 0.49.0) states; each is optional.

## Public Interface

All items are in crate `ordin_organisation_profile`. Every name not in 2C, 2D or the HLD is *chosen here*; every section and key name of the document is 2D-2's, except the three marked as additions.

### The service

```rust
pub struct OrganisationProfileService { /* Arc<Inner> */ }

pub enum ProfileMode {
    /// Desktop and personal server.
    Read {
        /// The SQLite pool of the Journal's `Datastore`, handed over by the program (D4, R20).
        pool: sqlx::SqlitePool,
        data_root: std::path::PathBuf,
        http: reqwest::Client,
        /// Values a branded build carries (ADR-041); used when installer.json is absent.
        build_defaults: InstallerSettings,
    },
    /// Agent profile: no fetch, no tables, defaults only.
    NotUsed,
}

pub struct ProfileOptions {
    pub refresh_interval: std::time::Duration,   // 60 minutes
    pub clock: std::sync::Arc<dyn Clock>,
}
pub trait Clock: Send + Sync { fn now(&self) -> Timestamp; }

impl OrganisationProfileService {
    /// Loads the installer settings and the cached profile, then starts the refresh loop.
    /// Never fails for network reasons; fails only on an unusable datastore.
    pub async fn start(mode: ProfileMode, options: ProfileOptions) -> Result<OrganisationProfileService, ProfileError>;
    /// The settings in force: the active verified profile, or the defaults.
    pub fn current(&self) -> std::sync::Arc<ProfileSnapshot>;
    /// Notified each time a newer verified profile becomes active (and once on subscribe).
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>>;
    /// Fetch now (the person's "Try again", Setup Assistant repair). Concurrent calls share one fetch.
    pub async fn refresh(&self) -> Result<RefreshOutcome, ProfileError>;
    pub fn status(&self) -> ProfileStatus;
    pub fn installer_settings(&self) -> InstallerSettings;
    /// Personal server: the setup command's `profile` step (2D-3; SA-NAME-05). The command takes
    /// the address and one or two signing keys (`--signing-key KEY`, repeatable up to twice
    /// *(flag chosen here)*); an address with no key is refused with PROF_INSTALLER_INVALID.
    pub async fn set_profile_address(&self, url: &str, signing_keys: Vec<String>) -> Result<(), ProfileError>;
    pub async fn remote_agents(&self) -> Result<Vec<RemoteAgent>, ProfileError>;
    pub async fn remote_agent(&self, id: RemoteAgentId) -> Result<RemoteAgent, ProfileError>;
    /// The identity settings of a hosted agent's entry, from the active profile.
    pub fn remote_agent_identity(&self, id: RemoteAgentId) -> Option<RemoteIdentity>;
    pub async fn mark_remote_agent_enabled(&self, id: RemoteAgentId) -> Result<RemoteAgent, ProfileError>;
    pub async fn mark_remote_agent_verified(&self, id: RemoteAgentId) -> Result<RemoteAgent, ProfileError>;
    /// Stops the refresh loop; an in-flight fetch is abandoned.
    pub async fn shutdown(&self);
}

pub const MODULE_NAME: &str = "organisation_profile";
pub const SQLITE_MIGRATIONS: &[(i64, &str, &str)];   // see Data Model

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSnapshot {
    pub document: OrganisationProfileDocument,
    pub source: SnapshotSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotSource {
    Verified { profile_id: OrganisationProfileId, profile_version: String, activated_at: Timestamp },
    Defaults,
}

impl ProfileSnapshot {
    /// Every section at the default 2D-2 gives it.
    pub fn defaults() -> ProfileSnapshot;
    /// The harness a new agent instance uses when nothing else names one: the first entry of
    /// `document.harnesses` (2D-2 default `[claude_code]`, so never empty after validation).
    /// The program maps `HarnessName::ClaudeCode` → `HarnessKind::ClaudeCode` and
    /// `HarnessName::Copilot` → `HarnessKind::CopilotCli` for the Package Catalogue's
    /// `HarnessSettings`.
    pub fn default_harness(&self) -> HarnessName;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    /// A newer verified profile is now active.
    Activated { profile_version: String },
    /// The address served the active profile again (HTTP 304, or the same digest).
    Unchanged,
    /// The fetched profile failed a check; the last verified one is kept.
    Refused { code: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileStatus {
    pub state: ProfileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<ProfileFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileState {
    /// Agent profile.
    NotUsed,
    /// No profile address is known (desktop: R-11, setup cannot finish; server: A-40 defaults).
    NoAddress,
    /// An address is known and no verified profile has been activated yet.
    NeverFetched,
    /// A verified profile is active and the last check succeeded within 3 hours.
    Active,
    /// A verified profile is active; no check has succeeded for 3 hours *(chosen here)*.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileFailure { pub code: String, pub at: Timestamp }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct InstallerSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    /// Each a base64url (no padding) raw 32-byte Ed25519 public key; at most two.
    #[serde(default)]
    pub profile_signing_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_feed_url: Option<String>,
}
```

### The organisation profile document

The `default = "…"` helpers (`yes`, `two`, `three`, `four`, `ten`, `twenty`, `thirty`, `sixty`, `balanced`, `heavy`, `when_nearly_full`, `no_pause`, `tls`, `slack_only`, `en_gb`, `empty_object`, `all_sign_in_methods`, `inbox_and_sent`, `default_harnesses`) are private functions returning the value their name or trailing comment states; a section that is absent takes its type's `Default`, which uses the same values.

Top-level keys are 2D-2's sections, plus `profile_version` *(addition chosen here; 2C's `OrganisationProfile.profile_version` needs a source in the document)*. Every struct refuses unknown keys. `#[serde(default)]` marks a section that may be absent and takes the default shown.

```rust
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrganisationProfileDocument {
    pub format_version: u32,
    /// `MAJOR.MINOR.PATCH`, each part 0–999,999,999, compared numerically.
    pub profile_version: String,
    #[serde(default)] pub agent_catalogue: AgentCatalogueSection,
    #[serde(default)] pub remote_agents: Vec<RemoteAgentEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub slack: Option<SlackSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub confluence: Option<ConfluenceSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub microsoft: Option<MicrosoftSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub google: Option<GoogleSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub identity_provider: Option<IdentityProviderSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub gitlab: Option<GitlabSection>,   // SA-CLOSE-02
    #[serde(default = "default_harnesses")] pub harnesses: Vec<HarnessEntry>,
    #[serde(default)] pub usage: UsageSection,
    #[serde(default)] pub voice: VoiceSection,
    #[serde(default)] pub limits: LimitsSection,
    #[serde(default)] pub history: HistorySection,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub email: Option<EmailSection>,
    #[serde(default)] pub channels: ChannelsSection,
    #[serde(default)] pub telegram: TelegramSection,
    #[serde(default)] pub server: ServerSection,
    #[serde(default)] pub nats: NatsSection,
    #[serde(default)] pub craft: CraftSection,
    #[serde(default)] pub language_servers: Vec<LanguageServerEntry>,   // SA-CLOSE-03
    #[serde(default)] pub engineering: EngineeringSection,
    #[serde(default)] pub workflows: WorkflowsSection,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub brand: Option<BrandSection>,
    #[serde(default)] pub locales: LocalesSection,
    #[serde(default)] pub help: HelpSection,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub updates: Option<UpdatesSection>,
    #[serde(default)] pub sandbox: SandboxSection,
}

// ---- agent_catalogue (08:32; ADR-082, ADR-086)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentCatalogueSection {
    #[serde(default)] pub repositories: Vec<AgentRepositoryEntry>,
    #[serde(default = "yes")] pub person_may_add: bool,                       // default true
    /// Executable-trust policy over tool bindings and hooks; typed and applied by the Checker.
    /// Organisation Profile checks only that it is a JSON object of at most 262,144 bytes.
    #[serde(default = "empty_object")] pub trust_policy: serde_json::Value,   // default {}
    /// Default: `help.destination`.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub it_approval_destination: Option<HelpDestination>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentRepositoryEntry {
    /// Stable key that workflow files format 3 cite as `profile_key` *(addition chosen here; 2C wire I-2)*.
    pub key: String,
    pub display_name: String,
    pub host: String,
    pub repository: String,
    pub protected_branch: String,
    pub format: RepositoryFormat,
    #[serde(default)] pub signed_merges_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryFormat { Ordin, Apm }

// ---- remote_agents (02:698; ADR-098 item 8)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteAgentEntry {
    pub name: String,
    pub route: RemoteAgentRoute,
    /// `hosted_agent`: the agent's `https` base address. `slack`: the agent's Slack account name.
    pub address: String,
    /// Required for `hosted_agent`; absent for `slack`.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub identity: Option<RemoteIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentRoute { HostedAgent, Slack }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteIdentity { pub issuer: String, pub client_id: String, pub audience: String }

// ---- slack, confluence, microsoft, google, identity_provider
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackSection {
    /// The Slack Thread Adapter's workspace and dedicated channel (ADR-008). *Amended January 2026 (ADR-102):*
    /// optional, so IT can set the keys below without the Slack route; absent → the Slack Thread
    /// Adapter offers no Slack route, as when the whole section is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub dedicated_channel: Option<String>,
    /// The Slack app IT approves, PKCE-enabled (08:82, revision 0.49.0).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub client_id: Option<String>,
    /// IT can turn the Slack tools, linked channels, drafts and Ordin's reactions off (Slack Service);
    /// the person's own handover notices are not affected (`server.external_actions_off` stops those).
    #[serde(default = "yes")] pub tools_enabled: bool,
    /// only these channel ids may be linked to a workspace or named by a lifecycle stage;
    /// `None` = any channel the person belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub allowed_channels: Option<Vec<String>>,
    /// IT can turn every "without asking" rule off; Slack posts then always ask.
    #[serde(default = "yes")] pub rules_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceSection {
    pub site: String,
    pub oauth_client_id: String,
    /// A company-hosted token exchange, used only if C-12 shows Atlassian 3LO needs a client secret (08:82).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub token_exchange_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MicrosoftSection { pub tenant_id: String, pub client_id: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GoogleSection {
    pub client_id: String,
    /// The non-confidential secret of a Google Desktop OAuth client (08:82, revision 0.49.0).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub client_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentityProviderSection { pub issuer: String, pub client_id: String, #[serde(default)] pub scopes: Vec<String> }

// ---- harnesses (02:707, :829) — default [{name: claude_code, tested_versions: []}]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HarnessEntry { pub name: HarnessName, #[serde(default)] pub tested_versions: Vec<String> }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessName { ClaudeCode, Copilot }

// ---- usage (ADR-062; 08:71)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageSection {
    #[serde(default = "balanced")] pub plan_mode: PlanMode,                              // balanced
    /// Missing kinds allow all four levels.
    #[serde(default)] pub levels_allowed: BTreeMap<WorkKind, Vec<ModelLevel>>,
    /// harness → level → model name. Missing entries take the defaults below.
    #[serde(default)] pub level_models: BTreeMap<HarnessName, BTreeMap<ModelLevel, String>>,
    #[serde(default = "two")] pub agents_at_once: u32,                                   // 2
    #[serde(default = "three")] pub agents_at_once_max: u32,                             // 3
    #[serde(default)] pub defaults: UsageDefaults,
    /// `None` = the person may change every usage setting.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub person_may_change: Option<Vec<UsageSettingName>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanMode { Save, Balanced, Best }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind { Talking, Coordinating, Routine, Documents, Code }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLevel { Automatic, Smaller, Standard, MostCapable }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageDefaults {
    #[serde(default = "yes")] pub answer_without_model: bool,
    #[serde(default = "yes")] pub reuse_answers: bool,
    #[serde(default = "yes")] pub parts_only: bool,
    #[serde(default = "when_nearly_full")] pub summarise: SummariseWhen,
    #[serde(default = "yes")] pub continue_from_summary: bool,
    #[serde(default = "heavy")] pub ask_before: AskBefore,
    /// `None` = "true in workspaces with code" (2D-2).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub show_usage: Option<bool>,
    #[serde(default = "no_pause")] pub daily_pause: DailyPause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummariseWhen { Early, WhenNearlyFull, OnlyWhenAsked }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskBefore { Heavy, MediumOrHeavy, Never }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DailyPause {
    #[serde(rename = "none")] NoPause,
    #[serde(rename = "100")] After100,
    #[serde(rename = "250")] After250,
    #[serde(rename = "500")] After500,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSettingName {
    PlanMode, LevelsAllowed, AgentsAtOnce, AnswerWithoutModel, ReuseAnswers, PartsOnly,
    Summarise, ContinueFromSummary, AskBefore, ShowUsage, DailyPause,
}

// ---- voice (ADR-063; 08:73)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VoiceSection {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub tone: Option<VoiceTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub register: Option<VoiceRegister>,
    #[serde(default)] pub terms_use: Vec<String>,
    #[serde(default)] pub terms_avoid: Vec<String>,
}

// ---- limits (07:44–49; 02:486)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LimitsSection { #[serde(default)] pub crew: CrewLimits, #[serde(default)] pub run: RunLimitsSection }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewLimits {
    #[serde(default = "ten")] pub rounds: u32,                    // 10, 1–30
    #[serde(default = "sixty")] pub wall_clock_minutes: u32,      // 60, 1–480
    #[serde(default = "four")] pub members_at_once: u32,          // 4, 1–8
    #[serde(default, skip_serializing_if = "Option::is_none")] pub request_budget: Option<u32>, // none, ≥ 1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunLimitsSection { #[serde(default = "sixty")] pub wall_clock_minutes: u32 }   // 60, 1–480

// ---- history (2D-2; 1D Configuration pass item 2)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HistorySection { #[serde(default, skip_serializing_if = "Option::is_none")] pub max_days: Option<u32> }

// ---- email (08:72; ADR-070, ADR-071)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EmailSection {
    pub system: MailSystem,
    #[serde(default = "yes")] pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub imap: Option<MailServer>,  // port default 993
    #[serde(default, skip_serializing_if = "Option::is_none")] pub smtp: Option<MailServer>,  // port default 465
    #[serde(default, skip_serializing_if = "Option::is_none")] pub ca_certificate: Option<String>,
    #[serde(default = "all_sign_in_methods")] pub sign_in_methods: Vec<MailSignInMethod>,
    #[serde(default = "inbox_and_sent")] pub people_link_folders: Vec<String>,
    #[serde(default)] pub server_saves_sent: bool,
    #[serde(default)] pub company_domains: Vec<String>,
    #[serde(default)] pub internal_only: bool,
    #[serde(default = "twenty")] pub max_recipients: u32,
    #[serde(default)] pub no_attach_places: Vec<String>,
    #[serde(default)] pub foot_line_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailSystem { CompanyServer, Microsoft365, GoogleWorkspace }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailServer {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub port: Option<u16>,
    #[serde(default = "tls")] pub security: MailSecurity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailSecurity { Tls, Starttls }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailSignInMethod { SingleSignOn, AppPassword, MailPassword }

// ---- channels, telegram (A-40; ADR-075, ADR-077)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChannelsSection { #[serde(default = "slack_only")] pub allowed: Vec<ChatChannel> }   // [slack]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel { Telegram, Slack }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramSection {
    #[serde(default = "yes")] pub tools_enabled: bool,
    /// `None` = any chat a workspace links.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub allowed_chats: Option<Vec<i64>>,   // Telegram chat ids are integers
    #[serde(default)] pub foot_line_required: bool,
}

// ---- server (ADR-073, ADR-074, ADR-096 item 4)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ServerSection {
    #[serde(default)] pub external_actions_off: Vec<ExternalActionKind>,
    #[serde(default = "yes")] pub listening_allowed: bool,
    #[serde(default)] pub client_certificate_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub client_certificate_name: Option<String>,
    #[serde(default)] pub triggers: ServerTriggers,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ServerTriggers { #[serde(default = "twenty")] pub daily_runs: u32 }   // 20, 1–200

// ---- nats (ADR-074)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NatsSection {
    #[serde(default)] pub servers: Vec<String>,
    #[serde(default)] pub tls: NatsTls,
    #[serde(default)] pub subjects_allowed: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NatsTls { #[serde(default, skip_serializing_if = "Option::is_none")] pub ca_certificate: Option<String> }

// ---- craft (02:700, :747)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftSection {
    #[serde(default)] pub apps: Vec<CraftApp>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub mirror_url: Option<String>,
    /// file extension (no dot) → app name.
    #[serde(default)] pub file_types: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftApp {
    /// The app's identifier (the `craft.file_types` values name it).
    pub app_id: String,
    pub version: String,
    /// The app's allowed list (ADR-017); `None` = every tool the app's MCP mode lists (SA-CLOSE-01).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub allowed_tools: Option<Vec<String>>,
    pub platforms: BTreeMap<ProfilePlatform, CraftAsset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftAsset { pub asset_url: String, pub sha256: Sha256Hex, pub file_name: String }

/// Platforms a pinned download names (craft apps and language servers; K9, K12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProfilePlatform {
    #[serde(rename = "macos_arm64")] MacosArm64,
    #[serde(rename = "macos_x86_64")] MacosX86_64,
    #[serde(rename = "windows_x86_64")] WindowsX86_64,
    #[serde(rename = "linux_x86_64")] LinuxX86_64,
}

// ---- gitlab (D9; SA-CLOSE-02)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GitlabSection {
    #[serde(default = "gitlab_com")] pub host: String,                                   // "gitlab.com"
    #[serde(default, skip_serializing_if = "Option::is_none")] pub client_id: Option<String>,
}

// ---- language_servers (ADR-055; SA-CLOSE-03)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LanguageServerEntry {
    pub language: ServerLanguage,
    pub name: String,
    pub version: String,
    pub platforms: BTreeMap<ProfilePlatform, LanguageServerAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerLanguage { Go, Rust, Java, Typescript }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LanguageServerAsset { pub asset_url: String, pub sha256: Sha256Hex, pub file_name: String, pub binary_path: String }

// ---- engineering (02:766; 04:139; SA-GH-01)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EngineeringSection {
    #[serde(default)] pub github: GithubPolicy,
    #[serde(default)] pub coding_tasks: CodingTasksPolicy,   // amended January 2026: 2D-2's key, read by the Project Service
}

/// `engineering.coding_tasks` (2D-2; ADR-053). An absent section or key reads as the default, 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CodingTasksPolicy { #[serde(default = "three")] pub max_attempts: u32 }   // Default: max_attempts 3

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GithubPolicy { #[serde(default)] pub always_approved_kinds: Vec<String> }

// ---- workflows (08:20; ADR-066)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsSection {
    #[serde(default, skip_serializing_if = "Option::is_none")] pub company_repository: Option<CompanyRepository>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyRepository { pub host: String, pub repository: String, pub folder: String }

// ---- brand (08:33; ADR-041). Brand fetches each asset, checks its hash and the contrast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandSection {
    pub brand_identifier: String,
    pub version: String,
    /// locale tag → strings.
    pub strings: BTreeMap<String, BrandStringsEntry>,
    pub logos: ThemedAsset,
    pub icon_set: ThemedAsset,
    pub design_tokens: BTreeMap<String, String>,
    #[serde(default)] pub fonts: Vec<BrandAsset>,
    pub links: BrandLinks,
    /// locale tag → legal text.
    pub legal_text: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandStringsEntry { pub product_name: String, pub coordinator_name: String, pub about_text: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ThemedAsset { pub light: BrandAsset, pub dark: BrandAsset }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandAsset { pub url: String, pub sha256: Sha256Hex, pub media_type: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandLinks { pub help_url: String, pub support_url: String, pub privacy_url: String }

// ---- locales (ADR-042; 08:34)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalesSection {
    /// Empty = every bundled locale (Localisation intersects with its bundled catalogues).
    #[serde(default)] pub offered: Vec<String>,
    #[serde(default = "en_gb")] pub fallback: String,      // "en-GB"
}

// ---- help (02:704; 08:14–15; OQ-HELP-REPLY decision (b))
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpSection {
    /// `None` = "Send to IT" goes through the person's own mail app (08:14).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub destination: Option<HelpDestination>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpDestination { pub kind: HelpDestinationKind, pub address: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelpDestinationKind { TicketUrl, Email, SlackChannel }

// ---- updates (2D-2; see the reading on ADR-013 below)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UpdatesSection { pub feed_url: String }

// ---- sandbox (09:737; 04:403)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SandboxSection { #[serde(default = "thirty")] pub session_idle_minutes: u32 }   // 30, 1–240

impl OrganisationProfileDocument {
    /// The model name a harness uses at a level, applying 2D-2's defaults:
    /// claude_code: smaller → "haiku", standard → "sonnet", most_capable → "opus" *(model aliases assumed here)*;
    /// copilot: automatic → None (Copilot's own selection).
    pub fn level_model(&self, harness: HarnessName, level: ModelLevel) -> Option<String>;
    /// `agent_catalogue.it_approval_destination`, else `help.destination`.
    pub fn it_approval_destination(&self) -> Option<&HelpDestination>;
}
```

`updates.feed_url`: a verified profile's feed replaces the installer's (2D-2b, 08:77; decision D10 — the contrary sentence of ADR-013's revision 0.48.0 amendment is withdrawn). This component only carries the value; the Setup Assistant owns the update check (R16).

### Service API commands *(names chosen here; SA-API-01)*

| Command | Input | Result | Method called |
|---|---|---|---|
| `profile.status` | `{}` | `ProfileStatus` | `status` |
| `profile.refresh` | `{}` | `RefreshOutcome` | `refresh` |
| `profile.get_document` | `{}` | `{ "source": "verified" \| "defaults", "profile_version": string?, "document": OrganisationProfileDocument }` | `current` (Diagnostics, 04:412 "shown in Diagnostics") |
| `remote_agents.list` | `{}` | `{ "items": [RemoteAgent] }` | `remote_agents` |

`profile.status` on the agent profile returns `state = not_used`. These rows are the definitive names and result types; the Service API catalogue takes them as written (R1): `profile.refresh` returns `RefreshOutcome` (not `ProfileStatus`), `profile.get_document` returns the object above (not `ProfileDocument { text }`), `remote_agents.list` returns `{ "items": [RemoteAgent] }`.

#### How consumers read the profile (reconciliation rows 520–528; R6)

The profile publishes no per-consumer getter trait. Every consumer reads `current()` (an `Arc<ProfileSnapshot>`) and, to follow changes, `subscribe()`; it uses the section types of this spec as written, with the section defaults already applied (sections are never `Option` unless marked below). Where a consumer has no edge to this crate, the program builds that consumer's own small input from `current()` and `subscribe()`.

| Consumer | Reads | Notes |
|---|---|---|
| Catalogue Service | `document.agent_catalogue` (`AgentRepositoryEntry` with `key` and `RepositoryFormat`) | no `AgentCatalogueSettings`/`ProfileAgentCatalogue` |
| Channel Gateway | `document.channels.allowed: Vec<ChatChannel>` (never absent; default `[slack]`) | the gateway maps `ChatChannel` to its `ChannelKind` |
| NATS Adapter, Trigger Service | `document.nats.servers` (empty = off), `document.nats.tls.ca_certificate`, `document.nats.subjects_allowed`; `document.server.triggers.daily_runs` (`u32`, default 20) | sections are not `Option` |
| Mail Service | `document.email` (`Option<EmailSection>`, with `MailSystem`, `MailServer`, `MailSecurity`) | MailService applies the port defaults 993/465 when `port` is `None` |
| Telegram Service | `document.telegram` (`allowed_chats: Option<Vec<i64>>`) and `document.channels.allowed` | the program builds `TelegramRules` from them |
| Slack Service | `document.slack` (`tools_enabled`, `allowed_channels`, `rules_allowed`, `client_id`; section defaults when absent) and, on the personal server, `document.server.external_actions_off` (`slack_post` named or not) | the program builds `SlackRules` from them (SlackService.md) and follows `subscribe()` |
| Identity and Credentials (through the program) | `document.slack.client_id` | the program builds `ProviderConfig::Slack`, or uses the person's own app's client id when absent (IdentityAndCredentials.md) |
| Craft Toolkit | `document.craft` (`CraftSection`, `CraftApp`, `CraftAsset`, `ProfilePlatform`) through `subscribe()` | SA-CLOSE-01 |
| Identity and Credentials (through the program) | `document.gitlab` (`GitlabSection`; absent → host `gitlab.com`, no client id) | the program builds `ProviderConfig::Gitlab` or offers `save_gitlab_token` (SA-CLOSE-02) |
| Setup Assistant, Project Service | `document.language_servers` (`LanguageServerEntry`) | empty for a language → the build's `language-servers.json` (SA-CLOSE-03) |
| Package Catalogue | `ProfileSnapshot::default_harness()` through the program's `HarnessSettings` | |
| Remote API | `document.server.listening_allowed`, `.client_certificate_required`, `.client_certificate_name` at start and on each `subscribe()` change | |
| Setup Assistant | `document.help.destination`; `None` means **Send to IT** opens the person's own mail app (08:14), never "off" | |
| Rules Engine | the sections its own spec names, through `current()` and `subscribe()` (it has an edge to this crate, 2B-3) | |
| Journal (through the program) | `document.history.max_days` passed to `Journal::set_company_max_days` | |
| Crew Orchestrator | `document.limits.crew` (`CrewLimits`: `rounds`, `wall_clock_minutes`, `members_at_once`, `request_budget`) through `current()` | |
| Localisation | `document.locales` (`LocalesSection`) through `current()` at start and `subscribe()` after | |
| Brand | `document.brand` (`Option<BrandSection>`) through `subscribe()` | `None` activates the newest build pack |
| Checker | `document.agent_catalogue.trust_policy` (untyped JSON, typed by the Checker's `TrustPolicy`) through `current()` | the programs call `reevaluate_trust` on each `subscribe()` change |
| Usage Planner | `document.usage` (`UsageSection`: `plan_mode`, `levels_allowed`, `level_models`) through `current()` and `subscribe()`, and `ProfileSnapshot::defaults()` | it holds `Arc<OrganisationProfileService>` |

#### Who calls each public item

| Item | Called by |
|---|---|
| `start`, `shutdown` | desktop program, server program (personal and agent profiles) |
| `current`, `subscribe` | the consumers in the table above; the programs |
| `refresh`, `status` | the programs (`profile.refresh`, `profile.status`); Setup Assistant (repair step) |
| `installer_settings` | Setup Assistant (installer feed address, D10) |
| `set_profile_address` | server program, `ordin-server setup profile <URL> --signing-key <KEY>` |
| `remote_agents`, `remote_agent` | the programs (`remote_agents.list`), Hosted Agent Adapter's `RemoteAgentDirectory` built by the program |
| `remote_agent_identity` | the program, building the Hosted Agent Adapter's `OidcSettings` for an entry |
| `mark_remote_agent_enabled`, `mark_remote_agent_verified` | the program, on the adapter's first sign-in and first successful read for that agent's route (step 7) |
| `MODULE_NAME`, `SQLITE_MIGRATIONS` | the programs, registering them with the Journal's `MigrationRegistry` (R20) |
| `ProfileMode::Read.pool` | the program passes the SQLite pool of the Journal's `Datastore` (`datastore.sqlite()`); this crate has no Journal edge (D4, R20) |

#### Example

Installer file written by the company installer:

```json
{
  "profile_url": "https://it.example.com/ordin/profile.json",
  "profile_signing_keys": ["11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"],
  "update_feed_url": "https://it.example.com/ordin/updates/latest.json"
}
```

Files IT publishes: `https://it.example.com/ordin/profile.json` (the document) and `https://it.example.com/ordin/profile.json.jws` *(companion address chosen here)* holding the detached JWS:

```
eyJhbGciOiJFZERTQSIsImtpZCI6IjNmYTk0MWMyMGI3ZDE0ZTUifQ..hR8t0m5o2Gq0Yb2cJb2y4kW9wQ7l3m1oF5uP0q8t6v8e9Q1a3s5d7f9h1j3k5l7n9p1r3t5v7x9z1B3D5F7H9J1K
```

Header decoded: `{"alg":"EdDSA","kid":"3fa941c20b7d14e5"}`. Document excerpt:

```json
{
  "format_version": 1,
  "profile_version": "2026.10.3",
  "agent_catalogue": {
    "repositories": [{"key": "company-agents", "display_name": "Company agents", "host": "github.com",
                      "repository": "example-co/agents", "protected_branch": "main", "format": "ordin"}],
    "person_may_add": true
  },
  "remote_agents": [{"name": "Finance assistant", "route": "hosted_agent",
                     "address": "https://finance-agent.example.com",
                     "identity": {"issuer": "https://example.okta.com", "client_id": "0oa1b2c3", "audience": "api://finance-agent"}}],
  "harnesses": [{"name": "claude_code", "tested_versions": ["2.1.304"]}],
  "usage": {"plan_mode": "balanced", "agents_at_once_max": 2},
  "locales": {"offered": ["en-GB", "de-DE"], "fallback": "en-GB"},
  "help": {"destination": {"kind": "email", "address": "it-help@example.com"}}
}
```

`profile.refresh` reply when the address serves the same version: `{"kind":"ok","result":"unchanged"}`; when the fetched signature fails: `{"kind":"ok","result":{"refused":{"code":"PROF_SIGNATURE_INVALID"}}}` — the reply is a result, not an error, because the last verified profile is still in force.

## Internal Logic

### 1. Start — `OrganisationProfileService::start`

1. `ProfileMode::NotUsed` → hold `ProfileSnapshot::defaults()`, `ProfileStatus { state: not_used }`; start no task; return. Every other method returns its empty answer (`remote_agents` → empty list; `refresh` → `PROF_NOT_USED`; `set_profile_address` → `PROF_NOT_USED`).
2. **Installer settings.** Read `<data_root>/installer.json` (at most 16 KiB). Absent → use `build_defaults`. Present but unparseable, or with a non-`https` URL, more than two keys, or a key that does not decode to exactly 32 bytes of base64url → log `profile.installer.invalid` with `code = PROF_INSTALLER_INVALID` and use `build_defaults`. No `profile_url` and no signing key in either → state `no_address`.
3. **Cached profile.** Read the row with `state = 'active'` from `organisation_profiles`. When present, read `active.json` and `active.jws` from its `cached_path` folder and run *verify* (step 3) against the current installer keys, skipping the version-order check. Pass → snapshot `Verified` from that row, state `active`, `last_success_at = activated_at`. Fail (files missing, keys rotated, document invalid) → log `profile.cache.invalid` with the failure code, keep defaults, state `never_fetched`; the row stays `active` until a fetched profile replaces it.
4. Publish the snapshot on the watch channel.
5. When an address is known, spawn the refresh loop (step 6) whose first fetch runs at once.
6. A datastore error in steps 3 → `PROF_DATASTORE_UNAVAILABLE`; `start` fails and the program stops with that code (no settings source can be trusted without it).

### 2. Fetching — inside `refresh`

1. Single flight: a `tokio::sync::Mutex` guards one in-flight fetch; a caller arriving while one runs awaits its outcome.
2. Address check: `profile_url` must be an absolute `https` URL of at most 2,048 bytes (`PROF_ADDRESS_INVALID`); none → `PROF_NO_ADDRESS`.
3. `GET profile_url` with `Accept: application/json`, `If-None-Match: <etag of the active profile>` when one is held in memory, connect timeout 10 s, total timeout 30 s, at most 3 redirects and only to `https` addresses. Read the body with a cap of 1,048,576 bytes (`PROF_TOO_LARGE` beyond).
   - Network error or timeout → `PROF_UNREACHABLE` (retryable).
   - `304 Not Modified` → outcome `Unchanged`; set `last_success_at`; done.
   - Any status other than 200 or 304 → `PROF_HTTP_STATUS` (retryable for 408, 429 and 5xx; permanent otherwise), logged with `status`.
4. `GET profile_url + ".jws"` with the same limits and a body cap of 8,192 bytes. 404 → `PROF_SIGNATURE_MISSING`; other failures as in 2.3.
5. `document_digest` = SHA-256 of the document bytes. When it equals the active row's digest → outcome `Unchanged`, set `last_success_at`; done.
6. When a row with this digest already exists in `failed_verification` → outcome `Refused { code }` with that row's stored code; no new row; done (a bad profile is not re-recorded every hour).
7. Insert a row: new `OrganisationProfileId`, `state = fetched`, `signature` = the JWS text, `document_digest`, `cached_path` = `<data_root>/profile/` *(the folder; the file names are fixed)*, `fetched_at = now`, `profile_version = "unknown"` until parsed *(chosen here; replaced in step 3.4)*.
8. Run *verify* (step 3). Failure → update the row to `failed_verification` with `refusal_reason` = the code and, for content failures, the dotted key path (for example `PROF_CONTENT_INVALID usage.agents_at_once_max`), ≤ 500 characters; set status `last_failure`; log `profile.refused` with `code`, `key_path`; outcome `Refused`.
9. Success → *activate* (step 4); outcome `Activated`.
10. After each attempt set `last_attempt_at`; on success set `last_success_at` and clear `last_failure`.

### 3. Verify

1. **JWS form.** The text is `<header>..<signature>` — three segments split on `.`, the middle one empty, each other segment base64url without padding. Otherwise `PROF_SIGNATURE_INVALID`.
2. **Header.** JSON object with `alg` (must be `EdDSA`), optional `kid` (16 lowercase hex characters) and optional `typ`; any other key, or `b64` present → `PROF_SIGNATURE_INVALID`.
3. **Key.** Each installer key's `kid` is the first 16 hex characters of SHA-256 of its raw 32 bytes. With `kid` present, use the matching key, none matching → `PROF_KEY_UNKNOWN`; with `kid` absent, try each key in order.
4. **Signature.** Signing input = `<header segment> + "." + base64url(document bytes)`. `ed25519_dalek::VerifyingKey::verify_strict` with the 64-byte signature; failure → `PROF_SIGNATURE_INVALID`. Nothing in the document is parsed before this passes.
5. **Parse.** UTF-8 JSON into `OrganisationProfileDocument` (unknown keys refused). A parse failure → `PROF_CONTENT_INVALID` with the serde path. `format_version != 1` → `PROF_FORMAT_UNSUPPORTED` (checked by reading `format_version` alone first, so a newer format is reported as such and not as a content error).
6. **Bounds.** Apply every rule of the *Validation table* below in table order; first failure → `PROF_CONTENT_INVALID` with its key path.
7. **Order.** `profile_version` must be strictly greater than the active row's (component-wise numeric comparison of `MAJOR.MINOR.PATCH`) → else `PROF_VERSION_NOT_NEWER`. Equal version with a different digest is the same refusal (a re-signed profile must raise its version).
8. Update the row's `profile_version` and, on success, `state = verified`, `verified_at = now`.

### 4. Activate

1. Write `active.json.tmp` and `active.jws.tmp` in `<data_root>/profile/` (folder mode `0700`, files `0600` on macOS and Linux), `fsync`, then rename over `active.json` and `active.jws`. A write failure is logged `profile.cache.write_failed` and activation continues (the next start re-fetches).
2. One SQLite transaction:
   1. `UPDATE organisation_profiles SET state = 'replaced' WHERE state = 'active'`.
   2. `UPDATE organisation_profiles SET state = 'active', activated_at = now WHERE id = ? AND state = 'verified'`; zero rows changed → roll back, `PROF_UNVERIFIED_ACTIVATION` (a defect; logged at `error`).
   3. Sync remote agents (step 5).
   4. Delete rows in `replaced` or `failed_verification` beyond the newest 20 by `fetched_at` *(chosen here)*.
3. Commit; a commit failure → `PROF_DATASTORE_UNAVAILABLE` (retryable), the in-memory snapshot is unchanged.
4. Replace the snapshot with `Verified { profile_id, profile_version, activated_at }` and send it on the watch channel; remember the response `ETag` in memory.
5. Log `profile.activated` with `profile_version`, `previous_version`.

### 5. Remote agent catalogue sync (inside the activation transaction)

1. For each `RemoteAgentEntry` (matched to an existing non-withdrawn row by `display_name = name`):
   - map `route`: `hosted_agent` → `http` with `deployment_address = address`, `identity_provider_ref = name`; `slack` → `slack` with `slack_account = address`, `slack_channel = slack.dedicated_channel` (the `slack` section is required when any entry uses this route; checked in step 3.6);
   - no row → insert with a new `RemoteAgentId`, `status = listed`, `listed_at = now`;
   - row exists, same route and address → unchanged (status kept);
   - row exists, route or address changed → update the fields and set `status = listed` *(chosen here: a changed address must be re-verified)*.
2. Every non-withdrawn row whose name is no longer listed → `status = withdrawn`.
3. Log `profile.remote_agents.synced` with counts `listed_new`, `changed`, `withdrawn`.

### 6. Refresh loop

1. While no verified profile is active (state `never_fetched`): attempt; on failure wait 5 s, then double each time up to 5 minutes, with ±20 % jitter, without end.
2. Once active: wait `refresh_interval` (60 minutes) between attempts; a failure leaves the active profile in force and the next attempt is at the next interval.
3. Status `stale` when an active profile exists and `now − last_success_at > 3 hours`.
4. `shutdown` cancels the loop.

### 7. Remote agent status changes

1. `mark_remote_agent_enabled(id)`: row must exist (`PROF_REMOTE_AGENT_NOT_FOUND`) and not be `withdrawn` (`PROF_REMOTE_AGENT_WITHDRAWN`); `listed → enabled`; `enabled` and `verified` are left unchanged (idempotent). The program calls it when an adapter reports the person's first sign-in for that agent's route.
2. `mark_remote_agent_verified(id)`: same checks; `enabled → verified`; `listed` → `PROF_REMOTE_AGENT_NOT_ENABLED`; `verified` unchanged. The program calls it on the adapter's first successful read of the deployment or channel.
3. `remote_agent_identity(id)`: the `identity` of the entry named by the row's `identity_provider_ref` in the current snapshot; `None` for Slack agents.

### 8. Setting the address on the personal server — `set_profile_address`

1. `url`: absolute `https`, ≤ 2,048 bytes → else `PROF_ADDRESS_INVALID`. `signing_keys`: 1 or 2 entries, each base64url of exactly 32 bytes → else `PROF_INSTALLER_INVALID`.
2. Write `installer.json` atomically (`.tmp`, `fsync`, rename; mode `0600`), keeping an existing `update_feed_url`. A write failure → `PROF_INSTALLER_WRITE_FAILED`.
3. Replace the in-memory installer settings; when the address changed, drop the in-memory `ETag`, set state `never_fetched` if no profile is active, and start the refresh loop if it was not running.
4. Run `refresh()` once and return; a refresh failure does not fail this call (the setup command prints the status line from `status()`).

### 9. Default harness — `ProfileSnapshot::default_harness`

1. Return `document.harnesses[0].name`. Validation (step 3.6) guarantees 1–2 entries; the defaults hold `[claude_code]`. Pure; no I/O.

### Validation table (step 3.6)

| Key path | Rule | Source |
|---|---|---|
| `profile_version` | `^\d{1,9}\.\d{1,9}\.\d{1,9}$` | chosen here |
| `agent_catalogue.repositories` | ≤ 50 entries *(assumed)*; `key` `^[a-z0-9-]{1,64}$` unique; `display_name` 1–120 chars unique; `host` a DNS name ≤ 253 chars; `repository` `owner/name`, each part `^[A-Za-z0-9_.-]{1,100}$`; `protected_branch` 1–255 chars | 2D-2; LD-16 |
| `agent_catalogue.trust_policy` | a JSON object, ≤ 262,144 bytes | 2D-2 |
| `agent_catalogue.it_approval_destination`, `help.destination` | `ticket_url`: `https` URL ≤ 2,048 bytes; `email`: one address ≤ 254 chars with one `@`; `slack_channel`: `^#?[a-z0-9_-]{1,80}$` or a Slack channel id `^C[A-Z0-9]{8,12}$` | 2D-2 |
| `remote_agents` | ≤ 100 *(assumed)*; `name` 1–120 chars unique; `hosted_agent`: `address` `https` URL ≤ 2,048 bytes and `identity` present (`issuer` `https` URL, `client_id` and `audience` 1–255 chars); `slack`: `address` a Slack member id matching `^[UW][A-Z0-9]{8,20}$` (it becomes `RemoteAgent.slack_account`, SA-CLOSE-04), `identity` absent, and the `slack` section present | 2D-2; 2C RemoteAgent |
| `slack` | `workspace` 1–255 chars when present; `dedicated_channel` a Slack channel id matching `^[CG][A-Z0-9]{8,20}$` (SA-CLOSE-04) when present; `workspace` and `dedicated_channel` both present or both absent; `client_id` 1–255 chars when present; `allowed_channels` at most 200 entries *(chosen here)*, each matching `^[CG][A-Z0-9]{8,20}$` (SA-CLOSE-04), distinct | 2D-2; 08:82; ADR-102 |
| `confluence.site` | `https` URL; `oauth_client_id` 1–255 chars; `token_exchange_url` `https` URL ≤ 2,048 bytes when present | 2D-2; 08:82 |
| `microsoft` | `tenant_id` a UUID or a DNS name ≤ 253; `client_id` a UUID | C-14 |
| `google.client_id` | 1–255 chars; `client_secret` 1–255 chars when present | 2D-2; 08:82 |
| `identity_provider` | `issuer` `https` only; `client_id` 1–255; `scopes` ≤ 20, each `^[\x21-\x7e]{1,128}$` | 2D-2 |
| `harnesses` | 1–2 entries, names unique; `tested_versions` ≤ 50, each 1–64 chars | 2D-2 |
| `usage.levels_allowed` | each listed kind has 1–4 distinct levels | ADR-062 |
| `usage.level_models` | model names 1–100 chars, matching the adapters' `accepts_model` (C-3) | ADR-062 |
| `usage.agents_at_once`, `agents_at_once_max` | each 1–3; `agents_at_once ≤ agents_at_once_max` | 2D-2 |
| `usage.person_may_change` | distinct names | 2D-2 |
| `voice.terms_use`, `voice.terms_avoid` | ≤ 200 each *(assumed)*, each 1–80 chars | ADR-063 |
| `limits.crew` | `rounds` 1–30; `wall_clock_minutes` 1–480; `members_at_once` 1–8; `request_budget` ≥ 1 | 07:44–49 |
| `limits.run.wall_clock_minutes` | 1–480 | 2D-2 |
| `history.max_days` | ≥ 1 | 2D-2 |
| `email` | `company_server` requires `imap`, `smtp` and ≥ 1 `company_domains`; hosts DNS names ≤ 253; ports 1–65535 (defaults 993 IMAP, 465 SMTP); `ca_certificate` PEM ≤ 65,536 bytes starting `-----BEGIN CERTIFICATE-----`; `sign_in_methods` 1–3 distinct; `people_link_folders` 1–20, each 1–255; `company_domains` ≤ 100 DNS names; `max_recipients` 1–500; `no_attach_places` ≤ 100 patterns of 1–1,024 chars | 08:72; 2D-2 |
| `channels.allowed` | distinct | A-40 |
| `telegram.allowed_chats` | ≤ 500 entries, each a non-zero integer (a Telegram chat id; group ids are negative) | ADR-075 |
| `server` | `client_certificate_name` 1–255 chars, required when `client_certificate_required`; `triggers.daily_runs` 1–200; `external_actions_off` distinct | 08:85; 02:633 |
| `nats` | `servers` ≤ 10, each `tls://` or `wss://`; `tls.ca_certificate` PEM ≤ 65,536 bytes; `subjects_allowed` ≤ 200, each `^[A-Za-z0-9_.*>-]{1,255}$` | ADR-074 |
| `craft` | `apps` ≤ 50, `app_id` `^[a-z0-9][a-z0-9-]{0,39}$` unique, `version` 1–64, `allowed_tools` ≤ 200 entries each 1–128 chars when present, `platforms` 1–4 entries keyed by the four platform names, each `asset_url` `https` ≤ 2,048 bytes, `sha256` 64 lowercase hex, `file_name` 1–255 chars with no `/`, `\\` or `..`; `mirror_url` `https`; `file_types` keys `^[a-z0-9]{1,16}$`, values a listed `app_id` | 02:747; LD-32; SA-CLOSE-01 |
| `gitlab` | `host` a DNS name ≤ 253 chars, no scheme, path or port; `client_id` 1–256 chars | D9; SA-CLOSE-02 |
| `language_servers` | ≤ 4 entries, `language` distinct; `name` `^[a-z0-9][a-z0-9_.-]{0,63}$`; `version` 1–64 chars with no `/`; `platforms` 1–4 entries keyed by the four platform names, each `asset_url` `https` ≤ 2,048 bytes, `sha256` 64 lowercase hex, `file_name` 1–255 chars with no `/`, `\\` or `..`, `binary_path` 1–255 chars, relative, `/`-separated, no `..` | ADR-055; SA-CLOSE-03 |
| `engineering.github.always_approved_kinds` | each one of SA-GH-01's write kinds: `comment`, `open_pull_request`, `update_pull_request`, `request_review`, `push_branch`, `create_branch`, `delete_branch`, `label`, `merge`, `release`, `close` | SA-GH-01 |
| `engineering.coding_tasks.max_attempts` | ≥ 1 (amended January 2026) | 2D-2; ADR-053 |
| `workflows.company_repository` | `host` DNS name; `repository` as above; `folder` 1–255 chars, no `..` | 08:20 |
| `brand` | `brand_identifier` `^[a-z0-9-]{1,64}$`; `version` `MAJOR.MINOR.PATCH`; ≥ 1 `strings` entry; locale keys BCP 47 ≤ 35 chars; `product_name`, `coordinator_name` 1–60 chars; `about_text` and each `legal_text` 1–8,000 chars; every asset `url` `https` ≤ 2,048 bytes, `media_type` `type/subtype` ≤ 255, `sha256` 64 hex; `design_tokens` ≤ 500 entries, values ≤ 256 chars; `fonts` ≤ 20; links `https` ≤ 2,048 bytes | LD-07; 2C BrandPack |
| `locales` | `offered` ≤ 50 distinct BCP 47 tags (`^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$`, ≤ 35 chars); `fallback` the same form, and a member of `offered` when `offered` is non-empty | 2D-2 |
| `updates.feed_url` | `https` ≤ 2,048 bytes | 2D-2 |
| `sandbox.session_idle_minutes` | 1–240 | 2D-2 |

## Data Model

Tables owned and written only by `ordin-organisation-profile`, in the journal database (2C). This crate holds no Journal edge (2B-3): the program gives it the `sqlx::SqlitePool` that the Journal's `Datastore::Sqlite` wraps and registers `SQLITE_MIGRATIONS` under module `organisation_profile` with the Journal's migration registry. No PostgreSQL variant: the agent profile reads no organisation profile.

```sql
-- organisation_profile v1: "create organisation_profiles and remote_agents"
CREATE TABLE organisation_profiles (
    id               TEXT PRIMARY KEY CHECK (length(id) = 36),
    profile_version  TEXT NOT NULL CHECK (length(profile_version) BETWEEN 1 AND 64),
    signature        TEXT NOT NULL CHECK (length(signature) BETWEEN 1 AND 8192),
    document_digest  TEXT NOT NULL UNIQUE CHECK (length(document_digest) = 64),
    cached_path      TEXT NOT NULL CHECK (length(cached_path) BETWEEN 1 AND 4096),
    state            TEXT NOT NULL CHECK (state IN ('fetched','verified','active','replaced','failed_verification')),
    refusal_reason   TEXT CHECK (refusal_reason IS NULL OR length(refusal_reason) <= 500),
    fetched_at       TEXT NOT NULL,
    verified_at      TEXT,
    activated_at     TEXT,
    CHECK ((state = 'failed_verification') = (refusal_reason IS NOT NULL)),
    CHECK (state NOT IN ('active','replaced') OR (verified_at IS NOT NULL AND activated_at IS NOT NULL))
);
CREATE UNIQUE INDEX organisation_profiles_one_active ON organisation_profiles (state) WHERE state = 'active';
CREATE INDEX organisation_profiles_fetched ON organisation_profiles (fetched_at DESC);

CREATE TABLE remote_agents (
    id                     TEXT PRIMARY KEY CHECK (length(id) = 36),
    display_name           TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    route                  TEXT NOT NULL CHECK (route IN ('http','slack')),
    deployment_address     TEXT CHECK (deployment_address IS NULL OR length(deployment_address) <= 2048),
    slack_account          TEXT CHECK (slack_account IS NULL OR length(slack_account) <= 80),
    slack_channel          TEXT CHECK (slack_channel IS NULL OR length(slack_channel) <= 80),
    identity_provider_ref  TEXT CHECK (identity_provider_ref IS NULL OR length(identity_provider_ref) <= 120),
    status                 TEXT NOT NULL CHECK (status IN ('listed','enabled','verified','withdrawn')),
    listed_at              TEXT NOT NULL,
    CHECK (route <> 'http'  OR (deployment_address IS NOT NULL AND identity_provider_ref IS NOT NULL
                                AND slack_account IS NULL AND slack_channel IS NULL)),
    CHECK (route <> 'slack' OR (slack_account IS NOT NULL AND slack_channel IS NOT NULL
                                AND deployment_address IS NULL))
);
CREATE UNIQUE INDEX remote_agents_name_live ON remote_agents (display_name) WHERE status <> 'withdrawn';
```

Files under `<application data root>`: `installer.json` (read; written only by `set_profile_address` on the server), `profile/active.json`, `profile/active.jws`.

Destructive operations: rows beyond the newest 20 replaced or refused profiles are deleted at each activation; the active row is never deleted. Withdrawn remote agents are kept (their identifiers stay valid in old journal records).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No profile address known (desktop: setup cannot finish, R-11) | `Err(permanent)` | `PROF_NO_ADDRESS` | `{"error":"ordin can't reach your company's settings.","code":"PROF_NO_ADDRESS","action":"send_to_it"}` |
| The address cannot be reached (DNS, TLS, proxy, timeout) | `Err(retryable)` | `PROF_UNREACHABLE` | `{"error":"ordin can't reach your company's settings.","code":"PROF_UNREACHABLE","action":"try_again"}` (11:1042 †) |
| The address answers a status other than 200 or 304 | `Err(retryable)` for 408, 429, 5xx; `Err(permanent)` otherwise | `PROF_HTTP_STATUS` | same sentence; `action` `try_again` or `send_to_it` |
| Document over 1 MiB or signature over 8 KiB | `Err(permanent)` → `Refused` | `PROF_TOO_LARGE` | `profile.refresh` result `{"refused":{"code":"PROF_TOO_LARGE"}}` |
| Signature file absent | `Err(permanent)` → `Refused` | `PROF_SIGNATURE_MISSING` | `{"refused":{"code":"PROF_SIGNATURE_MISSING"}}` |
| JWS malformed, wrong algorithm, or signature fails | `Err(permanent)` → `Refused` | `PROF_SIGNATURE_INVALID` | `{"refused":{"code":"PROF_SIGNATURE_INVALID"}}` |
| `kid` names no installer key | `Err(permanent)` → `Refused` | `PROF_KEY_UNKNOWN` | `{"refused":{"code":"PROF_KEY_UNKNOWN"}}` |
| `format_version` not read by this build | `Err(permanent)` → `Refused` | `PROF_FORMAT_UNSUPPORTED` | `{"refused":{"code":"PROF_FORMAT_UNSUPPORTED"}}` |
| Unknown key, wrong type, or a bound broken | `Err(permanent)` → `Refused` | `PROF_CONTENT_INVALID` | `{"refused":{"code":"PROF_CONTENT_INVALID"}}`; key path in Diagnostics only |
| Version not newer than the active one | `Err(permanent)` → `Refused` | `PROF_VERSION_NOT_NEWER` | `{"refused":{"code":"PROF_VERSION_NOT_NEWER"}}` |
| Activation of a row that is not `verified` | `Err(permanent)` (defect) | `PROF_UNVERIFIED_ACTIVATION` | log only |
| A second `active` row (unique index) | `Err(permanent)` (defect) | `PROF_MULTIPLE_ACTIVE` | log only |
| Datastore read or write fails | `Err(retryable)` | `PROF_DATASTORE_UNAVAILABLE` | `{"error":"Something went wrong. Try again.","code":"PROF_DATASTORE_UNAVAILABLE","action":"try_again"}` † |
| `installer.json` unreadable or invalid | logged; build defaults used | `PROF_INSTALLER_INVALID` | setup command: "The profile address or signing key is not valid." † |
| Setup command gives a non-`https` or over-long address | `Err(permanent)` | `PROF_ADDRESS_INVALID` | setup command: "Enter the profile's https address." † |
| `installer.json` cannot be written | `Err(retryable)` | `PROF_INSTALLER_WRITE_FAILED` | setup command: "ordin couldn't save the profile address in its data folder." † |
| Cached profile fails verification at start | logged; defaults until a fetch succeeds | `PROF_CACHE_INVALID` | Diagnostics only |
| Command used in the agent profile | `Err(permanent)` | `PROF_NOT_USED` | `{"error":"This server doesn't use a company settings file.","code":"PROF_NOT_USED","action":null}` † |
| Remote agent id unknown | `Err(permanent)` | `PROF_REMOTE_AGENT_NOT_FOUND` | `{"error":"This agent is no longer available.","code":"PROF_REMOTE_AGENT_NOT_FOUND","action":null}` † |
| Status change on a withdrawn remote agent | `Err(permanent)` | `PROF_REMOTE_AGENT_WITHDRAWN` | `{"error":"Your company no longer offers this agent.","code":"PROF_REMOTE_AGENT_WITHDRAWN","action":null}` † |
| `verified` asked for a `listed` remote agent | `Err(permanent)` | `PROF_REMOTE_AGENT_NOT_ENABLED` | log only (a caller ordering defect) |
| A remote agent row breaks a 2C rule | `Err(permanent)` | `RAG_NAME_INVALID`, `RAG_HTTP_FIELDS_INVALID`, `RAG_SLACK_FIELDS_INVALID` | cannot occur after step 3.6; log only |

Sentences marked † are *chosen here* and go to the Phase 4 error registry; the unmarked sentence is 11:1042's.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Organisation Profile

  Background:
    Given installer.json names "https://it.example.com/ordin/profile.json" and one signing key K1

  Scenario: First start activates a validly signed profile
    Given no profile is cached
    And the address serves version "1.0.0" signed with K1
    When the service starts
    Then status is "active" with active_version "1.0.0"
    And current() returns a Verified snapshot of version "1.0.0"
    And organisation_profiles holds one row in state "active"
    And "profile/active.json" and "profile/active.jws" exist with mode 0600

  Scenario: An offline start uses the cached profile
    Given version "1.0.0" is cached and active
    And the address cannot be reached
    When the service starts
    Then current() returns a Verified snapshot of version "1.0.0" before any fetch completes
    And after the fetch fails, status last_failure code is "PROF_UNREACHABLE"

  Scenario: A newer profile replaces the active one and subscribers are told
    Given version "1.0.0" is active and a subscriber holds a watch receiver
    When the address serves version "1.1.0" signed with K1 and refresh runs
    Then the outcome is Activated "1.1.0"
    And the "1.0.0" row is in state "replaced"
    And the subscriber receives a snapshot of version "1.1.0"

  Scenario: A profile with a bad signature is ignored
    Given version "1.0.0" is active
    When the address serves version "1.1.0" whose signature does not verify with K1
    Then the outcome is Refused "PROF_SIGNATURE_INVALID"
    And current() still returns version "1.0.0"
    And a row in state "failed_verification" records the code

  Scenario: The same bad profile is not recorded twice
    Given a document with digest D was refused with PROF_SIGNATURE_INVALID
    When refresh fetches the document with digest D again
    Then the outcome is Refused "PROF_SIGNATURE_INVALID"
    And no new row is added

  Scenario: An older version is refused
    Given version "2.0.0" is active
    When the address serves a validly signed version "1.9.0"
    Then the outcome is Refused "PROF_VERSION_NOT_NEWER"

  Scenario Outline: Content outside the bounds is refused
    Given version "1.0.0" is active
    When the address serves a validly signed version "1.0.1" with <defect>
    Then the outcome is Refused "PROF_CONTENT_INVALID"
    And the refusal reason names "<key path>"

    Examples:
      | defect                                   | key path                     |
      | usage.agents_at_once_max = 4             | usage.agents_at_once_max     |
      | limits.crew.rounds = 31                  | limits.crew.rounds           |
      | locales.fallback "fr-FR" not in offered  | locales.fallback             |
      | an unknown top-level key "marketplace"   | marketplace                  |
      | updates.feed_url "http://x.example"      | updates.feed_url             |
      | email.system company_server without imap | email.imap                   |
      | craft.apps[0].platforms.macos_arm64.sha256 "abc" | craft.apps[0].platforms.macos_arm64.sha256 |
      | two language_servers entries for go      | language_servers             |
      | gitlab.host "https://gitlab.example.com" | gitlab.host                  |
      | slack.dedicated_channel "#agents"        | slack.dedicated_channel      |
      | remote_agents[0] slack address "agent-bot" | remote_agents[0].address   |

  Scenario: Craft apps, GitLab and language servers in the closing-round shape are read
    Given a validly signed document with craft.apps [{"app_id":"krita","version":"5.2.6","platforms":{"macos_arm64":{"asset_url":"https://mirror.example/krita-5.2.6.dmg","sha256":"<64 hex>","file_name":"krita-5.2.6.dmg"}}}], gitlab {"host":"gitlab.example.com","client_id":"a1b2c3"} and language_servers [{"language":"go","name":"gopls","version":"0.16.2","platforms":{"linux_x86_64":{"asset_url":"https://mirror.example/gopls.tar.gz","sha256":"<64 hex>","file_name":"gopls.tar.gz","binary_path":"bin/gopls"}}}]
    When it is activated
    Then current().document.craft.apps[0].allowed_tools is None
    And current().document.gitlab.host is "gitlab.example.com"
    And current().document.language_servers[0].language is Go

  Scenario: A newer format is reported as such
    When the address serves a validly signed document with format_version 2
    Then the outcome is Refused "PROF_FORMAT_UNSUPPORTED"

  Scenario: Unchanged profile answered with 304
    Given version "1.0.0" is active with ETag "abc"
    When refresh runs and the address answers 304 to If-None-Match "abc"
    Then the outcome is Unchanged
    And status last_success_at is now

  Scenario: No address on the desktop
    Given installer.json is absent and the build carries no profile address
    When the service starts
    Then status is "no_address"
    And current() returns the defaults with harnesses [claude_code] and locales fallback "en-GB"

  Scenario: Remote agents follow the profile
    Given remote agent "Finance assistant" is "verified" at "https://finance-agent.example.com"
    When a newer profile lists it at "https://finance2.example.com" and drops "Legal assistant"
    Then "Finance assistant" has status "listed" and the new address
    And "Legal assistant" has status "withdrawn"

  Scenario: Remote agent status moves forward only through enabled
    Given remote agent R is "listed"
    When mark_remote_agent_verified(R) is called
    Then it fails with PROF_REMOTE_AGENT_NOT_ENABLED
    When mark_remote_agent_enabled(R) and then mark_remote_agent_verified(R) are called
    Then R has status "verified"

  Scenario: The personal server's setup command sets the address
    Given the service runs on the personal server with state "no_address"
    When set_profile_address("https://it.example.com/ordin/profile.json", [K1]) is called
    Then installer.json holds that address with mode 0600
    And a refresh is attempted

  Scenario: The setup command refuses an http address
    When set_profile_address("http://it.example.com/profile.json", [K1]) is called
    Then it fails with PROF_ADDRESS_INVALID

  Scenario: The agent profile does not read a profile
    Given the service is started in NotUsed mode
    Then status is "not_used"
    And refresh fails with PROF_NOT_USED
    And no HTTP request is made

  Scenario: Retry with backoff while nothing is active
    Given no profile is active and the address cannot be reached
    When the service runs for 2 minutes
    Then attempts were made at about 0 s, 5 s, 15 s, 35 s and 75 s

  Scenario: The default harness is the first harness entry
    Given the active profile lists harnesses [copilot, claude_code]
    When default_harness is called on current()
    Then it returns copilot

  Scenario: A Telegram chat id that is not an integer is refused
    Given a fetched profile with telegram.allowed_chats ["team-chat"]
    When the profile is verified
    Then the refresh outcome is Refused with code PROF_CONTENT_INVALID
    And the last verified profile stays in force

  Scenario: The server setup command needs a signing key with the address
    When set_profile_address is called with "https://it.example.com/ordin/profile.json" and no signing key
    Then it fails with PROF_INSTALLER_INVALID
    And installer.json is unchanged
```

## Performance / Security / Observability

- **Performance targets:**
  - `current()` and `status()`: no I/O, p95 ≤ 10 µs (an `Arc` clone); consumers may call them on every request.
  - Start with a cached profile: ≤ 50 ms to the first published snapshot (signature check ≈ 0.1 ms; JSON ≤ 1 MiB parse ≤ 20 ms), inside A-03's 3 s start.
  - Refresh: verify and activate ≤ 100 ms after the bytes arrive; network time bounded by the 30 s total timeout.
  - Memory: one active document (≤ 1 MiB) plus at most one being verified.
- **Security:**
  - The document is parsed only after the Ed25519 signature verifies (step 3.4); the signing keys come only from the installer file or the branded build, never from the profile itself.
  - Strict parsing: unknown keys refused, every bound checked; a document that fails anything is never served (02:707).
  - Downgrade protection: a lower or equal `profile_version` is refused, so an attacker who can serve an old signed profile cannot roll settings back.
  - HTTPS only, including redirects; system trust store and proxy (A-39); 1 MiB and 8 KiB caps.
  - The profile holds no secret (07 — *Configuration*); `email.ca_certificate` and `nats.tls.ca_certificate` are public certificates.
  - Files: `installer.json`, `profile/` folder `0700`, files `0600` on macOS and Linux.
  - Service API inputs are empty objects; no person-supplied value reaches this component except the server owner's address and keys through the setup command, validated in step 8.
  - Never logged: the document body, the signature text, key material beyond its `kid`.
- **Observability** (`tracing`, JSON lines; `component = "organisation_profile"`):
  - `profile.installer.invalid` {code}; `profile.cache.invalid` {code}; `profile.cache.write_failed` {}
  - `profile.fetch.done` {http_status, bytes, duration_ms}; `profile.fetch.failed` {code, http_status?}
  - `profile.refused` {code, key_path?, profile_version?}; `profile.activated` {profile_version, previous_version?}
  - `profile.remote_agents.synced` {listed_new, changed, withdrawn}; `profile.remote_agent.status` {remote_agent_id, status}
  - `profile.address.set` {host} (the host only, not the full URL)
  - No metrics or spans: the component does not run in the agent profile (SA-XC-04, SA-XC-05).

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

The sections above carry the change in place:

- **`SlackSection`:** `workspace` and `dedicated_channel` are optional (both present or both absent; absent → the Slack Thread Adapter offers no Slack route); the Slack Service's keys `tools_enabled` (default `true`; turns off the Slack tools, linked channels, drafts and Ordin's reactions, never the person's own handover notices), `allowed_channels` (`None` = any channel the person belongs to) and `rules_allowed` (default `true`). `document.slack` stays `Option<SlackSection>`; when it is absent, the Slack Service reads those defaults (self-service, ADR-102): IT does nothing for Slack to be offered.
- **Validation (step 3.6):** the `slack` row covers the optional pair, `client_id` and `allowed_channels` (at most 200 entries, each a channel id, distinct); a breach is `PROF_CONTENT_INVALID` with the dotted key path (for example `PROF_CONTENT_INVALID slack.allowed_channels[3]`).
- **Consumers:** the Slack Service (the program builds `SlackRules` from `document.slack` and, on the personal server, `document.server.external_actions_off`) and Identity and Credentials through the program (`document.slack.client_id`).

### Acceptance Criteria (Gherkin) — added scenarios


```gherkin
  Scenario: a profile with no slack section
    Given a verified profile document with no "slack" key
    Then document.slack is None
    And the Slack Service's rules read tools_enabled true, allowed_channels none and rules_allowed true

  Scenario: IT limits Slack without the Slack route
    Given a profile whose slack section is {"tools_enabled": true, "allowed_channels": ["C0123ABCD45"], "rules_allowed": false}
    When it is verified
    Then it is activated with workspace and dedicated_channel absent

  Scenario: an allowed channel that is not a channel id
    Given a profile whose slack.allowed_channels is ["#design-reviews"]
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming slack.allowed_channels[0]

  Scenario: the Slack route half configured
    Given a profile whose slack section has workspace "acme" and no dedicated_channel
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming slack.dedicated_channel
```

### Gaps

None.

---

## Amendment January 2026 — Reading other teams' knowledge (ADR-110)

> Source: decision 90 (the user's decisions of January 2026); HLD revision 0.59.0, ADR-110 (amends ADR-103: one own folder, any number of read folders; adds Adopt and folders the company names).
> Every rule below adds to the sections above. A profile without `team_knowledge`, or with an empty `read_folders`, verifies, activates and is read exactly as before, and no workspace gains a read folder.

### Purpose (addition)

The profile may name **read folders** in a new section, `team_knowledge.read_folders[]`: team knowledge folders that every applicable workspace reads and the person cannot stop reading unless the entry allows it (ADR-110). This module only carries and validates the list; the Team Knowledge Service applies it. Nothing in the entry grants access: each folder is read with the person's own sign-in.

### Approach (addition)

The section follows the pattern of `agent_catalogue.repositories[]` (ADR-086): a stable key per entry, a display name shown as *Set by your company*, the host and repository, and the consumer reconciling its own state on each verified profile. The section is optional with an empty default, so the no-company-set-up rule holds (decision 83 D1). Rejected: *carrying a token for a named folder* — repository access stays the only gate (ADR-103 item 8); *naming a workspace's own folder* — sharing stays each person's choice behind a card (ADR-029).

Assumptions (*chosen here*):

- **SA-PROFR-01** At most 20 entries, the same bound as a workspace's read folders, so a profile cannot by itself exceed the per-workspace limit.
- **SA-PROFR-02** `host` must be `github.com` or the profile's own `gitlab.host` (default `gitlab.com`); any other host is refused at verification rather than at the first check, so IT sees the mistake in the profile's status.
- **SA-PROFR-03** `person_may_stop` (default `false`) is a choice made with decision 90: a company may name a folder as recommended rather than required.
- **SA-PROFR-04** An installation from before this amendment refuses a document carrying `team_knowledge` (unknown keys are refused, step 3.5) and keeps its last verified profile until it is updated (R-110). This is the existing rule for every new key; the documentation for administrators says so.

### Shared Context (additions)

```rust
// ---- team_knowledge (ADR-110; 2D-2 amended January 2026, decision 90)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeSection {
    #[serde(default)] pub read_folders: Vec<CompanyReadFolder>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyReadFolder {
    /// Stable key; the Team Knowledge Service records it as the binding's company origin.
    pub key: String,
    /// Shown as the folder's name, with *Set by your company*.
    pub display_name: String,
    pub host: String,
    /// "owner/name"
    pub repository: String,
    pub branch: String,
    /// Relative to the repository root; "" = the root.
    #[serde(default)] pub folder: String,
    #[serde(default)] pub applies_to: AppliesTo,
    /// Read only while the branch requires review before a change is merged.
    #[serde(default)] pub review_required: bool,
    /// Whether the person may stop reading it.
    #[serde(default)] pub person_may_stop: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppliesTo {
    /// Every workspace in Team Knowledge mode.
    #[default] TeamKnowledge,
    /// Also every workspace with an active `GitRepositoryFolder` link (a code repository, WorkspaceService.md:118, :1047).
    Code,
}
```

### Public Interface (additions)

`OrganisationProfileDocument` gains one field, after `sandbox`:

```rust
    #[serde(default)] pub team_knowledge: TeamKnowledgeSection,
```

*How consumers read the profile* gains the row:

| Consumer | Reads | Notes |
|---|---|---|
| Team Knowledge Service (through the program) | `document.team_knowledge.read_folders` (`CompanyReadFolder`, `AppliesTo`) and `document.gitlab` (host check) through `current()` at start and `subscribe()` after | the program passes the list to the Team Knowledge Service on each verified profile; the service reconciles bindings (TeamKnowledgeService.md amendment January 2026) |

Example excerpt of a document carrying the section:

```json
{
  "format_version": 1,
  "profile_version": "2026.10.4",
  "team_knowledge": {
    "read_folders": [
      {"key": "security-standards", "display_name": "Security standards", "host": "github.com",
       "repository": "example-co/standards", "branch": "main", "folder": "knowledge/security",
       "applies_to": "code", "review_required": true},
      {"key": "architecture", "display_name": "Architecture notes", "host": "github.com",
       "repository": "example-co/architecture", "branch": "main", "folder": "knowledge",
       "person_may_stop": true}
    ]
  }
}
```

### Internal Logic (additions)

Validation table (step 3.6) gains, after the `agent_catalogue.repositories` row:

| Key path | Rule | Source |
|---|---|---|
| `team_knowledge.read_folders` | ≤ 20 entries (SA-PROFR-01); `key` `^[a-z0-9-]{1,64}$` unique; `display_name` 1–120 chars, no control characters, unique; `host` `github.com` or the document's `gitlab.host` (`gitlab.com` when `gitlab` is absent) (SA-PROFR-02); `repository` `owner/name`, each part `^[A-Za-z0-9_.-]{1,100}$`; `branch` 1–255 bytes, a valid git ref name; `folder` ≤ 1,024 bytes, relative, no segment `..` or `.`, no leading or trailing `/`; the tuple (`host`, `repository`, `branch`, `folder`) unique | 2D-2; ADR-110 |

A breach is `PROF_CONTENT_INVALID` with the dotted key path, for example `PROF_CONTENT_INVALID team_knowledge.read_folders[2].host`. An unknown `applies_to` value is a parse failure at step 3.5 with the same code. Steps 4 to 9 are unchanged: the Team Knowledge Service sees the new list only through the snapshot after activation, so a refused profile adds and removes no folder (ADR-013).

### Data Model

N/A — the section travels inside the document already cached as `active.json` and stored in `organisation_profiles`; no table or column changes.

### Error Table (rows added)

N/A — no new code; every breach above is the existing `PROF_CONTENT_INVALID` row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: no team_knowledge section
    Given a verified profile document with no "team_knowledge" key
    Then document.team_knowledge.read_folders is empty

  Scenario: a named folder with its defaults
    Given a profile whose team_knowledge.read_folders holds one entry with key "security-standards", host "github.com", repository "example-co/standards", branch "main" and no other optional key
    When it is verified
    Then it is activated and the entry reads folder "", applies_to team_knowledge, review_required false and person_may_stop false

  Scenario: a host that is neither GitHub nor the company's GitLab
    Given a profile whose gitlab.host is "gitlab.example.com" and whose team_knowledge.read_folders[0].host is "git.other.example"
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming team_knowledge.read_folders[0].host
    And the last verified profile stays in force

  Scenario: two entries with one key
    Given a profile whose team_knowledge.read_folders holds two entries with key "architecture"
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming team_knowledge.read_folders[1].key

  Scenario: a folder path that leaves its folder
    Given a profile whose team_knowledge.read_folders[0].folder is "knowledge/../secrets"
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming team_knowledge.read_folders[0].folder

  Scenario: too many named folders
    Given a profile whose team_knowledge.read_folders holds 21 entries
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming team_knowledge.read_folders

  Scenario: an unsigned profile adds no folder
    Given an active profile without team_knowledge
    And a fetched profile naming read folder "security-standards" whose signature fails
    When refresh runs
    Then the result is refused with PROF_SIGNATURE_INVALID
    And the snapshot sent to subscribers is unchanged, so no workspace gains a read folder

  Scenario: a newer profile drops a folder
    Given an active profile naming read folders "security-standards" and "architecture"
    When a verified profile with a higher profile_version names only "architecture"
    Then subscribers receive a snapshot whose team_knowledge.read_folders holds only "architecture"
```

### Performance, Security, Observability (additions)

- **Performance:** none; at most 20 entries checked once per verified profile.
- **Security:** the entry carries no credential and grants nothing; the document is parsed only after its signature verifies (step 3.4), so an unsigned list never reaches the Team Knowledge Service. `display_name` is shown as data.
- **Observability:** none added; `profile.activated` already records the version. Repository names in the list are not logged.

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents (ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-113 (a person adds a front door to a workspace themselves; an organisation profile may also name a default front door, which the person can still remove). Consumer: `components/KnowledgeQueryClient.md` (through the program, as the Team Knowledge Service reads `team_knowledge.read_folders[]`).
> Every rule below adds to the sections above. A profile without `knowledge_agents`, or with an empty `front_doors`, verifies, activates and is read exactly as before, and no workspace gains a front door.

### Purpose (addition)

The profile may name **front doors** in a new section, `knowledge_agents.front_doors[]`: knowledge agents that every applicable workspace connects to by default. This module only carries and validates the list; the Knowledge Query Client applies it and lets the person remove any entry from a workspace. Nothing in the entry grants access: each front door admits the person only with their own company sign-in and only when they are in its audience.

### Approach (addition)

The section follows the pattern of `team_knowledge.read_folders[]` (ADR-110): a stable key per entry, a display name shown as *Set by your company*, the address, and `applies_to` with the same two values; the consumer reconciles its own state on each verified profile. Unlike a company read folder, a front door can always be removed by the person (ADR-113), so there is no `person_may_stop`. The section is optional with an empty default, so the no-company-set-up rule holds. Rejected: *carrying a credential or a service token for the front door* — sign-in stays the person's (ADR-097 item 9); *a front door required on every workspace* — a workspace with no connection must work as before.

Assumptions (*chosen here*):

- **SA-PROF-KA-01** At most 8 entries, the same bound as a workspace's connections, so a profile cannot by itself exceed the per-workspace limit.
- **SA-PROF-KA-02** `url` is the front door's base address: `https` only, no user information, query or fragment, at most 2,048 bytes, no trailing `/`. The Knowledge Query Client appends `/v1/knowledge/…`. A plain-HTTP address is refused at verification, so IT sees the mistake in the profile's status.
- **SA-PROF-KA-03** `applies_to` reuses `AppliesTo` unchanged: `team_knowledge` (every workspace in Team Knowledge mode) is the default, `code` also covers every workspace that links a code repository. A wider "every workspace" value is not added in this change; a person adds a front door to any other workspace themselves.
- **SA-PROF-KA-04** An installation from before this amendment refuses a document carrying `knowledge_agents` (unknown keys are refused, step 3.5) and keeps its last verified profile until it is updated, as SA-PROFR-04 states for `team_knowledge`.

### Shared Context (additions)

```rust
// ---- knowledge_agents (ADR-113; 2D-2 amended January 2026, decision 92)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAgentsSection {
    #[serde(default)] pub front_doors: Vec<CompanyFrontDoor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyFrontDoor {
    /// Stable key; the Knowledge Query Client records it as the connection's company origin.
    pub key: String,
    /// Shown as the front door's name, with *Set by your company*.
    pub display_name: String,
    /// Base address, https only (SA-PROF-KA-02).
    pub url: String,
    #[serde(default)] pub applies_to: AppliesTo,   // the ADR-110 type, unchanged
}
```

### Public Interface (additions)

`OrganisationProfileDocument` gains one field, after `team_knowledge`:

```rust
    #[serde(default)] pub knowledge_agents: KnowledgeAgentsSection,
```

*How consumers read the profile* gains the row:

| Consumer | Reads | Notes |
|---|---|---|
| Knowledge Query Client (through the program) | `document.knowledge_agents.front_doors` (`CompanyFrontDoor`, `AppliesTo`) through `current()` at start and `subscribe()` after | the program passes the list to the Knowledge Query Client on each verified profile; the client reconciles each applicable workspace's company connections and keeps the person's removals (KnowledgeQueryClient.md) |

Example excerpt of a document carrying the section:

```json
{
  "format_version": 1,
  "profile_version": "2026.10.5",
  "knowledge_agents": {
    "front_doors": [
      {"key": "procurement-knowledge", "display_name": "Procurement knowledge",
       "url": "https://knowledge-procurement.example.org", "applies_to": "team_knowledge"}
    ]
  }
}
```

### Internal Logic (additions)

Validation table (step 3.6) gains, after the `team_knowledge.read_folders` row:

| Key path | Rule | Source |
|---|---|---|
| `knowledge_agents.front_doors` | ≤ 8 entries (SA-PROF-KA-01); `key` `^[a-z0-9-]{1,64}$` unique; `display_name` 1–120 chars, no control characters, unique; `url` per SA-PROF-KA-02 and unique after lower-casing the scheme and host | 2D-2; ADR-113 |

A breach is `PROF_CONTENT_INVALID` with the dotted key path, for example `PROF_CONTENT_INVALID knowledge_agents.front_doors[0].url`. An unknown `applies_to` value is a parse failure at step 3.5 with the same code. Steps 4 to 9 are unchanged: the Knowledge Query Client sees the list only through the snapshot after activation, so a refused profile adds and removes no front door (ADR-013).

### Data Model

N/A — the section travels inside the document already cached as `active.json` and stored in `organisation_profiles`; no table or column changes.

### Error Table (rows added)

N/A — no new code; every breach above is the existing `PROF_CONTENT_INVALID` row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: no knowledge_agents section
    Given a verified profile document with no "knowledge_agents" key
    Then document.knowledge_agents.front_doors is empty

  Scenario: a named front door with its defaults
    Given a profile whose knowledge_agents.front_doors holds one entry with key "procurement-knowledge", display_name "Procurement knowledge" and url "https://knowledge-procurement.example.org"
    When it is verified
    Then it is activated and the entry reads applies_to team_knowledge

  Scenario: a front door over plain HTTP
    Given a profile whose knowledge_agents.front_doors[0].url is "http://knowledge.example.org"
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming knowledge_agents.front_doors[0].url
    And the last verified profile stays in force

  Scenario: an address carrying a query
    Given a profile whose knowledge_agents.front_doors[0].url is "https://knowledge.example.org?token=x"
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming knowledge_agents.front_doors[0].url

  Scenario: too many front doors
    Given a profile whose knowledge_agents.front_doors holds 9 entries
    When it is verified
    Then it is refused with PROF_CONTENT_INVALID naming knowledge_agents.front_doors

  Scenario: a newer profile drops a front door
    Given an active profile naming front doors "procurement-knowledge" and "finance-knowledge"
    When a verified profile with a higher profile_version names only "finance-knowledge"
    Then subscribers receive a snapshot whose knowledge_agents.front_doors holds only "finance-knowledge"
```

### Performance, Security, Observability (additions)

- **Performance:** none; at most 8 entries checked once per verified profile.
- **Security:** the entry carries no credential and grants nothing; the document is parsed only after its signature verifies (step 3.4), so an unsigned list never reaches the Knowledge Query Client; an `https`-only address keeps the person's token off plain HTTP. `display_name` is shown as data.
- **Observability:** none added; addresses in the list are not logged.

### Gaps

None.
