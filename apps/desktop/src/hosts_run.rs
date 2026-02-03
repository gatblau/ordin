//! Program-side implementations of the port traits the modules of rows 3.24–3.40 own (OrdinDesktop.md
//! *Program-side implementations and calls*). Each one delegates to the module the trait names and
//! adds no decision. A module built after the adapter is handed over is read from [`Late`]; a call
//! before it is set gets the adapter's permanent answer.
//
// SPEC-GAP-8: `CrewHost::agent_status` of a remote agent member (`AgentRef::RemoteAgent`): the
// remote agents' ids exist only behind the Organisation Profile's asynchronous `remote_agents()`,
// whose entries carry no version. OQ-REC-1 is settled, and its answer comes with the `hosted-agents`
// and `slack` switches; the marker stays until then. A remote member is answered `Missing`; a build
// with no remote adapter has none anyway.
// spec: ordin-desktop/components/OrdinDesktop#OQ-REC-1
//
// A session's `attachment_paths` follow DSK-REC-26 and DSK-REC-29, `shell_allowed` DSK-REC-27, and
// `package_servers` is empty in R1 (DSK-REC-28: a package declares no program to start).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use ordin_agent_folders::{AgentFolderManager, FolderLease, LockMode, SessionAccess};
use ordin_claude_adapter::{SessionSetup, SessionSetupError, SessionSetupSource};
use ordin_command_bridge::StreamKind;
use ordin_confluence::ConfluenceSettings;
use ordin_crew_orchestrator::{AgentAvailability, CrewHost, CrewInput, CrewView};
use ordin_files::FileService;
use ordin_installers::{HarnessLaunch, InstallError, Installers};
use ordin_journal::Journal;
use ordin_localisation::Localisation;
use ordin_mcp_front::McpFront;
use ordin_organisation_profile::{
    HarnessName, OrganisationProfileService, ProfileSnapshot, ServerLanguage, SnapshotSource,
};
use ordin_package_catalogue::{HarnessSettings, InstanceActivity};
use ordin_projects::{
    ApprovedServers, ProjectSettings, ServerInstaller, ServerLaunch, ServerPin, StreamId,
    StreamPayload, StreamSink,
};
use ordin_push::{StoreFailure, WorkingCopyEntry, WorkingCopyStore};
use ordin_rules::CardWording;
use ordin_runtime_adapter::AdapterKind;
use ordin_types::common::AgentRef;
use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::domain::places::{Artefact, ArtefactLocation, WorkingCopyState};
use ordin_types::domain::runs::{CrewCharter, RunOrigin, WorkflowStep};
use ordin_types::domain::HarnessKind;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{
    AgentInstanceId, ArtefactId, CodedError, CrewCharterId, RemoteAgentId, RunId, WorkflowId,
    WorkingCopyItemId, WorkspaceId,
};
use ordin_usage_planner::UsagePlanner;
use ordin_workspaces::WorkspaceService;
use tokio::sync::{broadcast, watch};

use crate::error::START_FAILED;
use crate::hosts::Late;
use crate::words::Words;

// ---------------------------------------------------------------- CrewHost (row 3.24)

/// `CrewHost` over the Package Catalogue, the Ordin Host, the File Service and the Rules Engine.
pub struct DesktopCrewHost {
    pub late: Arc<Late>,
    pub localisation: Arc<Localisation>,
    pub profile: Arc<OrganisationProfileService>,
    pub mcp_front: Arc<McpFront>,
}

#[async_trait::async_trait]
impl CrewHost for DesktopCrewHost {
    async fn agent_status(
        &self,
        workspace_id: WorkspaceId,
        member: &AgentRef,
    ) -> AgentAvailability {
        let AgentRef::AgentInstance { agent_instance_id } = member else {
            // SPEC-GAP-8.
            return AgentAvailability::Missing;
        };
        let Some(packages) = self.late.packages.get() else {
            return AgentAvailability::Missing;
        };
        match packages.get(*agent_instance_id).await {
            Ok(agent)
                if agent.instance.status != AgentInstanceStatus::Removed
                    && agent.workspaces.contains(&workspace_id) =>
            {
                AgentAvailability::Present {
                    version: agent.instance.package_version,
                }
            }
            _ => AgentAvailability::Missing,
        }
    }

    async fn saved_agent_versions(
        &self,
        workflow_id: WorkflowId,
        version: u32,
    ) -> Vec<(AgentRef, String)> {
        let Some(host) = self.late.ordin_host.get() else {
            return Vec::new();
        };
        host.saved_agent_versions(workflow_id, version)
            .await
            .unwrap_or_default()
    }

