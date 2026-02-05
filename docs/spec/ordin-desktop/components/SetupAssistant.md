# SPEC: Setup Assistant

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (ADR-072) · amended January 2026 (`bundled_language_servers`, SET-REC-1; Draft, awaiting the user's approval) · amended January 2026 (the highest version started, SET-REC-2, closing OrdinDesktop.md OQ-REC-11; Approved by the user 2026-01-10) · amended January 2026 (Claude Code found outside the short PATH, SET-R1-7 and SET-R1-8; no sentence sends the person to IT, SET-R1-9; user decision) · amended 2026-02-02 (ADR-123: the Copilot CLI is the person's own install; Approved by the user 2026-02-02)
> **Mode:** greenfield
> **derivedFromHld:** 0.66.0
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

The Setup Assistant makes the computer ready without technical steps from the person (02:710–717). On the first start, and on a start after the organisation profile changed what setup requires, it runs the short guided sequence "Getting you ready": read the company's settings, check the harness the profile names (Claude Code first, Copilot as the alternative) and start the harness's own sign-in only when it is signed out, have the Catalogue Service clone the profile's agent repositories, and check that every program ordin brings starts (03:12–57; ADR-044, ADR-072). On every start, in the background and on demand it repeats those checks and repairs what it can without asking: a stopped CLI server, an expiring token, a damaged clone, a damaged agent folder. When a repair fails it gives one plain sentence and one action; it offers no help request to anyone (plan 05 FU-019: **Send to IT** was removed). It owns setup state, the results of checks and the harness attachments (02:716 amended), and the effective update feed address (2D-2, 2D-2b).

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
pub struct HelpSection { pub destination: Option<HelpDestination> }        // document.help; kept so a profile with a help section loads; read by nothing (SET-R1-1, removed)
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
| Copilot on the server | not offered until C-48 is confirmed (`COPILOT_ON_SERVER = false`) |

### Words shown to the person (owned here; quoted from 11 unless marked †)

| Situation | Sentence | Action |
|---|---|---|
| Guided sequence header | "Getting you ready… {done} of {total} done" | — |
| Already signed in | "Already signed in through Claude Code" | — |
| Signed out | (button) "Sign in to Claude" / "Sign in with GitHub to use Copilot" | Sign in |
| Company settings unreachable | "ordin can't reach your company's settings." † | (retries in the background) |
| No Claude access | "You don't have Claude access yet." (*amended January 2026 (user decision; Approved by the user January 2026)*: no action asks anyone; SET-R1-9) | — |
| No Copilot seat | "You don't have Copilot access yet." (*amended January 2026 (user decision; Approved by the user January 2026)*: no action asks anyone; SET-R1-9) | — |
| Harness missing | "Claude Code isn't on this computer yet. Install it, then choose Check again." (*amended January 2026 (user decision; Approved by the user January 2026)*, SET-R1-9) | Install Claude Code, Check again (ScreensModule.md SM-FS-9) |
| Harness version not tested † | "This version of Claude Code hasn't been checked with ordin yet. Install a checked version, then try again." (*amended January 2026 (user decision; Approved by the user January 2026)*, SET-R1-9) | — |
| Program stopped by company settings | "Your company's settings stopped {program} from opening on this computer." † followed by what is off ("You can still use everything else; images open in your usual app.") (*amended January 2026 (user decision; Approved by the user January 2026)*, SET-R1-9) | — |
| Agent library unreachable | "Can't reach the company's agent library right now" | Try again |
| Server harness sign-in ended | "Your Claude Code sign-in on the server has ended, so new work waits. Ask whoever runs your ordin server to sign in again." | — |
| Terms under the steps | "To do the work, Ordin sends what it reads to [Claude or Copilot]. Your documents stay where they live." | What Ordin reads and sends — read the terms |

### Confirmation items carried as conditional

| Item | Assertion here | Fallback the HLD gives |
|---|---|---|
| C-25 | `copilot login` started with no terminal completes through the browser (Claude half passed). | The person signs in from the Copilot CLI itself; the step shows "Sign in with GitHub to use Copilot" and re-checks status until signed in (ADR-044). |
| C-46 | Per-user install and update of ordin, and a vendor harness installer for the current user, with no administrator prompt. | A harness that is not on the computer is installed by the person: "Claude Code isn't on this computer yet. Install it, then choose Check again." with **Install Claude Code** and **Check again** (*amended January 2026 (user decision; Approved by the user January 2026)*, SET-R1-9). ordin itself does not install the harness in this release (ADR-044). |
| C-47 | Pinned ArtCraft apps are signed and start under the data root without a system prompt. | A refused app is named in one sentence that says to check the computer's security settings (program name, publisher and hash are recorded for Diagnostics); only its features are off (UX-A46; *amended January 2026 (user decision; Approved by the user January 2026)*, SET-R1-9). |
| C-58 | Telegram's per-user install is found without reading its data. | Telegram tools are not offered on the desktop; no step depends on it here. Detection is by the `tg:` link handler (C-59) and belongs to the Telegram Service; this component reports nothing for Telegram. |
| C-48 | Copilot device sign-in on a Linux server. | The server offers Claude only. |

### Reconciliation assumptions

