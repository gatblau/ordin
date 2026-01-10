# SPEC: Setup Assistant

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-setup` · **Build phase:** 8 · **Programs:** D P
> **Dependencies:** Organisation Profile, Identity, Package Catalogue, Catalogue Service, Craft Toolkit, Journal

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

The Setup Assistant makes the computer ready without technical steps from the person (02:710–717). On the first start, and on a start after the organisation profile changed what setup requires, it runs the short guided sequence "Getting you ready": read the company's settings, check the harness the profile names (Claude Code first, Copilot as the alternative) and start the harness's own sign-in only when it is signed out, have the Catalogue Service clone the profile's agent repositories, and check that every program ordin brings starts (03:12–57; ADR-044, ADR-072). On every start, in the background and on demand it repeats those checks and repairs what it can without asking: a stopped CLI server, an expiring token, a damaged clone, a damaged agent folder. When a repair fails it gives one plain sentence and one action, and its **Send to IT** builds a redacted help request to the destination the profile names (08:14; OQ-53 decision (b)). It owns setup state, the results of checks and the harness attachments (02:716 amended), and the effective update feed address (2D-2, 2D-2b).

## Approach

A table-driven check runner: each check is a value of `CheckKind` with a fixed order, a probe, an optional repair tried once, and the words and action shown on failure; the guided sequence is the same runner with a progress counter over the steps that apply. Harness checks go through a `HarnessProbe` trait owned here (ADR-100 hub pattern): this crate ships the Claude Code probe (commands named in ADR-044), and the desktop program implements the Copilot probe as a thin wrapper over the Copilot Local Adapter and registers it, because the Setup Assistant has no edge to the adapters and the adapter (build phase 5) cannot name this crate's trait. Rejected: calling the Copilot SDK from here (adds an edge 2B-3 does not list); a free-form script of steps (every failure must map to one fixed sentence and action, 11 §6).

## Shared Context

### Types copied from Phase 2C (domain part, `ordin-types`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, `Z`

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HarnessAttachmentId(pub uuid::Uuid); // UUIDv7

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String); // 64 lowercase hex

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HarnessAttachment {
    pub id: HarnessAttachmentId,
    pub harness: HarnessKind,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub tested: bool,
    pub signed_in: bool,
    /// As the harness reports it; free text, never a credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in_method: Option<String>,
    pub state: HarnessAttachmentState,
    pub last_checked_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessAttachmentState { Found, Checked, Ready, NeedsSignIn, NeedsUpdate }
```

| Field (2C) | Rule | Error code |
|---|---|---|
| harness | unique per installation | `HARN_DUPLICATE` |
| location | absolute path ≤ 4096 bytes | `HARN_PATH_INVALID` |
| version | ≤ 64 chars | `HARN_VERSION_INVALID` |
| sign_in_method | ≤ 120 chars; never a token | `HARN_SIGN_IN_METHOD_INVALID` |
| state | `ready` requires `tested` and `signed_in` | `HARN_STATE_INCONSISTENT` |

Transitions (2C): `— → found` (harness found) → `checked` (version and sign-in read) → `ready` | `needs_sign_in` | `needs_update`; each of those → `checked` at the next check.

### Organisation Profile API used (crate `ordin-organisation-profile`)

```rust
impl OrganisationProfileService {
    pub fn current(&self) -> std::sync::Arc<ProfileSnapshot>;
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>>;
    pub async fn refresh(&self) -> Result<RefreshOutcome, ProfileError>;
    pub fn status(&self) -> ProfileStatus;
    pub fn installer_settings(&self) -> InstallerSettings;
}
pub struct ProfileSnapshot { pub document: OrganisationProfileDocument, pub source: SnapshotSource }
pub enum SnapshotSource { Verified { profile_id: OrganisationProfileId, profile_version: String, activated_at: Timestamp }, Defaults }
pub struct ProfileStatus { pub state: ProfileState, pub active_version: Option<String>, pub last_attempt_at: Option<Timestamp>, pub last_success_at: Option<Timestamp>, pub last_failure: Option<ProfileFailure> }
pub enum ProfileState { NotUsed, NoAddress, NeverFetched, Active, Stale }
pub struct InstallerSettings { pub profile_url: Option<String>, pub profile_signing_keys: Vec<String>, pub update_feed_url: Option<String> }

// Fields of OrganisationProfileDocument read here:
pub struct HelpSection { pub destination: Option<HelpDestination> }        // document.help; None = "Send to IT" goes through the person's own mail app (08:14)
pub struct HelpDestination { pub kind: HelpDestinationKind, pub address: String }
pub enum HelpDestinationKind { TicketUrl, Email, SlackChannel }            // "ticket_url", "email", "slack_channel"
pub struct HarnessEntry { pub name: HarnessName, pub tested_versions: Vec<String> } // document.harnesses; default [{claude_code, []}]
pub enum HarnessName { ClaudeCode, Copilot }                               // "claude_code", "copilot"
pub struct UpdatesSection { pub feed_url: String }                         // document.updates: Option<…>
pub struct AgentCatalogueSection { pub repositories: Vec<AgentRepositoryEntry>, pub person_may_add: bool /* … */ } // document.agent_catalogue
pub struct CraftSection { pub apps: Vec<CraftApp>, pub mirror_url: Option<String>, pub file_types: std::collections::BTreeMap<String, String> }
```
`HarnessName::ClaudeCode` ↔ `HarnessKind::ClaudeCode`; `HarnessName::Copilot` ↔ `HarnessKind::CopilotCli`.

### Identity and Credentials API used (crate `ordin-identity`)

```rust
impl IdentityService {
    pub async fn refresh_due(&self) -> Result<RefreshReport, CredError>;
    pub async fn sign_in_state(&self, kind: CredentialKind) -> Result<SignInState, CredError>;
    pub async fn access_token(&self, kind: CredentialKind, account_label: Option<&str>) -> Result<AccessToken, CredError>;
}
pub struct RefreshReport { pub refreshed: u32, pub needs_sign_in: Vec<CredentialId>, pub unreachable: Vec<CredentialId> }
pub enum SignInState { SignedOut, SignedIn, NeedsSignIn }
```

### Other components' APIs used (as their specs define them)

```rust
// ordin-catalogue (Catalogue Service)
impl CatalogueService {
    /// Clone missing, fetch present, re-clone damaged repositories the profile names (ADR-086).
    pub async fn sync_profile_repositories(&self) -> Result<RepositorySyncReport, CatalogueError>;
}
pub struct RepositorySyncReport { pub results: Vec<RepositorySyncResult> }
pub struct RepositorySyncResult { pub display_name: String, pub outcome: RepositorySyncOutcome }
pub enum RepositorySyncOutcome { Cloned, Fetched, Repaired, Failed { code: String } }

// ordin-package-catalogue (Package Catalogue)
impl PackageCatalogue {
    /// Verify each installed agent folder against its lock file; with `repair`, re-install a damaged one (SA-INST-01).
    pub async fn verify_agent_folders(&self, repair: bool) -> Result<AgentFolderReport, PackageError>;
}
pub struct AgentFolderReport { pub checked: u32, pub repaired: u32, pub failed: Vec<AgentFolderFailure> }
pub struct AgentFolderFailure { pub agent_name: String, pub code: String }

// ordin-craft-toolkit (Craft Toolkit)
impl CraftToolkit {
    /// Each pinned app's state; with `repair`, runs `sync_installations` first (Craft Toolkit SA-RECON-02).
    pub async fn check_apps(&self, repair: bool) -> Result<Vec<CraftProgramCheck>, CraftError>;
}
pub struct CraftProgramCheck { pub app_name: String, pub version: String, pub state: ProgramCheckState }
pub enum ProgramCheckState { Ready, Repaired, Missing, Refused { publisher: String, sha256: Sha256Hex } }
```

### Fixed constants (chosen here unless traced)

| Constant | Value |
|---|---|
| Background check interval (desktop) | 60 minutes *(assumed here, SA-SETUP-01)* |
| Harness sign-in check on the server | every 6 hours and before each harness session (2D-4, 02:626) |
| Harness command timeout (`--version`, `auth status`) | 10 seconds |
| Harness sign-in wait (`claude auth login`, `copilot login`) | 10 minutes, status re-checked every 3 seconds |
| Program start check (`--version` or equivalent) | 10 seconds; a kill by `SIGKILL` within 2 seconds of start with no output counts as refused |
| Help request preview lifetime | 30 minutes |
| Help request text limits | what the person was doing ≤ 500 chars; failure sentence ≤ 300; ≤ 50 detail lines of ≤ 2,000 chars; whole body ≤ 64 KiB |
| Help request HTTP timeout | 15 seconds, one retry after 2 seconds on timeout or 5xx |
| Copilot on the server | not offered until C-48 is confirmed (`COPILOT_ON_SERVER = false`) |

### Words shown to the person (owned here; quoted from 11 unless marked †)

| Situation | Sentence | Action |
|---|---|---|
| Guided sequence header | "Getting you ready… {done} of {total} done" | — |
| Already signed in | "Already signed in through Claude Code" | — |
| Signed out | (button) "Sign in to Claude" / "Sign in with GitHub to use Copilot" | Sign in |
| Company settings unreachable | "ordin can't reach your company's settings." † | (retries in the background) |
| No Claude access | "You don't have Claude access yet." | Ask IT for Claude access |
| No Copilot seat | "You don't have Copilot access yet." | Ask IT for Copilot access |
| Harness only IT can install / missing | "Claude Code needs to be installed by IT on this computer" | Send to IT |
| Harness version not tested † | "Claude Code on this computer is a version your company hasn't tested yet." | Send to IT |
| Program stopped by company settings | "Your company's settings stopped {program} from opening on this computer." † followed by what is off ("You can still use everything else; images open in your usual app until IT allows it.") | Send to IT |
| Agent library unreachable | "Can't reach the company's agent library right now" | Try again |
| Server harness sign-in ended | "Your Claude Code sign-in on the server has ended, so new work waits. Ask whoever runs your ordin server to sign in again." | — |
| Help sent | "Sent to IT. IT will reply to you directly." | — |
| Help opened in the mail app † | "Your mail app has the message to IT ready to send." | — |
| Terms under the steps | "To do the work, Ordin sends what it reads to [Claude or Copilot]. Your documents stay where they live." | What Ordin reads and sends — read the terms |

### Confirmation items carried as conditional

| Item | Assertion here | Fallback the HLD gives |
|---|---|---|
| C-25 | `copilot login` started with no terminal completes through the browser (Claude half passed). | The person signs in from the Copilot CLI itself; the step shows "Sign in with GitHub to use Copilot" and re-checks status until signed in (ADR-044). |
| C-46 | Per-user install and update of ordin, and a vendor harness installer for the current user, with no administrator prompt. | A harness only installable machine-wide is left to IT: "Claude Code needs to be installed by IT on this computer" with Send to IT. Harness installation itself is the later step (ADR-044) and is not built in this release. |
| C-47 | Pinned ArtCraft apps are signed and start under the data root without a system prompt. | A refused app is named with Send to IT (program name, publisher, hash); only its features are off (UX-A46). |
| C-58 | Telegram's per-user install is found without reading its data. | Telegram tools are not offered on the desktop; no step depends on it here. Detection is by the `tg:` link handler (C-59) and belongs to the Telegram Service; this component reports nothing for Telegram. |
| C-48 | Copilot device sign-in on a Linux server. | The server offers Claude only. |

### Reconciliation assumptions

