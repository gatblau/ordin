//! The public value types and the two traits the programs implement (ClaudeLocalAdapter.md
//! *Public Interface*). Every `Debug` here that could reach a secret prints names only.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ordin_mcp_front::ProgramProfile;
use ordin_runtime_adapter::ToolCaller;
use ordin_types::domain::agent_profile::{HarnessModelSettings, RunStarter};
use ordin_types::{AgentInstanceId, ApprovalId, ArtefactId, CrewCharterId, RunId};

#[derive(Debug, Clone)]
pub struct ClaudeAdapterConfig {
    /// Absolute path of the `claude` executable found by the Setup Assistant.
    pub claude_path: PathBuf,
    /// Absolute path of `ordin-hook` (`ordin-hook.exe`).
    pub hook_program: PathBuf,
    /// Application data root; per-session files go under `<root>/run/sessions/<run_id>/`.
    pub app_data_root: PathBuf,
    pub profile: ProgramProfile,
    pub tested_versions: Vec<String>,
    pub sign_in: SignInMode,
    /// 10 s on the desktop and personal server (SA-STOP-01); the deployment's stop period in the
    /// agent profile.
    pub stop_deadline: Duration,
}

/// Chosen here.
#[derive(Clone)]
pub enum SignInMode {
    /// Desktop and personal server: the person's own sign-in (09 ADR-044).
    PersonSignIn,
    /// Agent profile: the organisation's key (09 ADR-097 item 2).
    /// `models` is Model Policy's `ordin_types::agent_profile::HarnessModelSettings` (row 249,
    /// SA-RECON-01): `models.key` (`Option<OrganisationKey>`) is the key; `models.fallback_model`
    /// gives `--fallback-model`; `models.provider` is not read here (ADR-107: no provider-specific
    /// code); `models.gateway_url` becomes `ANTHROPIC_BASE_URL` and each `models.environment` entry
    /// is set in the child's environment (SA-CLOSE-01).
    // spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-01
    OrganisationKey {
        models: HarnessModelSettings,
        route: KeyRoute,
    },
}

/// Prints the variant only: the model settings hold the key and the declared values.
impl std::fmt::Debug for SignInMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignInMode::PersonSignIn => f.write_str("PersonSignIn"),
            SignInMode::OrganisationKey { route, .. } => f
                .debug_struct("OrganisationKey")
                .field("route", route)
                .finish_non_exhaustive(),
        }
    }
}

/// How the key reaches Claude Code (10 C-74). Chosen here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyRoute {
    /// `ANTHROPIC_API_KEY` in the Claude Code child's environment only (default while C-74 is
    /// unconfirmed or passes).
    Environment,
    /// C-74 failed: the key is given through Claude Code's own `apiKeyHelper` setting in `--settings`.
    SettingsApiKeyHelper,
}

/// Everything about one session that the adapter cannot derive from StartRun.
/// Implemented by the program from the Session Supervisor, the Agent Folder Manager and the
/// Usage Planner, Installers and Model Policy, plus the Crew Orchestrator (`crew_charter_id`) and,
/// in the agent profile, the Run Queue (`run_starter`) (owned here; D3 pattern).
#[async_trait::async_trait]
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-REC-2
pub trait SessionSetupSource: Send + Sync {
    /// `attachments`: `StartRun.attachments` of the run being started, in the order given
    /// (at most 50 after `check_start`, 2C). The program turns them into
    /// `SessionSetup.attachment_paths`; the adapter does not read them itself.
    async fn session_setup(
        &self,
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
        attachments: &[ArtefactId],
    ) -> Result<SessionSetup, SessionSetupError>;
}

/// The program could not assemble a session's setup. Named so that it does not clash with
/// `ordin_setup::SetupError` (SetupAssistant.md SA-RECON-07). `code` is the error code of the
/// owner whose call failed (for example a Session Supervisor, Installers or Usage Planner code).
#[derive(Debug, thiserror::Error)]
#[error("session setup failed: {code}")]
pub struct SessionSetupError {
    pub code: String,
}

