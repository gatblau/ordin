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

/// `MinutesWriter` over the Ordin Host (OrdinDesktop.md DSK-MIN-1): the notice names the AI service
/// Ordin's runs use, and minutes are written by `OrdinHost::write_minutes`. A host refusal becomes
/// `MIN_WRITE_FAILED`, logged with the host's code only.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
#[cfg(feature = "minutes")]
pub struct MinutesWriterOverHost {
    pub host: Arc<ordin_host::OrdinHost>,
}

#[cfg(feature = "minutes")]
#[async_trait::async_trait]
impl ordin_minutes::MinutesWriter for MinutesWriterOverHost {
    fn writer_name(&self) -> String {
        self.host.minutes_writer_name()
    }

    async fn write(
        &self,
        req: ordin_minutes::WriteMinutesRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<ordin_types::MinutesId, ordin_minutes::MinutesError> {
        let run = ordin_host::WriteMinutesRun {
            workspace_id: req.workspace_id,
            source: req.source,
            source_name: req.source_name,
            recording: req.recording,
            transcript_text: req.transcript_text,
        };
        self.host.write_minutes(run, cancel).await.map_err(|e| {
            tracing::warn!(
                component = "desktop",
                code = e.code(),
                job_id = req.job_id.to_string().as_str(),
                "minutes.write_failed"
            );
            ordin_minutes::MinutesError::WriteFailed
        })
    }
}

/// `MinutesPort` for Ordin's minutes and transcription tools (OrdinDesktop.md DSK-MIN-6): the
/// Journal and the Minutes Service, a `LocalFile` joined to its place's folder and kept inside it,
/// and the export written with DSK-MIN-4's writer, never over an existing file. Every error keeps
/// its owner's code with its sentence in the active locale.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-6
#[cfg(feature = "minutes")]
pub struct MinutesPortOverService {
    pub minutes: Arc<ordin_minutes::MinutesService>,
    pub journal: Arc<ordin_journal::Journal>,
    pub files: Arc<FileService>,
    pub localisation: Arc<Localisation>,
}

#[cfg(feature = "minutes")]
mod minutes_port {
    use std::path::{Path, PathBuf};

    use ordin_host::{DepError, LocalFile, MinutesExportFormat};
    use ordin_localisation::MessageArgs;
    use ordin_service_api::commands::knowledge::KnowledgeExportFormat;
    use ordin_types::domain::places::PlaceKind;
    use ordin_types::id::{MinutesId, MinutesJobId, WorkspaceId};
    use ordin_types::paging::{Cursor, Page};
    use ordin_types::wire::minutes::{Minutes, MinutesJob, MinutesSummary, Transcript};
    use ordin_types::CodedError;

    use super::MinutesPortOverService;

    /// OH-MIN-8: `<stem> (2).<ext>` to `(99)`, then this code.
    const EXPORT_EXISTS: &str = "OH_MINUTES_EXPORT_EXISTS";
    const PATH_INVALID: &str = "OH_MINUTES_PATH_INVALID";
    const FOLDER_UNKNOWN: &str = "OH_MINUTES_FOLDER_UNKNOWN";
    const SAVE_FAILED: &str = "DSK_SAVE_COPY_FAILED";

    impl MinutesPortOverService {
        fn refuse(&self, code: &str, retryable: bool) -> DepError {
            DepError {
                code: code.to_owned(),
                retryable,
                message: self.localisation.error_sentence(code, &MessageArgs::new()),
            }
        }

        fn owner<E: CodedError>(&self, e: &E) -> DepError {
            self.refuse(e.code(), e.retryable())
        }

        /// The place's folder on this computer, resolved; a place of another kind is unknown.
        async fn folder(&self, file: &LocalFile) -> Result<PathBuf, DepError> {
            let place = self
                .files
                .place(file.place_id)
                .await
                .map_err(|_| self.refuse(FOLDER_UNKNOWN, false))?;
            if !matches!(
                place.kind,
                PlaceKind::PersonsFolder | PlaceKind::WorkspaceFolder | PlaceKind::AgentFolder
            ) {
                return Err(self.refuse(FOLDER_UNKNOWN, false));
            }
            std::fs::canonicalize(&place.path).map_err(|_| self.refuse(FOLDER_UNKNOWN, false))
        }

        /// OH-MIN-6: an existing file inside the folder once its links are resolved.
        async fn existing(&self, file: &LocalFile) -> Result<PathBuf, DepError> {
            if !ordin_host::check_path(&file.path) {
                return Err(self.refuse(PATH_INVALID, false));
            }
            let root = self.folder(file).await?;
            let full = std::fs::canonicalize(root.join(relative(&file.path)))
                .map_err(|_| self.refuse(PATH_INVALID, false))?;
            if !full.starts_with(&root) || !full.is_file() {
                return Err(self.refuse(PATH_INVALID, false));
            }
            Ok(full)
        }
    }

    /// `/` and `\` both separate segments (OH-MIN-6).
    fn relative(path: &str) -> PathBuf {
        path.split(['/', '\\']).collect()
    }

