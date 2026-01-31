//! The calls this crate makes to its dependencies (OrdinHost.md *Interactions* X-1 … X-31), each
//! behind a narrow trait. `OrdinHost::new` wraps the concrete services of `OrdinHostDeps` in the
//! `Real*` adapters below; this crate's tests put stand-ins behind the same traits. The traits name
//! only the calls the spec lists, with the dependencies' own types wherever those can be cloned.

use std::sync::Arc;

use futures::stream::BoxStream;
use ordin_crew_orchestrator::{CrewInput, CrewStart, PlanCheck};
use ordin_journal::{ArtefactFilter, NewArtefact, NewJournalEntry};
use ordin_memory_service::{
    AgentMemoryRules, FactView, MemoryCaller, MemoryPlace, RecallInput, RecallResult,
    RememberFactsInput, RememberFactsReceipt, RememberInput, RememberReceipt, RememberedInRun,
    VocabularyView,
};
use ordin_package_catalogue::{
    AgentFound, AgentMatch, ChangeChoiceInput, PendingChoice, TrialInput,
};
use ordin_runtime_adapter::{ResumeDecision, RunEvent};
use ordin_session_supervisor::ConversationStart;
use ordin_types::domain::runs::{CrewCharter, CrewRunState, Dashboard, ReusedAnswer, Run, Task};
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::CodedError;
use ordin_types::{
    AgentInstanceId, AgentRepositoryId, ArtefactId, CodingTaskId, CrewRunId, DashboardId,
    KnowledgePageId, MemoryFactId, MemoryThingId, PendingPushId, PlaceId, ProjectId,
    ReusedAnswerId, RunId, TaskId, WorkspaceId,
};
use ordin_usage_planner::{
    AnswerToIndex, EstimateRequest, LevelChoice, ReuseQuery, UsageEstimate, WorkKind,
};
use tokio::sync::broadcast;

/// A dependency's refusal, as this crate passes it on: its registry code, whether the same call may
/// succeed later, and its own sentence (never logged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepError {
    pub code: String,
    pub retryable: bool,
    pub message: String,
}

impl DepError {
    /// Used by the stand-ins of `testkit`; the real adapters build theirs with `of`.
    #[cfg_attr(not(any(test, feature = "testkit")), allow(dead_code))]
    pub fn new(code: &str, retryable: bool) -> Self {
        DepError {
            code: code.to_owned(),
            retryable,
            message: String::new(),
        }
    }

    pub(crate) fn of<E: CodedError>(e: &E) -> Self {
        DepError {
            code: e.code().to_owned(),
            retryable: e.retryable(),
            message: e.to_string(),
        }
    }
}

// ---------------------------------------------------------------- X-1 Session Supervisor

/// A run as `run_task` answers it: the run, its workspace and its task (OH-TA-1).
pub type RunWithTask = (Run, WorkspaceId, Option<Task>);

#[async_trait::async_trait]
pub trait SupervisorPort: Send + Sync {
    async fn start_conversation(&self, req: ConversationStart) -> Result<RunId, DepError>;
    fn subscribe(&self, run_id: RunId) -> Result<broadcast::Receiver<RunEvent>, DepError>;
    async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ResumeDecision,
    ) -> Result<(), DepError>;
    async fn cancel(&self, run_id: RunId) -> Result<(), DepError>;
    async fn submit_task(
        &self,
        task: Task,
        work_kind: WorkKind,
        started_by_person: bool,
        coding_task_id: Option<CodingTaskId>,
    ) -> Result<RunId, DepError>;
    /// The workspace's runs (first page), for the seed's carried items.
    async fn list(&self, workspace_id: WorkspaceId) -> Result<Vec<Run>, DepError>;
    /// One run with its workspace and its task (`get`), for **Try again** (OH-TA-1); `None` when the
    /// Supervisor does not know the run (`SupervisorError::RunNotFound`).
    async fn run_task(&self, run_id: RunId) -> Result<Option<RunWithTask>, DepError>;
    /// Conditional on C-29 (SA-CLOSE-07).
    fn context_used_percent(&self, run_id: RunId) -> Option<u8>;
}

pub struct RealSupervisor(pub Arc<ordin_session_supervisor::SessionSupervisor>);