- **SA-RECON-01** This crate owns the update check (ruling R16): when to check, the offer and its words; the desktop program supplies Tauri's updater through `AppUpdater`. A verified profile's `updates.feed_url` replaces the installer's (D10).
- **SA-RECON-02** The setup tables live in the Journal's datastore through a Journal edge (D4, R20); the crate exports `migrations()` and receives the `Datastore`, not a pool.
- **SA-RECON-03** The Service API catalogue takes this crate's `setup.*` commands and the `setup_progress` payload as written (R1, R6); a harness sign-in is `setup.sign_in_harness`, never an Identity command (rows 689–694).
- **SA-RECON-04** The program wraps the Copilot Local Adapter's `CopilotServer` in a `HarnessProbe` and registers it; the adapter itself is unchanged (row 700, D3).
- **SA-RECON-05** The Project Service's `ServerInstaller` is implemented by the program over `ensure_language_server` (row 701).
- **SA-CLOSE-01** *(K12, chosen here; closes SA-G-LSP)* A language server's install recipe is data, not code: the organisation profile's `language_servers[]` entry for the language (`{ language, name, version, platforms: { <platform>: { asset_url, sha256, file_name, binary_path } } }`, OrganisationProfile.md SA-CLOSE-03), or, without one, the entry of the same shape in the table compiled into this crate (`language-servers.json`, embedded with `include_str!`, pinned at release: gopls and rust-analyzer per ADR-056, `jdtls` 1.63.0 and TypeScript's servers per 05). Install = download `asset_url`, verify SHA-256, unpack by `file_name` with the archive libraries 05 already names (`zip`; `tar` with `flate2`), into `<data root>/language-servers/<name>/<version>/`; the server runs as `<that folder>/<binary_path>`. The table pins archives that run without a separately installed runtime (for `jdtls` and the TypeScript servers, archives that carry their own Java or Node.js runtime, assembled by the release build). The table in the repository is `[]`: the asset address and SHA-256 of each archive are release-build data that no document records, so until the release build fills the table a language with no profile entry is refused with `SETUP_LANGUAGE_SERVER_NOT_APPROVED`.
- **SA-CLOSE-02** *(chosen here)* `<data root>` is the folder of the Journal's SQLite file (`<application data root>`, JournalAndArtefactStore.md *Configuration*), read from the `Datastore` given to `SetupAssistant::new`. `SetupConfig` carries no data-root field.
- **SA-RECON-06** *(removed January 2026, plan 05 FU-019)* There is no **Send to IT**: the help request and its mail-app fallback are gone (SET-R1-1, removed). The id stays so earlier citations resolve.
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
pub enum PersonAction { TryAgain, SignInAgain, SignIn }

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
| `setup.update_offer` | `{}` | `{"offer": UpdateOffer?}` | `update_offer` (desktop) |
| `setup.install_update` | `{}` | `{}` | `install_update` (desktop) |

Event `setup_progress { progress: SetupProgress }` (2C `EventKind::SetupProgress`), emitted by the program from `progress()` through `Journal::append_event`. These commands and this payload are the definitive ones; the Service API catalogue takes them as written (R1, R6): `setup.status` returns the object above (not `GettingReady`); retry is `setup.check_now` (there is no per-step `setup.retry`); there is no help command (plan 05 FU-019); a Claude or Copilot sign-in is `setup.sign_in_harness`, never an Identity command. The ServiceApi names its event variant so it does not clash with this crate's `SetupProgress` struct.

### Who calls each public item (reconciliation §3; R6)

| Item | Called by |
|---|---|
| `new`, `register_probe`, `register_updater`, `start_background_checks`, `close` | desktop program (registers the Copilot `HarnessProbe` it wraps around the Copilot Local Adapter's `CopilotServer`, and the Tauri `AppUpdater`); server program (no updater) |
| `needs_guided_setup`, `run_guided`, `run_checks(OnDemand)`, `sign_in_harness`, `harness_attachments`, `progress`, `update_offer`, `install_update` | the programs, for the `setup.*` commands |
| `run_checks(Start)` | the programs at start, before `start_background_checks` |
| `run_checks(BeforeSession)` | server program, before each harness session (2D-4) |
| `ready_harness` | desktop program, in its `SessionSetupSource` implementation (it passes `ready_harness().location` as the Claude Local Adapter's `claude_path`) |
| `effective_update_feed` | this crate (step F); the server program for its announcement through `check_update_feed` |
| `check_update_feed` | server program, once a day *(chosen here)*, announcing a newer version once |
| `ensure_language_server` | Project Service, through the program's `ServerInstaller` (`ensure(language, &ServerPin { name, version })` → `ensure_language_server(language, &name, &version)`) |
| `migrations` | the programs, registering it with the Journal's `MigrationRegistry` |

*(Amended 2026-02-02 by SET-CPX-1: the Copilot CLI is not a bundled program.)* The bundled programs the desktop program lists in `SetupConfig.bundled_programs` include the ordin hook (`ordin-hook`, `version_args = ["--version"]`) and the Copilot CLI; step D.3 is therefore where the hook's version run happens.

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
   "message_args":{"program":"PhotoCraft","off":"images open in your usual app"},
   "refused_program":{"name":"PhotoCraft","publisher":"ArtCraft","sha256":"9b1f…64 hex…"},"checked_at":"2026-01-10T08:00:06.400Z"}]}