- **SA-RECON-01** This crate owns the update check (ruling R16): when to check, the offer and its words; the desktop program supplies Tauri's updater through `AppUpdater`. A verified profile's `updates.feed_url` replaces the installer's (D10).
- **SA-RECON-02** The setup tables live in the Journal's datastore through a Journal edge (D4, R20); the crate exports `migrations()` and receives the `Datastore`, not a pool.
- **SA-RECON-03** The Service API catalogue takes this crate's `setup.*` commands and the `setup_progress` payload as written (R1, R6); a harness sign-in is `setup.sign_in_harness`, never an Identity command (rows 689–694).
- **SA-RECON-04** The program wraps the Copilot Local Adapter's `CopilotServer` in a `HarnessProbe` and registers it; the adapter itself is unchanged (row 700, D3).
- **SA-RECON-05** The Project Service's `ServerInstaller` is implemented by the program over `ensure_language_server` (row 701).
- **SA-CLOSE-01** *(K12, chosen here; closes SA-G-LSP)* A language server's install recipe is data, not code: the organisation profile's `language_servers[]` entry for the language (`{ language, name, version, platforms: { <platform>: { asset_url, sha256, file_name, binary_path } } }`, OrganisationProfile.md SA-CLOSE-03), or, without one, the entry of the same shape in the table compiled into this crate (`language-servers.json`, embedded with `include_str!`, pinned at release: gopls and rust-analyzer per ADR-056, `jdtls` 1.63.0 and TypeScript's servers per 05). Install = download `asset_url`, verify SHA-256, unpack by `file_name` with the archive libraries 05 already names (`zip`; `tar` with `flate2`), into `<data root>/language-servers/<name>/<version>/`; the server runs as `<that folder>/<binary_path>`. The table pins archives that run without a separately installed runtime (for `jdtls` and the TypeScript servers, archives that carry their own Java or Node.js runtime, assembled by the release build).
- **SA-RECON-06** With no help destination in the profile, **Send to IT** goes through the person's own mail app (08:14); it is never switched off (row 529).
- **SA-RECON-07** The Claude and Copilot local adapters name their own error `SessionSetupError`, so `ordin_setup::SetupError` keeps its name (row 702).

## Public Interface

Crate `ordin_setup`.

```rust
/// Registered by the program with the Journal's `MigrationRegistry` (D4, R20).
pub fn migrations() -> ordin_journal::ModuleMigrations; // { module: "setup", sqlite: &[(1, "create setup tables", MIGRATION_1_SQL)], postgres: &[] }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProgram { Desktop, PersonalServer }

/// A program ordin brings (bundled or placed under the data root), listed by the program at build time.
#[derive(Debug, Clone)]
pub struct BundledProgram {
    pub name: String,                 // plain name shown to the person, e.g. "the ordin hook", "GitHub CLI"
    pub publisher: String,            // as signed, e.g. "gatblau"
    pub path: std::path::PathBuf,     // absolute
    pub version_args: Vec<String>,    // e.g. ["--version"]
    pub expected_sha256: Option<Sha256Hex>,
    /// A pristine copy inside the application's own files; a damaged copy at `path` is restored from it once.
    pub restore_from: Option<std::path::PathBuf>,
    /// Plain words for what is off when it cannot start, e.g. "agent sessions" (shown after the refusal sentence).
    pub needed_for: String,
}

pub struct SetupConfig {
    pub program: SetupProgram,
    pub bundled_programs: Vec<BundledProgram>,
    pub application_version: String,
    pub http: reqwest::Client,
    /// Opens a URL with the operating system. Implemented by the desktop program (`apps/desktop`) over
    /// `tauri-plugin-opener` (OrdinDesktop.md, after row 3.43); None on the server, which has no system
    /// browser and does not wire it (OrdinServer.md, not-wired list).
    pub url_opener: Option<std::sync::Arc<dyn UrlOpener>>,
}

/// Implemented by the desktop program (OrdinDesktop.md); the server program does not implement it.
pub trait UrlOpener: Send + Sync { fn open(&self, url: &str) -> Result<(), String>; }

pub struct SetupDeps {
    pub profile: std::sync::Arc<ordin_organisation_profile::OrganisationProfileService>,
    pub identity: std::sync::Arc<ordin_identity::IdentityService>,
    pub catalogue: std::sync::Arc<ordin_catalogue::CatalogueService>,
    pub packages: std::sync::Arc<ordin_package_catalogue::PackageCatalogue>,
    pub craft: std::sync::Arc<ordin_craft_toolkit::CraftToolkit>,
}

pub struct SetupAssistant { /* private */ }

impl SetupAssistant {
    /// `store` is the Journal's `Datastore`; this crate uses `store.sqlite()` (the desktop and the
    /// personal server are SQLite only) and refuses a PostgreSQL store with SETUP_STORE_UNSUPPORTED.
    pub fn new(deps: SetupDeps, config: SetupConfig, store: ordin_journal::Datastore) -> Result<Self, SetupError>;
    /// Register a harness probe (the program registers the Copilot probe from the Copilot Local Adapter).
    /// This crate registers `ClaudeCodeProbe` itself in `new`. A second probe for the same harness replaces the first.
    pub fn register_probe(&self, probe: std::sync::Arc<dyn HarnessProbe>);

    /// True on the first start, or when the profile's setup requirements changed since the last completed run.
    pub async fn needs_guided_setup(&self) -> Result<bool, SetupError>;
    /// The guided sequence. Returns when every applicable step has ended; never asks anything but a sign-in.
    pub async fn run_guided(&self) -> Result<SetupReport, SetupError>;
    /// The same checks without the guided screen; repairs once each; never starts a sign-in.
    pub async fn run_checks(&self, trigger: CheckTrigger) -> Result<SetupReport, SetupError>;
    /// Starts the harness's own sign-in (the person pressed "Sign in to Claude" / "Sign in again").
    pub async fn sign_in_harness(&self, harness: HarnessKind) -> Result<HarnessAttachment, SetupError>;
    pub async fn harness_attachments(&self) -> Result<Vec<HarnessAttachment>, SetupError>;
    /// The harness a session should use: first in profile order whose attachment is `ready`.
    pub async fn ready_harness(&self) -> Result<HarnessAttachment, SetupError>;
    pub fn progress(&self) -> tokio::sync::watch::Receiver<SetupProgress>;
    /// Installer's `update_feed_url`, replaced by a verified profile's `updates.feed_url` (2D-2, 2D-2b; D10).
    pub fn effective_update_feed(&self) -> Option<String>;

    // ---- added in reconciliation (rows 695–697, 701; R16: the Setup Assistant owns update checks)
    /// Desktop: registers the program's updater (Tauri's updater plug-in behind `AppUpdater`).
    /// Without one, update checks on the desktop are off.
    pub fn register_updater(&self, updater: std::sync::Arc<dyn AppUpdater>);
    /// Spawns the background timer of step D.4 (checks) and step F.3 (update checks). Called once,
    /// after `run_checks(Start)`; a second call does nothing. Ends at `close`.
    pub async fn start_background_checks(&self);
    /// Stops the timer; waits at most 2 s for a running check.
    pub async fn close(&self);
    /// The update the desktop found and has not installed; `None` when none.
    pub fn update_offer(&self) -> Option<UpdateOffer>;
    /// Desktop: installs the offered update through the `AppUpdater`; restarting is the person's choice.
    pub async fn install_update(&self) -> Result<(), SetupError>;
    /// Server: reads the effective feed once and returns the newer version it names, if any
    /// (step F.4). The server program announces it once (ADR-076).
    pub async fn check_update_feed(&self) -> Result<Option<String>, SetupError>;
    /// Installs a pinned language server (or its toolchain) under the data root on first use and
    /// returns how to start it (step H). The program implements the Project Service's
    /// `ServerInstaller` over this call.
    pub async fn ensure_language_server(&self, language: &str, server_name: &str, version: &str)
        -> Result<LanguageServerLaunch, SetupError>;

    pub async fn prepare_help_request(&self, input: HelpRequestInput) -> Result<HelpRequestPreview, SetupError>;
    pub async fn send_help_request(&self, preview_id: HelpPreviewId, fallback_address: Option<String>)
        -> Result<HelpRequestOutcome, SetupError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckTrigger { Start, Background, OnDemand, BeforeSession }

/// Owned here, implemented by the desktop program over Tauri's updater (ADR-100 pattern, D3).
#[async_trait::async_trait]
pub trait AppUpdater: Send + Sync {
    /// Reads the feed; `Ok(None)` when no newer signed version exists.
    async fn check(&self, feed_url: &url::Url) -> Result<Option<UpdateOffer>, UpdaterError>;
    /// Downloads, verifies with the build's update key (2D-2b) and installs; no restart.
    async fn install(&self, offer: &UpdateOffer) -> Result<(), UpdaterError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateOffer {
    /// Semantic version, 1–64 characters.
    pub version: String,
    /// The feed's release notes, at most 4,000 characters (cut beyond).
    pub notes: String,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdaterError {
    #[error("feed unreachable")] Unreachable,          // → SETUP_UPDATE_UNREACHABLE, retryable
    #[error("signature refused")] SignatureInvalid,     // → SETUP_UPDATE_SIGNATURE_INVALID
    #[error("install failed")] InstallFailed,           // → SETUP_UPDATE_INSTALL_FAILED, retryable
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageServerLaunch {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}
```

### Harness probe (hub trait owned here, *chosen here*)

```rust
#[derive(Debug, Clone)]
pub struct HarnessLocation { pub path: std::path::PathBuf, pub version: String }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessSignIn { SignedIn { method: Option<String> }, SignedOut, NoAccess }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeRepair { NothingToDo, Repaired, Failed }

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("not found")] NotFound,
    #[error("refused by the operating system")] Refused,
    #[error("timed out")] Timeout,
    #[error("unreadable output")] Unreadable,
    #[error("not offered here")] NotOffered,
}

#[async_trait::async_trait]
pub trait HarnessProbe: Send + Sync {
    fn harness(&self) -> HarnessKind;
    async fn locate(&self) -> Result<Option<HarnessLocation>, ProbeError>;
    async fn sign_in_status(&self) -> Result<HarnessSignIn, ProbeError>;
    /// Starts the vendor's own sign-in flow and returns when its process exits.
    async fn start_sign_in(&self) -> Result<(), ProbeError>;
    /// Restart what the harness needs (for Copilot: its CLI server). Claude Code: NothingToDo.
    async fn repair(&self) -> Result<ProbeRepair, ProbeError>;
}

/// Built in: Claude Code, per ADR-044.
pub struct ClaudeCodeProbe { /* private */ }
```

### Reports and progress

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind { CompanySettings, Harness, HarnessSignIn, Tokens, AgentRepositories, AgentFolders, Programs, CraftApps }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome { Passed, Repaired, Failed, Skipped }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonAction { TryAgain, SignInAgain, SendToIt, AskItForClaudeAccess, AskItForCopilotAccess, SignIn }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CheckResult {
    pub check: CheckKind,
    pub outcome: CheckOutcome,
    /// Error code for Diagnostics; None when passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Message identifier (Localisation) of the one sentence shown; None when passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Values for the message's placeholders (program name, harness name, what is off).
    pub message_args: std::collections::BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<PersonAction>,
    /// For a refused program: what Send to IT carries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
    pub checked_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RefusedProgram { pub name: String, pub publisher: String, pub sha256: Sha256Hex }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SetupReport {
    pub trigger: Option<CheckTrigger>,   // None for the guided sequence
    pub results: Vec<CheckResult>,
    pub ready: bool,                     // a harness is ready
    pub started_at: Timestamp,
    pub ended_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SetupProgress {
    pub running: bool,
    pub steps: Vec<ProgressStep>,
    pub done: u32,
    pub total: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProgressStep { pub check: CheckKind, pub state: StepState, pub message_id: String }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState { Waiting, Running, Done, NeedsYou, Failed }
```

### Help requests

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HelpPreviewId(pub uuid::Uuid); // UUIDv7, chosen here

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpRequestInput {
    pub what_doing: String,              // 1–500 chars
    pub failure_sentence: String,        // 1–300 chars, the sentence the person saw
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,       // Diagnostics line id, "r-81" (UX-A70); ^r-[0-9]{1,12}$
    pub details: Vec<String>,            // error codes and diagnostic lines; redacted here
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HelpRequestBody {             // format chosen here; JSON body of a ticket_url POST
    pub format: u32,                     // 1
    pub what_doing: String,
    pub failure_sentence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub details: Vec<String>,            // after redaction
    pub application_version: String,
    pub sidecar_versions: std::collections::BTreeMap<String, String>, // harness and bundled program versions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
    pub sent_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HelpRequestPreview {
    pub preview_id: HelpPreviewId,
    pub destination: Option<HelpDestinationView>, // None → the person enters an email address
    pub body: HelpRequestBody,                    // exactly what will be sent
    pub expires_at: Timestamp,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HelpDestinationView { pub kind: HelpDestinationKind, pub address: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HelpRequestOutcome {
    /// ticket_url or slack_channel: delivered. Words: "Sent to IT. IT will reply to you directly."
    Sent { record: HelpRequestRecord },
    /// email destination, or no destination with an address: the person's mail app opened pre-filled.
    OpenedInMailApp { record: HelpRequestRecord },
    /// Server with an email destination: the text for the chat to show; the person forwards it.
    TextForPerson { text: String, record: HelpRequestRecord },
}

/// The journal copy (04 Data Lifecycle "a copy in the journal"); written by the program through the Journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HelpRequestRecord { pub preview_id: HelpPreviewId, pub destination_kind: Option<HelpDestinationKind>, pub body_sha256: Sha256Hex, pub reference: Option<String>, pub sent_at: Timestamp }
```

### Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{code}")] Retryable { code: &'static str },
    #[error("{code}")] Permanent { code: &'static str },
}
/// 2C `CodedError`: `repair()` is the action of the Error Table row of the code.
impl ordin_types::CodedError for SetupError {
    fn code(&self) -> &'static str;
    fn retryable(&self) -> bool;
    fn repair(&self) -> Option<ordin_types::RepairAction>;
}
```

### Service API commands (implemented by the programs; names *chosen here*)

| Command | Input | Result | Calls |
|---|---|---|---|
| `setup.status` | `{}` | `{"needs_guided": bool, "progress": SetupProgress, "attachments": [HarnessAttachment]}` | `needs_guided_setup`, `progress`, `harness_attachments` |
| `setup.run_guided` | `{}` | `SetupReport` | `run_guided` |
| `setup.check_now` | `{}` | `SetupReport` | `run_checks(OnDemand)` |
| `setup.sign_in_harness` | `{"harness": "claude_code" \| "copilot_cli"}` | `HarnessAttachment` | `sign_in_harness` |
| `setup.help_preview` | `HelpRequestInput` | `HelpRequestPreview` | `prepare_help_request` |
| `setup.help_send` | `{"preview_id": HelpPreviewId, "fallback_address": string?}` | `HelpRequestOutcome` without `record` | `send_help_request`; the program journals `record` |
| `setup.update_offer` | `{}` | `{"offer": UpdateOffer?}` | `update_offer` (desktop) |
| `setup.install_update` | `{}` | `{}` | `install_update` (desktop) |

Event `setup_progress { progress: SetupProgress }` (2C `EventKind::SetupProgress`), emitted by the program from `progress()` through `Journal::append_event`. These commands and this payload are the definitive ones; the Service API catalogue takes them as written (R1, R6): `setup.status` returns the object above (not `GettingReady`); retry is `setup.check_now` (there is no per-step `setup.retry`); help is the two-step `setup.help_preview` → `setup.help_send` with the screens' approval-style card, not `help.send_to_it` or an Approval Service card; a Claude or Copilot sign-in is `setup.sign_in_harness`, never an Identity command. The ServiceApi names its event variant so it does not clash with this crate's `SetupProgress` struct.

### Who calls each public item (reconciliation §3; R6)

| Item | Called by |
|---|---|
| `new`, `register_probe`, `register_updater`, `start_background_checks`, `close` | desktop program (registers the Copilot `HarnessProbe` it wraps around the Copilot Local Adapter's `CopilotServer`, and the Tauri `AppUpdater`); server program (no updater) |
| `needs_guided_setup`, `run_guided`, `run_checks(OnDemand)`, `sign_in_harness`, `harness_attachments`, `progress`, `prepare_help_request`, `send_help_request`, `update_offer`, `install_update` | the programs, for the `setup.*` commands |
| `run_checks(Start)` | the programs at start, before `start_background_checks` |
| `run_checks(BeforeSession)` | server program, before each harness session (2D-4) |
| `ready_harness` | desktop program, in its `SessionSetupSource` implementation (it passes `ready_harness().location` as the Claude Local Adapter's `claude_path`) |
| `effective_update_feed` | this crate (step F); the server program for its announcement through `check_update_feed` |
| `check_update_feed` | server program, once a day *(chosen here)*, announcing a newer version once |
| `ensure_language_server` | Project Service, through the program's `ServerInstaller` (`ensure(language, &ServerPin { name, version })` → `ensure_language_server(language, &name, &version)`) |
| `migrations` | the programs, registering it with the Journal's `MigrationRegistry` |

The bundled programs the desktop program lists in `SetupConfig.bundled_programs` include the ordin hook (`ordin-hook`, `version_args = ["--version"]`) and the Copilot CLI; step D.3 is therefore where the hook's version run happens.

### Example

First start, Claude Code signed in, two repositories:
```json
{"trigger":null,"ready":true,"started_at":"2026-01-10T08:00:00.120Z","ended_at":"2026-01-10T08:00:06.410Z",
 "results":[
  {"check":"company_settings","outcome":"passed","message_args":{},"checked_at":"2026-01-10T08:00:01.002Z"},
  {"check":"harness","outcome":"passed","message_args":{},"checked_at":"2026-01-10T08:00:01.340Z"},
  {"check":"harness_sign_in","outcome":"passed","message_args":{},"checked_at":"2026-01-10T08:00:01.910Z"},
  {"check":"agent_repositories","outcome":"passed","message_args":{},"checked_at":"2026-01-10T08:00:05.700Z"},
  {"check":"programs","outcome":"failed","code":"SETUP_PROGRAM_REFUSED","message_id":"setup-program-refused",
   "message_args":{"program":"PhotoCraft","off":"images open in your usual app"},"action":"send_to_it",
   "refused_program":{"name":"PhotoCraft","publisher":"ArtCraft","sha256":"9b1f…64 hex…"},"checked_at":"2026-01-10T08:00:06.400Z"}]}
```

## Internal Logic

### A. Guided sequence (`run_guided`)

1. Take the run lock (one guided run or check run at a time; a second caller awaits the running one and receives its report). Compute the steps that apply: `CompanySettings`; `Harness`; `HarnessSignIn` (counted only once `Harness` found the harness signed out, so the count reflects real steps, 11 §5.1); `AgentRepositories` when the profile names at least one; `Programs` and `CraftApps` when `bundled_programs` or `craft.apps` is non-empty. Publish `SetupProgress { running: true, done: 0, total }`.
2. **CompanySettings.** Read `profile.status()`. `NoAddress` (desktop installer carried none) → result `Failed`, code `SETUP_NO_PROFILE_ADDRESS`, message "ordin can't reach your company's settings." †, action Send to IT; the sequence continues with defaults (A-40 on the server). `Active` → `Passed`. Otherwise call `profile.refresh()`; success → `Passed`; failure → `Failed` with `SETUP_PROFILE_UNREACHABLE`, no action, a background retry every 60 minutes (the Organisation Profile's own refresh) and the sequence continues with the last verified profile or defaults (03:55).
3. **Harness.** Walk `document.harnesses` in order (default `[claude_code]`), skipping `copilot` on the server (C-48). For the first harness with a registered probe: `locate()` (D.1). Not found → `Failed`, `SETUP_HARNESS_MISSING`, "Claude Code needs to be installed by IT on this computer", Send to IT; try the next listed harness; if none is found, the sequence ends with `ready = false`. Found → write the attachment (`found`, then version → `checked`); `tested` = `tested_versions` empty or containing the version exactly *(assumed here, SA-SETUP-02)*. Untested → state `needs_update`, `Failed`, `SETUP_HARNESS_UNTESTED`, Send to IT, then the next listed harness is tried.
4. **HarnessSignIn.** `sign_in_status()`: `SignedIn` → state `ready`, words "Already signed in through Claude Code", `Passed`, nothing asked. `NoAccess` → `Failed`, `SETUP_NO_HARNESS_ACCESS`, action Ask IT for Claude access (or Copilot access). `SignedOut` → state `needs_sign_in`, step `NeedsYou`, action `SignIn`; the guided run waits for `sign_in_harness` (B) and resumes when the attachment is `ready`. On the server the step is `Failed` with `SETUP_SIGN_IN_ON_SERVER` and the server sentence (11:784), never started from here.
5. **AgentRepositories.** `catalogue.sync_profile_repositories()`; each `Failed` repository makes the step `Failed` with `SETUP_CATALOGUE_FAILED`, "Can't reach the company's agent library right now", Try again; at least one cloned or fetched repository keeps the agents from it usable.
6. **Programs.** For each `BundledProgram`, D.3. **CraftApps.** `craft.check_apps(true)`; each `Refused` maps to the refused-program result with the app's name, publisher and hash; `Missing` after its repair → `SETUP_CRAFT_APP_FAILED`, Try again.
7. Write `setup_state` (`last_guided_completed_at`, `requirements_digest` per C.1) only when `ready = true`; write every result to `setup_checks` (replace per `check`); publish the final progress (`running: false`); return the report. The program then journals the report and starts Ordin's session through the Ordin Host (03:44–46).

### B. `sign_in_harness`

1. No probe for `harness` → `SETUP_HARNESS_UNKNOWN`. Server → `SETUP_SIGN_IN_ON_SERVER`.
2. `probe.start_sign_in()` on a task; poll `sign_in_status()` every 3 s for up to 10 minutes. `SignedIn` → attachment `ready` (when `tested`), `signed_in = true`, `sign_in_method` = method (≤ 120 chars, truncated); return it. `NoAccess` → `SETUP_NO_HARNESS_ACCESS`. Timeout → `SETUP_SIGN_IN_TIMEOUT` (action Try again); the attachment stays `needs_sign_in`. `ProbeError::Refused` → refused-program path (D.3.4). C-25: for Copilot, a `start_sign_in` that fails with `NotOffered` leaves the step at `NeedsYou` and keeps polling, so a sign-in the person makes in the CLI itself is picked up.

### C. `needs_guided_setup`

1. `requirements_digest` = SHA-256 of the canonical JSON `{"harnesses": document.harnesses, "repositories": [display_name of each agent_catalogue.repositories entry, sorted]}`.
2. True when `setup_state` has no row, `last_guided_completed_at` is NULL, or the stored digest differs (03:51 "a start after the organisation profile changed what setup requires").

### D. Checks

1. **Claude Code probe (`ClaudeCodeProbe`).** All commands run without a shell, with an environment built explicitly: `PATH`, `HOME` (`USERPROFILE` on Windows), `LANG`, and nothing else, so `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN` and other credential variables never reach them (2D-1c).
   - `locate`: search each `PATH` entry for `claude` (`claude.exe`, `claude.cmd` on Windows) *(assumed here, SA-SETUP-03)*; run `claude --version`; version = first whitespace-separated token matching `^[0-9]+\.[0-9]+\.[0-9]+`; no match → `Unreadable`.
   - `sign_in_status`: `claude auth status --json`; exit 0 and `loggedIn: true` → `SignedIn { method }` with `method` from the `authMethod` field when present *(field name assumed, SA-SETUP-04)*; non-zero exit or `loggedIn: false` → `SignedOut`. ordin never reads `~/.claude/.credentials.json` (ADR-073).
   - `start_sign_in`: `claude auth login` with no terminal and null stdin (C-25 passed for Claude Code); returns on process exit.
   - `repair`: `NothingToDo`.
2. **Before a session (`run_checks(BeforeSession)`):** only `Harness` and `HarnessSignIn` for the harness `ready_harness` would return; the server runs this before each harness session (2D-4).
3. **Program start check** (each `BundledProgram`):
   1. File missing, or `expected_sha256` given and the file's SHA-256 differs → repair once: copy `restore_from` over `path` (same permission bits as the source) and re-check; no `restore_from` or still failing → `SETUP_PROGRAM_DAMAGED`, Send to IT ("A bundled tool does not respond: … reinstalls it from the application's own files once, then offers Send to IT", 03:57).
   2. Spawn `path` with `version_args`, stdin null, the D.1 environment, timeout 10 s.
   3. Exit 0 → `Passed`.
   4. Refused by the operating system: spawn error `PermissionDenied`; Windows raw OS error 1260 (`ERROR_ACCESS_DISABLED_BY_POLICY`) or 4551 (`ERROR_SYSTEM_INTEGRITY_POLICY_VIOLATION`); macOS or Linux termination by `SIGKILL` within 2 s with empty output. → `Failed`, `SETUP_PROGRAM_REFUSED`, message with `program` and `off` = `needed_for`, `refused_program` = name, publisher, SHA-256 of the file; Send to IT. Only that program's features are off; checks continue (UX-A46). Never retried by another route (ADR-072 "respected, not worked round").
   5. Other non-zero exit or timeout → `SETUP_PROGRAM_FAILED`, Try again.
4. **`run_checks(Start | Background | OnDemand)`** runs, in order: `CompanySettings` (status only; no wait), `Harness` and `HarnessSignIn` for each listed harness with a probe (never starting a sign-in; a signed-out harness becomes `needs_sign_in` and the run that needs it pauses with Sign in again, 03:178), probe `repair()` (restarts a stopped CLI server), `Tokens` (`identity.refresh_due()`: `needs_sign_in` entries → `Failed`, `CRED_SIGN_IN_REQUIRED`, Sign in again; `unreachable` → `Failed`, retried next interval), `AgentRepositories` (`sync_profile_repositories`, which fetches damaged clones), `AgentFolders` (`packages.verify_agent_folders(true)`), `Programs`, `CraftApps`. A repair is attempted once per check per run. Background runs start 60 s after `Start` and then every 60 minutes on the desktop, every 6 hours on the server; a failed background check is reported as a quiet status (progress with `running: false`, step `Failed`), not as repeated pop-ups (11 §6).

### E. `ready_harness`

1. Read attachments; walk `document.harnesses` order; return the first `ready` attachment whose `last_checked_at` is within the background interval; when older, run D.2 for it first.
2. None ready → `SETUP_NO_READY_HARNESS` with the action of the first harness's failure (Sign in again, Send to IT).

### F. `effective_update_feed`

1. Snapshot source `Verified` and `document.updates` is `Some` → `updates.feed_url` (2D-2). Otherwise `installer_settings().update_feed_url`. Both absent → None (updates off; logged once per start as `setup.no_update_feed`).
2. This crate decides when to check and what the person sees (R16); the desktop program's `AppUpdater` (Tauri's updater) does the download and install, and the server reads the same feed through `check_update_feed` to announce a new version once (ADR-076). The update signing key is fixed at build (2D-2b). A verified profile may redirect the feed (decision D10; the contrary ADR-013 amendment sentence is withdrawn). Earlier note B2-D-A1 is recorded in the findings; this follows 2D-2 and 08:77.

3. **Update checks (desktop).** With an `AppUpdater` registered and `effective_update_feed()` an `https` URL: check 60 s after start and then every 24 hours *(chosen here)*, on the same timer as D.4. `Ok(Some(offer))` → keep it as `update_offer` and log `setup.update_offered` (`version`); the screens read it with `setup.update_offer`. `Ok(None)` → clear any offer whose version is not newer than `application_version`. `Unreachable` → logged, retried at the next check, never shown as a pop-up. `SignatureInvalid` → logged `warn`, offer cleared, Send to IT available through Diagnostics. No feed → no check. `install_update`: no offer → `SETUP_UPDATE_NONE`; else `updater.install(offer)`; `InstallFailed` → `SETUP_UPDATE_INSTALL_FAILED` (Try again); success → logged `setup.update_installed`; the screens show "Restart to finish updating." †.
4. **`check_update_feed` (server).** No feed → `Ok(None)`. `GET` the feed (timeout 15 s; `SETUP_UPDATE_UNREACHABLE` on failure, retryable), parse its `version` field (Tauri's updater format); not semantic-version text → `SETUP_UPDATE_FEED_INVALID`. Newer than `application_version` → `Ok(Some(version))`, else `Ok(None)`. The server installs nothing (its package manager or image does).
5. *Amended January 2026 (ADR-114):* steps 3 and 4 are extended by SET-UP-1 to SET-UP-24 of the amendment below: background download, install on a press or at quit, **What's new**, and a personal server that replaces itself when the person asks; the agent profile stays announce-only.

### G. Help requests

1. `prepare_help_request`: validate lengths (`SETUP_HELP_INPUT_INVALID`); redact (G.3); build `HelpRequestBody` with `application_version` and `sidecar_versions` (each attachment's harness and version, each bundled program's name and its last read version); destination from `document.help.destination` (None → the person will enter an address; on the server None → `SETUP_HELP_DESTINATION_MISSING`). Keep the preview in memory for 30 minutes (at most 20 previews; the oldest is dropped). The program shows it on an approval-style card before sending (11 §6).
2. `send_help_request`: unknown or expired id → `SETUP_HELP_PREVIEW_UNKNOWN`. By destination:
   - `ticket_url`: the address must be an `https` URL ≤ 2048 bytes (`SETUP_HELP_DESTINATION_INVALID`). POST the body as `application/json`; add `Authorization: Bearer <token>` from `identity.access_token(Oidc, None)` when the person has an OIDC sign-in, otherwise no header. 2xx → `Sent`. Timeout or 5xx → one retry after 2 s, then `SETUP_HELP_SEND_FAILED` (Try again). 4xx → `SETUP_HELP_REFUSED` (Try again).
   - `slack_channel`: `identity.access_token(Slack, None)`; `CRED_SIGN_IN_REQUIRED` → returned as is (Sign in again). POST `https://slack.com/api/chat.postMessage` with `channel` = address and `text` = the body rendered as plain text (G.4); `"ok": false` or non-2xx → `SETUP_HELP_SEND_FAILED`. → `Sent`.
   - `email`, or no destination with `fallback_address` (must match `^[^@\s]{1,64}@[^@\s]{1,253}$`, else `SETUP_HELP_ADDRESS_INVALID`): desktop → build `mailto:<address>?subject=<"ordin help request" + reference>&body=<G.4 text>` percent-encoded, truncated at 8,000 characters with the line "(details shortened)"; `url_opener.open` (failure → `SETUP_HELP_MAIL_APP_FAILED`, Try again) → `OpenedInMailApp`. Server → `TextForPerson`.
   - No destination and no address on the desktop → `SETUP_HELP_DESTINATION_MISSING` (the screen asks for an address).
   Each outcome carries `HelpRequestRecord` (body SHA-256, never the body) for the program to journal; the preview is removed. ordin reads no reply (OQ-53 (b)).
3. **Redaction** (applied to `what_doing`, `failure_sentence` and every detail line, in order): replace with `[removed]` every match of — `Bearer\s+\S+`; `(?i)(password|passwd|secret|token|api[_-]?key)\s*[:=]\s*\S+`; `gh[pousr]_[A-Za-z0-9]{20,}`; `xox[abpr]-[A-Za-z0-9-]{10,}`; `eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+` (JWT); any run of ≥ 40 characters from `[A-Za-z0-9+/=_-]`; email addresses other than the destination's; then replace the person's home folder prefix with `~`. Lines over 2,000 characters are cut. Redaction is applied again to the body before sending.
4. Plain-text rendering: "What I was doing: …", "What went wrong: …", "Reference: r-…", "ordin {version}; {harness} {version}", then each detail on its own line.

### H. `ensure_language_server(language, server_name, version)`

1. Find the entry (SA-CLOSE-01): the profile's `document.language_servers` entry whose `language` equals `language`; when the profile has none for that language, the `language-servers.json` entry for it. The entry's `name` and `version` must equal `server_name` and `version` → else `SETUP_LANGUAGE_SERVER_NOT_APPROVED` (permanent, Send to IT). `language` outside `go`, `rust`, `java`, `typescript` → the same code. The platform is this computer's (`std::env::consts::{OS, ARCH}`: `macos`+`aarch64` → `macos_arm64`, `macos`+`x86_64` → `macos_x86_64`, `windows`+`x86_64` → `windows_x86_64`, `linux`+`x86_64` → `linux_x86_64`); no `platforms` entry for it → `SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED` (permanent, no action).
2. Folder `<data root>/language-servers/<server_name>/<version>/` with a `ready` marker file → return its `LanguageServerLaunch` from the marker (JSON `{program, args, env}`). One install per (name, version) at a time; concurrent callers wait for it.
3. Otherwise install into a temporary sibling folder `<data root>/language-servers/<server_name>/.<version>.tmp-<random>`:
    1. Download `asset_url` with `SetupConfig.http` (system proxy, A-39), streaming to `<tmp>/<file_name>`; timeout 10 minutes in all, at most 500 MiB (beyond → abort). Network failure, a non-2xx status or the size limit → `SETUP_LANGUAGE_SERVER_INSTALL_FAILED` (retryable).
    2. SHA-256 of the downloaded file must equal the entry's `sha256` → else `SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH` (permanent, Send to IT; the file is deleted; log `setup.language_server.digest_mismatch` with `server`, `version`, never the URL's query).
    3. Unpack by `file_name`: `.zip` → `zip`; `.tar.gz` or `.tgz` → `tar` over `flate2`; `.gz` (one compressed file) → `flate2` into `<tmp>/<file_name without .gz>`; any other suffix → the file is used as it is. Every entry's path is checked as 05 states (no absolute path, no `..`, at most 20,000 entries and 2 GiB unpacked; a breach → `SETUP_LANGUAGE_SERVER_INSTALL_FAILED`, permanent). The archive file is deleted after unpacking.
    4. `<tmp>/<binary_path>` must exist and be a regular file → else `SETUP_LANGUAGE_SERVER_INSTALL_FAILED` (permanent); on macOS and Linux set its mode `0755`.
    5. Write the marker `ready` as JSON `{ "program": "<final folder>/<binary_path>", "args": [], "env": [] }`, rename `<tmp>` to `<data root>/language-servers/<server_name>/<version>/`. Any failure removes `<tmp>`. Log `setup.language_server.installed` (`server`, `version`, `elapsed_ms`).

## Data Model

SQLite, journal database (contract §1); `MIGRATION_1_SQL`. No PostgreSQL variant (D P only).

```sql
CREATE TABLE harness_attachments (
    id               TEXT PRIMARY KEY CHECK (length(id) = 36),
    harness          TEXT NOT NULL UNIQUE CHECK (harness IN ('claude_code','copilot_cli')),
    location         TEXT NOT NULL CHECK (length(CAST(location AS BLOB)) BETWEEN 1 AND 4096),
    version          TEXT CHECK (version IS NULL OR length(version) <= 64),
    tested           INTEGER NOT NULL CHECK (tested IN (0,1)),
    signed_in        INTEGER NOT NULL CHECK (signed_in IN (0,1)),
    sign_in_method   TEXT CHECK (sign_in_method IS NULL OR length(sign_in_method) <= 120),
    state            TEXT NOT NULL CHECK (state IN ('found','checked','ready','needs_sign_in','needs_update')),
    last_checked_at  TEXT NOT NULL,
    CHECK (state <> 'ready' OR (tested = 1 AND signed_in = 1))
);

-- chosen here: one row
CREATE TABLE setup_state (
    singleton                 INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_guided_completed_at  TEXT,
    requirements_digest       TEXT CHECK (requirements_digest IS NULL OR length(requirements_digest) = 64)
);

-- chosen here: latest result per check
CREATE TABLE setup_checks (
    check_kind   TEXT PRIMARY KEY CHECK (check_kind IN ('company_settings','harness','harness_sign_in','tokens',
                   'agent_repositories','agent_folders','programs','craft_apps')),
    outcome      TEXT NOT NULL CHECK (outcome IN ('passed','repaired','failed','skipped')),
    code         TEXT,
    result_json  TEXT NOT NULL,          -- the CheckResult as JSON (no secrets: codes, names, hashes only)
    checked_at   TEXT NOT NULL
);
```
Rollback: creation only; restore the pre-migration backup (SA-XC-07).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Installer carried no profile address | Err(permanent) in the result | `SETUP_NO_PROFILE_ADDRESS` | `{"code":…,"action":"send_to_it"}` |
| `new` given a PostgreSQL datastore | Err(permanent); start stops (program defect) | `SETUP_STORE_UNSUPPORTED` | log only |
| Update feed unreachable | Err(retryable); logged, no pop-up | `SETUP_UPDATE_UNREACHABLE` | `{"code":…,"action":"try_again"}` |
| Update signature refused by the build's update key | Err(permanent); offer cleared | `SETUP_UPDATE_SIGNATURE_INVALID` | `{"code":…,"action":"send_to_it"}` |
| Server feed has no readable `version` | Err(permanent) | `SETUP_UPDATE_FEED_INVALID` | log only (server) |
| `install_update` with no offer | Err(permanent) | `SETUP_UPDATE_NONE` | `{"code":…,"action":null}` |
| Installing the update failed | Err(retryable) | `SETUP_UPDATE_INSTALL_FAILED` | `{"code":…,"action":"try_again"}` |
| Language server not approved by the profile | Err(permanent) | `SETUP_LANGUAGE_SERVER_NOT_APPROVED` | `{"code":…,"action":"send_to_it"}` |
| Language server install failed (download, unpack, missing binary) | Err(retryable), or Err(permanent) for an archive breaking the unpack limits or lacking `binary_path` | `SETUP_LANGUAGE_SERVER_INSTALL_FAILED` | `{"code":…,"action":"try_again"}` (retryable) / `{"code":…,"action":"send_to_it"}` (permanent) |
| Downloaded language server's SHA-256 differs from the pinned one | Err(permanent) | `SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH` | `{"code":…,"action":"send_to_it"}` |
| No download for this computer's platform | Err(permanent) | `SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED` | `{"code":…,"action":null}` |
| Profile cannot be fetched or verified | Err(retryable) in the result | `SETUP_PROFILE_UNREACHABLE` | `{"code":…,"action":null}` (background retry) |
| No listed harness found | Err(permanent) in the result | `SETUP_HARNESS_MISSING` | `{"code":…,"action":"send_to_it"}` |
| Harness version not in `tested_versions` | Err(permanent) in the result | `SETUP_HARNESS_UNTESTED` | `{"code":…,"action":"send_to_it"}` |
| Harness reports no access / no seat | Err(permanent) | `SETUP_NO_HARNESS_ACCESS` | `{"code":…,"action":"send_to_it"}` (words: Ask IT for Claude / Copilot access) |
| Harness sign-in not finished in 10 min | Err(retryable) | `SETUP_SIGN_IN_TIMEOUT` | `{"code":…,"action":"try_again"}` |
| Harness sign-in asked on the server | Err(permanent) | `SETUP_SIGN_IN_ON_SERVER` | `{"code":…,"action":null}` |
| No probe registered for the harness | Err(permanent) | `SETUP_HARNESS_UNKNOWN` | — (wiring defect) |
| No ready harness for a session | Err(permanent) | `SETUP_NO_READY_HARNESS` | `{"code":…,"action":"sign_in_again"}` or `send_to_it` |
| Harness output unreadable / command timed out | Err(retryable) in the result | `SETUP_HARNESS_CHECK_FAILED` | `{"code":…,"action":"try_again"}` |
| Agent repository clone or fetch failed | Err(retryable) in the result | `SETUP_CATALOGUE_FAILED` | `{"code":…,"action":"try_again"}` |
| Agent folder could not be re-installed | Err(permanent) in the result | `SETUP_AGENT_FOLDER_FAILED` | `{"code":…,"action":"send_to_it"}` |
| Bundled program missing or damaged, restore failed | Err(permanent) in the result | `SETUP_PROGRAM_DAMAGED` | `{"code":…,"action":"send_to_it"}` |
| Program refused by the operating system | Err(permanent) in the result | `SETUP_PROGRAM_REFUSED` | `{"code":…,"action":"send_to_it"}` |
| Program exited non-zero or timed out | Err(retryable) in the result | `SETUP_PROGRAM_FAILED` | `{"code":…,"action":"try_again"}` |
| ArtCraft app missing after repair | Err(retryable) in the result | `SETUP_CRAFT_APP_FAILED` | `{"code":…,"action":"try_again"}` |
| A token needs a new sign-in | Err(permanent) in the result | `CRED_SIGN_IN_REQUIRED` (from Identity) | `{"code":…,"action":"sign_in_again"}` |
| Database write failed | Err(retryable) | `SETUP_STORE_FAILED` | `{"code":…,"action":"try_again"}` |
| `harness_attachments` rule broken (2C) | Err(permanent) | `HARN_DUPLICATE`, `HARN_PATH_INVALID`, `HARN_VERSION_INVALID`, `HARN_SIGN_IN_METHOD_INVALID`, `HARN_STATE_INCONSISTENT` | — (emitter defect) |
| Help input too long or empty | Err(permanent) | `SETUP_HELP_INPUT_INVALID` | `{"code":…,"action":null}` |
| Preview unknown or expired | Err(permanent) | `SETUP_HELP_PREVIEW_UNKNOWN` | `{"code":…,"action":"try_again"}` |
| No destination and no address | Err(permanent) | `SETUP_HELP_DESTINATION_MISSING` | `{"code":…,"action":null}` |
| Destination not a valid `https` URL | Err(permanent) | `SETUP_HELP_DESTINATION_INVALID` | `{"code":…,"action":null}` |
| Entered address invalid | Err(permanent) | `SETUP_HELP_ADDRESS_INVALID` | `{"code":…,"action":null}` |
| Delivery timed out, 5xx, Slack `ok:false` | Err(retryable) | `SETUP_HELP_SEND_FAILED` | `{"code":…,"action":"try_again"}` |
| Ticket URL answered 4xx | Err(permanent) | `SETUP_HELP_REFUSED` | `{"code":…,"action":"try_again"}` |
| Mail app could not be opened | Err(retryable) | `SETUP_HELP_MAIL_APP_FAILED` | `{"code":…,"action":"try_again"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Setup Assistant

  Scenario: Happy path — first start, Claude Code already signed in
    Given no setup_state row and a verified profile naming harness claude_code and one agent repository
    And "claude auth status --json" exits 0 with loggedIn true
    When run_guided is called
    Then progress shows total 4 and nothing is asked of the person
    And the claude_code attachment is "ready"
    And the report has ready true and setup_state records the requirements digest

  Scenario: Happy path — signed out, one-button sign-in
    Given claude_code is found at a tested version and reports loggedIn false
    When run_guided reaches the sign-in step
    Then the step state is "needs_you" with action "sign_in"
    When sign_in_harness(claude_code) is called and the status turns signed in within 10 minutes
    Then the attachment is "ready" and the guided run completes

  Scenario: Edge — profile changed what setup requires
    Given setup completed with harnesses [claude_code]
    When a newer verified profile lists harnesses [copilot, claude_code]
    Then needs_guided_setup returns true

  Scenario: Edge — untested harness version
    Given the profile lists claude_code with tested_versions ["2.1.302"] and the installed version is "2.1.310"
    When run_guided runs
    Then the attachment state is "needs_update" and the result code is SETUP_HARNESS_UNTESTED with action send_to_it

  Scenario: Error — harness missing
    Given no claude executable on PATH and no Copilot probe registered
    When run_guided runs
    Then the result code is SETUP_HARNESS_MISSING and the sentence is "Claude Code needs to be installed by IT on this computer"
    And ready is false

  Scenario: Error — program stopped by company allow-listing
    Given the bundled program "PhotoCraft" is killed by SIGKILL within 2 seconds with no output
    When run_checks(OnDemand) runs
    Then the Programs result is SETUP_PROGRAM_REFUSED with refused_program name, publisher and SHA-256
    And the other checks still run

  Scenario: Edge — damaged bundled program restored once
    Given the ordin hook's file hash differs from expected and restore_from holds a good copy
    When run_checks(Start) runs
    Then the file is restored and the Programs outcome is "repaired"

  Scenario: Error — profile unreachable on first start
    Given the profile address does not answer
    When run_guided runs
    Then the CompanySettings result is SETUP_PROFILE_UNREACHABLE with no action
    And the sequence continues with default settings

  Scenario: Edge — background token refresh
    Given the GitHub credential expires in 2 minutes
    When the background check runs
    Then identity.refresh_due is called and the Tokens outcome is "repaired"

  Scenario: Error — refresh refused needs sign-in
    Given refresh_due reports a credential in needs_sign_in
    When the background check runs
    Then the Tokens result is CRED_SIGN_IN_REQUIRED with action sign_in_again

  Scenario: Happy path — Send to IT to a ticket URL
    Given the profile help destination is ticket_url "https://help.acme.com/api/ordin"
    When prepare_help_request is called with a detail line containing "Authorization: Bearer abc.def.ghi"
    Then the preview body shows "[removed]" in place of the token
    When send_help_request is called and the URL answers 201
    Then the outcome is Sent and the record carries the body's SHA-256

  Scenario: Edge — no destination on the desktop
    Given the profile has no help destination
    When send_help_request is called with fallback_address "it@acme.com"
    Then the mail app opens a pre-filled mailto message and the outcome is OpenedInMailApp

  Scenario: Error — ticket URL down
    Given the ticket URL times out twice
    When send_help_request is called
    Then it fails with SETUP_HELP_SEND_FAILED marked retryable

  Scenario: Error — expired preview
    Given a preview made 31 minutes ago
    When send_help_request is called with its id
    Then it fails with SETUP_HELP_PREVIEW_UNKNOWN

  Scenario: Edge — server harness sign-in
    Given the program is PersonalServer and claude_code reports signed out
    When run_checks(Background) runs
    Then the attachment is "needs_sign_in" and the result code is SETUP_SIGN_IN_ON_SERVER
    And no sign-in process is started

  Scenario: Happy path — the desktop finds and installs an update
    Given an AppUpdater is registered and the verified profile names feed "https://it.example.com/ordin/updates/latest.json"
    And the feed offers version "1.4.0" while the application is "1.3.2"
    When start_background_checks has run for 60 seconds
    Then update_offer returns version "1.4.0"
    When install_update is called
    Then the updater installs "1.4.0" and the application is not restarted

  Scenario: Error — an update with a bad signature is not offered
    Given the feed's update fails the build's update key
    When the update check runs
    Then update_offer returns None and SETUP_UPDATE_SIGNATURE_INVALID is logged

  Scenario: Happy path — the server reads the feed once
    Given the server's effective feed names version "1.4.0" and the server runs "1.3.2"
    When check_update_feed is called
    Then it returns "1.4.0" and nothing is installed

  Scenario: Error — a language server the profile did not approve
    Given the profile's language_servers entry for "go" is gopls "0.20.0"
    When ensure_language_server("go", "gopls", "0.19.1") is called
    Then it fails with SETUP_LANGUAGE_SERVER_NOT_APPROVED

  Scenario: Happy — a language server from the build's table is installed once
    Given the profile has no language_servers entry for "rust"
    And language-servers.json pins rust-analyzer "2026-01-10" for linux_x86_64 as "rust-analyzer-x86_64-unknown-linux-gnu.gz" with binary_path "rust-analyzer-x86_64-unknown-linux-gnu"
    And the download's SHA-256 equals the pinned one
    When ensure_language_server("rust", "rust-analyzer", "2026-01-10") is called on Linux x86-64
    Then the folder <data root>/language-servers/rust-analyzer/January 2026/ holds the binary and a ready marker
    And the result's program is that binary with no args
    And a second call returns the same launch without downloading

  Scenario: Error — a download whose digest differs
    Given the profile pins gopls "0.20.0" for macos_arm64 with sha256 S
    And the downloaded file's SHA-256 is not S
    When ensure_language_server("go", "gopls", "0.20.0") is called
    Then it fails with SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH and no folder for gopls 0.20.0 exists

  Scenario: Edge — no download for this platform
    Given the profile pins gopls "0.20.0" for linux_x86_64 only
    When ensure_language_server("go", "gopls", "0.20.0") is called on Windows x86-64
    Then it fails with SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED

  Scenario: Edge — background checks start once
    When start_background_checks is called twice
    Then one timer runs
```

## Performance, Security, Observability

- **Performance targets:** `needs_guided_setup`, `harness_attachments`, `progress` p95 ≤ 10 ms. Guided sequence excluding the person's sign-in and the repository clones: ≤ 3 s on a supported machine (A-03 start budget). `run_checks(BeforeSession)` p95 ≤ 1.5 s (two harness commands). Background checks run at low priority and never block a command. Memory ≤ 4 MiB besides the callees'.
- **Security:**
  - Entry points: in-process calls and the six Service API commands (Tauri capabilities on the desktop; Remote API desktop key on the server). Command inputs deserialise with unknown fields refused (SA-API-02).
  - Every child process is started without a shell, with an explicit minimal environment, so no credential variable reaches a harness command (2D-1c); arguments are fixed, never built from input.
  - Never asks for administrator rights; never runs a machine-wide installer; never works round a refusal (ADR-072).
  - ordin never reads, stores or relays a harness credential (ADR-044); `sign_in_method` is the harness's own free text.
  - Help requests are redacted (G.3) and shown before sending; only the body's hash is journalled.
  - Never logged: help request bodies, tokens fetched for delivery, environment values, harness command output beyond the parsed version and the boolean sign-in state.
- **Observability:** `tracing` JSON lines only (desktop, personal server; SA-XC-04/05). Target `ordin_setup`:
  - `setup.guided.started` / `setup.guided.completed` (info): `total`, `ready`, `duration_ms`.
  - `setup.check` (info on change, debug otherwise): `check`, `outcome`, `code`, `trigger`.
  - `setup.harness` (info): `harness`, `version`, `tested`, `signed_in`, `state`.
  - `setup.program_refused` (warn): `program`, `publisher`, `sha256`.
  - `setup.repair` (info): `check`, `outcome`.
  - `setup.help.sent` (info): `destination_kind`, `reference`, `body_sha256`.
  - `setup.help.failed` (warn): `destination_kind`, `code`.
  - `setup.no_update_feed` (warn).

## Gaps

None.

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 (amends ADR-076's *Updates* item, ADR-041 and ADR-072), UX-A81. The desktop checks when the person asks, downloads and verifies in the background, and installs when the person presses **Restart to update** or quits; the first start of a new version shows its release notes once (**What's new**); a personal server replaces itself when the person asks, never unasked; the agent profile stays announce-only (ADR-114 item 6). Every section above stays in force except where this amendment states otherwise. Steps F.3's sentence "`install_update` … `updater.install(offer)` … the screens show 'Restart to finish updating.'" and the scenario *Happy path — the desktop finds and installs an update* are replaced by SET-UP-5 to SET-UP-9 and the scenarios below.
>
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

### Public Interface (changed and added)

```rust
/// Registered by the program with the Journal's `MigrationRegistry` (D4, R20). Amended: step 2.
pub fn migrations() -> ordin_journal::ModuleMigrations;
// { module: "setup", sqlite: &[(1, "create setup tables", MIGRATION_1_SQL),
//                             (2, "releases and updates", MIGRATION_2_SQL)], postgres: &[] }

/// Owned here, implemented by the desktop program over Tauri's updater (ADR-100 pattern, D3).
/// Replaces the two-method trait above.
#[async_trait::async_trait]
pub trait AppUpdater: Send + Sync {
    /// Reads the feed; `Ok(None)` when no newer signed version exists. The offer's `state` is ignored
    /// by this crate, which sets it (SET-UP-3).
    async fn check(&self, feed_url: &url::Url) -> Result<Option<UpdateOffer>, UpdaterError>;
    /// Downloads the offered version and verifies it with the build's update key (2D-2b); the updater
    /// keeps the verified download for one of the two install calls. A second call for the same
    /// version returns `Ok(())` without downloading again.
    async fn download(&self, offer: &UpdateOffer) -> Result<(), UpdaterError>;
    /// Installs the kept download and restarts ordin. Called only after the program's quit flow has
    /// stopped every module (OrdinDesktop.md amendment January 2026).
    async fn install_and_restart(&self) -> Result<(), UpdaterError>;
    /// Installs the kept download while ordin exits; no restart.
    async fn install_on_exit(&self) -> Result<(), UpdaterError>;
}

#[derive(Debug, thiserror::Error)]
pub enum UpdaterError {
    #[error("feed unreachable")] Unreachable,          // → SETUP_UPDATE_UNREACHABLE, retryable
    #[error("signature refused")] SignatureInvalid,     // → SETUP_UPDATE_SIGNATURE_INVALID
    #[error("install failed")] InstallFailed,           // → SETUP_UPDATE_INSTALL_FAILED, retryable
    /// Added: an install call with no kept download (chosen here).
    #[error("nothing downloaded")] NothingDownloaded,   // → SETUP_UPDATE_NONE
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateOffer {
    /// Semantic version, 1–64 characters.
    pub version: String,
    /// The feed's release notes, at most 4,000 characters (cut beyond).
    pub notes: String,
    /// Added (ADR-114).
    pub state: UpdateState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState { Downloading, Ready }

/// The release notes of the version now running, kept when its download was verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WhatsNew {
    pub version: String,   // the running version
    pub notes: String,     // at most 4,000 characters
    /// Chosen here: true once `whats_new_seen` was called for this version, so Settings › About can
    /// show the notes again while the screens draw the What's new card only when it is false.
    pub seen: bool,
}

/// A newer version a personal server's feed offers (`setup.server_update`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServerUpdate {
    pub version: String,
    pub notes: String,     // at most 4,000 characters
    /// False when this server cannot replace itself (in a container, or not run by its user unit).
    pub self_update: bool,
}

/// How the program ends after a ready update (chosen here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFinish {
    /// The person pressed Restart to update; the program has run its quit flow.
    Restart,
    /// The person quit ordin; install on the way out, no restart.
    OnExit,
}

/// The platform parts of a personal server's own update, implemented by the server program
/// (OrdinServer.md amendment January 2026) so this crate stays testable with a double (chosen here).
#[async_trait::async_trait]
pub trait ServerPlatform: Send + Sync {
    /// How this process was started.
    fn run_mode(&self) -> ServerRunMode;
    /// The folder holding the running `ordin-server` and `ordin-hook` (the directory of the running executable).
    fn program_dir(&self) -> std::path::PathBuf;
    /// The key of this build's operating system and architecture in the feed's platform entries (C-99).
    fn feed_platform(&self) -> &'static str;
    /// Asks systemd to restart the user unit `ordin-server.service`; returns once the request is
    /// accepted. The unit's normal stop order then runs (OrdinServer.md *Internal Logic* step 11).
    async fn restart_unit(&self) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerRunMode { UserUnit, Container, Other }

/// Result of a server update run in the foreground (`ordin-server update`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerUpdateOutcome {
    /// No newer version in the feed.
    UpToDate,
    /// Programs replaced and the unit's restart requested.
    Restarting { version: String },
}

impl SetupAssistant {
    // ---- desktop (SetupProgram::Desktop) ----
    /// `setup.check_update`: an on-demand check (trigger OnDemand); one at a time (SET-UP-2).
    pub async fn check_update(&self) -> Result<Option<UpdateOffer>, SetupError>;
    /// Amended: no longer installs. With an offer in state `Ready` returns Ok(()) and the program then
    /// runs its quit flow and calls `finish_update(UpdateFinish::Restart)`; otherwise SETUP_UPDATE_NONE.
    pub async fn install_update(&self) -> Result<(), SetupError>;
    /// Called by the program as the last step of its quit flow. `Ok(false)` when no download is
    /// ready (nothing to install); `Ok(true)` when the install ran.
    pub async fn finish_update(&self, how: UpdateFinish) -> Result<bool, SetupError>;
    /// `setup.whats_new`: the running version's kept notes, `None` when none were kept.
    pub async fn whats_new(&self) -> Result<Option<WhatsNew>, SetupError>;
    /// `setup.whats_new_seen { version }`.
    pub async fn whats_new_seen(&self, version: &str) -> Result<(), SetupError>;

    // ---- personal server (SetupProgram::PersonalServer) ----
    /// Registers the server program's platform parts. Without one, self-update is unavailable.
    pub fn register_server_platform(&self, platform: std::sync::Arc<dyn ServerPlatform>);
    /// `setup.server_update`.
    pub async fn server_update(&self) -> Result<Option<ServerUpdate>, SetupError>;
    /// `setup.update_server { version }`: validates, then runs SET-UP-14 to SET-UP-22 on a task.
    pub async fn update_server(&self, version: &str) -> Result<(), SetupError>;
    /// `ordin-server update` (foreground, the owner at a terminal): SET-UP-14 to SET-UP-22.
    pub async fn run_server_update(&self) -> Result<ServerUpdateOutcome, SetupError>;
}

/// The code the server program returns for `setup.server_update` and `setup.update_server` in the
/// agent profile, which constructs no Setup Assistant (chosen here).
pub const SELF_UPDATE_UNAVAILABLE: &str = "SETUP_SELF_UPDATE_UNAVAILABLE";
```

### Service API commands (added and changed)

| Command | Input | Result | Calls |
|---|---|---|---|
| `setup.check_update` | `{}` | `{"offer": UpdateOffer?}` | `check_update` (desktop) |
| `setup.install_update` | `{}` | `{}` | `install_update`, then the program's quit flow and `finish_update(Restart)` (desktop) |
| `setup.whats_new` | `{}` | `{"whats_new": WhatsNew?}` | `whats_new` (desktop) |
| `setup.whats_new_seen` | `{"version": string}` | `{}` | `whats_new_seen` (desktop) |
| `setup.server_update` | `{}` | `{"update": ServerUpdate?}` | `server_update` (personal server); the desktop and the agent profile answer `SETUP_SELF_UPDATE_UNAVAILABLE` |
| `setup.update_server` | `{"version": string}` | `{"accepted": true}` | `update_server` (personal server); the desktop and the agent profile answer `SETUP_SELF_UPDATE_UNAVAILABLE` |

Scopes are the Service API's: `setup.check_update`, `setup.whats_new`, `setup.whats_new_seen` are `Local`; `setup.server_update` and `setup.update_server` are `Any`, sent by a paired desktop to its server's backend.

### Who calls each added item

| Item | Called by |
|---|---|
| `check_update`, `whats_new`, `whats_new_seen`, `install_update` | desktop program, for the `setup.*` commands |
| `finish_update` | desktop program, as the last step of its quit flow (`Restart` after `setup.install_update`; `OnExit` on every other quit) |
| `register_server_platform` | server program, personal profile, after `new` |
| `server_update`, `update_server` | server program, personal profile, for the two `setup.*` commands |
| `run_server_update` | server program, for the command `ordin-server update`; `ordin-server update --check` calls `server_update` and prints its `version`, or nothing |

### Internal Logic (added to section F)

**Desktop: check, download, install**

1. **SET-UP-1 Feed read.** Every check (timer of F.3, or SET-UP-2) needs a registered `AppUpdater` and `effective_update_feed()` an `https` URL; otherwise `check_update` returns `Ok(None)` and logs `setup.no_update_feed` once per start. The timer of F.3 is unchanged: 60 s after start, then every 24 hours.
2. **SET-UP-2 On demand.** `check_update` runs the same check at once with trigger `OnDemand`. One check runs at a time: a call while a check runs (timer or on demand) waits for it and returns its result. The result is the offer as SET-UP-3 leaves it (with its `state`), or `None`. `Unreachable` → `SETUP_UPDATE_UNREACHABLE` (retryable), shown to the person because they asked; on the timer it stays logged only (F.3).
3. **SET-UP-3 Offer.** `check` → `Ok(Some(o))` with `o.version` newer than `application_version` (semantic-version order) → when the kept offer has the same version, keep it unchanged (no second download); otherwise keep `UpdateOffer { version, notes, state: Downloading }`, log `setup.update_offered` (`version`) and start SET-UP-4 on a task. `Ok(None)` → clear any offer whose version is not newer than `application_version`.
4. **SET-UP-4 Download.** `updater.download(&offer)`, at most one at a time; a newer offer arriving meanwhile waits for it, then replaces it. `Ok(())` → in one transaction write `setup_whats_new(version, notes, kept_at)` (replace on version; delete rows beyond the newest 5) and set `state: Ready`; log `setup.update_ready` (`version`). `Unreachable` → clear the offer, log `setup.update_download_failed` (`code`), retried at the next check, never a pop-up. `SignatureInvalid` → clear the offer, log `warn` `SETUP_UPDATE_SIGNATURE_INVALID` (Send to IT through Diagnostics, as F.3). No question is asked before downloading (ADR-114 item 4).
5. **SET-UP-5 Restart to update.** `install_update`: no offer, or an offer in state `Downloading` → `SETUP_UPDATE_NONE`. State `Ready` → log `setup.update_install_requested` (`version`) and return `Ok(())`; the program then runs its quit flow, which asks first when sessions run (OrdinDesktop.md *Internal Logic* step 10), and calls `finish_update(UpdateFinish::Restart)` last.
6. **SET-UP-6 Finish.** `finish_update(how)`: no `Ready` offer → `Ok(false)`. Otherwise `Restart` → `updater.install_and_restart()`; `OnExit` → `updater.install_on_exit()`. `Ok` → log `setup.update_installed` (`version`, `how`) and return `Ok(true)`. `NothingDownloaded` → clear the offer, `Ok(false)`. `InstallFailed` → log `setup.update_install_failed`, keep the offer `Ready`, return `SETUP_UPDATE_INSTALL_FAILED` (retryable); the program reports it as its spec states. `finish_update` reads and writes no store (the offer is held in memory), so the program may call it after `close()` and after the Journal's datastore is closed (OrdinDesktop.md DSK-UP-12, DSK-UP-13).
7. **SET-UP-7 Quit with a ready update.** On every quit other than SET-UP-5's the program calls `finish_update(UpdateFinish::OnExit)` as its last step; the update installs on the way out with no question and no restart.
8. **SET-UP-8 What's new.** `whats_new`: read `setup_whats_new` where `version = application_version` → `Some(WhatsNew { version, notes, seen: seen_at IS NOT NULL })`; no row → `None` (an install from a fresh installer keeps no notes). `whats_new_seen(version)`: a `version` outside 1–64 characters is refused by the Service API's input check (`SVC_INPUT_INVALID`) before it arrives here; set `seen_at` for that row when NULL; no row → `Ok(())` (nothing to mark). The screens draw the **What's new** card while `seen` is false and show the notes in Settings › About whenever `whats_new` returns a row (ScreensModule.md amendment January 2026).
9. **SET-UP-9 Store failures.** A failed write in SET-UP-4 leaves the offer `Downloading`, logs `SETUP_STORE_FAILED` and is retried at the next check; a failed read or write in SET-UP-8 returns `SETUP_STORE_FAILED` (retryable).

**Personal server: announce, offer, replace itself**

10. **SET-UP-10 Programs.** SET-UP-11 to SET-UP-22 apply to `SetupProgram::PersonalServer` only. On the desktop, `server_update`, `update_server` and `run_server_update` return `SETUP_SELF_UPDATE_UNAVAILABLE`. The agent profile constructs no Setup Assistant; its program returns the constant `SELF_UPDATE_UNAVAILABLE` for the two commands and keeps announcing new versions only (ADR-114 item 6; OrdinServer.md).
11. **SET-UP-11 Feed with notes.** The server's feed read is F.4, extended to keep the feed's `notes` (cut at 4,000 characters) and its platform entries, each a download address and a signature (Tauri's updater format; C-99). The feed address and every platform entry's download address must use `https` → otherwise `SETUP_UPDATE_FEED_INVALID` (permanent; nothing is downloaded). The daily announcement of F.4 is unchanged.
12. **SET-UP-12 `server_update`.** Read the feed (SET-UP-11). No newer version → `Ok(None)`. Newer → `Some(ServerUpdate { version, notes, self_update })` with `self_update` true only when a `ServerPlatform` is registered and its `run_mode()` is `UserUnit`.
13. **SET-UP-13 `update_server(version)`.** Check SET-UP-14 and SET-UP-15 at once, then read the feed: its newer version must equal `version` → else `SETUP_UPDATE_VERSION_CHANGED` (no newer version at all → `SETUP_UPDATE_NONE`). Return `Ok(())` (the command answers `{ accepted: true }`) and run SET-UP-16 to SET-UP-22 on a task; their outcome is logged and recorded in `setup_server_update`.
14. **SET-UP-14 May it replace itself.** No `ServerPlatform` registered, or `run_mode()` is `Container` or `Other` → `SETUP_SELF_UPDATE_UNAVAILABLE` ("This server can't update itself. Whoever runs it installs new versions.").
15. **SET-UP-15 One at a time.** In one transaction: a `setup_server_update` row in state `running` whose `started_at` is less than 30 minutes old → `SETUP_UPDATE_IN_PROGRESS`; otherwise replace the row with `{ version, state: running, started_at: now }`. A `running` row older than 30 minutes is taken as abandoned and replaced.
16. **SET-UP-16 Archive.** `run_server_update` first runs SET-UP-14, reads the feed (no newer version → release the row, `Ok(ServerUpdateOutcome::UpToDate)`) and SET-UP-15. Take the platform entry keyed by `feed_platform()` → none → `SETUP_UPDATE_NO_ARCHIVE`.
17. **SET-UP-17 Download.** Into a new folder `<program_dir>/.update-<version>-<random>/`, stream the archive with `SetupConfig.http` (system proxy, A-39), from an `https` address only (a redirect to any other scheme → `SETUP_UPDATE_FEED_INVALID`); at most 512 MiB and 10 minutes in all. Network failure, a non-2xx status or the size limit → `SETUP_UPDATE_UNREACHABLE` (retryable).
18. **SET-UP-18 Verify.** Verify the downloaded file against the entry's signature with the build's update public key (2D-2b), using `minisign-verify` (*chosen here*; both decoded as Tauri's updater writes them, C-99). Failure → `SETUP_UPDATE_SIGNATURE_INVALID`; the folder is deleted; log `warn`.
19. **SET-UP-19 Unpack and test.** Unpack the `.tar.gz` with `tar` over `flate2` (the libraries of H.3.3), with H.3.3's path and size checks. Every entry that is not a regular file — a symbolic link, a hard link, a device or other special file, or a folder other than `bin/` — fails the archive with `SETUP_UPDATE_FEED_INVALID` before anything is written. Only `bin/ordin-server` and `bin/ordin-hook` are extracted; every other regular file is skipped. Both must exist as regular files (written with mode `0755`), and `bin/ordin-server --version`, run without a shell with the D.1 environment and a 10 s timeout, must print `version` as its first token → otherwise `SETUP_UPDATE_FEED_INVALID` (permanent; the signed archive is not the release it claims). The archive file is deleted after unpacking.
20. **SET-UP-20 Replace.** For `ordin-hook`, then `ordin-server`, in `program_dir()`: rename the current file to `<name>.previous` (replacing an older one), then rename the unpacked file into place. Both renames are within one folder, so each is atomic. Any failure → undo the renames already made, in reverse order, delete the update folder, `SETUP_UPDATE_REPLACE_FAILED` (retryable; the running version is kept). The running process keeps its open executable; the new files take effect at the next start.
21. **SET-UP-21 Restart.** Set the row's state to `restarting`, delete the update folder, log `setup.server_update.installed` (`version`), then `restart_unit()`. `Ok` → `ServerUpdateOutcome::Restarting { version }`; the unit's stop order ends runs as OrdinServer.md states (runs may finish until 30 s, then are marked interrupted). `Err` → log `setup.server_update.restart_failed` (`reason`, at most 200 characters), then restore both programs from `<name>.previous` by rename, `ordin-server` first, then `ordin-hook`, so the running server keeps starting its own `ordin-hook`; set the row's state to `failed` with code `SETUP_UPDATE_REPLACE_FAILED` and return that code (retryable; the server is still on its current version). A restore rename that fails is logged `error` with the file name and the same code is returned; the owner's next `ordin-server update` replaces both again.
22. **SET-UP-22 Failures.** Any error of SET-UP-16 to SET-UP-20 sets the row's state to `failed` with its `code` and `ended_at`, removes the update folder, and is returned to `run_server_update`'s caller or, for `update_server`, logged as `setup.server_update.failed` (`code`).
23. **SET-UP-23 After the restart.** In `run_checks(Start)` on the personal server: a row in state `restarting` whose `version` equals `application_version` → state `done`, `ended_at` now, log `setup.server_update.done`; a `restarting` row with another version → state `failed`, code `SETUP_UPDATE_REPLACE_FAILED`, log `warn` (the new program did not start; the owner can rename `<name>.previous` back, R-125). The paired desktop sees the result through the server's version in `ServiceHello` and its update row (ScreensModule.md amendment January 2026).
24. **SET-UP-24 Never unasked.** No timer, event or profile setting starts SET-UP-16 to SET-UP-22; only `update_server` (a person's press on a paired desktop) and `run_server_update` (the owner's command) do (ADR-114 item 5).

### Data Model (added)

SQLite, journal database; `MIGRATION_2_SQL` (migration step 2, module `setup`). No PostgreSQL variant (D P only, as above).

```sql
-- chosen here: release notes kept when a desktop download is verified; newest 5 kept
CREATE TABLE setup_whats_new (
    version   TEXT PRIMARY KEY CHECK (length(version) BETWEEN 1 AND 64),
    notes     TEXT NOT NULL CHECK (length(notes) <= 4000),
    kept_at   TEXT NOT NULL,
    seen_at   TEXT
);

-- chosen here: the personal server's own update, one row
CREATE TABLE setup_server_update (
    singleton   INTEGER PRIMARY KEY CHECK (singleton = 1),
    version     TEXT NOT NULL CHECK (length(version) BETWEEN 1 AND 64),
    state       TEXT NOT NULL CHECK (state IN ('running','restarting','done','failed')),
    code        TEXT CHECK (code IS NULL OR length(code) <= 64),
    started_at  TEXT NOT NULL,
    ended_at    TEXT,
    CHECK (state IN ('running','restarting') OR ended_at IS NOT NULL)
);
```
Rollback: creation only; restore the pre-migration backup (SA-XC-07).

### Error Table (added and changed rows)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `install_update` with no offer, or the offer still downloading (changed) | Err(permanent) | `SETUP_UPDATE_NONE` | `{"code":…,"action":null}` |
| On-demand check cannot reach the feed | Err(retryable) | `SETUP_UPDATE_UNREACHABLE` | `{"code":…,"action":"try_again"}` |
| `finish_update` install failed; offer kept ready | Err(retryable) | `SETUP_UPDATE_INSTALL_FAILED` | `{"code":…,"action":"try_again"}` |
| Server command on the desktop or the agent profile; server in a container, not run by its user unit, or with no platform registered | Err(permanent) | `SETUP_SELF_UPDATE_UNAVAILABLE` | `{"code":…,"action":null}` |
| Feed has no archive for this server's architecture | Err(permanent) | `SETUP_UPDATE_NO_ARCHIVE` | `{"code":…,"action":null}` |
| Server archive download failed or over 512 MiB | Err(retryable) | `SETUP_UPDATE_UNREACHABLE` | `{"code":…,"action":"try_again"}` |
| Server archive signature refused | Err(permanent) | `SETUP_UPDATE_SIGNATURE_INVALID` | `{"code":…,"action":"send_to_it"}` |
| Signed archive lacks a program, or its version differs from the feed's | Err(permanent) | `SETUP_UPDATE_FEED_INVALID` | `{"code":…,"action":null}` |
| Program files could not be replaced; running version kept | Err(retryable) | `SETUP_UPDATE_REPLACE_FAILED` | `{"code":…,"action":"try_again"}` |
| A server update is already under way | Err(permanent) | `SETUP_UPDATE_IN_PROGRESS` | `{"code":…,"action":null}` |
| `update_server`'s version is not the one the feed now offers | Err(permanent) | `SETUP_UPDATE_VERSION_CHANGED` | `{"code":…,"action":null}` |
| `update_server` when the feed offers no newer version | Err(permanent) | `SETUP_UPDATE_NONE` | `{"code":…,"action":null}` |
| What's-new record or server-update row cannot be read or written | Err(retryable) | `SETUP_STORE_FAILED` | `{"code":…,"action":"try_again"}` |

Sentences (en-GB, through the Service API's `ErrorSentences`): `SETUP_SELF_UPDATE_UNAVAILABLE` "This server can't update itself. Whoever runs it installs new versions."; `SETUP_UPDATE_NO_ARCHIVE` "This version has no download for this server yet."; `SETUP_UPDATE_REPLACE_FAILED` "The update couldn't be installed. The server is still on its current version."; `SETUP_UPDATE_IN_PROGRESS` "An update is already under way."; `SETUP_UPDATE_VERSION_CHANGED` "A newer version came out meanwhile. Check again."

### Configuration

Fixed constants added to 2D-4 (Configuration.md amendment January 2026): server update download at most 512 MiB and 10 minutes; a `running` server update taken as abandoned after 30 minutes; at most 5 kept release notes. The feed address and update key are unchanged (2D-2, 2D-2b); the unbranded open-source build's installer carries the project's feed address as its `update_feed_url` (ADR-114 item 3).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Setup Assistant — releases and updates (ADR-114)

  Scenario: Happy path — an update is downloaded without a question and offered as ready
    Given an AppUpdater is registered and the effective feed offers "0.2.0" while the application is "0.1.0"
    When the update check runs
    Then update_offer returns version "0.2.0" with state "downloading"
    When the updater's download of "0.2.0" succeeds
    Then update_offer returns state "ready" and setup_whats_new holds the notes of "0.2.0"

  Scenario: Happy path — the person asks for a check
    Given the feed offers "0.2.0" while the application is "0.1.0"
    When check_update is called
    Then it returns the offer for "0.2.0"

  Scenario: Edge — two checks at once run one check
    Given a background update check is running
    When check_update is called
    Then the feed is read once and both receive the same result

  Scenario: Error — the person's check cannot reach the feed
    Given the feed does not answer
    When check_update is called
    Then it fails with SETUP_UPDATE_UNREACHABLE marked retryable

  Scenario: Error — a download with a bad signature is dropped
    Given the feed offers "0.2.0" and the download fails the build's update key
    When the download ends
    Then update_offer returns None and SETUP_UPDATE_SIGNATURE_INVALID is logged

  Scenario: Error — Restart to update before the download is ready
    Given the offer for "0.2.0" is in state "downloading"
    When install_update is called
    Then it fails with SETUP_UPDATE_NONE

  Scenario: Happy path — Restart to update
    Given the offer for "0.2.0" is in state "ready"
    When install_update is called
    Then it returns Ok
    When the program has run its quit flow and calls finish_update(Restart)
    Then the updater's install_and_restart is called and finish_update returns true

  Scenario: Happy path — quitting installs a ready update
    Given the offer for "0.2.0" is in state "ready"
    When the person quits and the program calls finish_update(OnExit)
    Then the updater's install_on_exit is called and the application is not restarted

  Scenario: Edge — quitting with nothing ready
    Given no update offer
    When the program calls finish_update(OnExit)
    Then it returns false and the updater is not called

  Scenario: Error — the install fails
    Given the offer for "0.2.0" is ready and install_and_restart fails
    When finish_update(Restart) is called
    Then it fails with SETUP_UPDATE_INSTALL_FAILED and the offer stays ready

  Scenario: Happy path — What's new once, then in About
    Given setup_whats_new holds notes for "0.2.0" and the application is "0.2.0"
    When whats_new is called
    Then it returns version "0.2.0" with seen false
    When whats_new_seen("0.2.0") is called
    Then whats_new returns version "0.2.0" with seen true

  Scenario: Edge — a fresh install keeps no notes
    Given setup_whats_new has no row for the running version
    When whats_new is called
    Then it returns None

  Scenario: Happy path — the owner updates a personal server
    Given a personal server run by its user unit at "0.4.0" and a feed offering "0.5.0" with a signed archive for its platform
    When run_server_update is called
    Then ordin-hook and ordin-server are replaced, each old file kept as "<name>.previous"
    And the unit's restart is requested and the outcome is Restarting "0.5.0"

  Scenario: Happy path — a paired desktop asks the server to update
    Given a personal server run by its user unit at "0.4.0" and a feed offering "0.5.0"
    When server_update is called
    Then it returns version "0.5.0" with self_update true
    When update_server("0.5.0") is called
    Then it returns Ok and the update runs on a task

  Scenario: Edge — after the restart the update is recorded as done
    Given setup_server_update holds version "0.5.0" in state "restarting" and the server now runs "0.5.0"
    When run_checks(Start) runs
    Then the row's state is "done"

  Scenario: Error — the server commands on the desktop
    Given the program is Desktop
    When server_update is called
    Then it fails with SETUP_SELF_UPDATE_UNAVAILABLE

  Scenario: Error — a server in a container cannot replace itself
    Given a personal server whose run mode is Container and a feed offering "0.5.0"
    When server_update is called
    Then it returns version "0.5.0" with self_update false
    When update_server("0.5.0") is called
    Then it fails with SETUP_SELF_UPDATE_UNAVAILABLE

  Scenario: Error — no archive for this architecture
    Given the feed offers "0.5.0" with no platform entry for the server's feed platform
    When run_server_update is called
    Then it fails with SETUP_UPDATE_NO_ARCHIVE and nothing is replaced

  Scenario: Error — a server archive with a bad signature
    Given the server archive of "0.5.0" fails the build's update key
    When run_server_update is called
    Then it fails with SETUP_UPDATE_SIGNATURE_INVALID and the update folder is deleted

  Scenario: Error — the signed archive is not the version offered
    Given the archive's ordin-server prints version "0.4.9" while the feed offers "0.5.0"
    When run_server_update is called
    Then it fails with SETUP_UPDATE_FEED_INVALID and nothing is replaced

  Scenario: Error — replacing a program fails
    Given renaming the new ordin-server into place fails after ordin-hook was replaced
    When run_server_update is called
    Then ordin-hook is restored from "ordin-hook.previous"
    And it fails with SETUP_UPDATE_REPLACE_FAILED marked retryable

  Scenario: Error — a second update while one runs
    Given setup_server_update holds a "running" row started 5 minutes ago
    When update_server("0.5.0") is called
    Then it fails with SETUP_UPDATE_IN_PROGRESS

  Scenario: Error — a newer version came out meanwhile
    Given the feed now offers "0.5.1"
    When update_server("0.5.0") is called
    Then it fails with SETUP_UPDATE_VERSION_CHANGED

  Scenario: Error — systemd refuses the restart
    Given the programs were replaced and restart_unit fails
    When run_server_update ends
    Then ordin-server and ordin-hook are restored from their .previous copies
    And it fails with SETUP_UPDATE_REPLACE_FAILED and setup.server_update.restart_failed is logged

  Scenario: Error — the archive holds a symbolic link
    Given a signed archive for "0.5.0" whose bin/ordin-hook is a symbolic link
    When run_server_update unpacks it
    Then it fails with SETUP_UPDATE_FEED_INVALID
    And no file is written outside the update folder and the update folder is removed

  Scenario: Error — a download address that is not https
    Given the feed's entry for this architecture has an http address
    When run_server_update reads the feed
    Then it fails with SETUP_UPDATE_FEED_INVALID and nothing is downloaded

  Scenario: Edge — a server never updates unasked
    Given a personal server whose feed offers "0.5.0"
    When the daily feed read runs
    Then the new version is announced once and no program file changes
```

### Performance, Security, Observability (added)

- **Performance:** `whats_new` and `server_update`'s store reads p95 ≤ 10 ms (the feed read is bounded by F.4's 15 s timeout). Downloads run on their own task and never block a command.
- **Security:** every program file a server writes comes from an archive verified with the update key compiled into the build (2D-2b), and the new `ordin-server` must report the offered version before anything is replaced; no administrator rights and no shell are used (ADR-072); renames stay inside the program folder; a desktop download is verified before it is offered as ready. Never logged: download addresses' query strings, signatures, release notes.
- **Observability** (target `ordin_setup`): `setup.update_ready` (info: `version`), `setup.update_download_failed` (warn: `code`), `setup.update_install_requested` (info: `version`), `setup.update_installed` (info: `version`, `how`), `setup.update_install_failed` (warn), `setup.server_update.installed` / `.done` (info: `version`), `setup.server_update.restart_failed` (warn: `reason`), `setup.server_update.failed` (warn: `code`).

### Confirmation items

| Item | Assertion here | Fallback |
|---|---|---|
| C-99 | Tauri's updater format carries Linux archive entries the desktop ignores, and their signatures verify with `minisign-verify` against the update public key. | Without it, `server_update` returns `self_update` false and `update_server` returns `SETUP_UPDATE_NO_ARCHIVE`; the owner installs the archive. |
| C-100 | A user unit's main process can ask systemd to restart its own unit, with lingering on and no administrator. | The programs are restored from `.previous` and `SETUP_UPDATE_REPLACE_FAILED` is returned; the server keeps announcing only until C-100 is confirmed. |

### Gaps

None.