#[async_trait::async_trait]
impl SupervisorPort for RealSupervisor {
    async fn start_conversation(&self, req: ConversationStart) -> Result<RunId, DepError> {
        self.0
            .start_conversation(req)
            .await
            .map_err(|e| DepError::of(&e))
    }
    fn subscribe(&self, run_id: RunId) -> Result<broadcast::Receiver<RunEvent>, DepError> {
        self.0.subscribe(run_id).map_err(|e| DepError::of(&e))
    }
    async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ResumeDecision,
    ) -> Result<(), DepError> {
        self.0
            .resume_with_decision(run_id, decision)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn cancel(&self, run_id: RunId) -> Result<(), DepError> {
        self.0.cancel(run_id).await.map_err(|e| DepError::of(&e))
    }
    async fn submit_task(
        &self,
        task: Task,
        work_kind: WorkKind,
        started_by_person: bool,
        coding_task_id: Option<CodingTaskId>,
    ) -> Result<RunId, DepError> {
        self.0
            .submit_task(task, work_kind, started_by_person, coding_task_id)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn list(&self, workspace_id: WorkspaceId) -> Result<Vec<Run>, DepError> {
        self.0
            .list(Some(workspace_id), None)
            .await
            .map(|page| page.items)
            .map_err(|e| DepError::of(&e))
    }
    async fn run_task(&self, run_id: RunId) -> Result<Option<RunWithTask>, DepError> {
        match self.0.get(run_id).await {
            Ok(view) => Ok(Some((view.run, view.workspace_id, view.task))),
            Err(ordin_session_supervisor::SupervisorError::RunNotFound) => Ok(None),
            Err(e) => Err(DepError::of(&e)),
        }
    }
    fn context_used_percent(&self, run_id: RunId) -> Option<u8> {
        self.0.context_used_percent(run_id)
    }
}

// ---------------------------------------------------------------- X-4 Crew Orchestrator

#[async_trait::async_trait]
pub trait OrchestratorPort: Send + Sync {
    async fn check_plan(&self, charter: &CrewCharter, inputs: &[CrewInput]) -> PlanCheck;
    async fn start(&self, start: CrewStart) -> Result<CrewRunId, DepError>;
    async fn kill(&self, crew_run_id: CrewRunId) -> Result<(), DepError>;
    /// `None` when the crew run is unknown (`CrewError::NotFound`).
    async fn state(&self, crew_run_id: CrewRunId) -> Result<Option<CrewRunState>, DepError>;
    async fn running_crew(&self, workspace_id: WorkspaceId) -> Result<Option<CrewRunId>, DepError>;
    /// The workspace a crew run belongs to, from `get` (X-4); `None` when the crew run is unknown
    /// (`CrewError::NotFound`).
    async fn workspace_of(&self, crew_run_id: CrewRunId) -> Result<Option<WorkspaceId>, DepError>;
    /// OH-CLS-1: how many crews of the workspace are paused for a restart (`running_crew`, then
    /// `get`): 0 or 1, one crew at a time per workspace.
    async fn paused_for_restart(&self, workspace_id: WorkspaceId) -> Result<u32, DepError>;
    async fn request_take_over(
        &self,
        crew_run_id: CrewRunId,
        from_position: String,
        reason: String,
    ) -> Result<(), DepError>;
}

pub struct RealOrchestrator(pub Arc<ordin_crew_orchestrator::CrewOrchestrator>);

#[async_trait::async_trait]
impl OrchestratorPort for RealOrchestrator {
    async fn check_plan(&self, charter: &CrewCharter, inputs: &[CrewInput]) -> PlanCheck {
        self.0.check_plan(charter, inputs).await
    }
    async fn start(&self, start: CrewStart) -> Result<CrewRunId, DepError> {
        self.0.start(start).await.map_err(|e| DepError::of(&e))
    }
    async fn kill(&self, crew_run_id: CrewRunId) -> Result<(), DepError> {
        self.0.kill(crew_run_id).await.map_err(|e| DepError::of(&e))
    }
    async fn state(&self, crew_run_id: CrewRunId) -> Result<Option<CrewRunState>, DepError> {
        match self.0.get(crew_run_id).await {
            Ok((run, _)) => Ok(Some(run.state)),
            Err(ordin_crew_orchestrator::CrewError::NotFound) => Ok(None),
            Err(e) => Err(DepError::of(&e)),
        }
    }
    async fn running_crew(&self, workspace_id: WorkspaceId) -> Result<Option<CrewRunId>, DepError> {
        self.0
            .running_crew(workspace_id)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn workspace_of(&self, crew_run_id: CrewRunId) -> Result<Option<WorkspaceId>, DepError> {
        match self.0.get(crew_run_id).await {
            Ok((run, _)) => Ok(Some(run.workspace_id)),
            Err(ordin_crew_orchestrator::CrewError::NotFound) => Ok(None),
            Err(e) => Err(DepError::of(&e)),
        }
    }
    async fn paused_for_restart(&self, workspace_id: WorkspaceId) -> Result<u32, DepError> {
        let Some(id) = self.running_crew(workspace_id).await? else {
            return Ok(0);
        };
        match self.0.get(id).await {
            Ok((run, _)) => Ok(u32::from(
                run.state == CrewRunState::Paused
                    && run.pause_reason
                        == Some(ordin_types::domain::runs::CrewPauseReason::Restart),
            )),
            Err(ordin_crew_orchestrator::CrewError::NotFound) => Ok(0),
            Err(e) => Err(DepError::of(&e)),
        }
    }
    async fn request_take_over(
        &self,
        crew_run_id: CrewRunId,
        from_position: String,
        reason: String,
    ) -> Result<(), DepError> {
        self.0
            .request_take_over(crew_run_id, from_position, reason)
            .await
            .map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-5 Package Inspector

/// One command of an installed agent, as the directory lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectoryCommand {
    pub name: String,
    /// `<plugin>:<command>` on Claude Code (the name the session accepts).
    pub session_name: Option<String>,
    pub argument_hint: Option<String>,
    /// The Command part's identifier (`TaskEntry.command_id`); names the task to `runs.start_task`.
    // spec: ordin-desktop/components/PackageInspector#PKI-TF-1
    pub command_id: String,
    /// `TaskEntry.title`.
    pub title: Option<String>,
    /// `TaskEntry.inputs`; None: the task's one input is `arguments`, long text, not required.
    pub inputs: Option<Vec<ordin_package_inspector::TaskInput>>,
    /// `TaskEntry.form_unreadable`.
    pub form_unreadable: bool,
}

/// One installed agent instance of the workspace (`AgentDirectoryEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryAgent {
    pub agent_instance_id: AgentInstanceId,
    pub display_name: String,
    pub description: String,
    pub plugin_name: Option<String>,
    pub commands: Vec<DirectoryCommand>,
    pub skills: Vec<String>,
    pub lifecycle: Option<String>,
    /// The authority, as its snake_case wire word.
    pub authority: String,
    pub declared_rules: Vec<String>,
}

/// The memory rules an agent declares in its Memory part (SA-CLOSE-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredMemory {
    pub retains: Vec<String>,
    pub excludes: Vec<String>,
    pub max_age_days: Option<u32>,
    pub unbounded: bool,
    pub store_location: String,
}

#[async_trait::async_trait]
pub trait InspectorPort: Send + Sync {
    async fn directory(&self, workspace_id: WorkspaceId) -> Result<Vec<DirectoryAgent>, DepError>;
    fn changes(&self) -> BoxStream<'static, WorkspaceId>;
    async fn memory_contract(
        &self,
        id: AgentInstanceId,
    ) -> Result<Option<DeclaredMemory>, DepError>;
}

pub struct RealInspector(pub Arc<ordin_package_inspector::PackageInspector>);

#[async_trait::async_trait]
impl InspectorPort for RealInspector {
    async fn directory(&self, workspace_id: WorkspaceId) -> Result<Vec<DirectoryAgent>, DepError> {
        let entries = self
            .0
            .directory(workspace_id)
            .await
            .map_err(|e| DepError::of(&e))?;
        Ok(entries
            .into_iter()
            .map(|e| DirectoryAgent {
                agent_instance_id: e.agent_instance_id,
                display_name: e.display_name,
                description: e.description,
                plugin_name: e.plugin_name,
                commands: e
                    .tasks
                    .into_iter()
                    .map(|t| DirectoryCommand {
                        name: t.name,
                        session_name: t.session_name,
                        argument_hint: t.argument_hint,
                        command_id: t.command_id,
                        title: t.title,
                        inputs: t.inputs,
                        form_unreadable: t.form_unreadable,
                    })
                    .collect(),
                skills: e.skills,
                lifecycle: e.lifecycle,
                authority: match serde_json::to_value(e.authority) {
                    Ok(serde_json::Value::String(s)) => s,
                    _ => String::new(),
                },
                declared_rules: e.declared_rules,
            })
            .collect())
    }
    fn changes(&self) -> BoxStream<'static, WorkspaceId> {
        self.0.changes()
    }
    async fn memory_contract(
        &self,
        id: AgentInstanceId,
    ) -> Result<Option<DeclaredMemory>, DepError> {
        let contract = self
            .0
            .memory_contract(id)
            .await
            .map_err(|e| DepError::of(&e))?;
        Ok(contract.map(|c| DeclaredMemory {
            retains: c.retains,
            excludes: c.excludes,
            max_age_days: c.max_age_days,
            unbounded: c.unbounded,
            store_location: c.store_location,
        }))
    }
}

// ---------------------------------------------------------------- X-6 Usage Planner

#[async_trait::async_trait]
pub trait UsagePort: Send + Sync {
    fn choose_talking(&self, harness: ordin_types::wire::run::AdapterKind) -> Option<LevelChoice>;
    async fn may_start_ordin_work(&self) -> Result<(), DepError>;
    fn should_summarise(
        &self,
        context_used_permille: Option<u32>,
        turns_since_summary: u32,
    ) -> bool;
    fn continue_from_summary(&self, last_message_at: Timestamp, now: Timestamp) -> bool;
    fn answer_without_model(&self) -> bool;
    async fn record_answered_without_model(&self) -> Result<(), DepError>;
    async fn find_reusable(&self, q: ReuseQuery) -> Result<Option<ReusedAnswer>, DepError>;
    async fn record_reuse(&self, id: ReusedAnswerId, run_id: Option<RunId>)
        -> Result<(), DepError>;
    async fn index_answer(&self, a: AnswerToIndex) -> Result<ReusedAnswerId, DepError>;
    async fn estimate(&self, req: EstimateRequest) -> Result<UsageEstimate, DepError>;
}