    async fn workflow_steps(
        &self,
        workspace_id: WorkspaceId,
        workflow_id: WorkflowId,
    ) -> Option<Vec<WorkflowStep>> {
        let host = self.late.ordin_host.get()?;
        host.workflow_steps(workspace_id, workflow_id)
            .await
            .ok()
            .flatten()
    }

    /// R1 limit: every input counts as changed, so a `document_changed` condition takes its first
    /// path and never skips work the person asked for (G-REC-2: no owner keeps the versions an
    /// input had at the workflow's previous run).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-6
    async fn document_changed(&self, workspace_id: WorkspaceId, input: &CrewInput) -> bool {
        tracing::debug!(
            component = "desktop",
            workspace_id = workspace_id.to_string().as_str(),
            label = input.label.as_str(),
            "crew_document_changed_assumed"
        );
        true
    }

    fn word_step_approval(
        &self,
        view: &CrewView,
        step_label: &str,
        _member: &AgentRef,
        line: &str,
    ) -> CardWording {
        Words {
            localisation: &self.localisation,
        }
        .crew_step(view, step_label, line)
    }

    async fn startup_items(&self, charter: &CrewCharter) -> Vec<StartUpReportItem> {
        // SPEC-GAP-4: the Rules Engine's items only.
        let Some(rules) = self.late.rules.get() else {
            return Vec::new();
        };
        charter
            .members
            .iter()
            .filter_map(|m| match m.member {
                AgentRef::AgentInstance { agent_instance_id } => Some(agent_instance_id),
                AgentRef::RemoteAgent { .. } => None,
            })
            .flat_map(|id| rules.startup_items(id))
            .collect()
    }

    fn remote_route(&self, _remote_agent_id: RemoteAgentId) -> Option<AdapterKind> {
        // SPEC-GAP-5.
        None
    }
}

// ---------------------------------------------------------------- the run register (DSK-REC-8)

/// One in-memory register `RunId → (AgentInstanceId, FolderLease)`: each local run's agent instance
/// and the `Session` lease held for the run's life. Filled by the `SessionSetupSource`, emptied by
/// the `SupervisorHost`'s `run_ended`, and read by `InstanceActivity` (DSK-REC-8, DSK-REC-10).
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-8
pub struct RunRegister {
    entries: Mutex<HashMap<RunId, (AgentInstanceId, FolderLease)>>,
    /// Sent an instance's id when its last run ends (capacity 64).
    idle: broadcast::Sender<AgentInstanceId>,
}

impl Default for RunRegister {
    fn default() -> RunRegister {
        let (idle, _) = broadcast::channel(64);
        RunRegister {
            entries: Mutex::new(HashMap::new()),
            idle,
        }
    }
}

impl RunRegister {
    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<RunId, (AgentInstanceId, FolderLease)>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Stores `lease` under `run_id`; an earlier entry for the run is replaced and its lease dropped.
    pub fn hold(&self, run_id: RunId, agent_instance_id: AgentInstanceId, lease: FolderLease) {
        let earlier = self.entries().insert(run_id, (agent_instance_id, lease));
        drop(earlier);
    }

    /// Removes the run's entry and drops its lease; `idle` is sent the instance's id when that was
    /// its last run. An absent entry is no error.
    pub fn release(&self, run_id: RunId) {
        let (removed, last) = {
            let mut entries = self.entries();
            let removed = entries.remove(&run_id);
            let last = removed
                .as_ref()
                .is_some_and(|(id, _)| !entries.values().any(|(other, _)| other == id));
            (removed, last)
        };
        if let Some((agent_instance_id, lease)) = removed {
            drop(lease);
            if last {
                // A send with no receiver is no error.
                let _ = self.idle.send(agent_instance_id);
            }
        }
    }

    /// On quit, after the Session Supervisor's shutdown: every lease is dropped (DSK-REC-8 step 3).
    pub fn release_all(&self) {
        let all: Vec<_> = self.entries().drain().collect();
        drop(all);
    }