#[derive(Debug, Clone)]
pub struct SessionSetup {
    /// Absolute path of the agent folder (the CLI's working folder, A-11). The program fills it from
    /// the Installers' `HarnessLaunch::ClaudeCode.working_dir` (row 251).
    pub agent_folder: PathBuf,
    /// `HarnessLaunch::ClaudeCode.append_system_prompt_file` (`--append-system-prompt-file`).
    pub append_system_prompt_file: PathBuf,
    /// `HarnessLaunch::ClaudeCode.plugin_dir` (`--plugin-dir`).
    pub plugin_dir: PathBuf,
    /// `HarnessLaunch::ClaudeCode.plugin_name`; commands are `<plugin_name>:<command>` (R-94).
    pub plugin_name: String,
    /// Model chosen by the Usage Planner for this step (09 ADR-062; 10 C-37).
    pub model: Option<String>,
    pub effort: Option<ClaudeEffort>,
    /// Agent profile: the configuration's fallback model (09 ADR-098 item 3), copied by the
    /// program from `HarnessModelSettings.fallback_model`; `None` elsewhere.
    pub fallback_model: Option<String>,
    /// Absolute paths of the attachments, already checked against the run's folder grants.
    pub attachment_paths: Vec<PathBuf>,
    /// Directories outside the agent folder the run may read and write (folder grants, 09 ADR-025).
    pub extra_dirs: Vec<PathBuf>,
    /// True only for a system-changing package on a project with approved shell commands (A-06; 09 ADR-037).
    pub shell_allowed: bool,
    /// Package-declared MCP servers with their environment (SA-CRED-01), merged into --mcp-config.
    pub package_servers: Vec<PackageMcpServer>,
    /// Present when the run continues an earlier harness session (Recovery after restart;
    /// agent-profile resume).
    pub resume_session_id: Option<String>,
    /// Crew member runs (`StartRun.caller` is `ToolCaller::CrewMember { crew_run_id }`): the crew
    /// run's charter, which the program reads from `CrewOrchestrator::get(crew_run_id)`
    /// (`CrewRun.crew_charter_id`); `None` for every other caller.
    pub crew_charter_id: Option<CrewCharterId>,
    /// Agent profile: the run record's `started_by` (2C `RunStarter`), which the program reads from
    /// the Run Queue's `TakenRun.view.record.started_by`; `None` on the desktop and the personal server.
    pub run_starter: Option<RunStarter>,
    /// Agent profile: the run as the Session Supervisor holds it, while it is paused on an approval;
    /// `None` for any other run, an ended one included. A second instance adopts only such a run.
    pub paused: Option<PausedRun>,
}

/// What the program reads from the run's row for a run another instance paused
/// (`pause_approval_id`, and the caller the run was started with).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PausedRun {
    pub approval_id: ApprovalId,
    pub caller: ToolCaller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeEffort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl ClaudeEffort {
    /// The value of `--effort`.
    pub(crate) fn flag_value(self) -> &'static str {
        match self {
            ClaudeEffort::Low => "low",
            ClaudeEffort::Medium => "medium",
            ClaudeEffort::High => "high",
            ClaudeEffort::Xhigh => "xhigh",
            ClaudeEffort::Max => "max",
        }
    }
}

#[derive(Clone)]
pub struct PackageMcpServer {
    /// 1–64 chars, `^[a-zA-Z0-9_.-]+$` (LW-19).
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// May hold secrets; never logged.
    pub env: BTreeMap<String, String>,
}

/// Prints the name and the variable names only: `env` may hold secrets.
impl std::fmt::Debug for PackageMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageMcpServer")
            .field("name", &self.name)
            .field("command", &self.command)
            .field("env_names", &self.env.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// Agent profile: where the harness's session record is kept between a pause and a resume
/// (09 ADR-098 item 3; 10 C-80). Implemented by the program over the Session Supervisor's
/// `save_session_record` / `load_session_record`.
// spec: ordin-desktop/components/ClaudeLocalAdapter#C-80
#[async_trait::async_trait]
pub trait SessionRecordStore: Send + Sync {
    async fn save(
        &self,
        run_id: RunId,
        runtime_session_id: &str,
        record: Vec<u8>,
    ) -> Result<(), RecordError>;
    async fn load(
        &self,
        run_id: RunId,
        runtime_session_id: &str,
    ) -> Result<Option<Vec<u8>>, RecordError>;
}

/// A failure of the program's `SessionRecordStore`. The program builds it from the
/// `SupervisorError` that `save_session_record` / `load_session_record` returned:
/// `code` = `SupervisorError::code()` (for example `SUP_RUN_NOT_FOUND`), `retryable` =
/// `SupervisorError::retryable()`.
#[derive(Debug, thiserror::Error)]
#[error("session record store failed: {code}")]
pub struct RecordError {
    pub code: &'static str,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInStatus {
    SignedIn,
    SignedOut,
}
