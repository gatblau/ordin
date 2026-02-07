//! What the program gives the adapter: its configuration, the session setup it assembles for each
//! run, and the Copilot CLI the person installed (CopilotLocalAdapter.md *Public Interface*,
//! CPA-REC-1, CPA-CPX-2).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ordin_mcp_front::ProgramProfile;
use ordin_types::{AgentInstanceId, ArtefactId, CrewCharterId, RunId};

/// The adapter's configuration. The Copilot CLI is not part of it: it is asked for at every `start`
/// and probe call (CPA-CPX-2).
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-2
#[derive(Debug, Clone)]
pub struct CopilotAdapterConfig {
    /// The organisation profile's `harnesses[]` versions with `name: copilot`; empty accepts any.
    pub tested_versions: Vec<String>,
    pub app_data_root: PathBuf,
    pub profile: ProgramProfile,
    /// SA-STOP-01: 10 s.
    pub stop_deadline: Duration,
}

/// The Copilot CLI's device sign-in on a Linux server is unconfirmed (10 C-48); until it is, the
/// personal server offers Claude only. Compiled constant, chosen here.
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SERVER_SIGN_IN_UNCONFIRMED
pub const C48_SERVER_SIGN_IN_CONFIRMED: bool = false;

/// What `StartRun` does not carry. Implemented by the program over the Session Supervisor, the
/// Installers, the Usage Planner and the Crew Orchestrator (`crew_charter_id`).
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-REC-1
#[async_trait::async_trait]
pub trait SessionSetupSource: Send + Sync {
    /// `attachments`: `StartRun.attachments` of the run being started, in the order given (at most
    /// 50 after `check_start`, 2C). The program turns them into
    /// `CopilotSessionSetup.attachment_paths`; the adapter does not read them itself.
    async fn session_setup(
        &self,
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
        attachments: &[ArtefactId],
    ) -> Result<CopilotSessionSetup, SessionSetupError>;
}

/// The program could not assemble a session's setup. `code` is the error code of the owner whose
/// call failed (for example a Session Supervisor, Installers or Usage Planner code).
#[derive(Debug, thiserror::Error)]
#[error("session setup failed: {code}")]
pub struct SessionSetupError {
    pub code: String,
}

/// The run's setup. `extra_dirs` are the folder grants applied for this run, as for the Claude
/// Local Adapter; the CLI may read them besides the agent folder (CPA-DIR-1).
#[derive(Debug, Clone)]
pub struct CopilotSessionSetup {
    pub agent_folder: PathBuf,
    /// Absolute folders outside the agent folder the session may use (the run's folder grants).
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-DIR-1
    pub extra_dirs: Vec<PathBuf>,
    /// `None` = Copilot's automatic selection (2D-2 `automatic`; 10 C-37).
    pub model: Option<String>,
    pub attachment_paths: Vec<PathBuf>,
    pub package_servers: Vec<PackageMcpServer>,
    /// Present when a persisted Copilot session continues (Recovery after restart; 10 C-05).
    pub resume_session_id: Option<String>,
    /// Crew member runs: the crew run's charter; `None` for every other caller.
    pub crew_charter_id: Option<CrewCharterId>,
}

/// One of the package's own MCP servers.
#[derive(Clone)]
pub struct PackageMcpServer {
    /// 1–64 characters of `[a-zA-Z0-9_.-]`.
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    /// May hold secrets; never logged.
    pub env: BTreeMap<String, String>,
}

impl std::fmt::Debug for PackageMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // *Security*: `env` values are never logged; the names are enough to tell servers apart.
        f.debug_struct("PackageMcpServer")
            .field("name", &self.name)
            .field("command", &self.command)
            .field("args", &self.args)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// The Copilot CLI the person installed (ADR-123). Implemented by the program.
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-2
#[async_trait::async_trait]
pub trait CopilotCliSource: Send + Sync {
    /// The CLI to use now; `None` when none is installed.
    async fn current(&self) -> Option<CopilotCli>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotCli {
    pub path: PathBuf,
    pub version: String,
}