    /// Whether an entry with this agent instance exists.
    pub fn running(&self, id: AgentInstanceId) -> bool {
        self.entries().values().any(|(other, _)| *other == id)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentInstanceId> {
        self.idle.subscribe()
    }
}

// ---------------------------------------------------------------- SessionSetupSource (row 3.32)

/// `SessionSetupSource` of the Claude Local Adapter over the Installers' record, the Session
/// Supervisor's run, the Crew Orchestrator's crew run and the Agent Folder Manager's grants.
pub struct DesktopSessionSetup {
    pub late: Arc<Late>,
    pub installers: Arc<Installers>,
    pub usage: Arc<UsagePlanner>,
    pub agent_folders: Arc<AgentFolderManager>,
    /// Reads one attached artefact (JRNL-REC-1).
    pub journal: Arc<Journal>,
    /// Reads the folder of an attached artefact in a place.
    pub files: Arc<FileService>,
    /// Reads whether a place holds a project, for `shell_allowed` (DSK-REC-27).
    pub projects: Arc<ordin_projects::ProjectService>,
}

fn setup_error(code: &str) -> SessionSetupError {
    SessionSetupError {
        code: code.to_owned(),
    }
}

impl DesktopSessionSetup {
    /// The crew charter of a crew member's run; `None` for any other origin.
    async fn crew_charter(
        &self,
        origin: RunOrigin,
    ) -> Result<Option<CrewCharterId>, SessionSetupError> {
        match origin {
            RunOrigin::CrewRound { crew_run_id, .. } => {
                let orchestrator = self
                    .late
                    .orchestrator
                    .get()
                    .ok_or_else(|| setup_error(START_FAILED))?;
                let (crew_run, _) = orchestrator
                    .get(crew_run_id)
                    .await
                    .map_err(|e| setup_error(e.code()))?;
                Ok(Some(crew_run.crew_charter_id))
            }
            RunOrigin::Conversation { .. } | RunOrigin::Task { .. } => Ok(None),
        }
    }

    /// `SessionSetup.shell_allowed`: a system-changing package on a workspace with a project. A
    /// failed read is `false`, logged; the session still starts, without a shell.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-27
    async fn shell_allowed(
        &self,
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
        workspace_id: WorkspaceId,
    ) -> bool {
        let reads = async {
            let packages = self
                .late
                .packages
                .get()
                .ok_or_else(|| START_FAILED.to_owned())?;
            let authority = packages
                .get(agent_instance_id)
                .await
                .map_err(|e| e.code().to_owned())?
                .instance
                .authority;
            if authority != ordin_types::domain::agents::Authority::SystemChanging {
                return Ok(false);
            }
            let workspaces = self
                .late
                .workspaces
                .get()
                .ok_or_else(|| START_FAILED.to_owned())?;
            let places = workspaces
                .places(workspace_id)
                .await
                .map_err(|e| e.code().to_owned())?;
            for place in places {
                let project = self
                    .projects
                    .project_for_place(place.id)
                    .await
                    .map_err(|e| e.code().to_owned())?;
                if project.is_some() {
                    return Ok(true);
                }
            }
            Ok::<bool, String>(false)
        };
        match reads.await {
            Ok(allowed) => allowed,
            Err(code) => {
                tracing::warn!(
                    component = "desktop",
                    run_id = %run_id,
                    code = code.as_str(),
                    "shell_allowed_unread"
                );
                false
            }
        }
    }