```

## Internal Logic

### A. Guided sequence (`run_guided`)

1. Take the run lock (one guided run or check run at a time; a second caller awaits the running one and receives its report). Compute the steps that apply: `CompanySettings`; `Harness`; `HarnessSignIn` (counted only once `Harness` found the harness signed out, so the count reflects real steps, 11 §5.1); `AgentRepositories` when the profile names at least one; `Programs` and `CraftApps` when `bundled_programs` or `craft.apps` is non-empty. Publish `SetupProgress { running: true, done: 0, total }`.
2. **CompanySettings.** Read `profile.status()`. `NoAddress` (desktop installer carried none) → result `Failed`, code `SETUP_NO_PROFILE_ADDRESS`, message "ordin can't reach your company's settings." †, action Send to IT; the sequence continues with defaults (A-40 on the server). `Active` → `Passed`. Otherwise call `profile.refresh()`; success → `Passed`; failure → `Failed` with `SETUP_PROFILE_UNREACHABLE`, no action, a background retry every 60 minutes (the Organisation Profile's own refresh) and the sequence continues with the last verified profile or defaults (03:55).
3. **Harness.** Walk `document.harnesses` in order (default `[claude_code]`), skipping `copilot` on the server (C-48). For the first harness with a registered probe: `locate()` (D.1). Not found → `Failed`, `SETUP_HARNESS_MISSING`, "Claude Code isn't on this computer yet. Install it, then choose Check again.", no action (*amended January 2026 (user decision; Approved by the user January 2026)*, SET-R1-9); try the next listed harness; if none is found, the sequence ends with `ready = false`. Found → write the attachment (`found`, then version → `checked`); `tested` = `tested_versions` empty or containing the version exactly *(assumed here, SA-SETUP-02)*. Untested → state `needs_update`, `Failed`, `SETUP_HARNESS_UNTESTED`, no action (*amended January 2026 (user decision; Approved by the user January 2026)*, SET-R1-9), then the next listed harness is tried.
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

1. **Claude Code probe (`ClaudeCodeProbe`).** All commands run without a shell, with an environment built explicitly: `PATH`, `HOME` (`USERPROFILE` on Windows), `USER` (`USERNAME` on Windows; SET-R1-2), `LANG`, and nothing else, so `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN` and other credential variables never reach them (2D-1c).
   - `locate`: search each `PATH` entry for `claude` (`claude.exe`, `claude.cmd` on Windows) *(assumed here, SA-SETUP-03)*, then the person's own folders of SET-R1-7 (*amended January 2026 (user decision; Approved by the user January 2026)*); the commands run with the folder `claude` was found in on their `PATH` (SET-R1-8); run `claude --version`; version = first whitespace-separated token matching `^[0-9]+\.[0-9]+\.[0-9]+`; no match → `Unreadable`.
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
| Installer carried no profile address | Err(permanent) in the result | `SETUP_NO_PROFILE_ADDRESS` | `{"code":…}` |
| `new` given a PostgreSQL datastore | Err(permanent); start stops (program defect) | `SETUP_STORE_UNSUPPORTED` | log only |
| Update feed unreachable | Err(retryable); logged, no pop-up | `SETUP_UPDATE_UNREACHABLE` | `{"code":…,"action":"try_again"}` |
| Update signature refused by the build's update key | Err(permanent); offer cleared | `SETUP_UPDATE_SIGNATURE_INVALID` | `{"code":…}` |
| Server feed has no readable `version` | Err(permanent) | `SETUP_UPDATE_FEED_INVALID` | log only (server) |
| `install_update` with no offer | Err(permanent) | `SETUP_UPDATE_NONE` | `{"code":…,"action":null}` |
| Installing the update failed | Err(retryable) | `SETUP_UPDATE_INSTALL_FAILED` | `{"code":…,"action":"try_again"}` |
| Language server not approved by the profile | Err(permanent) | `SETUP_LANGUAGE_SERVER_NOT_APPROVED` | `{"code":…}` |
| Language server install failed (download, unpack, missing binary) | Err(retryable), or Err(permanent) for an archive breaking the unpack limits or lacking `binary_path` | `SETUP_LANGUAGE_SERVER_INSTALL_FAILED` | `{"code":…,"action":"try_again"}` (retryable) / `{"code":…}` (permanent) |
| Downloaded language server's SHA-256 differs from the pinned one | Err(permanent) | `SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH` | `{"code":…}` |
| No download for this computer's platform | Err(permanent) | `SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED` | `{"code":…,"action":null}` |
| Profile cannot be fetched or verified | Err(retryable) in the result | `SETUP_PROFILE_UNREACHABLE` | `{"code":…,"action":null}` (background retry) |
| No listed harness found | Err(permanent) in the result | `SETUP_HARNESS_MISSING` | `{"code":…}` |
| Harness version not in `tested_versions` | Err(permanent) in the result | `SETUP_HARNESS_UNTESTED` | `{"code":…}` |
| Harness reports no access / no seat | Err(permanent) | `SETUP_NO_HARNESS_ACCESS` | `{"code":…}` (words: Ask IT for Claude / Copilot access) |
| Harness sign-in not finished in 10 min | Err(retryable) | `SETUP_SIGN_IN_TIMEOUT` | `{"code":…,"action":"try_again"}` |
| Harness sign-in asked on the server | Err(permanent) | `SETUP_SIGN_IN_ON_SERVER` | `{"code":…,"action":null}` |
| No probe registered for the harness | Err(permanent) | `SETUP_HARNESS_UNKNOWN` | — (wiring defect) |
| No ready harness for a session | Err(permanent) | `SETUP_NO_READY_HARNESS` | `{"code":…,"action":"sign_in_again"}` |
| Harness output unreadable / command timed out | Err(retryable) in the result | `SETUP_HARNESS_CHECK_FAILED` | `{"code":…,"action":"try_again"}` |
| Agent repository clone or fetch failed | Err(retryable) in the result | `SETUP_CATALOGUE_FAILED` | `{"code":…,"action":"try_again"}` |
| Agent folder could not be re-installed | Err(permanent) in the result | `SETUP_AGENT_FOLDER_FAILED` | `{"code":…}` |
| Bundled program missing or damaged, restore failed | Err(permanent) in the result | `SETUP_PROGRAM_DAMAGED` | `{"code":…}` |
| Program refused by the operating system | Err(permanent) in the result | `SETUP_PROGRAM_REFUSED` | `{"code":…}` |
| Program exited non-zero or timed out | Err(retryable) in the result | `SETUP_PROGRAM_FAILED` | `{"code":…,"action":"try_again"}` |
| ArtCraft app missing after repair | Err(retryable) in the result | `SETUP_CRAFT_APP_FAILED` | `{"code":…,"action":"try_again"}` |
| A token needs a new sign-in | Err(permanent) in the result | `CRED_SIGN_IN_REQUIRED` (from Identity) | `{"code":…,"action":"sign_in_again"}` |
| Database write failed | Err(retryable) | `SETUP_STORE_FAILED` | `{"code":…,"action":"try_again"}` |
| `harness_attachments` rule broken (2C) | Err(permanent) | `HARN_DUPLICATE`, `HARN_PATH_INVALID`, `HARN_VERSION_INVALID`, `HARN_SIGN_IN_METHOD_INVALID`, `HARN_STATE_INCONSISTENT` | — (emitter defect) |

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
    Then the attachment state is "needs_update" and the result code is SETUP_HARNESS_UNTESTED and no action

  Scenario: Error — harness missing
    Given no claude executable on PATH or in the person's own folders (SET-R1-7) and no Copilot probe registered
    When run_guided runs
    Then the result code is SETUP_HARNESS_MISSING and the sentence is "Claude Code isn't on this computer yet. Install it, then choose Check again."
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
    Then the folder <data root>/language-servers/rust-analyzer/2026-01-10/ holds the binary and a ready marker
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
  - Entry points: in-process calls and the Service API commands (Tauri capabilities on the desktop; Remote API desktop key on the server). Command inputs deserialise with unknown fields refused (SA-API-02).
  - Every child process is started without a shell, with an explicit minimal environment, so no credential variable reaches a harness command (2D-1c); arguments are fixed, never built from input.
  - Never asks for administrator rights; never runs a machine-wide installer; never works round a refusal (ADR-072).
  - ordin never reads, stores or relays a harness credential (ADR-044); `sign_in_method` is the harness's own free text.
- **Observability:** `tracing` JSON lines only (desktop, personal server; SA-XC-04/05). Target `ordin_setup`:
  - `setup.guided.started` / `setup.guided.completed` (info): `total`, `ready`, `duration_ms`.
  - `setup.check` (info on change, debug otherwise): `check`, `outcome`, `code`, `trigger`.
  - `setup.harness` (info): `harness`, `version`, `tested`, `signed_in`, `state`.
  - `setup.program_refused` (warn): `program`, `publisher`, `sha256`.
  - `setup.repair` (info): `check`, `outcome`.
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
18. **SET-UP-18 Verify.** Verify the downloaded file against the entry's signature with the build's update public key (2D-2b), using `minisign-verify` (*chosen here*; both decoded as Tauri's updater writes them, C-99). Failure → `SETUP_UPDATE_SIGNATURE_INVALID`; the folder is deleted; log `warn`. **Where the key comes from** (*chosen here*, 2026-01-10): the maintainer's update public key is a public key file committed in the repository and compiled into `ordin-setup` with `include_str!`, so every build carries it and nothing reads it at run time. Until the maintainer commits that file, the build carries no update key, and every server archive is refused here with `SETUP_UPDATE_SIGNATURE_INVALID` (fail closed): a personal server then keeps announcing new versions, and its owner installs them.
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
| Server archive signature refused | Err(permanent) | `SETUP_UPDATE_SIGNATURE_INVALID` | `{"code":…}` |
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

None open in this specification. One input is still to be supplied: the committed update public key file of SET-UP-18. Until the maintainer commits it, server archives are always refused with `SETUP_UPDATE_SIGNATURE_INVALID`; the code keeps a `SPEC-GAP-4` marker at the place the key is read.


## Amendment January 2026 — The install choice (ADR-072)

> Source: decision 95 (the user's decisions of January 2026); HLD revision 0.64.0, ADR-072 as amended. A person may install ordin for everyone on the computer. Such an install cannot write its own folder, so it never downloads an update and tells the person to ask their administrator. Every section above stays in force except where this amendment states otherwise.

### Public Interface (changed and added)

```rust
/// Where this copy of the program is installed, found once at start by the desktop program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind { PerUser, WholeComputer }

/// Replaces the two-value enum above. `AdministratorNeeded`: a newer version exists and this install
/// cannot take it; nothing was downloaded.
pub enum UpdateState { Downloading, Ready, AdministratorNeeded }

pub trait AppUpdater: Send + Sync {
    // The four methods above stay. Added:
    /// Fixed for the life of the process.
    fn install_kind(&self) -> InstallKind;
    /// The other install on this computer that this account can start, if there is one.
    fn other_install(&self) -> Option<std::path::PathBuf>;
}

// `CheckKind` gains `SecondInstall`, after `CraftApps` (wire form `second_install`).