pub struct RealUsage(pub Arc<ordin_usage_planner::UsagePlanner>);

#[async_trait::async_trait]
impl UsagePort for RealUsage {
    fn choose_talking(&self, harness: ordin_types::wire::run::AdapterKind) -> Option<LevelChoice> {
        Some(self.0.choose(WorkKind::Talking, harness))
    }
    async fn may_start_ordin_work(&self) -> Result<(), DepError> {
        self.0
            .may_start_ordin_work()
            .await
            .map_err(|e| DepError::of(&e))
    }
    fn should_summarise(
        &self,
        context_used_permille: Option<u32>,
        turns_since_summary: u32,
    ) -> bool {
        self.0
            .should_summarise(context_used_permille, turns_since_summary)
    }
    fn continue_from_summary(&self, last_message_at: Timestamp, now: Timestamp) -> bool {
        self.0.continue_from_summary(last_message_at, now)
    }
    fn answer_without_model(&self) -> bool {
        self.0.answer_without_model()
    }
    async fn record_answered_without_model(&self) -> Result<(), DepError> {
        self.0
            .record_answered_without_model()
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn find_reusable(&self, q: ReuseQuery) -> Result<Option<ReusedAnswer>, DepError> {
        self.0.find_reusable(q).await.map_err(|e| DepError::of(&e))
    }
    async fn record_reuse(
        &self,
        id: ReusedAnswerId,
        run_id: Option<RunId>,
    ) -> Result<(), DepError> {
        self.0
            .record_reuse(id, run_id)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn index_answer(&self, a: AnswerToIndex) -> Result<ReusedAnswerId, DepError> {
        self.0.index_answer(a).await.map_err(|e| DepError::of(&e))
    }
    async fn estimate(&self, req: EstimateRequest) -> Result<UsageEstimate, DepError> {
        self.0.estimate(req).await.map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-7 Workspace Service

/// A repository linked to the workspace (`LinkedRepository`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedRepo {
    pub host: ordin_types::domain::ShareHost,
    /// `owner/name`.
    pub repository: String,
    pub host_name: String,
    pub place_id: PlaceId,
}

/// What the seed needs of the workspace (`WorkspaceSummary`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFacts {
    pub name: String,
    pub places: Vec<String>,
}

/// OH-ACC-1: one folder of a workspace (an active link with a place) and whether an agent may use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFolder {
    pub place_id: PlaceId,
    pub name: String,
    /// A Jira issue: read, never changed.
    pub read_only: bool,
    /// The agent holds a grant on it, at any access.
    pub granted: bool,
}

/// OH-WC-1: a linked document changed in its working copy and not yet sent back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedDocument {
    pub item: ordin_types::WorkingCopyItemId,
    /// The document's path at its origin.
    pub path: String,
    /// The link's display name.
    pub link: String,
}

/// OH-TR-2: one working-copy item of the workspace's links, as the version lookup reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDocument {
    /// The document's path at its origin.
    pub origin_path: String,
    pub origin_version_read: String,
    pub local_version: u32,
}