    /// `apply_grants` at session start and on resume; `session_access` when a resumed session's
    /// setup is rebuilt and its grants are still held.
    async fn grants(
        &self,
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
        resumed: bool,
        workspace_id: WorkspaceId,
        crew_charter_id: Option<CrewCharterId>,
    ) -> Result<Option<SessionAccess>, SessionSetupError> {
        if resumed {
            if let Ok(access) = self.agent_folders.session_access(run_id).await {
                return Ok(Some(access));
            }
        }
        self.agent_folders
            .apply_grants(run_id, agent_instance_id, workspace_id, crew_charter_id)
            .await
            .map(Some)
            .map_err(|e| setup_error(e.code()))
    }
}

#[async_trait::async_trait]
impl SessionSetupSource for DesktopSessionSetup {
    async fn session_setup(
        &self,
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
        attachments: &[ArtefactId],
    ) -> Result<SessionSetup, SessionSetupError> {
        // DSK-REC-8: the `Session` lease first, held for the run's life.
        let lease = self
            .agent_folders
            .lock(agent_instance_id, LockMode::Session)
            .await
            .map_err(|e| setup_error(e.code()))?;
        let record = self
            .installers
            .record(agent_instance_id)
            .await
            .map_err(|e| setup_error(e.code()))?;
        let HarnessLaunch::ClaudeCode {
            working_dir,
            append_system_prompt_file,
            plugin_dir,
            plugin_name,
        } = record.launch
        else {
            let error = InstallError::HarnessUnsupported {
                harness: record.harness,
            };
            return Err(setup_error(error.code()));
        };
        let supervisor = self
            .late
            .supervisor
            .get()
            .ok_or_else(|| setup_error(START_FAILED))?;
        // DSK-REC-9: the workspace of any run, a task run included, from the run view.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-9
        let view = supervisor
            .get(run_id)
            .await
            .map_err(|e| setup_error(e.code()))?;
        let workspace_id = view.workspace_id;
        let run = view.run;
        let crew_charter_id = self.crew_charter(run.origin).await?;
        let resume_session_id = run.runtime_session_id;
        let access = self
            .grants(
                run_id,
                agent_instance_id,
                resume_session_id.is_some(),
                workspace_id,
                crew_charter_id,
            )
            .await?;
        let extra_dirs: Vec<std::path::PathBuf> = access
            .map(|a| a.grants.into_iter().map(|g| g.path).collect())
            .unwrap_or_default();
        let attachment_paths = attachment_paths(
            self,
            AttachmentRun {
                run_id,
                crew_round: matches!(run.origin, RunOrigin::CrewRound { .. }),
                agent_folder: &working_dir,
                extra_dirs: &extra_dirs,
            },
            attachments,
        )
        .await?;
        self.late.runs.hold(run_id, agent_instance_id, lease);
        let mut setup = assemble(SetupParts {
            working_dir,
            append_system_prompt_file,
            plugin_dir,
            plugin_name,
            extra_dirs,
            resume_session_id,
            crew_charter_id,
        });
        setup.attachment_paths = attachment_paths;
        setup.shell_allowed = self
            .shell_allowed(run_id, agent_instance_id, workspace_id)
            .await;
        Ok(setup)
    }
}

// ---------------------------------------------------------------- SessionSetupSource (row 3.33)

/// `SessionSetupSource` of the Copilot Local Adapter over the same modules as the Claude one: the
/// `Session` lease held for the run's life, the Installers' `HarnessLaunch::Copilot`, the crew run's
/// charter and the attachments. `CopilotSessionSetup` carries no granted folders, so no grant is
/// applied and attachments are checked against the agent folder only (OrdinDesktop.md
/// `SessionSetupSource` row). `model` is `None` (DSK-REC-7); no package server (DSK-REC-28).
#[cfg(feature = "copilot")]
pub struct DesktopCopilotSessionSetup {
    pub inner: Arc<DesktopSessionSetup>,
}

#[cfg(feature = "copilot")]
fn copilot_setup_error(error: SessionSetupError) -> ordin_copilot_adapter::SessionSetupError {
    ordin_copilot_adapter::SessionSetupError { code: error.code }
}

#[cfg(feature = "copilot")]
#[async_trait::async_trait]
impl ordin_copilot_adapter::SessionSetupSource for DesktopCopilotSessionSetup {
    async fn session_setup(
        &self,
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
        attachments: &[ArtefactId],
    ) -> Result<ordin_copilot_adapter::CopilotSessionSetup, ordin_copilot_adapter::SessionSetupError>
    {
        let this = &self.inner;
        // DSK-REC-8: the `Session` lease first, held for the run's life.
        let lease = this
            .agent_folders
            .lock(agent_instance_id, LockMode::Session)
            .await
            .map_err(|e| copilot_setup_error(setup_error(e.code())))?;
        let record = this
            .installers
            .record(agent_instance_id)
            .await
            .map_err(|e| copilot_setup_error(setup_error(e.code())))?;
        let HarnessLaunch::Copilot { working_dir } = record.launch else {
            let error = InstallError::HarnessUnsupported {
                harness: record.harness,
            };
            return Err(copilot_setup_error(setup_error(error.code())));
        };
        let supervisor = this
            .late
            .supervisor
            .get()
            .ok_or_else(|| copilot_setup_error(setup_error(START_FAILED)))?;
        let view = supervisor
            .get(run_id)
            .await
            .map_err(|e| copilot_setup_error(setup_error(e.code())))?;
        let run = view.run;
        let crew_charter_id = this
            .crew_charter(run.origin)
            .await
            .map_err(copilot_setup_error)?;
        let attachment_paths = attachment_paths(
            this.as_ref(),
            AttachmentRun {
                run_id,
                crew_round: matches!(run.origin, RunOrigin::CrewRound { .. }),
                agent_folder: &working_dir,
                extra_dirs: &[],
            },
            attachments,
        )
        .await
        .map_err(copilot_setup_error)?;
        this.late.runs.hold(run_id, agent_instance_id, lease);
        Ok(ordin_copilot_adapter::CopilotSessionSetup {
            agent_folder: working_dir,
            model: None,
            attachment_paths,
            package_servers: Vec::new(),
            resume_session_id: run.runtime_session_id,
            crew_charter_id,
        })
    }
}

// ---------------------------------------------------------------- attachments (DSK-REC-26, 29)

/// The reads DSK-REC-26 steps 2 and 3 make for one attached artefact; each error is the owner's code.
#[async_trait::async_trait]
pub trait AttachmentReads: Send + Sync {
    /// `journal.artefact(id)` (JRNL-REC-1).
    async fn artefact(&self, id: ArtefactId) -> Result<Option<Artefact>, SessionSetupError>;
    /// The folder an artefact's `path` is relative to: a place's `Place.path`, or an agent
    /// instance's `agent_folder_path`.
    async fn folder(
        &self,
        location: ArtefactLocation,
    ) -> Result<std::path::PathBuf, SessionSetupError>;
}

#[async_trait::async_trait]
impl AttachmentReads for DesktopSessionSetup {
    async fn artefact(&self, id: ArtefactId) -> Result<Option<Artefact>, SessionSetupError> {
        self.journal
            .artefact(id)
            .await
            .map_err(|e| setup_error(e.code()))
    }

