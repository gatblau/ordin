//! The crate's public types and the traits it owns (SetupAssistant.md *Public Interface*, *Harness
//! probe*, *Reports and progress*, and the ADR-114 and ADR-072 amendments).

use std::collections::BTreeMap;
use std::sync::Arc;

use ordin_types::domain::identity::HarnessKind;
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProgram {
    Desktop,
    PersonalServer,
}

/// A program ordin brings (bundled or placed under the data root), listed by the program at build
/// time.
#[derive(Debug, Clone)]
pub struct BundledProgram {
    /// Plain name shown to the person, e.g. "the ordin hook", "GitHub CLI".
    pub name: String,
    /// As signed, e.g. "gatblau".
    pub publisher: String,
    /// Absolute.
    pub path: std::path::PathBuf,
    /// e.g. `["--version"]`.
    pub version_args: Vec<String>,
    pub expected_sha256: Option<Sha256Hex>,
    /// A pristine copy inside the application's own files; a damaged copy at `path` is restored
    /// from it once.
    pub restore_from: Option<std::path::PathBuf>,
    /// Plain words for what is off when it cannot start, e.g. "agent sessions".
    pub needed_for: String,
}

pub struct SetupConfig {
    pub program: SetupProgram,
    pub bundled_programs: Vec<BundledProgram>,
    pub application_version: String,
    pub http: reqwest::Client,
}

/// Implemented by the desktop program; the server program does not implement it.
pub trait UrlOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), String>;
}

pub struct SetupDeps {
    pub profile: Arc<ordin_organisation_profile::OrganisationProfileService>,
    pub identity: Arc<ordin_identity::IdentityService>,
    pub catalogue: Arc<ordin_catalogue::CatalogueService>,
    pub packages: Arc<ordin_package_catalogue::PackageCatalogue>,
    pub craft: Arc<ordin_craft_toolkit::CraftToolkit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckTrigger {
    Start,
    Background,
    OnDemand,
    BeforeSession,
}

// ---------------------------------------------------------------- updates (ADR-114, ADR-072)

/// Owned here, implemented by the desktop program over Tauri's updater.
// spec: ordin-desktop/components/SetupAssistant#SA-RECON-01
#[async_trait::async_trait]
pub trait AppUpdater: Send + Sync {
    /// Reads the feed; `Ok(None)` when no newer signed version exists. The offer's `state` is
    /// ignored by this crate, which sets it.
    async fn check(&self, feed_url: &url::Url) -> Result<Option<UpdateOffer>, UpdaterError>;
    /// Downloads the offered version and verifies it with the build's update key; a second call
    /// for the same version returns `Ok(())` without downloading again.
    async fn download(&self, offer: &UpdateOffer) -> Result<(), UpdaterError>;
    /// Installs the kept download and restarts ordin.
    async fn install_and_restart(&self) -> Result<(), UpdaterError>;
    /// Installs the kept download while ordin exits; no restart.
    async fn install_on_exit(&self) -> Result<(), UpdaterError>;
    /// Fixed for the life of the process.
    fn install_kind(&self) -> InstallKind;
    /// The other install on this computer that this account can start, if there is one.
    fn other_install(&self) -> Option<std::path::PathBuf>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateOffer {
    /// Semantic version, 1–64 characters.
    pub version: String,
    /// The feed's release notes, at most 4,000 characters (cut beyond).
    pub notes: String,
    pub state: UpdateState,
}

/// `AdministratorNeeded`: a newer version exists and this install cannot take it; nothing was
/// downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    Downloading,
    Ready,
    AdministratorNeeded,
}

/// Where this copy of the program is installed, found once at start by the desktop program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind {
    PerUser,
    WholeComputer,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdaterError {
    /// → `SETUP_UPDATE_UNREACHABLE`, retryable.
    #[error("feed unreachable")]
    Unreachable,
    /// → `SETUP_UPDATE_SIGNATURE_INVALID`.
    #[error("signature refused")]
    SignatureInvalid,
    /// → `SETUP_UPDATE_INSTALL_FAILED`, retryable.
    #[error("install failed")]
    InstallFailed,
    /// An install call with no kept download → `SETUP_UPDATE_NONE`.
    #[error("nothing downloaded")]
    NothingDownloaded,
}

/// The release notes of the version now running, kept when its download was verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WhatsNew {
    /// The running version.
    pub version: String,
    /// At most 4,000 characters.
    pub notes: String,
    /// True once `whats_new_seen` was called for this version.
    pub seen: bool,
}

/// A newer version a personal server's feed offers (`setup.server_update`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServerUpdate {
    pub version: String,
    /// At most 4,000 characters.
    pub notes: String,
    /// False when this server cannot replace itself.
    pub self_update: bool,
}

/// How the program ends after a ready update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateFinish {
    /// The person pressed Restart to update; the program has run its quit flow.
    Restart,
    /// The person quit ordin; install on the way out, no restart.
    OnExit,
}