/// A command-line tool that ordin starts from the computer's `PATH` and that is missing from it (SET-UP-28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MissingTool {
    /// The command, `gh` or `glab`.
    pub command: String,
    /// What to install, in plain words: "GitHub CLI" or "GitLab CLI".
    pub install: String,
}
```

### Internal Logic (added to section F)

25. **SET-UP-25 Whole-computer offer.** In SET-UP-3, when `updater.install_kind()` is `WholeComputer`, the kept offer is `UpdateOffer { version, notes, state: AdministratorNeeded }` and SET-UP-4 does not run. In one transaction write `setup_whats_new(version, notes, kept_at)` as SET-UP-4 does, so that **What's new** shows once for this account after an administrator installs the version; an account that never read this offer has no row, as with a fresh installer. Log `setup.update_offered` (`version`). No download is attempted, so no password is asked for.
26. **SET-UP-26 No install.** `install_update` with an offer in state `AdministratorNeeded` → `SETUP_UPDATE_NONE`. `finish_update` already returns `Ok(false)` because no offer is `Ready`.
27. **SET-UP-27 Second install.** `run_checks(Start)` on the desktop, last, with an `AppUpdater` registered: `updater.other_install()` is `Some(path)` → a `CheckResult { check: SecondInstall, outcome: Failed, code: Some("SETUP_SECOND_INSTALL"), message_id: Some("setup-second-install"), message_args: { "folder": <the folder holding the other copy> }, action: None }`. `None` → `Passed`. This check turns nothing off, repairs nothing and removes nothing. Any other trigger does not run it. The result is returned in the `SetupReport` and is not written to `setup_checks`: that table's `check_kind` constraint (migration step 1) lists the eight earlier kinds and this amendment adds no migration step, so `second_install` is not an allowed value there.
28. **SET-UP-28 Command-line tools on `PATH`.** `SetupAssistant::missing_path_tools(&self) -> Vec<MissingTool>` reads the process's `PATH` and returns, in this order, `MissingTool { command: "gh", install: "GitHub CLI" }` when no executable file `gh` (`gh.exe` on Windows) is in any `PATH` entry, then `MissingTool { command: "glab", install: "GitLab CLI" }` on the same test for `glab`; empty when both are present. It installs, changes and removes nothing, adds no `CheckKind` and writes nothing to `setup_checks`. The program shows the result at start (ADR-072 as amended; the tools are not yet placed under the data root).

### Error Table (added and changed rows)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `install_update` with no offer, with the offer still downloading, or with an offer in state `AdministratorNeeded` (changed) | Err(permanent) | `SETUP_UPDATE_NONE` | `{"code":…,"action":null}` |
| A second install of ordin on this computer (a check result, never an error returned) | `CheckResult` `Failed` | `SETUP_SECOND_INSTALL` | message `setup-second-install` with `folder`; `action` null |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a whole-computer install is told to ask its administrator
    Given an AppUpdater whose install kind is WholeComputer and a feed offering "0.2.0" while the application is "0.1.0"
    When check_update is called
    Then the offer for "0.2.0" has state administrator_needed
    And updater.download is not called
    And setup_whats_new holds the notes for "0.2.0"

  Scenario: Error — a whole-computer install cannot install an update
    Given an offer for "0.2.0" in state administrator_needed
    When install_update is called
    Then it fails with SETUP_UPDATE_NONE

  Scenario: Happy path — What's new shows after an administrator updates
    Given setup_whats_new holds notes for "0.2.0" kept by a whole-computer offer and the application is now "0.2.0"
    When whats_new is called
    Then it returns version "0.2.0" with seen false

  Scenario: Edge — two installs on one account
    Given an AppUpdater whose other_install names "/Applications/ordin.app"
    When run_checks(Start) runs
    Then the SecondInstall result is Failed with code SETUP_SECOND_INSTALL and folder "/Applications"
    And no other check changes and no file is removed

  Scenario: Edge — one install on one account
    Given an AppUpdater whose other_install is None
    When run_checks(Start) runs
    Then the SecondInstall result is Passed
```

### Gaps

None.

## Amendment January 2026 — The build's language-server table, readable by the program

> **Status:** Approved (user, 2026-01-10). Counterpart: `components/OrdinDesktop.md`, amendment January 2026 — Phase 11 reconciliation (DSK-REC-11; SPEC-GAP-12).

- **SET-REC-1** The Project Service's `ProjectSettings.language_servers` falls back to "the build's `language-servers.json` table" (ProjectService.md SA-CLOSE-02), which this crate embeds privately (SA-CLOSE-01). The program builds those settings at row 3.31, before this crate is constructed (row 3.43), so the table is read through a free function:

```rust
/// The build's language-server table (SA-CLOSE-01), parsed; empty when the embedded table is
/// empty or does not parse.
pub fn bundled_language_servers() -> Vec<LanguageServerEntry>;
```

  `LanguageServerEntry` is the Organisation Profile's type, the shape the table already has. The function reads the same embedded table as `ensure_language_server`, so both see the same entries.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the program reads the build's table
    Given language-servers.json pins rust-analyzer "2026-01-10"
    When bundled_language_servers is called
    Then it returns that entry

  Scenario: Edge case — the table in the repository is empty
    Given language-servers.json is []
    When bundled_language_servers is called
    Then it returns an empty list
```

### Gaps

None.

## Amendment January 2026 — The highest version started (closes OrdinDesktop.md OQ-REC-11)

> **Status:** Approved by the user 2026-01-10 ("proceed with recommendations", option (a) of OQ-REC-11). Every section above stays in force except where SET-REC-2 names what it replaces. Counterpart: `components/OrdinDesktop.md` DSK-REC-44 (amends DSK-UP-14).
>
> Evidence read: SET-UP-3 (semantic-version order), SET-UP-4, SET-UP-8 and SET-UP-9 above; the *Data Model* of the ADR-114 amendment (`setup_whats_new`, migration step 2 of module `setup`); OrdinDesktop.md DSK-UP-14 and OQ-REC-11.

### Public Interface (added)

```rust
impl SetupAssistant {
    /// Called by the desktop program once at start, after row 3.43 (OrdinDesktop.md DSK-REC-44).
    /// Answers the highest version recorded as started before this call (`None` on the first start),
    /// and records the running version when it is higher.
    pub async fn record_started(&self) -> Result<Option<String>, SetupError>;
}
```

### Internal Logic

**SET-REC-2 The highest version started.**
1. `record_started`: read `setup_started`. No row, or a stored `highest_version` that does not parse as a semantic version → the answer is `None` and the running version (`application_version`) is written. A stored version lower than the running one by semantic-version order (SET-UP-3) → the answer is the stored version and the running version is written. Equal or higher → the answer is the stored version and nothing is written. The write replaces the one row with `recorded_at` now.
2. **What's new after a downgrade (amends SET-UP-8).** `whats_new` answers `None` when `setup_started` holds a version higher than `application_version`, whatever `setup_whats_new` holds; otherwise as SET-UP-8 states. So **What's new** is not shown for a lower version until a version higher than the highest started runs.
3. A failed read or write is `SETUP_STORE_FAILED` (retryable), as SET-UP-9.

### Data Model (added)

Migration step 3 of module `setup`, SQLite only, as the steps above:

```sql
-- chosen here: the highest application version started on this data root, one row
CREATE TABLE setup_started (
    singleton        INTEGER PRIMARY KEY CHECK (singleton = 1),
    highest_version  TEXT NOT NULL CHECK (length(highest_version) BETWEEN 1 AND 64),
    recorded_at      TEXT NOT NULL
);
```

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the first start records the running version (SET-REC-2)
    Given setup_started has no row and the application is "0.2.0"
    When record_started is called
    Then it answers None and setup_started holds "0.2.0"

  Scenario: Happy path — a newer version replaces the record (SET-REC-2)
    Given setup_started holds "0.2.0" and the application is "0.3.0"
    When record_started is called
    Then it answers "0.2.0" and setup_started holds "0.3.0"

  Scenario: Edge case — a lower version keeps the record and hides What's new (SET-REC-2)
    Given setup_started holds "0.3.0", setup_whats_new holds notes for "0.2.0" and the application is "0.2.0"
    When record_started and then whats_new are called
    Then record_started answers "0.3.0", setup_started still holds "0.3.0" and whats_new answers None
```

### Gaps

None.

## Amendment January 2026 — The help request in the journal (D-5), removed

> **Status:** Approved by the user, January 2026 ("close all the follow ups in plan 5"; "Send to IT … should be removed", "yes" to removing it from the whole product). Source: plan 05 FU-019. Counterpart: `components/ServiceApi.md`, SVC-CL-3.