    async fn folder(
        &self,
        location: ArtefactLocation,
    ) -> Result<std::path::PathBuf, SessionSetupError> {
        match location {
            ArtefactLocation::Place { place_id } => self
                .files
                .place(place_id)
                .await
                .map(|place| place.path.into())
                .map_err(|e| setup_error(e.code())),
            ArtefactLocation::AgentFolder { agent_instance_id } => {
                let packages = self
                    .late
                    .packages
                    .get()
                    .ok_or_else(|| setup_error(START_FAILED))?;
                packages
                    .get(agent_instance_id)
                    .await
                    .map(|agent| agent.instance.agent_folder_path.into())
                    .map_err(|e| setup_error(e.code()))
            }
        }
    }
}

/// The run whose attachments are resolved, and the folders its session may read.
pub struct AttachmentRun<'a> {
    pub run_id: RunId,
    /// `RunView.origin` is `RunOrigin::CrewRound` (DSK-REC-29, crew-round rule).
    pub crew_round: bool,
    /// The run's own agent folder (`SessionSetup.agent_folder`).
    pub agent_folder: &'a std::path::Path,
    /// The folder grants applied for this run (`SessionSetup.extra_dirs`, DSK-REC-9).
    pub extra_dirs: &'a [std::path::PathBuf],
}

const ATTACHMENT_NOT_GRANTED: &str = "RA_ATTACHMENT_NOT_GRANTED";
const MAX_ATTACHMENTS: usize = 50;

