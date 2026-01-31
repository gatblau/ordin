//! Program-side implementations of the port traits the modules of rows 3.1–3.23 own (OrdinDesktop.md
//! *Program-side implementations and calls*). Each one delegates to the module the trait names and
//! adds no decision. A module built after the adapter is handed over is *late-bound* (step 3 note):
//! the adapter reads it from [`Late`], which the program fills right after that module's row, and a
//! call before then gets the adapter's permanent answer.
//
// SPEC-GAP-4: `SupervisorHost::startup_items` is "over the Rules Engine, the MCP Front's offered tools
// and the Installers' unavailable parts", but no spec states how an `OfferedTool` (which carries no
// `ToolOrigin`) or an install record maps to a `StartUpReportItem`, nor the wording of a
// `VerbNotOffered` line. Only the Rules Engine's own items are returned.
//
// SPEC-GAP-5: `SupervisorHost::remote_route` is synchronous, but the remote agents' ids exist only
// behind the Organisation Profile's asynchronous `remote_agents()`. OQ-REC-1 is settled: the
// synchronous `remote_agent_route` accessor comes with the `hosted-agents` and `slack` switches, and
// the marker stays until then. `None` is returned (the Session Supervisor then refuses the run with
// `RA_TARGET_MISMATCH`); a build with no remote adapter (`hosted-agents` and `slack` off) registers
// none anyway.
// spec: ordin-desktop/components/OrdinDesktop#OQ-REC-1
//
// SPEC-GAP-6: `SupervisorHost::run_resuming` is "over the Rules Engine's `resume_run`", whose error
// the specs do not map. A run with no Rules record (`RULE_RUN_UNKNOWN`) resumes; any other error
// keeps the run from resuming, named by its code (fail-safe, as the Rules Engine is elsewhere).

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use ordin_agent_folders::AgentFolderManager;
use ordin_files::{AgentWriteTarget, FileService};
use ordin_hook_relay::{AgentWriteError, AgentWrites, WriteTarget};
use ordin_localisation::Localisation;
use ordin_memory_service::{AgentMemoryRules, MemoryService};
use ordin_organisation_profile::Clock;
use ordin_package_inspector::PackageInspector;
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, Proposal, RulesEngine, RulesError, RulesHost,
    RunKey,
};
use ordin_runtime_adapter::{AdapterKind, AdapterVerb, RunTarget, StartRun};
use ordin_session_supervisor::{
    ContentCheck, ContentPoint, HostError, RunControls, SessionSupervisor, SupervisorHost,
};
use ordin_types::domain::agent_profile::{CallPlace, ModelFailureCause};
use ordin_types::domain::runs::Run;
use ordin_types::time::Timestamp;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{CodedError, RemoteAgentId, RunId};

use crate::error::AGENT_PROFILE_ONLY;
use crate::words::Words;

/// The modules the adapters reach that are built after the adapter is handed over, each set once
/// right after its row.
#[derive(Default)]
pub struct Late {
    /// Row 3.2: set right after it, for the readers built later that name a remote agent.
    pub profile: OnceLock<Arc<ordin_organisation_profile::OrganisationProfileService>>,
    pub rules: OnceLock<Arc<RulesEngine>>,
    pub supervisor: OnceLock<Arc<SessionSupervisor>>,
    pub inspector: OnceLock<Arc<PackageInspector>>,
    pub files: OnceLock<Arc<FileService>>,
    pub memory: OnceLock<Arc<MemoryService>>,
    /// Row 3.24.
    pub orchestrator: OnceLock<Arc<ordin_crew_orchestrator::CrewOrchestrator>>,
    /// Row 3.35.
    pub workspaces: OnceLock<Arc<ordin_workspaces::WorkspaceService>>,
    /// Row 3.37.
    pub push: OnceLock<Arc<ordin_push::PushService>>,
    /// Row 3.38.
    pub knowledge: OnceLock<Arc<ordin_knowledge::KnowledgeService>>,
    /// Row 3.40.
    pub packages: OnceLock<Arc<ordin_package_catalogue::PackageCatalogue>>,
    /// Row 3.41.
    pub ordin_host: OnceLock<Arc<ordin_host::OrdinHost>>,
    /// Row 3.43.
    pub setup: OnceLock<Arc<ordin_setup::SetupAssistant>>,
    /// Row 3.49: the Command Bridge's screen streams.
    pub streams: OnceLock<ordin_command_bridge::ScreenStreams>,
    /// The run register of DSK-REC-8, shared by the `SessionSetupSource`, the `SupervisorHost` and
    /// `InstanceActivity`.
    pub runs: crate::hosts_run::RunRegister,
}

/// The wall clock for the Organisation Profile (`ProfileOptions.clock`).
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

// ---------------------------------------------------------------- SupervisorHost (row 3.19)