- **SET-R1-1 (removed)** The Setup Assistant has no help request: `prepare_help_request`, `send_help_request`, the types `HelpPreviewId`, `HelpRequestInput`, `HelpRequestBody`, `HelpRequestPreview`, `HelpDestinationView`, `HelpRequestOutcome` and `HelpRequestRecord`, the commands `setup.help_preview` and `setup.help_send`, the journal kind `help_request_sent`, the codes `SETUP_HELP_ADDRESS_INVALID`, `SETUP_HELP_DESTINATION_INVALID`, `SETUP_HELP_DESTINATION_MISSING`, `SETUP_HELP_INPUT_INVALID`, `SETUP_HELP_MAIL_APP_FAILED`, `SETUP_HELP_PREVIEW_UNKNOWN`, `SETUP_HELP_REFUSED` and `SETUP_HELP_SEND_FAILED`, the actions `send_to_it`, `ask_it_for_claude_access` and `ask_it_for_copilot_access` (a result that had one now has none; its sentence says what to do), and `SetupConfig.url_opener` (the desktop program still passes its opener to what draws links, not to the Setup Assistant) do not exist. The profile's `help` section is still parsed so a signed profile that has one loads; nothing reads it (OrganisationProfile.md).

## Amendment January 2026 — The account name in the Claude Code probe

> **Status:** Approved by the user 2026-01-28 ("proceed with recommendations, claude code is signed in this mac"). Found by ScreensModule's first start in the built program (FU-154).

- **SET-R1-2** D.1's environment also carries `USER` (`USERNAME` on Windows), read from the program's own environment and passed on when set. It is the account name, not a credential. Claude Code keeps its sign-in in the macOS Keychain under that name, so without it `claude auth status --json` answers `"loggedIn": false` for a person who is signed in, and the guided run asks for a sign-in nobody needs. The Claude adapter's session environment already carries it (ClaudeLocalAdapter.md).

```gherkin
  Scenario: Happy path — a signed-in Claude Code is found signed in (SET-R1-2)
    Given Claude Code is signed in for the person's account
    When the guided run reaches HarnessSignIn
    Then sign_in_status is SignedIn and nothing is asked
```

- **SET-R1-3** `ProgressStep` gains `problem: Option<StepProblem>`, where `StepProblem { message_id: String, message_args: BTreeMap<String, String> }` is the failed `CheckResult`'s message and arguments. When A's runner ends a step with `outcome = Failed`, it sets the step's `problem` from that result before publishing the progress; every other step has `problem = None` (omitted from the JSON). The `setup_progress` event and `setup.status` carry it unchanged (ServiceApi.md, `ProgressStep`). Found by ScreensModule SM-FS-7: during the run a failed step showed a mark with no reason.

```gherkin
  Scenario: Edge case — a failed step says why while the run goes on (SET-R1-3)
    Given the organisation profile has no address
    When the guided run ends the CompanySettings step
    Then the published progress shows that step failed with problem message_id "setup-no-profile-address"
    And the next steps are still running or waiting
```


## Amendment 2026-01-29 — Background checks after close

> **Status:** Approved 2026-01-29 (user decision: complete the high and medium follow-ups). Source: the desktop now calls `start_background_checks` when its start checks end, on a task start does not wait for (`components/OrdinDesktop.md` DSK-SC-1), so the call can arrive after the program's quit has already called `close`. Every section above stays in force.

- **SET-SC-1 No timer after `close`.** `close` records that the assistant is closed even when no timer is running. A `start_background_checks` call made after `close` starts no timer and returns at once; it counts as the one call, so a later call does nothing either. A timer started before `close` ends at `close`, as before.

```gherkin
  Scenario: Edge — background checks are not started after close (SET-SC-1)
    Given close has been called
    When start_background_checks is called
    Then no background timer is started
```

## Amendment 2026-01-31 — Claude Code is checked with Windows' own folders, and a failed check says why

> **Status:** Approved 2026-01-31 (user decision: "carry on with the fixes"). Source: the walk through the built program on the Windows build machine. D.1 (Internal Logic, Claude Code probe) builds the environment from `PATH`, `HOME` (`USERPROFILE` on Windows), `USER` (`USERNAME`), `LANG` "and nothing else". On Windows that leaves out `SystemRoot`, without which neither Node.js nor the command interpreter starts: with Claude Code 2.1.316 installed, `claude --version` run with exactly D.1's environment ended with status 1 and no output, and with `SystemRoot` added it printed `2.1.316 (Claude Code)`. So the guided run showed "Claude Code couldn't be checked" on every Windows computer, and the log said nothing about why. The Claude adapter's own session environment already carries Windows' folders (ClaudeLocalAdapter.md; `crates/ordin-claude-adapter/src/env.rs`, `WINDOWS_ONLY`). Every section above stays in force.