/// `SessionSetup.attachment_paths` from the start request's attachments: duplicates removed (first
/// kept), at most 50, each artefact read, joined with its folder, resolved through symbolic links and
/// required to be an existing regular file under the agent folder or a granted folder. For a crew
/// round, an existing file outside those folders is left out and logged; any other failure fails
/// the whole setup, so a session never starts with an attachment silently missing.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-26
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-29
pub async fn attachment_paths(
    reads: &dyn AttachmentReads,
    run: AttachmentRun<'_>,
    attachments: &[ArtefactId],
) -> Result<Vec<std::path::PathBuf>, SessionSetupError> {
    let mut ids: Vec<ArtefactId> = Vec::with_capacity(attachments.len());
    for id in attachments {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    if ids.len() > MAX_ATTACHMENTS {
        return Err(setup_error("RA_INPUT_TOO_LARGE"));
    }
    let mut allowed = Vec::with_capacity(run.extra_dirs.len() + 1);
    for dir in std::iter::once(run.agent_folder).chain(run.extra_dirs.iter().map(|d| d.as_path())) {
        if let Ok(dir) = tokio::fs::canonicalize(dir).await {
            allowed.push(dir);
        }
    }
    let mut paths = Vec::with_capacity(ids.len());
    for id in ids {
        let artefact = reads
            .artefact(id)
            .await?
            .ok_or_else(|| setup_error(ATTACHMENT_NOT_GRANTED))?;
        let folder = reads.folder(artefact.location).await?;
        let resolved = match tokio::fs::canonicalize(folder.join(&artefact.path)).await {
            Ok(path) if tokio::fs::metadata(&path).await.is_ok_and(|m| m.is_file()) => path,
            _ => return Err(setup_error(ATTACHMENT_NOT_GRANTED)),
        };
        if allowed.iter().any(|dir| resolved.starts_with(dir)) {
            paths.push(resolved);
        } else if run.crew_round {
            tracing::info!(
                component = "desktop",
                run_id = %run.run_id,
                artefact_id = id.to_string().as_str(),
                "crew_input_not_attached"
            );
        } else {
            return Err(setup_error(ATTACHMENT_NOT_GRANTED));
        }
    }
    tracing::debug!(
        component = "desktop",
        run_id = %run.run_id,
        count = paths.len(),
        "session_attachments"
    );
    Ok(paths)
}

/// What the program reads for one run's session setup: the Installers' launch, the Agent Folder
/// Manager's grants and the run's own row.
pub struct SetupParts {
    pub working_dir: std::path::PathBuf,
    pub append_system_prompt_file: std::path::PathBuf,
    pub plugin_dir: std::path::PathBuf,
    pub plugin_name: String,
    pub extra_dirs: Vec<std::path::PathBuf>,
    pub resume_session_id: Option<String>,
    pub crew_charter_id: Option<CrewCharterId>,
}

/// The session setup of one run. DSK-WIRE-3: the desktop never adopts a paused run, so `paused` is
/// `None` for every run, a run paused on an approval included, and nothing is read for it.
/// DSK-REC-7: `model` and `effort` are `None`; the Usage Planner's choice reaches the harness through
/// `StartRun.model`.
// spec: ordin-desktop/components/OrdinDesktop#DSK-WIRE-3
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-7
pub fn assemble(parts: SetupParts) -> SessionSetup {
    SessionSetup {
        agent_folder: parts.working_dir,
        append_system_prompt_file: parts.append_system_prompt_file,
        plugin_dir: parts.plugin_dir,
        plugin_name: parts.plugin_name,
        model: None,
        effort: None,
        fallback_model: None,
        // Filled by the `SessionSetupSource` (DSK-REC-26, DSK-REC-29).
        attachment_paths: Vec::new(),
        extra_dirs: parts.extra_dirs,
        // Set by the `SessionSetupSource` (DSK-REC-27).
        shell_allowed: false,
        // R1: no package-declared server can be started from what a package declares (DSK-REC-28).
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-28
        package_servers: Vec::new(),
        resume_session_id: parts.resume_session_id,
        crew_charter_id: parts.crew_charter_id,
        run_starter: None,
        paused: None,
    }
}

// ---------------------------------------------------------------- WorkingCopyStore (row 3.37)

/// `WorkingCopyStore` over the Workspace Service (the items) and the File Service (the bytes and
/// versions), ruling R12.
pub struct DesktopWorkingCopy {
    pub workspaces: Arc<WorkspaceService>,
    pub files: Arc<FileService>,
}

fn store_failure<E: CodedError>(error: E) -> StoreFailure {
    StoreFailure {
        code: error.code(),
        retryable: error.retryable(),
    }
}

impl DesktopWorkingCopy {
    /// The File Service version `local_version` of the item's file.
    async fn version_of(
        &self,
        entry: &WorkingCopyEntry,
        local_version: u32,
    ) -> Result<ordin_types::FileVersionId, StoreFailure> {
        self.files
            .versions(entry.place_id, &entry.relative_path)
            .await
            .map_err(store_failure)?
            .into_iter()
            .find(|v| v.version == local_version)
            .map(|v| v.id)
            .ok_or_else(|| store_failure(ordin_files::FileError::VersionNotFound))
    }
}

#[async_trait::async_trait]
impl WorkingCopyStore for DesktopWorkingCopy {
    async fn entry(&self, id: WorkingCopyItemId) -> Result<WorkingCopyEntry, StoreFailure> {
        let entry = self
            .workspaces
            .working_copy_entry(id)
            .await
            .map_err(store_failure)?;
        let (git_host, git_repository) = match entry.repository {
            Some(repository) => (Some(repository.host_name), Some(repository.repository)),
            None => (None, None),
        };
        Ok(WorkingCopyEntry {
            item: entry.item,
            workspace_id: entry.workspace_id,
            link_kind: entry.link_kind,
            origin_location: entry.origin_location,
            display_name: entry.display_name,
            place_id: entry.place_id,
            relative_path: entry.relative_path,
            git_branch: entry.git_branch,
            git_host,
            git_repository,
        })
    }

    async fn set_state(
        &self,
        id: WorkingCopyItemId,
        state: WorkingCopyState,
        origin_version: Option<String>,
    ) -> Result<(), StoreFailure> {
        self.workspaces
            .set_working_copy_state(id, state, origin_version)
            .await
            .map(|_| ())
            .map_err(store_failure)
    }

    async fn content(
        &self,
        id: WorkingCopyItemId,
        local_version: Option<u32>,
    ) -> Result<Vec<u8>, StoreFailure> {
        let entry = self.entry(id).await?;
        let bytes = match local_version {
            None => self
                .files
                .read_file(entry.place_id, &entry.relative_path)
                .await
                .map_err(store_failure)?,
            Some(number) => {
                let version = self.version_of(&entry, number).await?;
                self.files
                    .read_version(version)
                    .await
                    .map_err(store_failure)?
            }
        };
        Ok(bytes.bytes)
    }

    async fn restore_version(
        &self,
        id: WorkingCopyItemId,
        local_version: u32,
    ) -> Result<(), StoreFailure> {
        let entry = self.entry(id).await?;
        let version = self.version_of(&entry, local_version).await?;
        self.files
            .restore_version(version)
            .await
            .map(|_| ())
            .map_err(store_failure)
    }

    fn local_changes(&self) -> broadcast::Receiver<WorkingCopyItemId> {
        self.workspaces.subscribe_working_copy_changes()
    }
}

// ---------------------------------------------------------------- Package Catalogue (row 3.40)

/// `HarnessSettings` over the person's harness (Settings › Agents and models, DSK-AGM-1), else the
/// Organisation Profile's first harness.
pub struct ProfileHarness {
    pub profile: Arc<OrganisationProfileService>,
    /// The harness the program chose at start; `None` reads the profile's.
    pub chosen: Option<HarnessName>,
}

impl HarnessSettings for ProfileHarness {
    fn default_harness(&self) -> HarnessKind {
        // spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
        let harness = self
            .chosen
            .unwrap_or_else(|| self.profile.current().default_harness());
        match harness {
            HarnessName::ClaudeCode => HarnessKind::ClaudeCode,
            HarnessName::Copilot => HarnessKind::CopilotCli,
        }
    }
}

/// `InstanceActivity` over the run register of DSK-REC-8.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-10
pub struct SupervisorActivity {
    pub late: Arc<Late>,
}

impl InstanceActivity for SupervisorActivity {
    fn running(&self, id: AgentInstanceId) -> bool {
        self.late.runs.running(id)
    }

    fn idle(&self) -> broadcast::Receiver<AgentInstanceId> {
        self.late.runs.subscribe()
    }
}

// ---------------------------------------------------------------- Project Service (row 3.31)

/// `StreamSink` over the Command Bridge's `ScreenStreams` (row 3.49, late-bound). A message that
/// cannot be queued is dropped: terminal and LSP streams are never journalled (ADR-022).
pub struct BridgeStreams {
    pub late: Arc<Late>,
    /// The kind of each open stream, for its closing message.
    pub kinds: Mutex<HashMap<StreamId, StreamKind>>,
}

impl BridgeStreams {
    pub fn new(late: Arc<Late>) -> BridgeStreams {
        BridgeStreams {
            late,
            kinds: Mutex::new(HashMap::new()),
        }
    }
}

/// Base64url without padding (CommandBridge.md: terminal payload `{"bytes_b64": …}`).
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextets = chunk.len() + 1;
        for i in 0..sextets {
            let index = (n >> (18 - 6 * i)) & 0x3f;
            out.push(char::from(ALPHABET[index as usize]));
        }
    }
    out
}