/// The platform parts of a personal server's own update, implemented by the server program.
#[async_trait::async_trait]
pub trait ServerPlatform: Send + Sync {
    /// How this process was started.
    fn run_mode(&self) -> ServerRunMode;
    /// The folder holding the running `ordin-server` and `ordin-hook`.
    fn program_dir(&self) -> std::path::PathBuf;
    /// The key of this build's operating system and architecture in the feed's platform entries.
    fn feed_platform(&self) -> &'static str;
    /// Asks systemd to restart the user unit `ordin-server.service`; returns once the request is
    /// accepted.
    async fn restart_unit(&self) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerRunMode {
    UserUnit,
    Container,
    Other,
}

/// Result of a server update run in the foreground (`ordin-server update`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerUpdateOutcome {
    /// No newer version in the feed.
    UpToDate,
    /// Programs replaced and the unit's restart requested.
    Restarting { version: String },
}

/// The code the server program returns for `setup.server_update` and `setup.update_server` in the
/// agent profile, which constructs no Setup Assistant.
pub const SELF_UPDATE_UNAVAILABLE: &str = "SETUP_SELF_UPDATE_UNAVAILABLE";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageServerLaunch {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

// ---------------------------------------------------------------- harness probe

#[derive(Debug, Clone)]
pub struct HarnessLocation {
    pub path: std::path::PathBuf,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessSignIn {
    SignedIn { method: Option<String> },
    SignedOut,
    NoAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeRepair {
    NothingToDo,
    Repaired,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("not found")]
    NotFound,
    #[error("refused by the operating system")]
    Refused,
    #[error("timed out")]
    Timeout,
    #[error("unreadable output")]
    Unreadable,
    #[error("not offered here")]
    NotOffered,
}

/// The hub trait each harness implements; this crate ships `ClaudeCodeProbe`, the desktop program
/// registers the Copilot one.
// spec: ordin-desktop/components/SetupAssistant#SA-RECON-04
#[async_trait::async_trait]
pub trait HarnessProbe: Send + Sync {
    fn harness(&self) -> HarnessKind;
    async fn locate(&self) -> Result<Option<HarnessLocation>, ProbeError>;
    async fn sign_in_status(&self) -> Result<HarnessSignIn, ProbeError>;
    /// Starts the vendor's own sign-in flow and returns when its process exits.
    async fn start_sign_in(&self) -> Result<(), ProbeError>;
    /// Restart what the harness needs (for Copilot: its CLI server). Claude Code: `NothingToDo`.
    async fn repair(&self) -> Result<ProbeRepair, ProbeError>;
}

// ---------------------------------------------------------------- reports and progress

/// `SecondInstall` added by the ADR-072 amendment, after `CraftApps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    CompanySettings,
    Harness,
    HarnessSignIn,
    Tokens,
    AgentRepositories,
    AgentFolders,
    Programs,
    CraftApps,
    SecondInstall,
}

impl CheckKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            CheckKind::CompanySettings => "company_settings",
            CheckKind::Harness => "harness",
            CheckKind::HarnessSignIn => "harness_sign_in",
            CheckKind::Tokens => "tokens",
            CheckKind::AgentRepositories => "agent_repositories",
            CheckKind::AgentFolders => "agent_folders",
            CheckKind::Programs => "programs",
            CheckKind::CraftApps => "craft_apps",
            CheckKind::SecondInstall => "second_install",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Passed,
    Repaired,
    Failed,
    Skipped,
}

impl CheckOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            CheckOutcome::Passed => "passed",
            CheckOutcome::Repaired => "repaired",
            CheckOutcome::Failed => "failed",
            CheckOutcome::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonAction {
    TryAgain,
    SignInAgain,
    SignIn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub message_args: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<PersonAction>,
    /// For a refused program: what Send to IT carries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
    pub checked_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RefusedProgram {
    pub name: String,
    pub publisher: String,
    pub sha256: Sha256Hex,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SetupReport {
    /// None for the guided sequence.
    pub trigger: Option<CheckTrigger>,
    pub results: Vec<CheckResult>,
    /// A harness is ready.
    pub ready: bool,
    pub started_at: Timestamp,
    pub ended_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SetupProgress {
    pub running: bool,
    pub steps: Vec<ProgressStep>,
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProgressStep {
    pub check: CheckKind,
    pub state: StepState,
    pub message_id: String,
    /// Set when the step has failed: the failed result's sentence, so the screens can say why at
    /// once rather than when the run ends (SET-R1-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<StepProblem>,
}

/// Why a step failed: the `CheckResult`'s message and its arguments (SET-R1-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StepProblem {
    pub message_id: String,
    #[serde(default)]
    pub message_args: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Waiting,
    Running,
    Done,
    NeedsYou,
    Failed,
}

// ---------------------------------------------------------------- command-line tools (FU-094)

/// A command-line tool release 1 starts from the computer's `PATH`, missing from it.
// spec: ordin-desktop/components/SetupAssistant#SET-UP-28
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MissingTool {
    /// The command, `gh` or `glab`.
    pub command: String,
    /// What to install, in plain words: "GitHub CLI" or "GitLab CLI".
    pub install: String,
}