- **SET-R1-4 Windows' own folders.** On Windows, D.1's environment also carries `APPDATA`, `LOCALAPPDATA`, `SystemRoot` and `TEMP`, each read from the program's own environment and passed on when set. None is a credential; the credential variables of 2D-1c still never reach the probe. On macOS D.1 is unchanged.
- **SET-R1-5 A failed check says why.** When a probe step (`locate`, `sign_in_status`) fails for a reason other than "not found" or "refused", the result is `SETUP_HARNESS_CHECK_FAILED` as before, and the line `setup.harness_check_failed` is logged at `warn` with `harness`, `step`, `reason` (the probe error's words: timed out, unreadable output, not offered here) and `code`; never the program's output.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — Claude Code is found working on Windows (SET-R1-4)
    Given Windows and Claude Code installed on PATH
    When the guided run reaches the Harness step
    Then the step passes and the attachment holds Claude Code's version

  Scenario: Error path — a check that cannot be done is logged (SET-R1-5)
    Given a Claude Code whose `--version` prints nothing readable
    When the guided run reaches the Harness step
    Then the step fails with SETUP_HARNESS_CHECK_FAILED
    And setup.harness_check_failed is logged at warn with step "locate" and reason "unreadable output"
```

### Gaps

None.

## Amendment 2026-01-31 — An install with no company profile skips the company step

> **Status:** Approved 2026-01-31 (user decision: "agree with all recommendations, fix them"; the person's rule that nothing in ordin waits on IT). Source: the walk through the built program on Windows. Internal Logic A.2 fails the CompanySettings step with `SETUP_NO_PROFILE_ADDRESS`, "ordin can't reach your company's settings.", and **Send to IT** when the desktop installer carries no profile address. A build of ordin that no company has branded carries none, so every person met a failed step and an IT action on the first screen. Every section above stays in force.

- **SET-R1-6 No address, no company profile.** In A.2, `ProfileState::NoAddress` is read as `NotUsed`: the CompanySettings result is `Skipped`, with no code, no message and no action, as for a computer with no company profile. A profile address that is set and cannot be reached still fails with `SETUP_PROFILE_UNREACHABLE` as before. `SETUP_NO_PROFILE_ADDRESS` stays in the error registry and is no longer produced by this step.

```gherkin
  Scenario: Happy path — no company profile address, no failure (SET-R1-6)
    Given the installer carried no profile address
    When the guided run reaches the CompanySettings step
    Then the step is skipped with no message and no Send to IT

  Scenario: Error path — a company profile address that cannot be reached (SET-R1-6)
    Given the installer carried a profile address that cannot be reached
    When the guided run reaches the CompanySettings step
    Then the step fails with "Your company's settings can't be reached just now. ordin goes on with the last ones it has."
```

### Gaps

None.

## Amendment January 2026 — Claude Code is found outside the short PATH, and no sentence sends the person to IT

> **Status:** amended January 2026 (user decision; Approved by the user January 2026). Source: on the person's Mac, Claude Code is at `~/.local/bin/claude`; ordin opened from Finder or the Dock gets the system's short `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`), so the guided run said "Didn't work: Checking Claude Code is on this computer" for a Claude Code that works in the person's terminal. D.1 `locate` searched `PATH` only (SA-SETUP-03), while the desktop program already looked in the person's own folders for the Claude adapter's `claude` (OrdinDesktop.md OQ-REC-4, option (b)). Separately, the person's rule is that every feature is self-service: no sentence tells the person to ask IT, tell IT or wait for IT. Every section above stays in force except where this amendment changes it.

- **SET-R1-7 Where `claude` is looked for.** `locate` searches each `PATH` entry, in order, then the person's own folders, in order: `<home>/.local/bin`, where `<home>` is `HOME` (`USERPROFILE` on Windows) of the program's own environment, and on macOS also `/opt/homebrew/bin` and `/usr/local/bin`. The first executable file named `claude` (`claude.exe`, `claude.cmd` on Windows) is the one used. The list is kept in this crate only: `ordin_setup::find_claude_code(path, home)` answers the same file, and the desktop program uses it to find the Claude adapter's `claude_path` (OrdinDesktop.md OQ-REC-4), so the two never disagree.
- **SET-R1-8 The folder `claude` was found in is on its `PATH`.** Every command the probe runs (`claude --version`, `claude auth status --json`, `claude auth login`) gets D.1's environment with that folder added at the end of `PATH` when no entry is already that folder; when the program's `PATH` is absent, `PATH` is that folder alone. A Claude Code installed by a package manager starts its runtime from beside itself, which is found this way. No other variable changes; no credential variable reaches the probe (2D-1c).
- **SET-R1-9 Sentences say what the person can do.** No sentence of the Setup Assistant or of any catalogue message the person reads tells them to ask, tell or wait for IT. A sentence says what the person can do: install it, try again, reinstall ordin, check the computer's security settings, open the relevant setting, or choose **Save logs for support…** in Settings › Diagnostics when something is truly broken. Where the rule comes from the organisation's policy, the sentence says so without IT ("Your organisation's rules don't allow this agent.") and says what the person can do instead. The results above change their words only: `SETUP_HARNESS_MISSING` "Claude Code isn't on this computer yet. Install it, then choose Check again." (first start draws **Install Claude Code** and **Check again**, ScreensModule.md SM-FS-9); `SETUP_HARNESS_UNTESTED` "This version of Claude Code hasn't been checked with ordin yet. Install a checked version, then try again."; `SETUP_PROGRAM_REFUSED` "This computer stopped a program ordin needs. Check the computer's security settings, then try again."; `SETUP_PROGRAM_DAMAGED` "A program ordin needs is damaged. Reinstall ordin."; `SETUP_NO_READY_HARNESS` "ordin needs Claude Code, signed in, before it can start. Install Claude Code or sign in, then try again."; `SETUP_NO_HARNESS_ACCESS` "You don't have Claude access yet. Sign in to Claude Code with an account that has access."; `SETUP_HARNESS_UNKNOWN` "ordin can't sign you in to that assistant. Choose Claude Code or GitHub Copilot instead."; `SETUP_NO_PROFILE_ADDRESS` "ordin can't find your company's settings, so it goes on without them."; `SETUP_SELF_UPDATE_UNAVAILABLE` in the error catalogue (`error-setup-self-update-unavailable`) "This copy of ordin can't update itself. Reinstall ordin to get the new version." (an error sentence names no server, EH-SEN-2; the Servers row keeps SET-UP-14's own words). Codes, actions and outcomes do not change; the code names `SETUP_*` keep their spelling.

```gherkin
  Scenario: Happy path — Claude Code in the person's own folder is found from a short PATH (SET-R1-7, SET-R1-8)
    Given claude is only in <home>/.local/bin and PATH is "/usr/bin:/bin:/usr/sbin:/sbin"
    When the guided run reaches the Harness step
    Then the step passes with the path <home>/.local/bin/claude and its version
    And claude auth status --json runs with <home>/.local/bin at the end of its PATH

  Scenario: Edge case — PATH comes first (SET-R1-7)
    Given claude is on PATH and also in <home>/.local/bin
    When locate runs
    Then the claude on PATH is the one used

  Scenario: Error path — Claude Code is nowhere (SET-R1-7, SET-R1-9)
    Given no claude on PATH or in the person's own folders
    When the guided run reaches the Harness step
    Then the step fails with SETUP_HARNESS_MISSING and "Claude Code isn't on this computer yet. Install it, then choose Check again."
```

### Gaps

None.

## Amendment 2026-02-02 — The Copilot CLI is the person's own, found like Claude Code (SET-CPX-1 to SET-CPX-5)

> **Status:** Approved by the user, 2026-02-02 ("approve, and allow Copilot without a company profile"). Source: user decision 2026-02-02, "copilot installation should be a pre-requisite to enable copilot harness in ordin"; HLD ADR-123. **derivedFromHld:** 0.73.0. Every section above stays in force except where this amendment changes it.

- **SET-CPX-1 Copilot is not a bundled program.** `SetupConfig.bundled_programs` never names the Copilot CLI; the desktop's list is `[ordin-hook]` in every build. The sentence in *Configuration consumed* that the bundled programs "include … the Copilot CLI" is replaced by: the desktop's bundled programs are the ordin hook alone. Step D.3 (program start check) therefore never runs the Copilot CLI.
- **SET-CPX-2 Where `copilot` is looked for.** `ordin_setup::find_copilot_cli(path, home, local_app_data, app_data) -> Option<PathBuf>`, kept in this crate beside `find_claude_code` (SET-R1-7) so the probe and the adapter never disagree. It searches each `PATH` entry, in order, then the person's own folders, in order: `<home>/.local/bin` (GitHub's install script for a non-root install); on macOS also `/opt/homebrew/bin` and `/usr/local/bin` (Homebrew); on Windows `<LOCALAPPDATA>\Microsoft\WinGet\Links` (WinGet) and `<APPDATA>\npm` (npm). `<home>` is `HOME` (`USERPROFILE` on Windows); `<LOCALAPPDATA>` and `<APPDATA>` are the program's own `LOCALAPPDATA` and `APPDATA`; an absent variable skips its folder. The first executable file named `copilot` (`copilot.exe`, then `copilot.cmd`, on Windows) is the one used. The folders and names are *conditional on C-112*; until it is confirmed, a Copilot CLI installed elsewhere is found only on `PATH`.
- **SET-CPX-3 The Copilot probe's `locate`.** The desktop program's Copilot `HarnessProbe` (OrdinDesktop.md DSK-CPX-3) answers `locate` with `find_copilot_cli` and `<found> --version`, run with D.1's environment, the found folder added to `PATH` as SET-R1-8 adds Claude Code's, null stdin and a 10 s timeout. Version = the first whitespace-separated token matching `^[0-9]+\.[0-9]+\.[0-9]+` *(assumed here, SA-CPX-01; conditional on C-112)*; no match → `ProbeError::Unreadable`. Punctuation that ends the token after the number is not part of the version: the CLI prints `GitHub Copilot CLI 1.0.116.` and the version is `1.0.116`. Nothing found → `Ok(None)`. The Harness step (C.3) and `tested_versions` then work for `copilot` exactly as for `claude_code`.
- **SET-CPX-4 Copilot's own sentences (SET-R1-9).** For the `copilot` harness, `SETUP_HARNESS_MISSING` carries message id `setup-harness-missing-copilot`, "GitHub Copilot isn't on this computer yet. Install it, then choose Check again.", and `SETUP_HARNESS_UNTESTED` carries `setup-harness-untested-copilot`, "This version of GitHub Copilot hasn't been checked with ordin yet. Install a checked version, then try again." Neither offers an action: ordin opens no install page for Copilot (ADR-123). The codes do not change; no code is added.
- **SET-CPX-6 No company profile: every harness this build can check is checked (user decision 2026-02-02, "allow Copilot without a company profile").** When the computer has no company profile (A.2 `ProfileState::NotUsed`, which includes `NoAddress` by SET-R1-6), the Harness and HarnessSignIn steps of C.3, the checks of D.4 and `ready_harness` walk `[claude_code, copilot]` in that order, each with an empty `tested_versions`, in place of the 2D-2 default `[claude_code]`. A harness with no registered probe is skipped as before, so a build without `copilot` and the server walk `[claude_code]` only. When a company profile is in use, its `harnesses` list alone decides, as before. Claude Code stays the first, so `default_harness` and Settings › Agents and models' *Same as my organisation* are unchanged.
- **SET-CPX-5 Found after start.** A Copilot CLI the person installs while ordin is open is found by the next check (**Check again**, a background run, or `run_checks(BeforeSession)`), because `locate` runs on every check; nothing is cached between checks.

### Assumptions chosen here

- **SA-CPX-01** `copilot --version` prints the version as its first semantic-version token, as `claude --version` does. Confirmed with C-112.
- **SA-CPX-02** The order of the person's own folders puts the install script's folder first, then the package managers', the same order as SET-R1-7.

### Error Table — rows changed

| Condition | Result | Code | Message id |
|---|---|---|---|
| `copilot` listed by the profile, no Copilot CLI found | `Failed`, no action | `SETUP_HARNESS_MISSING` | `setup-harness-missing-copilot` |
| Copilot CLI found at a version outside a non-empty `tested_versions` | `needs_update`, `Failed`, no action | `SETUP_HARNESS_UNTESTED` | `setup-harness-untested-copilot` |
| Copilot CLI found, `--version` output has no version token or times out | Err(retryable) in the result, Try again | `SETUP_HARNESS_CHECK_FAILED` (the existing row for unreadable output) | as for Claude Code |

```gherkin
  Scenario: Happy path — a Copilot CLI in the person's own folder is found from a short PATH (SET-CPX-2, SET-CPX-3)
    Given the profile lists harnesses [copilot] with tested_versions []
    And copilot is only in <home>/.local/bin and PATH is "/usr/bin:/bin:/usr/sbin:/sbin"
    When the guided run reaches the Harness step
    Then the step passes with the path <home>/.local/bin/copilot and its version
    And copilot --version ran with <home>/.local/bin at the end of its PATH

  Scenario: Edge case — the bundled programs never include Copilot (SET-CPX-1)
    Given a desktop build with copilot on
    When the program start check runs
    Then it checks ordin-hook only

  Scenario: Edge case — Copilot installed while ordin is open is found by Check again (SET-CPX-5)
    Given the Harness step for copilot failed with SETUP_HARNESS_MISSING
    And copilot is then placed in <home>/.local/bin
    When the person chooses Check again
    Then the Harness step for copilot passes

  Scenario: Happy path — no company profile, Copilot installed and Claude Code not (SET-CPX-6)
    Given the computer has no company profile and a build with copilot on
    And no claude executable is found and copilot is in <home>/.local/bin
    When the guided run reaches the Harness step
    Then Claude Code's step fails with SETUP_HARNESS_MISSING
    And Copilot's step passes with the path <home>/.local/bin/copilot
    And the run goes on to sign in to GitHub Copilot

  Scenario: Edge case — a company profile that lists Claude Code only (SET-CPX-6)
    Given a company profile listing harnesses [claude_code] and copilot installed
    When the guided run reaches the Harness step
    Then Copilot is not checked

  Scenario: Error path — Copilot is nowhere (SET-CPX-4)
    Given the profile lists harnesses [copilot] and no copilot on PATH or in the person's own folders
    When the guided run reaches the Harness step
    Then the step fails with SETUP_HARNESS_MISSING and "GitHub Copilot isn't on this computer yet. Install it, then choose Check again."
    And no action is offered

  Scenario: Error path — an untested Copilot version (SET-CPX-4)
    Given the profile lists copilot with tested_versions ["1.0.98"] and the installed version is "1.0.99"
    When the guided run reaches the Harness step
    Then the attachment is needs_update and the step fails with SETUP_HARNESS_UNTESTED and "This version of GitHub Copilot hasn't been checked with ordin yet. Install a checked version, then try again."
```

### Gaps

None.

## Amendment 2026-02-05 — Copilot installed through a Node version manager is found (SET-CPX-7)

> **Status:** Approved by the user, 2026-02-05 ("can you not just fix the binary so it can find the correct path?"), after the 0.2.1-test.1 smoke test on a Mac whose Copilot CLI was installed with npm under a Node version manager and was not found. **derivedFromHld:** 0.75.0. Every section above stays in force except where this amendment changes it.

- **SET-CPX-7 The Node version manager's folders.** Off Windows, SET-CPX-2's person's own folders gain, after the Homebrew folders, the `bin` folder of every Node.js version under `<home>/.nvm/versions/node/` and then under `<home>/.nvm/versions/`: each sub-folder whose name is `v` followed by dot-separated numbers and which holds a `bin` folder, ordered newest version first by number. A program opened from the Dock or a desktop menu does not inherit the shell's version-manager `PATH`, so an npm install of `copilot` is otherwise never found. `node` lives in the same folder, and SET-CPX-3 already adds the found folder to `PATH`, so the npm `copilot` can start. A missing or unreadable folder adds nothing. Windows is unchanged: `<APPDATA>\npm` already covers npm there.

## Amendment 2026-02-04 — The person chooses the assistant at first start (SET-CHO-1 to SET-CHO-7)

> **Status:** Approved by the user, 2026-02-04 ("approve as written"). Source: HLD ADR-125, acceptance criteria 1–5 and 8; user decision 2026-02-04 ("go with your recommendations": the answer takes effect by ordin starting again once; criterion 7, an assistant without a licence, is specified later from the probe's evidence). HLD ADR-125. **derivedFromHld:** 0.75.0. Every section above stays in force except where this amendment changes it.

- **SET-CHO-1 The person's choice, read through a port.** This crate owns a second hub trait beside `HarnessProbe` (ADR-100 hub pattern), registered by the desktop program only:

  ```rust
  /// The person's own harness choice, kept by the program that owns the person's preferences.
  #[async_trait::async_trait]
  pub trait HarnessChoice: Send + Sync {
      /// The harness the person chose, if any.
      fn chosen(&self) -> Option<HarnessKind>;
      /// Keep `harness` as the person's choice. Err when it could not be kept.
      async fn choose(&self, harness: HarnessKind) -> Result<(), ChoiceError>;
  }

  #[derive(Debug, Clone, thiserror::Error)]
  #[error("the choice could not be kept")]
  pub struct ChoiceError;

  impl SetupAssistant {
      pub fn set_harness_choice(&self, choice: Arc<dyn HarnessChoice>); // replaces any earlier one
      pub async fn choose_harness(&self, harness: HarnessKind) -> Result<(), SetupError>; // SET-CHO-4
  }
  ```

  The server registers none, so it never asks (C-48) and its order is unchanged.
- **SET-CHO-2 The chosen harness is walked first.** With no company profile (`ProfileState::NotUsed` or `NoAddress`), when `chosen()` is `Some(h)` and `h` is in the SET-CPX-6 list, `listed_harnesses` returns `h` first and the other after it. With no choice, the order stays `[claude_code, copilot]`. A company profile's `harnesses` list alone decides, as before; `chosen()` is not read. The Harness and HarnessSignIn steps of C.3, the checks of D.4 and `ready_harness` (E) all use this order, so the attachment `ready_harness` returns is the harness the desktop runs.
- **SET-CHO-3 When the question is asked.** In the guided run's Harness step (A.3) the question is asked only when **all** of these hold: the program is the desktop; there is no company profile; a `HarnessChoice` is registered and `chosen()` is `None`; probes for both `claude_code` and `copilot` are registered; and **both** are found by `locate()` with a readable version. In that case, A.3 locates both (it does not stop at the first) and writes both attachments. For each found harness, A.4's `sign_in_status()` then runs once, without starting any sign-in: `SignedIn` makes the attachment `ready`; `SignedOut` or a `ProbeError` makes it `needs_sign_in`; `NoAccess` leaves it in state `checked` with `signed_in = false`. A harness is **offered** when its attachment is `ready` or `needs_sign_in`, so a harness with no access is not offered. No attachment state is added. If fewer than two harnesses are offered, no question is asked and A.3 goes on with the offered one, or with the first found when none is offered, exactly as before. Otherwise step `HarnessChoice` (new `CheckKind`, `as_str` `"harness_choice"`, step message `setup-step-harness-choice`) is inserted after `Harness` and counted in `total` only from then on (11 §5.1, the count shows real steps). It is set to `NeedsYou`, and the run waits for `choose_harness`. The attachments that `setup.status` returns tell the screens which harnesses are offered and whether each is signed in. No new field is added to `ProgressStep`. *Amended 2026-02-04 (plan 08 FU-030):* the `HarnessChoice` result is published in the progress and returned in the report, but it is not stored in `setup_checks`, as the `SecondInstall` result is not: that table's `check_kind` constraint has no `harness_choice` value, and the answer itself is kept by the program's `HarnessChoice`.
- **SET-CHO-4 `choose_harness(harness)`.**
  1. No guided run is waiting at `HarnessChoice` → `Err(permanent)` `SETUP_NO_CHOICE_PENDING`.
  2. `harness` is not one of the offered attachments → `Err(permanent)` `SETUP_HARNESS_NOT_OFFERED`.
  3. Call `HarnessChoice::choose(harness)`. If it fails → `Err(retryable)` `SETUP_CHOICE_NOT_SAVED`, and the step stays `NeedsYou`.
  4. Otherwise `HarnessChoice` becomes `done` with the result `Passed`. The waiting run continues with `harness` alone: HarnessSignIn (A.4) for it when its attachment is not `ready`; the other harness is not signed in and is not checked further in this run. Return `Ok(())`.
- **SET-CHO-5 Never asked again.** Once `chosen()` is `Some`, from this question or from Settings › Agents and models, SET-CHO-3 does not hold, so no later guided run asks. `needs_guided_setup` (C) does not change. A start that the desktop makes after the choice (OrdinDesktop.md DSK-CHO-2) finds `needs_guided` still true, because `setup_state` is written only when `ready = true` (A.7). It then runs the guided sequence again with the chosen harness first and no question.
- **SET-CHO-6 Background checks never ask.** `run_checks` (D) never inserts `HarnessChoice`; with no choice it keeps the SET-CPX-6 order.
- **SET-CHO-7 Service API command.** `setup.choose_harness` with input `{"harness": "claude_code" | "copilot_cli"}`, implemented by the desktop program (OrdinDesktop.md DSK-CHO-2), which calls `choose_harness`.

### Assumptions chosen here

- **SA-CHO-01** "Found" for the question means located with a readable version. A harness that is installed but signed out is offered and marked as needing sign-in (RFC 17 open question 1, the recommendation). It is signed in only after it is chosen.
- **SA-CHO-02** A harness whose `sign_in_status` reports `NoAccess` is not offered. This is the existing signal; how a missing licence shows is left to the later amendment that answers RFC 17 criterion 7.
- **SA-CHO-03** With no company profile, `tested_versions` is empty (SET-CPX-6), so every found harness counts as tested and none is excluded as untested.

### Error Table — rows added

| Condition | Result | Code | Message id | Retry |
|---|---|---|---|---|
| `choose_harness` with no guided run waiting at the question | `Err(permanent)` | `SETUP_NO_CHOICE_PENDING` | `setup-no-choice-pending` | no |
| `choose_harness` with a harness that is not offered | `Err(permanent)` | `SETUP_HARNESS_NOT_OFFERED` | `setup-harness-not-offered` | no |
| `HarnessChoice::choose` fails | `Err(retryable)` | `SETUP_CHOICE_NOT_SAVED` | `setup-choice-not-saved` | yes, the person chooses again |

```gherkin
Feature: The person chooses the assistant at first start (SET-CHO)

  Scenario: Happy path — both found, no profile, no choice: the run asks (SET-CHO-3, SET-CHO-4)
    Given the desktop with no company profile, no harness chosen, and probes for claude_code and copilot
    And claude and copilot are both found and both report signed in
    When the guided run reaches the Harness step
    Then both attachments are written in state ready
    And the step harness_choice is in state needs_you
    When choose_harness(copilot_cli) is called
    Then HarnessChoice.choose received copilot_cli
    And the step harness_choice is done and the run does not check claude_code again

  Scenario: Happy path — a chosen harness that is signed out is signed in next (SET-CHO-4, SA-CHO-01)
    Given the question is waiting and copilot reports signed out
    When choose_harness(copilot_cli) is called
    Then the step harness_sign_in for copilot is in state needs_you

  Scenario: Edge case — a choice already exists: no question, chosen first (SET-CHO-2, SET-CHO-5)
    Given no company profile and chosen() is copilot_cli, with both found and signed in
    When the guided run reaches the Harness step
    Then no step harness_choice is in the progress
    And ready_harness returns the copilot attachment

  Scenario: Edge case — only one found: no question (SET-CHO-3)
    Given no company profile, no choice, and only claude found
    When the guided run reaches the Harness step
    Then no step harness_choice is in the progress and Claude Code is checked as before

  Scenario: Edge case — a company profile decides (SET-CHO-2, SET-CHO-3)
    Given a company profile listing harnesses [claude_code], no choice, and both found
    When the guided run reaches the Harness step
    Then no step harness_choice is in the progress and Copilot is not checked

  Scenario: Edge case — a harness with no access is not offered (SET-CHO-3, SA-CHO-02)
    Given no profile, no choice, both found, and claude reports NoAccess
    When the guided run reaches the Harness step
    Then no step harness_choice is in the progress and the run goes on with copilot

  Scenario: Edge case — background checks never ask (SET-CHO-6)
    Given no profile, no choice, and both found
    When run_checks(Background) runs
    Then no step harness_choice is produced

  Scenario: Error path — choosing with no question waiting (SET-CHO-4)
    When choose_harness(claude_code) is called with no guided run waiting
    Then the error is SETUP_NO_CHOICE_PENDING

  Scenario: Error path — choosing a harness that was not offered (SET-CHO-4)
    Given the question is waiting with claude_code and copilot_cli offered
    And copilot's attachment has since been removed
    When choose_harness(copilot_cli) is called
    Then the error is SETUP_HARNESS_NOT_OFFERED

  Scenario: Error path — the choice cannot be kept (SET-CHO-4)
    Given the question is waiting and HarnessChoice.choose fails
    When choose_harness(copilot_cli) is called
    Then the error is SETUP_CHOICE_NOT_SAVED and the step harness_choice stays needs_you
```

### Gaps

None. RFC 17 criterion 7 (an assistant that is signed in without a licence) is outside this amendment. It is specified once the probe on each command line is recorded (RFC 17 open question 2).