impl StreamSink for BridgeStreams {
    fn send(&self, stream: StreamId, payload: StreamPayload) {
        let Some(streams) = self.late.streams.get() else {
            return;
        };
        let mut kinds = self.kinds.lock().unwrap_or_else(|p| p.into_inner());
        match payload {
            StreamPayload::TerminalBytes(bytes) => {
                kinds.insert(stream, StreamKind::Terminal);
                let body = serde_json::json!({ "bytes_b64": base64url(&bytes) });
                let _ = streams.send(stream.0, StreamKind::Terminal, body);
            }
            StreamPayload::Lsp(message) => {
                kinds.insert(stream, StreamKind::Lsp);
                let _ = streams.send(stream.0, StreamKind::Lsp, message);
            }
            StreamPayload::Closed { reason } => {
                let kind = kinds.remove(&stream).unwrap_or(StreamKind::Terminal);
                streams.close(stream.0, kind, &reason);
            }
        }
    }
}

/// `ServerInstaller` over `SetupAssistant::ensure_language_server` (row 3.43, late-bound).
pub struct SetupLanguageServers {
    pub late: Arc<Late>,
}

#[async_trait::async_trait]
impl ServerInstaller for SetupLanguageServers {
    async fn ensure(&self, language: &str, server: &ServerPin) -> Result<ServerLaunch, String> {
        let setup = self
            .late
            .setup
            .get()
            .ok_or_else(|| "the Setup Assistant is not built yet".to_owned())?;
        let launch = setup
            .ensure_language_server(language, &server.name, &server.version)
            .await
            .map_err(|e| e.code().to_owned())?;
        Ok(ServerLaunch {
            program: launch.program,
            args: launch.args,
            env: launch.env,
        })
    }
}