    /// OH-MIN-8: the first free name of `<stem>.<ext>`, `<stem> (2).<ext>` … `(99)`.
    fn free_name(folder: &Path, name: &Path) -> Option<PathBuf> {
        let first = folder.join(name);
        if !first.exists() {
            return Some(first);
        }
        let stem = name.file_stem()?.to_string_lossy().into_owned();
        let ext = name.extension()?.to_string_lossy().into_owned();
        (2..=99)
            .map(|n| folder.join(format!("{stem} ({n}).{ext}")))
            .find(|p| !p.exists())
    }

    #[async_trait::async_trait]
    impl ordin_host::MinutesPort for MinutesPortOverService {
        async fn list(
            &self,
            ws: WorkspaceId,
            cursor: Option<Cursor>,
        ) -> Result<Page<MinutesSummary>, DepError> {
            self.journal
                .list_minutes(ws, cursor)
                .await
                .map_err(|e| self.owner(&e))
        }
        async fn get(&self, id: MinutesId) -> Result<Minutes, DepError> {
            self.journal
                .minutes(id)
                .await
                .map_err(|e| self.owner(&e))?
                .ok_or_else(|| self.refuse("MIN_NOT_FOUND", false))
        }
        async fn transcript(&self, id: MinutesId) -> Result<Transcript, DepError> {
            self.minutes
                .transcript(id)
                .await
                .map_err(|e| self.owner(&e))
        }
        async fn save_edit(
            &self,
            id: MinutesId,
            body: String,
            base_revision: u32,
        ) -> Result<Minutes, DepError> {
            self.minutes
                .save_edit(id, body, base_revision)
                .await
                .map_err(|e| self.owner(&e))
        }
        async fn jobs(&self, ws: WorkspaceId) -> Result<Vec<MinutesJob>, DepError> {
            self.minutes.jobs(ws).await.map_err(|e| self.owner(&e))
        }
        async fn cancel_job(&self, job: MinutesJobId) -> Result<MinutesJob, DepError> {
            self.minutes
                .cancel_job(job)
                .await
                .map_err(|e| self.owner(&e))
        }
        async fn retry(&self, job: MinutesJobId) -> Result<MinutesJob, DepError> {
            self.minutes.retry(job).await.map_err(|e| self.owner(&e))
        }
        async fn import(&self, ws: WorkspaceId, file: LocalFile) -> Result<MinutesJob, DepError> {
            let path = self.existing(&file).await?;
            self.minutes
                .import(ws, path)
                .await
                .map_err(|e| self.owner(&e))
        }
        async fn transcribe(
            &self,
            ws: WorkspaceId,
            file: LocalFile,
        ) -> Result<MinutesJob, DepError> {
            let path = self.existing(&file).await?;
            self.minutes
                .transcribe(ws, path)
                .await
                .map_err(|e| self.owner(&e))
        }
        async fn transcription(&self, job: MinutesJobId) -> Result<Transcript, DepError> {
            self.minutes
                .transcription(job)
                .await
                .map_err(|e| self.owner(&e))
        }
        async fn export(
            &self,
            id: MinutesId,
            format: MinutesExportFormat,
            target: LocalFile,
        ) -> Result<String, DepError> {
            if !ordin_host::check_path(&target.path) {
                return Err(self.refuse(PATH_INVALID, false));
            }
            let minutes = self.get(id).await?;
            let root = self.folder(&target).await?;
            let wanted = relative(&target.path);
            let (dir, name) = match (wanted.parent(), wanted.file_name()) {
                (Some(dir), Some(name)) => (root.join(dir), PathBuf::from(name)),
                _ => return Err(self.refuse(PATH_INVALID, false)),
            };
            // The folder the file goes into must exist and stay inside the place's folder.
            let dir = std::fs::canonicalize(&dir).map_err(|_| self.refuse(PATH_INVALID, false))?;
            if !dir.starts_with(&root) {
                return Err(self.refuse(PATH_INVALID, false));
            }
            let path = free_name(&dir, &name).ok_or_else(|| self.refuse(EXPORT_EXISTS, false))?;
            let format = match format {
                MinutesExportFormat::Pdf => KnowledgeExportFormat::Pdf,
                MinutesExportFormat::Docx => KnowledgeExportFormat::Docx,
            };
            let bytes = crate::dashboard_export::render_page(&minutes.title, &minutes.body, format);
            crate::shell::write_atomically(&path, &bytes)
                .map_err(|_| self.refuse(SAVE_FAILED, true))?;
            let written = path.strip_prefix(&root).unwrap_or(&path);
            Ok(written
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::free_name;

        #[test]
        fn an_export_never_replaces_a_file() {
            let dir =
                std::env::temp_dir().join(format!("ordin-dsk-min-6-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&dir).unwrap();
            let name = std::path::Path::new("Supplier call.pdf");
            assert_eq!(free_name(&dir, name), Some(dir.join("Supplier call.pdf")));
            std::fs::write(dir.join("Supplier call.pdf"), b"x").unwrap();
            assert_eq!(
                free_name(&dir, name),
                Some(dir.join("Supplier call (2).pdf"))
            );
            for n in 2..=99 {
                std::fs::write(dir.join(format!("Supplier call ({n}).pdf")), b"x").unwrap();
            }
            assert_eq!(free_name(&dir, name), None);
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
}