/// `SupervisorHost` over the Rules Engine, the Package Inspector and the Memory Service.
pub struct DesktopSupervisorHost {
    pub late: Arc<Late>,
}

#[async_trait::async_trait]
impl SupervisorHost for DesktopSupervisorHost {
    async fn startup_items(
        &self,
        _run: &Run,
        target: &RunTarget,
        _unsupported: &[AdapterVerb],
    ) -> Vec<StartUpReportItem> {
        // SPEC-GAP-4: the Rules Engine's items only.
        match (target, self.late.rules.get()) {
            (RunTarget::AgentInstance { agent_instance_id }, Some(rules)) => {
                rules.startup_items(*agent_instance_id)
            }
            _ => Vec::new(),
        }
    }

    fn remote_route(&self, _remote_agent_id: RemoteAgentId) -> Option<AdapterKind> {
        // SPEC-GAP-5.
        None
    }

    async fn expand_command(
        &self,
        _target: &RunTarget,
        _name: &str,
        _arguments: &str,
    ) -> Result<String, HostError> {
        Err(HostError {
            code: AGENT_PROFILE_ONLY.to_owned(),
        })
    }

    async fn run_controls(&self, _run_id: RunId) -> RunControls {
        RunControls::default()
    }

    async fn run_resuming(&self, run_id: RunId) -> Option<String> {
        let rules = self.late.rules.get()?;
        match rules.resume_run(run_id).await {
            Ok(control) => control,
            // SPEC-GAP-6.
            Err(RulesError::RunUnknown) => None,
            Err(error) => {
                tracing::warn!(
                    component = "desktop",
                    run_id = run_id.to_string(),
                    code = error.code(),
                    "run_resume_refused"
                );
                Some(error.code().to_owned())
            }
        }
    }

    /// FU-087: the Rules Engine releases the run's leases and drops its facts; then the run's
    /// `Session` lease is dropped from the run register (DSK-REC-8 step 2).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-8
    async fn run_ended(&self, run_id: RunId) {
        if let Some(rules) = self.late.rules.get() {
            rules.end_run(run_id).await;
        }
        self.late.runs.release(run_id);
    }

    /// Calls the Slack Thread Adapter's `abandon` for `SlackThread`; nothing for any other kind. No
    /// build of this slice registers the Slack Thread Adapter (its crate has no `abandon` yet).
    fn run_abandoned(&self, _run_id: RunId, _adapter: AdapterKind) {}

    fn reply_for(&self, _cause: ModelFailureCause, _place: CallPlace) -> Option<String> {
        None
    }

    fn retry_after_block(&self, _carried_gathered_context: bool, _already_retried: bool) -> bool {
        false
    }

    /// The desktop has no content checks (ADR-107; Playbook §6.4).
    async fn check_content(
        &self,
        _run_id: RunId,
        _point: ContentPoint,
        _text: &str,
    ) -> ContentCheck {
        ContentCheck::Passed
    }

    /// ADR-111: the agent's memory rules from its Memory part, set or cleared before the run starts.
    async fn run_starting(&self, start: &StartRun) {
        let RunTarget::AgentInstance { agent_instance_id } = start.target else {
            return;
        };
        let (Some(inspector), Some(memory)) = (self.late.inspector.get(), self.late.memory.get())
        else {
            return;
        };
        let failed = |code: &str| {
            tracing::warn!(
                component = "desktop",
                agent_instance_id = agent_instance_id.to_string(),
                code,
                "desktop.memory_rules_failed"
            );
        };
        match inspector.memory_contract(agent_instance_id).await {
            Ok(Some(contract)) => {
                let rules = AgentMemoryRules {
                    retains: contract.retains,
                    excludes: contract.excludes,
                    max_age_days: if contract.unbounded {
                        None
                    } else {
                        contract.max_age_days
                    },
                    store_location: contract.store_location,
                };
                if let Err(error) = memory.set_agent_rules(agent_instance_id, rules) {
                    failed(error.code());
                }
            }
            Ok(None) => memory.clear_agent_rules(agent_instance_id),
            Err(error) => failed(error.code()),
        }
    }
}

// ---------------------------------------------------------------- RulesHost (row 3.14)

/// `RulesHost`: path facts over the run's agent folder, run facts over the Session Supervisor, no
/// settings (the Settings Store is agent-profile only), words from the catalogue (DSK-REC-1,
/// DSK-REC-3).
pub struct DesktopRulesHost {
    pub agent_folders: Arc<AgentFolderManager>,
    pub localisation: Arc<Localisation>,
    pub late: Arc<Late>,
}