fn language_key(language: ServerLanguage) -> &'static str {
    match language {
        ServerLanguage::Go => "go",
        ServerLanguage::Rust => "rust",
        ServerLanguage::Java => "java",
        ServerLanguage::Typescript => "typescript",
    }
}

/// `ProjectSettings` from one profile snapshot (ProjectService.md D-4; no check commands,
/// SA-CLOSE-10).
fn project_settings_of(snapshot: &ProfileSnapshot) -> ProjectSettings {
    let document = &snapshot.document;
    let mut language_servers: BTreeMap<String, ApprovedServers> = BTreeMap::new();
    for entry in &document.language_servers {
        let pin = ServerPin {
            name: entry.name.clone(),
            version: entry.version.clone(),
        };
        language_servers
            .entry(language_key(entry.language).to_owned())
            .or_insert_with(|| ApprovedServers {
                approved: Vec::new(),
                default: entry.name.clone(),
            })
            .approved
            .push(pin);
    }
    // DSK-REC-11: a language the profile does not name takes the build's table.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-11
    for entry in ordin_setup::bundled_language_servers() {
        let key = language_key(entry.language);
        if document
            .language_servers
            .iter()
            .any(|e| e.language == entry.language)
        {
            continue;
        }
        language_servers
            .entry(key.to_owned())
            .or_insert_with(|| ApprovedServers {
                approved: Vec::new(),
                default: entry.name.clone(),
            })
            .approved
            .push(ServerPin {
                name: entry.name.clone(),
                version: entry.version.clone(),
            });
    }
    ProjectSettings {
        language_servers,
        always_approved_kinds: document.engineering.github.always_approved_kinds.clone(),
        max_attempts: document.engineering.coding_tasks.max_attempts,
        gitlab_host: document
            .gitlab
            .as_ref()
            .map_or_else(|| "gitlab.com".to_owned(), |g| g.host.clone()),
        // The defaults carry no version.
        profile_version: match &snapshot.source {
            SnapshotSource::Verified {
                profile_version, ..
            } => profile_version.clone(),
            SnapshotSource::Defaults => String::new(),
        },
    }
}

/// `ConfluenceSettings` from one profile snapshot: `confluence.site` when it is an `https` URL.
fn confluence_settings_of(snapshot: &ProfileSnapshot) -> ConfluenceSettings {
    let site = snapshot
        .document
        .confluence
        .as_ref()
        .and_then(|c| reqwest::Url::parse(&c.site).ok())
        .filter(|url| url.scheme() == "https");
    ConfluenceSettings { site }
}

/// A channel holding `map` of the current profile, kept current by a task following
/// `profile.subscribe()` until every receiver is dropped.
fn follow<T, F>(profile: &OrganisationProfileService, map: F) -> watch::Receiver<T>
where
    T: Send + Sync + 'static,
    F: Fn(&ProfileSnapshot) -> T + Send + 'static,
{
    let (sender, receiver) = watch::channel(map(&profile.current()));
    let mut feed = profile.subscribe();
    tokio::spawn(async move {
        while feed.changed().await.is_ok() {
            let value = map(&feed.borrow_and_update());
            if sender.send(value).is_err() {
                break;
            }
        }
    });
    receiver
}

/// The Project Service's settings channel (row 3.31).
pub fn project_settings(profile: &OrganisationProfileService) -> watch::Receiver<ProjectSettings> {
    follow(profile, project_settings_of)
}

/// The Confluence Adapter's settings channel (row 3.30).
pub fn confluence_settings(
    profile: &OrganisationProfileService,
) -> watch::Receiver<ConfluenceSettings> {
    follow(profile, confluence_settings_of)
}