#[async_trait::async_trait]
pub trait WorkspacesPort: Send + Sync {
    async fn ensure_active(&self, id: WorkspaceId) -> Result<(), DepError>;
    async fn summary(&self, id: WorkspaceId) -> Result<WorkspaceFacts, DepError>;
    async fn linked_repositories(&self, id: WorkspaceId) -> Result<Vec<LinkedRepo>, DepError>;
    /// OH-ACC-1: the workspace's folders, each with whether `agent` may use it.
    async fn agent_folders(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<Vec<AgentFolder>, DepError>;
    /// OH-WC-1: the working-copy items in state `changed` of the workspace's active SharePoint,
    /// Confluence and repository-folder links ([`sent_back_directly`]).
    async fn changed_documents(&self, id: WorkspaceId) -> Result<Vec<ChangedDocument>, DepError>;
    /// OH-TR-2 (X-7 `source_links` and `working_copy_items`): every working-copy item of the
    /// workspace's links, link by link in the order `source_links` returns them.
    async fn source_documents(&self, id: WorkspaceId) -> Result<Vec<SourceDocument>, DepError>;
    /// OH-TF-1 step 7 (X-7 `set_grant`): an agent instance's access to one folder; None removes it.
    // spec: ordin-desktop/components/OrdinHost#OH-TF-1
    async fn set_agent_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
        place_id: PlaceId,
        access: Option<ordin_types::domain::places::FolderAccess>,
    ) -> Result<(), DepError>;
    /// OH-TW-2 (X-7 `results_folder`, WorkspaceService.md WS-TW-1): a team workspace's results
    /// folder; None when the workspace has none.
    // spec: ordin-desktop/components/OrdinHost#OH-TW-2
    async fn results_folder(&self, id: WorkspaceId) -> Result<Option<PlaceId>, DepError>;
    /// OH-TW-1 (X-7 `grants`): every folder grant of the workspace.
    // spec: ordin-desktop/components/OrdinHost#OH-TW-1
    async fn grants(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<ordin_types::domain::places::FolderGrant>, DepError>;
}

/// OH-WC-1: whether a link's changed items are named to Ordin — the links whose changes the Push
/// Service sends back itself: SharePoint, Confluence and a repository folder on this computer
/// (PushService.md §C step 4, Git). A local folder is its own origin, a repository linked without a
/// clone goes by pull request and a Jira issue is read only.
// spec: ordin-desktop/components/OrdinHost#OH-WC-1
pub(crate) fn sent_back_directly(kind: ordin_types::domain::places::SourceLinkKind) -> bool {
    use ordin_types::domain::places::SourceLinkKind;
    matches!(
        kind,
        SourceLinkKind::SharepointLocation
            | SourceLinkKind::ConfluenceLocation
            | SourceLinkKind::GitRepositoryFolder
    )
}

pub struct RealWorkspaces(pub Arc<ordin_workspaces::WorkspaceService>);

#[async_trait::async_trait]
impl WorkspacesPort for RealWorkspaces {
    async fn ensure_active(&self, id: WorkspaceId) -> Result<(), DepError> {
        self.0
            .ensure_active(id)
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
    async fn summary(&self, id: WorkspaceId) -> Result<WorkspaceFacts, DepError> {
        let s = self.0.summary(id).await.map_err(|e| DepError::of(&e))?;
        Ok(WorkspaceFacts {
            name: s.name,
            places: s.places,
        })
    }
    async fn linked_repositories(&self, id: WorkspaceId) -> Result<Vec<LinkedRepo>, DepError> {
        let repos = self
            .0
            .linked_repositories(id)
            .await
            .map_err(|e| DepError::of(&e))?;
        Ok(repos
            .into_iter()
            .map(|r| LinkedRepo {
                host: r.host,
                repository: r.repository,
                host_name: r.host_name,
                place_id: r.place_id,
            })
            .collect())
    }
    async fn agent_folders(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<Vec<AgentFolder>, DepError> {
        use ordin_types::domain::places::{Grantee, SourceLinkKind, SourceLinkState};
        let links = self
            .0
            .source_links(id)
            .await
            .map_err(|e| DepError::of(&e))?;
        let grants = self.0.grants(id).await.map_err(|e| DepError::of(&e))?;
        Ok(links
            .into_iter()
            .filter(|l| l.state == SourceLinkState::Active)
            .filter_map(|l| {
                let place_id = l.place_id?;
                let granted = grants.iter().any(|g| {
                    g.place_id == place_id
                        && matches!(g.grantee, Grantee::AgentInstance { agent_instance_id } if agent_instance_id == agent)
                });
                Some(AgentFolder {
                    place_id,
                    name: l.display_name,
                    read_only: l.kind == SourceLinkKind::JiraIssue,
                    granted,
                })
            })
            .collect())
    }
    async fn changed_documents(&self, id: WorkspaceId) -> Result<Vec<ChangedDocument>, DepError> {
        use ordin_types::domain::places::{SourceLinkState, WorkingCopyState};
        let links = self
            .0
            .source_links(id)
            .await
            .map_err(|e| DepError::of(&e))?;
        let mut out = Vec::new();
        for link in links
            .into_iter()
            .filter(|l| l.state == SourceLinkState::Active && sent_back_directly(l.kind))
        {
            let items = self
                .0
                .working_copy_items(link.id)
                .await
                .map_err(|e| DepError::of(&e))?;
            out.extend(
                items
                    .into_iter()
                    .filter(|i| i.state == WorkingCopyState::Changed)
                    .map(|i| ChangedDocument {
                        item: i.id,
                        path: i.origin_path,
                        link: link.display_name.clone(),
                    }),
            );
        }
        Ok(out)
    }
    // spec: ordin-desktop/components/OrdinHost#OH-TR-2
    async fn source_documents(&self, id: WorkspaceId) -> Result<Vec<SourceDocument>, DepError> {
        let links = self
            .0
            .source_links(id)
            .await
            .map_err(|e| DepError::of(&e))?;
        let mut out = Vec::new();
        for link in links {
            let items = self
                .0
                .working_copy_items(link.id)
                .await
                .map_err(|e| DepError::of(&e))?;
            out.extend(items.into_iter().map(|i| SourceDocument {
                origin_path: i.origin_path,
                origin_version_read: i.origin_version_read,
                local_version: i.local_version,
            }));
        }
        Ok(out)
    }
    async fn set_agent_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
        place_id: PlaceId,
        access: Option<ordin_types::domain::places::FolderAccess>,
    ) -> Result<(), DepError> {
        use ordin_types::domain::places::Grantee;
        self.0
            .set_grant(
                id,
                Grantee::AgentInstance {
                    agent_instance_id: agent,
                },
                place_id,
                access,
            )
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
    async fn results_folder(&self, id: WorkspaceId) -> Result<Option<PlaceId>, DepError> {
        self.0
            .results_folder(id)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn grants(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<ordin_types::domain::places::FolderGrant>, DepError> {
        self.0.grants(id).await.map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-8, X-24 Journal

#[async_trait::async_trait]
pub trait JournalPort: Send + Sync {
    async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<(), DepError>;
    async fn append_entry(&self, entry: NewJournalEntry) -> Result<(), DepError>;
    async fn dashboard(&self, id: DashboardId) -> Result<Option<Dashboard>, DepError>;
    async fn add_dashboard_refresh(&self, id: DashboardId, run_id: RunId) -> Result<(), DepError>;
    /// OH-KD-1: the workspace's reports, newest first (the first page of `list_reports`).
    async fn recent_reports(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ordin_journal::ReportSummary>, DepError>;
    /// OH-KD-1: `keep_dashboard`.
    async fn keep_dashboard(&self, new: ordin_journal::NewDashboard)
        -> Result<Dashboard, DepError>;
    /// OH-MIN-2: `keep_minutes` (JA-MIN-2).
    #[cfg(feature = "minutes")]
    async fn keep_minutes(
        &self,
        new: ordin_journal::NewMinutes,
    ) -> Result<ordin_types::wire::minutes::Minutes, DepError>;
    async fn record_artefact(&self, new: NewArtefact) -> Result<ArtefactId, DepError>;
    async fn external_actions_for_run(&self, run_id: RunId) -> Result<u32, DepError>;
    /// The names (paths) of the artefacts a run produced, first page.
    async fn artefact_names(&self, run_id: RunId) -> Result<Vec<String>, DepError>;
    /// OH-CLS-1: the run of each external action in state `unknown_outcome`, one entry per action,
    /// at most [`UNKNOWN_OUTCOME_MAX`]; an action with no run is left out.
    async fn unknown_outcome_runs(&self) -> Result<Vec<RunId>, DepError>;
    /// OH-DLG-2: the code a run ended with — the newest `run_code` entry's `data.code`
    /// (SessionSupervisor.md SA-SUP-END-1); None when the run journalled none.
    async fn run_code(&self, run_id: RunId) -> Result<Option<String>, DepError>;
}

/// OH-CLS-1: the most `unknown_outcome` actions read at one start.
pub const UNKNOWN_OUTCOME_MAX: usize = 1_000;

pub struct RealJournal(pub Arc<ordin_journal::Journal>);

#[async_trait::async_trait]
impl JournalPort for RealJournal {
    async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<(), DepError> {
        self.0
            .append_event(kind, data)
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
    async fn append_entry(&self, entry: NewJournalEntry) -> Result<(), DepError> {
        self.0
            .append_entry(entry)
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
    async fn dashboard(&self, id: DashboardId) -> Result<Option<Dashboard>, DepError> {
        self.0.dashboard(id).await.map_err(|e| DepError::of(&e))
    }
    async fn add_dashboard_refresh(&self, id: DashboardId, run_id: RunId) -> Result<(), DepError> {
        self.0
            .add_dashboard_refresh(id, run_id)
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
    async fn recent_reports(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ordin_journal::ReportSummary>, DepError> {
        self.0
            .list_reports(workspace_id, None)
            .await
            .map(|page| page.items)
            .map_err(|e| DepError::of(&e))
    }
    async fn keep_dashboard(
        &self,
        new: ordin_journal::NewDashboard,
    ) -> Result<Dashboard, DepError> {
        self.0
            .keep_dashboard(new)
            .await
            .map_err(|e| DepError::of(&e))
    }
    #[cfg(feature = "minutes")]
    async fn keep_minutes(
        &self,
        new: ordin_journal::NewMinutes,
    ) -> Result<ordin_types::wire::minutes::Minutes, DepError> {
        self.0.keep_minutes(new).await.map_err(|e| DepError::of(&e))
    }
    async fn record_artefact(&self, new: NewArtefact) -> Result<ArtefactId, DepError> {
        self.0
            .record_artefact(new)
            .await
            .map(|a| a.id)
            .map_err(|e| DepError::of(&e))
    }
    async fn external_actions_for_run(&self, run_id: RunId) -> Result<u32, DepError> {
        self.0
            .external_actions_for_run(run_id)
            .await
            .map(|v| u32::try_from(v.len()).unwrap_or(u32::MAX))
            .map_err(|e| DepError::of(&e))
    }
    async fn artefact_names(&self, run_id: RunId) -> Result<Vec<String>, DepError> {
        self.0
            .list_artefacts(
                ArtefactFilter {
                    location: None,
                    producing_run_id: Some(run_id),
                },
                None,
            )
            .await
            .map(|page| page.items.into_iter().map(|a| a.path).collect())
            .map_err(|e| DepError::of(&e))
    }
    // spec: ordin-desktop/components/SessionSupervisor#SA-SUP-END-1
    async fn run_code(&self, run_id: RunId) -> Result<Option<String>, DepError> {
        self.0
            .list_entries(
                ordin_journal::EntryFilter {
                    run_id: Some(run_id),
                    kind: Some("run_code".to_owned()),
                },
                None,
            )
            .await
            .map(|page| {
                page.items
                    .first()
                    .and_then(|e| e.data.get("code"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .map_err(|e| DepError::of(&e))
    }
    async fn unknown_outcome_runs(&self) -> Result<Vec<RunId>, DepError> {
        let mut runs = Vec::new();
        let mut read = 0usize;
        let mut cursor = None;
        loop {
            let page = self
                .0
                .list_external_actions(
                    Some(ordin_types::domain::runs::ExternalActionState::UnknownOutcome),
                    cursor,
                )
                .await
                .map_err(|e| DepError::of(&e))?;
            for action in page.items {
                if read == UNKNOWN_OUTCOME_MAX {
                    return Ok(runs);
                }
                read += 1;
                runs.extend(action.run_id);
            }
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok(runs),
            }
        }
    }
}

// ---------------------------------------------------------------- X-9, X-26 Package Catalogue

#[async_trait::async_trait]
pub trait CataloguePort: Send + Sync {
    async fn ensure_ready(&self, id: AgentInstanceId) -> Result<(), DepError>;
    async fn pending_choices(&self, id: AgentInstanceId) -> Result<Vec<PendingChoice>, DepError>;
    async fn record_answer(&self, input: ChangeChoiceInput) -> Result<(), DepError>;
    async fn match_workflow_agent(
        &self,
        workspace_id: WorkspaceId,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<AgentMatch, DepError>;
    async fn match_by_name(
        &self,
        workspace_id: WorkspaceId,
        name: &str,
    ) -> Result<AgentMatch, DepError>;
    /// (repository, identifier, version, name).
    async fn agent_identity(
        &self,
        id: AgentInstanceId,
    ) -> Result<(AgentRepositoryRef, String, String, String), DepError>;
    async fn find_agents(
        &self,
        workspace_id: WorkspaceId,
        need: &str,
    ) -> Result<Vec<AgentFound>, DepError>;
    async fn check_proposal(&self, repo: AgentRepositoryId, agent: &str) -> Result<(), DepError>;
    async fn can_send_email(&self, id: AgentInstanceId) -> bool;
    async fn add_trial(&self, input: TrialInput) -> Result<AgentInstanceId, DepError>;
}

pub struct RealCatalogue(pub Arc<ordin_package_catalogue::PackageCatalogue>);

#[async_trait::async_trait]
impl CataloguePort for RealCatalogue {
    async fn ensure_ready(&self, id: AgentInstanceId) -> Result<(), DepError> {
        self.0
            .ensure_ready(id)
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
    async fn pending_choices(&self, id: AgentInstanceId) -> Result<Vec<PendingChoice>, DepError> {
        self.0
            .pending_choices(id)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn record_answer(&self, input: ChangeChoiceInput) -> Result<(), DepError> {
        self.0
            .record_answer(input)
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
    async fn match_workflow_agent(
        &self,
        workspace_id: WorkspaceId,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<AgentMatch, DepError> {
        self.0
            .match_workflow_agent(workspace_id, repository, id, version)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn match_by_name(
        &self,
        workspace_id: WorkspaceId,
        name: &str,
    ) -> Result<AgentMatch, DepError> {
        self.0
            .match_by_name(workspace_id, name)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn agent_identity(
        &self,
        id: AgentInstanceId,
    ) -> Result<(AgentRepositoryRef, String, String, String), DepError> {
        self.0
            .agent_identity(id)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn find_agents(
        &self,
        workspace_id: WorkspaceId,
        need: &str,
    ) -> Result<Vec<AgentFound>, DepError> {
        self.0
            .find_agents(workspace_id, need)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn check_proposal(&self, repo: AgentRepositoryId, agent: &str) -> Result<(), DepError> {
        self.0
            .check_proposal(repo, agent)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn can_send_email(&self, id: AgentInstanceId) -> bool {
        self.0.can_send_email(id).await
    }
    async fn add_trial(&self, input: TrialInput) -> Result<AgentInstanceId, DepError> {
        self.0
            .add_trial(input)
            .await
            .map(|a| a.instance.id)
            .map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-11, X-23 Memory Service

#[async_trait::async_trait]
pub trait MemoryPort: Send + Sync {
    async fn remember(
        &self,
        who: MemoryCaller,
        input: RememberInput,
    ) -> Result<RememberReceipt, DepError>;
    /// Step 15.1 (amendment ADR-118).
    async fn remember_facts(
        &self,
        who: MemoryCaller,
        input: RememberFactsInput,
    ) -> Result<RememberFactsReceipt, DepError>;
    /// The seed's Link words section (amendment ADR-118).
    async fn vocabulary(&self, place: MemoryPlace) -> Result<VocabularyView, DepError>;
    async fn recall(&self, who: MemoryCaller, input: RecallInput)
        -> Result<RecallResult, DepError>;
    fn set_agent_rules(&self, id: AgentInstanceId, rules: AgentMemoryRules)
        -> Result<(), DepError>;
    fn clear_agent_rules(&self, id: AgentInstanceId);
    async fn remembered_in_run(&self, run_id: RunId) -> Result<RememberedInRun, DepError>;
    async fn last_consolidated(
        &self,
        workspace_id: WorkspaceId,
        thing: MemoryThingId,
    ) -> Result<Option<Timestamp>, DepError>;
    async fn get_facts(
        &self,
        workspace_id: WorkspaceId,
        ids: Vec<MemoryFactId>,
    ) -> Result<Vec<FactView>, DepError>;
    /// Settings › Memory as the Memory Service holds it (OH-MS-1; amended January 2026; user
    /// decision; Approved by the user January 2026).
    fn settings(&self) -> ordin_memory_service::MemorySettings;
}

pub struct RealMemory(pub Arc<ordin_memory_service::MemoryService>);

/// Step 15.3: a refusal of `remember_facts` goes back to the model with the Memory Service's sentence,
/// which says what to do instead (MemoryService.md MEM-TR-13), not only its code.
// spec: ordin-desktop/components/OrdinHost#OH-TR-3
pub(crate) fn memory_refusal(e: &ordin_memory_service::MemoryError) -> DepError {
    DepError {
        code: e.code().to_owned(),
        retryable: e.retryable(),
        message: e.tool_text(),
    }
}

#[async_trait::async_trait]
impl MemoryPort for RealMemory {
    async fn remember(
        &self,
        who: MemoryCaller,
        input: RememberInput,
    ) -> Result<RememberReceipt, DepError> {
        self.0
            .remember(who, input)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn remember_facts(
        &self,
        who: MemoryCaller,
        input: RememberFactsInput,
    ) -> Result<RememberFactsReceipt, DepError> {
        self.0
            .remember_facts(who, input)
            .await
            .map_err(|e| memory_refusal(&e))
    }
    async fn vocabulary(&self, place: MemoryPlace) -> Result<VocabularyView, DepError> {
        self.0.vocabulary(place).await.map_err(|e| DepError::of(&e))
    }
    async fn recall(
        &self,
        who: MemoryCaller,
        input: RecallInput,
    ) -> Result<RecallResult, DepError> {
        self.0
            .recall(who, input)
            .await
            .map_err(|e| DepError::of(&e))
    }
    fn set_agent_rules(
        &self,
        id: AgentInstanceId,
        rules: AgentMemoryRules,
    ) -> Result<(), DepError> {
        self.0
            .set_agent_rules(id, rules)
            .map_err(|e| DepError::of(&e))
    }
    fn clear_agent_rules(&self, id: AgentInstanceId) {
        self.0.clear_agent_rules(id)
    }
    async fn remembered_in_run(&self, run_id: RunId) -> Result<RememberedInRun, DepError> {
        self.0
            .remembered_in_run(run_id)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn last_consolidated(
        &self,
        workspace_id: WorkspaceId,
        thing: MemoryThingId,
    ) -> Result<Option<Timestamp>, DepError> {
        self.0
            .last_consolidated(workspace_id, thing)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn get_facts(
        &self,
        workspace_id: WorkspaceId,
        ids: Vec<MemoryFactId>,
    ) -> Result<Vec<FactView>, DepError> {
        self.0
            .get_facts(workspace_id, ids)
            .await
            .map_err(|e| DepError::of(&e))
    }

    fn settings(&self) -> ordin_memory_service::MemorySettings {
        self.0.settings()
    }
}

// ---------------------------------------------------------------- X-12, X-25 Knowledge Service

/// A page's title, body and sources (`PageView`).
#[derive(Debug, Clone, PartialEq)]
pub struct PageText {
    pub title: String,
    pub body: String,
    pub sources: Vec<ordin_types::domain::places::KnowledgeSource>,
}

#[async_trait::async_trait]
pub trait KnowledgePort: Send + Sync {
    async fn write_page(
        &self,
        req: ordin_knowledge::WritePage,
    ) -> Result<KnowledgePageId, DepError>;
    async fn get_page(&self, page_id: KnowledgePageId) -> Result<PageText, DepError>;
    async fn propose_update(
        &self,
        page_id: KnowledgePageId,
        body: String,
        sources: Vec<ordin_types::domain::places::KnowledgeSource>,
        run_id: RunId,
    ) -> Result<(), DepError>;
}

pub struct RealKnowledge(pub Arc<ordin_knowledge::KnowledgeService>);

#[async_trait::async_trait]
impl KnowledgePort for RealKnowledge {
    async fn write_page(
        &self,
        req: ordin_knowledge::WritePage,
    ) -> Result<KnowledgePageId, DepError> {
        self.0
            .write_page(ordin_types::domain::places::KnowledgeAuthor::Ordin, req)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn get_page(&self, page_id: KnowledgePageId) -> Result<PageText, DepError> {
        let view = self
            .0
            .get_page(page_id)
            .await
            .map_err(|e| DepError::of(&e))?;
        Ok(PageText {
            title: view.page.title,
            body: view.page.body_markdown,
            sources: view.page.sources,
        })
    }
    async fn propose_update(
        &self,
        page_id: KnowledgePageId,
        body: String,
        sources: Vec<ordin_types::domain::places::KnowledgeSource>,
        run_id: RunId,
    ) -> Result<(), DepError> {
        self.0
            .propose_update(page_id, body, sources, run_id)
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-13 Push Service

#[async_trait::async_trait]
pub trait PushPort: Send + Sync {
    async fn propose(&self, proposal: ordin_push::PushProposal) -> Result<PendingPushId, DepError>;
    async fn propose_pull_request(
        &self,
        req: ordin_push::PullRequestProposal,
    ) -> Result<ordin_push::SubmissionId, DepError>;
    fn subscribe(&self) -> broadcast::Receiver<ordin_push::PushNotice>;
    /// OH-PF-1: sends one failed item of an approved push again.
    async fn try_again(
        &self,
        pending_push_id: PendingPushId,
        item: ordin_types::WorkingCopyItemId,
    ) -> Result<(), DepError>;
}

pub struct RealPush(pub Arc<ordin_push::PushService>);

#[async_trait::async_trait]
impl PushPort for RealPush {
    async fn propose(&self, proposal: ordin_push::PushProposal) -> Result<PendingPushId, DepError> {
        self.0
            .propose(proposal)
            .await
            .map(|p| p.id)
            .map_err(|e| DepError::of(&e))
    }
    async fn propose_pull_request(
        &self,
        req: ordin_push::PullRequestProposal,
    ) -> Result<ordin_push::SubmissionId, DepError> {
        self.0
            .propose_pull_request(req)
            .await
            .map_err(|e| DepError::of(&e))
    }
    fn subscribe(&self) -> broadcast::Receiver<ordin_push::PushNotice> {
        self.0.subscribe()
    }
    async fn try_again(
        &self,
        pending_push_id: PendingPushId,
        item: ordin_types::WorkingCopyItemId,
    ) -> Result<(), DepError> {
        self.0
            .try_again(ordin_push::TryAgainInput {
                pending_push_id,
                working_copy_item_id: item,
            })
            .await
            .map(|_| ())
            .map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-15 Project Service

#[async_trait::async_trait]
pub trait ProjectsPort: Send + Sync {
    /// `(project, is_git_repository)`, or `None` when no project is open on the place.
    async fn project_for_place(
        &self,
        place_id: PlaceId,
    ) -> Result<Option<(ProjectId, bool)>, DepError>;
    async fn start_coding_task(
        &self,
        req: ordin_projects::StartCodingTask,
    ) -> Result<CodingTaskId, DepError>;
    /// True when a coding task was started for the task (ProjectService.md PROJ-ORIG-1, OH-TA-1).
    async fn is_coding_task(&self, task_id: TaskId) -> Result<bool, DepError>;
    async fn list_files(
        &self,
        folder: &ordin_projects::RepoFolderRef,
        glob: &str,
    ) -> Result<Vec<ordin_projects::RepoFile>, DepError>;
    async fn read_file(
        &self,
        folder: &ordin_projects::RepoFolderRef,
        name: &str,
    ) -> Result<(Vec<u8>, String), DepError>;
    async fn file_last_commit(
        &self,
        folder: &ordin_projects::RepoFolderRef,
        name: &str,
    ) -> Result<String, DepError>;
    async fn default_branch(&self, host: &str, repository: &str) -> Result<String, DepError>;
}

pub struct RealProjects(pub Arc<ordin_projects::ProjectService>);

#[async_trait::async_trait]
impl ProjectsPort for RealProjects {
    async fn project_for_place(
        &self,
        place_id: PlaceId,
    ) -> Result<Option<(ProjectId, bool)>, DepError> {
        self.0
            .project_for_place(place_id)
            .await
            .map(|p| p.map(|p| (p.id, p.is_git_repository)))
            .map_err(|e| DepError::of(&e))
    }
    async fn start_coding_task(
        &self,
        req: ordin_projects::StartCodingTask,
    ) -> Result<CodingTaskId, DepError> {
        self.0
            .start_coding_task(req)
            .await
            .map(|s| s.task.id)
            .map_err(|e| DepError::of(&e))
    }
    async fn is_coding_task(&self, task_id: TaskId) -> Result<bool, DepError> {
        self.0
            .is_coding_task_origin(ordin_types::domain::RunOrigin::Task { task_id })
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn list_files(
        &self,
        folder: &ordin_projects::RepoFolderRef,
        glob: &str,
    ) -> Result<Vec<ordin_projects::RepoFile>, DepError> {
        use ordin_projects::GithubAccess;
        self.0
            .list_files(folder, glob)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn read_file(
        &self,
        folder: &ordin_projects::RepoFolderRef,
        name: &str,
    ) -> Result<(Vec<u8>, String), DepError> {
        use ordin_projects::GithubAccess;
        self.0
            .read_file(folder, name)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn file_last_commit(
        &self,
        folder: &ordin_projects::RepoFolderRef,
        name: &str,
    ) -> Result<String, DepError> {
        use ordin_projects::GithubAccess;
        self.0
            .file_last_commit(folder, name)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn default_branch(&self, host: &str, repository: &str) -> Result<String, DepError> {
        <ordin_projects::ProjectService as ordin_projects::GithubAccess>::default_branch(
            &self.0, host, repository,
        )
        .await
        .map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-27 File Service

/// A place file's bytes as the Journal records them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceFile {
    pub hash: ordin_types::num::Sha256Hex,
    pub media_type: String,
}

#[async_trait::async_trait]
pub trait FilesPort: Send + Sync {
    async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<PlaceFile, DepError>;
    /// OH-TW-2 (X-27 `place`): one place, for its path.
    // spec: ordin-desktop/components/OrdinHost#OH-TW-2
    async fn place(
        &self,
        place_id: PlaceId,
    ) -> Result<ordin_types::domain::places::Place, DepError>;
}

pub struct RealFiles(pub Arc<ordin_files::FileService>);

#[async_trait::async_trait]
impl FilesPort for RealFiles {
    async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<PlaceFile, DepError> {
        self.0
            .read_file(place_id, path)
            .await
            .map(|b| PlaceFile {
                hash: b.hash,
                media_type: b.media_type,
            })
            .map_err(|e| DepError::of(&e))
    }
    async fn place(
        &self,
        place_id: PlaceId,
    ) -> Result<ordin_types::domain::places::Place, DepError> {
        self.0.place(place_id).await.map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-30 Localisation

/// The messages this crate formats (Shared Context *Localisation*; step 46.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocMessage {
    LanguageOffer {
        name: String,
        band: String,
        min: u32,
        max: u32,
    },
    LanguageOfferNoEstimate {
        name: String,
    },
    OfferDraft,
    OfferNotNow,
    Drafted {
        name: String,
        stored: u32,
        english: u32,
    },
    SlackTurn {
        count: u32,
        channel: String,
        link: String,
        ids: String,
    },
    /// OH-WK-1: what Ordin is doing, for the working line.
    DoingFindAgent,
    DoingDelegate {
        agent: String,
    },
    DoingRecall,
    DoingRemember,
    DoingWritePage,
    DoingShow,
    /// OH-MIN-11: the working line while Ordin uses a minutes tool.
    #[cfg(feature = "minutes")]
    DoingTranscribeRecording,
    #[cfg(feature = "minutes")]
    DoingImportMinutes,
    #[cfg(feature = "minutes")]
    DoingExportMinutes,
    /// OH-MIN-2: the words of the minutes body.
    #[cfg(feature = "minutes")]
    MinutesHeadingSummary,
    #[cfg(feature = "minutes")]
    MinutesHeadingDecisions,
    #[cfg(feature = "minutes")]
    MinutesHeadingActions,
    #[cfg(feature = "minutes")]
    MinutesHeadingOpenQuestions,
    #[cfg(feature = "minutes")]
    MinutesOwnerUnassigned,
    #[cfg(feature = "minutes")]
    MinutesNoneRecorded,
    /// OH-MIN-1: the harness's name as Settings › Agents and models shows it.
    #[cfg(feature = "minutes")]
    HarnessName(ordin_types::domain::HarnessKind),
}

/// Localisation's `LanguageMatch`, restated so the stand-ins need no Localisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    Choosable(String),
    NotConfirmed(String),
    NotOffered(String),
    Missing(String),
}

/// One en-GB source of a draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub message_id: String,
    pub english: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftEnding {
    Finished,
    Stopped,
    Paused,
}

#[async_trait::async_trait]
pub trait LocalisationPort: Send + Sync {
    /// `active().locale`.
    fn active_locale(&self) -> String;
    async fn set_locale(&self, locale: &str) -> Result<String, DepError>;
    fn message(&self, message: &LocMessage) -> String;
    fn match_language(&self, requested: &str) -> Result<Match, DepError>;
    fn draft_sources(&self, locale: &str) -> Result<Vec<Source>, DepError>;
    async fn begin_draft(&self, locale: &str, name: &str, total: u32) -> Result<(), DepError>;
    /// `(stored, left_out)`.
    async fn add_drafted(&self, locale: &str, reply: &str) -> Result<(u32, u32), DepError>;
    /// The `in_english` count of the language summary.
    async fn end_draft(&self, locale: &str, end: DraftEnding) -> Result<u32, DepError>;
}

pub struct RealLocalisation(pub Arc<ordin_localisation::Localisation>);

#[async_trait::async_trait]
impl LocalisationPort for RealLocalisation {
    fn active_locale(&self) -> String {
        self.0.active().locale
    }
    async fn set_locale(&self, locale: &str) -> Result<String, DepError> {
        self.0
            .set_locale(Some(locale))
            .await
            .map(|a| a.locale)
            .map_err(|e| DepError::of(&e))
    }
    fn message(&self, message: &LocMessage) -> String {
        use ordin_localisation::id;
        use ordin_localisation::{ArgValue, MessageArgs};
        let count = |n: u32| ArgValue::Count(u64::from(n));
        let (id, args) = match message {
            LocMessage::LanguageOffer {
                name,
                band,
                min,
                max,
            } => (
                id::ORDIN_LANGUAGE_OFFER,
                MessageArgs::new()
                    .with("name", ArgValue::Text(name.clone()))
                    .with("band", ArgValue::Text(band.clone()))
                    .with("min", count(*min))
                    .with("max", count(*max)),
            ),
            LocMessage::LanguageOfferNoEstimate { name } => (
                id::ORDIN_LANGUAGE_OFFER_NO_ESTIMATE,
                MessageArgs::new().with("name", ArgValue::Text(name.clone())),
            ),
            LocMessage::OfferDraft => (id::LANGUAGE_OFFER_DRAFT, MessageArgs::new()),
            LocMessage::OfferNotNow => (id::LANGUAGE_OFFER_NOT_NOW, MessageArgs::new()),
            LocMessage::Drafted {
                name,
                stored,
                english,
            } => (
                id::ORDIN_LANGUAGE_DRAFTED,
                MessageArgs::new()
                    .with("name", ArgValue::Text(name.clone()))
                    .with("stored", count(*stored))
                    .with("english", count(*english)),
            ),
            LocMessage::SlackTurn {
                count: n,
                channel,
                link,
                ids,
            } => (
                id::ORDIN_SLACK_TURN,
                MessageArgs::new()
                    .with("count", count(*n))
                    .with("channel", ArgValue::Text(channel.clone()))
                    .with("link", ArgValue::Text(link.clone()))
                    .with("ids", ArgValue::Text(ids.clone())),
            ),
            // spec: ordin-desktop/components/OrdinHost#OH-WK-1
            LocMessage::DoingFindAgent => (id::ORDIN_DOING_FIND_AGENT, MessageArgs::new()),
            LocMessage::DoingDelegate { agent } => (
                id::ORDIN_DOING_DELEGATE,
                MessageArgs::new().with("agent", ArgValue::Text(agent.clone())),
            ),
            LocMessage::DoingRecall => (id::ORDIN_DOING_RECALL, MessageArgs::new()),
            LocMessage::DoingRemember => (id::ORDIN_DOING_REMEMBER, MessageArgs::new()),
            LocMessage::DoingWritePage => (id::ORDIN_DOING_WRITE_PAGE, MessageArgs::new()),
            LocMessage::DoingShow => (id::ORDIN_DOING_SHOW, MessageArgs::new()),
            // spec: ordin-desktop/components/OrdinHost#OH-MIN-11
            #[cfg(feature = "minutes")]
            LocMessage::DoingTranscribeRecording => {
                (id::ORDIN_DOING_TRANSCRIBE_RECORDING, MessageArgs::new())
            }
            #[cfg(feature = "minutes")]
            LocMessage::DoingImportMinutes => (id::ORDIN_DOING_IMPORT_MINUTES, MessageArgs::new()),
            #[cfg(feature = "minutes")]
            LocMessage::DoingExportMinutes => (id::ORDIN_DOING_EXPORT_MINUTES, MessageArgs::new()),
            // spec: ordin-desktop/components/OrdinHost#OH-MIN-2
            #[cfg(feature = "minutes")]
            LocMessage::MinutesHeadingSummary => (id::MINUTES_HEADING_SUMMARY, MessageArgs::new()),
            #[cfg(feature = "minutes")]
            LocMessage::MinutesHeadingDecisions => {
                (id::MINUTES_HEADING_DECISIONS, MessageArgs::new())
            }
            #[cfg(feature = "minutes")]
            LocMessage::MinutesHeadingActions => (id::MINUTES_HEADING_ACTIONS, MessageArgs::new()),
            #[cfg(feature = "minutes")]
            LocMessage::MinutesHeadingOpenQuestions => {
                (id::MINUTES_HEADING_OPEN_QUESTIONS, MessageArgs::new())
            }
            #[cfg(feature = "minutes")]
            LocMessage::MinutesOwnerUnassigned => {
                (id::MINUTES_OWNER_UNASSIGNED, MessageArgs::new())
            }
            #[cfg(feature = "minutes")]
            LocMessage::MinutesNoneRecorded => (id::MINUTES_NONE_RECORDED, MessageArgs::new()),
            #[cfg(feature = "minutes")]
            LocMessage::HarnessName(kind) => (
                match kind {
                    ordin_types::domain::HarnessKind::ClaudeCode => id::HARNESS_CLAUDE_CODE,
                    ordin_types::domain::HarnessKind::CopilotCli => id::HARNESS_COPILOT,
                },
                MessageArgs::new(),
            ),
        };
        self.0.message(id, &args)
    }
    fn match_language(&self, requested: &str) -> Result<Match, DepError> {
        use ordin_localisation::LanguageMatch;
        self.0
            .match_language(requested)
            .map(|m| match m {
                LanguageMatch::Choosable { locale } => Match::Choosable(locale),
                LanguageMatch::NotConfirmed { locale } => Match::NotConfirmed(locale),
                LanguageMatch::NotOffered { locale } => Match::NotOffered(locale),
                LanguageMatch::Missing { locale } => Match::Missing(locale),
            })
            .map_err(|e| DepError::of(&e))
    }
    fn draft_sources(&self, locale: &str) -> Result<Vec<Source>, DepError> {
        self.0
            .draft_sources(locale)
            .map(|v| {
                v.into_iter()
                    .map(|s| Source {
                        message_id: s.message_id,
                        english: s.english,
                    })
                    .collect()
            })
            .map_err(|e| DepError::of(&e))
    }
    async fn begin_draft(&self, locale: &str, name: &str, total: u32) -> Result<(), DepError> {
        self.0
            .begin_draft(locale, name, total)
            .await
            .map_err(|e| DepError::of(&e))
    }
    async fn add_drafted(&self, locale: &str, reply: &str) -> Result<(u32, u32), DepError> {
        self.0
            .add_drafted(locale, reply)
            .await
            .map(|o| (o.stored, o.left_out))
            .map_err(|e| DepError::of(&e))
    }
    async fn end_draft(&self, locale: &str, end: DraftEnding) -> Result<u32, DepError> {
        use ordin_localisation::DraftEnd;
        let end = match end {
            DraftEnding::Finished => DraftEnd::Finished,
            DraftEnding::Stopped => DraftEnd::Stopped,
            DraftEnding::Paused => DraftEnd::Paused,
        };
        self.0
            .end_draft(locale, end)
            .await
            .map(|s| s.in_english)
            .map_err(|e| DepError::of(&e))
    }
}

// ---------------------------------------------------------------- X-10 Service API block check

#[async_trait::async_trait]
pub trait BlocksPort: Send + Sync {
    async fn check_block(
        &self,
        raw: &serde_json::Value,
        ctx: &ordin_service_api::blocks::BlockCheckContext,
    ) -> Result<ordin_service_api::blocks::Block, Vec<ordin_service_api::blocks::BlockRejection>>;
}

pub struct RealBlocks {
    pub check: Arc<dyn ordin_service_api::blocks::BlockCatalogueCheck>,
    pub refs: Arc<dyn ordin_service_api::blocks::RefResolver>,
}

#[async_trait::async_trait]
impl BlocksPort for RealBlocks {
    async fn check_block(
        &self,
        raw: &serde_json::Value,
        ctx: &ordin_service_api::blocks::BlockCheckContext,
    ) -> Result<ordin_service_api::blocks::Block, Vec<ordin_service_api::blocks::BlockRejection>>
    {
        self.check.check_block(raw, ctx, self.refs.as_ref()).await
    }
}

/// Every port, as the host holds them.
#[derive(Clone)]
pub struct Ports {
    pub supervisor: Arc<dyn SupervisorPort>,
    pub orchestrator: Arc<dyn OrchestratorPort>,
    pub inspector: Arc<dyn InspectorPort>,
    pub usage: Arc<dyn UsagePort>,
    pub workspaces: Arc<dyn WorkspacesPort>,
    pub journal: Arc<dyn JournalPort>,
    pub catalogue: Arc<dyn CataloguePort>,
    pub memory: Arc<dyn MemoryPort>,
    pub knowledge: Arc<dyn KnowledgePort>,
    pub push: Arc<dyn PushPort>,
    pub projects: Arc<dyn ProjectsPort>,
    pub files: Arc<dyn FilesPort>,
    pub localisation: Arc<dyn LocalisationPort>,
    pub blocks: Arc<dyn BlocksPort>,
}