impl DesktopRulesHost {
    /// `path` inside the run's agent folder. A path that is absolute or climbs out of the folder is
    /// not a fact this host answers.
    async fn inside(&self, run: &RunKey, path: &str) -> Result<PathBuf, FactError> {
        let folder = self
            .agent_folders
            .get(run.agent_instance_id)
            .await
            .map_err(|e| FactError(e.code().to_owned()))?;
        let relative = Path::new(path);
        let plain = relative
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        if !plain {
            return Err(FactError(format!("path outside the agent folder: {path}")));
        }
        Ok(folder.folder_path.join(relative))
    }

    fn supervisor(&self) -> Result<&Arc<SessionSupervisor>, FactError> {
        self.late
            .supervisor
            .get()
            .ok_or_else(|| FactError("the Session Supervisor is not built yet".to_owned()))
    }
}

fn io_fact(error: std::io::Error) -> FactError {
    FactError(error.kind().to_string())
}

#[async_trait::async_trait]
impl RulesHost for DesktopRulesHost {
    async fn path_exists(&self, run: &RunKey, path: &str) -> Result<bool, FactError> {
        let at = self.inside(run, path).await?;
        tokio::fs::try_exists(&at).await.map_err(io_fact)
    }

    async fn path_contains(&self, run: &RunKey, path: &str, text: &str) -> Result<bool, FactError> {
        Ok(self
            .read_text(run, path)
            .await?
            .is_some_and(|content| content.contains(text)))
    }

    async fn read_text(&self, run: &RunKey, path: &str) -> Result<Option<String>, FactError> {
        let at = self.inside(run, path).await?;
        match tokio::fs::read_to_string(&at).await {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_fact(error)),
        }
    }

    async fn append_line(&self, run: &RunKey, path: &str, line: &str) -> Result<(), FactError> {
        use tokio::io::AsyncWriteExt;
        let at = self.inside(run, path).await?;
        if let Some(parent) = at.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io_fact)?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&at)
            .await
            .map_err(io_fact)?;
        file.write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(io_fact)?;
        file.flush().await.map_err(io_fact)
    }

    async fn remove(&self, run: &RunKey, path: &str) -> Result<(), FactError> {
        let at = self.inside(run, path).await?;
        match tokio::fs::remove_file(&at).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_fact(error)),
        }
    }

    async fn runs_in_progress(&self, command: &str) -> Result<u32, FactError> {
        self.supervisor()?
            .runs_with_command(command)
            .await
            .map_err(|e| FactError(e.code().to_owned()))
    }

    async fn paused_run_with_command(
        &self,
        commands: &[String],
        except: RunId,
    ) -> Result<bool, FactError> {
        self.supervisor()?
            .paused_with_command(commands, except)
            .await
            .map_err(|e| FactError(e.code().to_owned()))
    }

    async fn run_ended(&self, run_id: &str) -> Result<Option<bool>, FactError> {
        let Ok(run_id) = RunId::parse(run_id) else {
            return Ok(None);
        };
        self.supervisor()?
            .has_ended(run_id)
            .await
            .map_err(|e| FactError(e.code().to_owned()))
    }

    async fn setting_value(
        &self,
        _scope: Option<&str>,
        _name: &str,
    ) -> Result<serde_json::Value, FactError> {
        Ok(serde_json::Value::Null)
    }

    async fn setting_rules(&self, _scope: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }

    fn word(&self, _run: &RunKey, proposal: &Proposal) -> CardWording {
        Words {
            localisation: &self.localisation,
        }
        .proposal(proposal)
    }

    fn plain_line(&self, line: &ControlLine) -> String {
        Words {
            localisation: &self.localisation,
        }
        .rules_line(line)
    }
}

// ---------------------------------------------------------------- AgentWrites (row 3.23)

/// The Hook Relay's `AgentWrites` over the File Service of row 3.22, a delegation without decisions
/// (amendment January 2026, DSK-WIRE-1; FileService.md SA-RECON-14).
// spec: ordin-desktop/components/OrdinDesktop#DSK-WIRE-1
pub struct FileAgentWrites {
    pub files: Arc<FileService>,
}

#[async_trait::async_trait]
impl AgentWrites for FileAgentWrites {
    async fn open(
        &self,
        run_id: RunId,
        tool: &str,
        target: WriteTarget,
    ) -> Result<Option<String>, AgentWriteError> {
        let target = match target {
            WriteTarget::File(path) => AgentWriteTarget::File(path),
            WriteTarget::Anywhere => AgentWriteTarget::Anywhere,
        };
        self.files
            .open_agent_write(run_id, tool, target)
            .await
            .map_err(|_| AgentWriteError)
    }

    async fn settle(&self, claim: &str) -> Result<(), AgentWriteError> {
        self.files
            .settle_agent_write(claim)
            .await
            .map_err(|_| AgentWriteError)
    }

    async fn run_ended(&self, run_id: RunId) -> Result<(), AgentWriteError> {
        self.files
            .agent_run_ended(run_id)
            .await
            .map_err(|_| AgentWriteError)
    }
}
