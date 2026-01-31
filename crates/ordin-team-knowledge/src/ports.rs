//! Calls into other modules: traits owned here and implemented by the program over each owner (D3).
//! Each keeps its owner's exact signature, so the program's implementation is a direct delegation
//! (TeamKnowledgeService.md *Shared Context* §2 and the team workspace amendment's Shared Context 3).

use std::path::PathBuf;
use std::sync::Arc;

use ordin_identity::{CredError, PersonProfile};
use ordin_knowledge::{KnowledgeError, TeamPageFile, TeamPageImport, TeamPageImported};
use ordin_memory_service::{MemoryError, TeamExportPage, TeamImportBatch, TeamImportOutcome};
use ordin_projects::{BranchReviewRules, GitCredential, GithubError, RepoPermission};
use ordin_push::{PublishCopyRequest, PushError, PushNotice, TeamShareProposal};
use ordin_types::domain::{
    FolderAccess, FolderGrant, Grantee, RunState, SourceLink, SourceLinkKind, WorkingCopyItem,
    Workspace,
};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::{
    AgentInstanceId, ApprovalId, KnowledgePageId, MemoryFactId, PlaceId, PublishedCopyId, RunId,
    SourceLinkId, TeamGrantId, TeamShareId, WorkspaceId,
};
use ordin_workspaces::{
    GrantDecision, LinkLocation, NewPendingGrant, OriginReach, PendingGrant, Place, TeamResultFile,
    WorkspaceError,
};

/// MemoryService (MemoryService.md, amendment January 2026 — Team knowledge).
#[async_trait::async_trait]
pub trait TeamMemory: Send + Sync {
    async fn export_team(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<String>,
    ) -> Result<TeamExportPage, MemoryError>;
    async fn import_team(
        &self,
        workspace_id: WorkspaceId,
        batch: TeamImportBatch,
    ) -> Result<Vec<TeamImportOutcome>, MemoryError>;
    async fn forget_team_fact(
        &self,
        workspace_id: WorkspaceId,
        id: MemoryFactId,
    ) -> Result<(), MemoryError>;
    async fn held_for_check(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<MemoryFactId>, MemoryError>;
}

/// KnowledgeService (KnowledgeService.md, amendment January 2026 — Team knowledge).
#[async_trait::async_trait]
pub trait TeamPages: Send + Sync {
    async fn export_team_pages(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<TeamPageFile>, KnowledgeError>;
    async fn import_team_page(
        &self,
        workspace_id: WorkspaceId,
        page: TeamPageImport,
    ) -> Result<TeamPageImported, KnowledgeError>;
    async fn remove_team_page(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    ) -> Result<(), KnowledgeError>;
}

/// PushService (PushService.md, amendment January 2026 — Team knowledge shares; PS-TW-1).
#[async_trait::async_trait]
pub trait TeamPush: Send + Sync {
    async fn propose_team_share(&self, p: TeamShareProposal) -> Result<ApprovalId, PushError>;
    async fn resend_team_share(
        &self,
        share_id: TeamShareId,
        commit: String,
    ) -> Result<(), PushError>;
    async fn withdraw_team_share(&self, share_id: TeamShareId) -> Result<(), PushError>;
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PushNotice>;
    // spec: ordin-desktop/components/PushService#PS-TW-2
    async fn publish_result(&self, req: PublishCopyRequest) -> Result<PublishedCopyId, PushError>;
}

/// WorkspaceService (and WorkspaceService.md WS-TW-1 to WS-TW-7 for the team workspace).
#[async_trait::async_trait]
pub trait TeamWorkspaces: Send + Sync {
    async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError>;
    async fn add_link(
        &self,
        id: WorkspaceId,
        location: LinkLocation,
        display_name: Option<String>,
    ) -> Result<SourceLink, WorkspaceError>;
    fn opened(&self) -> tokio::sync::broadcast::Receiver<WorkspaceId>;
    fn deleted(&self) -> tokio::sync::broadcast::Receiver<WorkspaceId>;
    /// Amendment January 2026 (ADR-103): each removed link.
    fn links_removed(&self) -> tokio::sync::broadcast::Receiver<(WorkspaceId, SourceLinkId)>;
    // The team workspace additions (existing methods of the Workspace Service).
    async fn create(&self, name: String) -> Result<Workspace, WorkspaceError>;
    async fn rename(&self, id: WorkspaceId, name: String) -> Result<Workspace, WorkspaceError>;
    async fn move_to_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError>;
    async fn delete_from_bin(&self, id: WorkspaceId) -> Result<(), WorkspaceError>;
    async fn remove_link(&self, link: SourceLinkId) -> Result<(), WorkspaceError>;
    async fn working_copy_items(
        &self,
        link: SourceLinkId,
    ) -> Result<Vec<WorkingCopyItem>, WorkspaceError>;
    async fn grants(&self, id: WorkspaceId) -> Result<Vec<FolderGrant>, WorkspaceError>;
    async fn set_grant(
        &self,
        id: WorkspaceId,
        grantee: Grantee,
        place_id: PlaceId,
        access: Option<FolderAccess>,
    ) -> Result<Option<FolderGrant>, WorkspaceError>;
    // WorkspaceService.md WS-TW-1 to WS-TW-7 (added).
    async fn check_origin(
        &self,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<OriginReach, WorkspaceError>;
    async fn add_link_from_origin(
        &self,
        id: WorkspaceId,
        kind: SourceLinkKind,
        origin_location: &str,
        display_name: &str,
    ) -> Result<SourceLink, WorkspaceError>;
    async fn make_results_folder(&self, id: WorkspaceId) -> Result<Place, WorkspaceError>;
    async fn results_folder(&self, id: WorkspaceId) -> Result<Option<PlaceId>, WorkspaceError>;
    async fn results_of_run(
        &self,
        id: WorkspaceId,
        run_id: RunId,
    ) -> Result<Vec<String>, WorkspaceError>;
    async fn unsent_results(&self, id: WorkspaceId) -> Result<Vec<TeamResultFile>, WorkspaceError>;
    async fn read_result(&self, id: WorkspaceId, path: &str) -> Result<Vec<u8>, WorkspaceError>;
    async fn store_team_result(
        &self,
        id: WorkspaceId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Sha256Hex, WorkspaceError>;
    async fn remove_team_result(&self, id: WorkspaceId, path: &str) -> Result<(), WorkspaceError>;
    async fn mark_results_sent(
        &self,
        id: WorkspaceId,
        sent: Vec<(String, Sha256Hex)>,
    ) -> Result<(), WorkspaceError>;
    async fn keep_both_results(
        &self,
        id: WorkspaceId,
        path: &str,
        theirs: Vec<u8>,
        mine_to: &str,
    ) -> Result<Sha256Hex, WorkspaceError>;
    async fn record_pending_grant(
        &self,
        g: NewPendingGrant,
    ) -> Result<PendingGrant, WorkspaceError>;
    async fn pending_grants(&self, id: WorkspaceId) -> Result<Vec<PendingGrant>, WorkspaceError>;
    async fn decide_pending_grant(
        &self,
        grant: TeamGrantId,
        decision: GrantDecision,
    ) -> Result<PendingGrant, WorkspaceError>;
    async fn end_team(&self, id: WorkspaceId) -> Result<(), WorkspaceError>;
    /// WorkspaceService.md WS-TW-6 step 3: where a team workspace's results are published (TW-G step 3).
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-6
    async fn resolve_origin(
        &self,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<LinkLocation, WorkspaceError>;
}

/// ProjectService as GithubAccess (ProjectService.md, "Repository hosts in process"), and
/// IdentityAndCredentials.
#[async_trait::async_trait]
pub trait TeamRepos: Send + Sync {
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError>;
    async fn repository_permission(
        &self,
        host: &str,
        repository: &str,
    ) -> Result<RepoPermission, GithubError>;
    async fn branch_review_rules(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewRules, GithubError>;
    async fn person(&self) -> Result<PersonProfile, CredError>;
}

/// The agents of a workspace, through the program over the Package Catalogue and the Catalogue
/// Service (the team workspace amendment, Shared Context 3).
#[async_trait::async_trait]
pub trait TeamAgents: Send + Sync {
    async fn agents_in_workspace(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<TeamAgentHere>, TeamAgentError>;
    async fn find_agent(
        &self,
        ws: WorkspaceId,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<TeamAgentFind, TeamAgentError>;
    async fn add_agent(
        &self,
        ws: WorkspaceId,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<AgentInstanceId, TeamAgentError>;
    async fn remove_agent(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<(), TeamAgentError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamAgentHere {
    pub agent_instance_id: AgentInstanceId,
    pub display_name: String,
    /// None for a trial of the person's own draft (PKG_TRIAL_NOT_NAMEABLE): it stays here.
    pub identity: Option<(AgentRepositoryRef, String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamAgentFind {
    Present {
        agent_instance_id: AgentInstanceId,
    },
    OtherVersion {
        agent_instance_id: AgentInstanceId,
        installed_version: String,
    },
    Available,
    NotInYourRepositories,
}

/// The owner's `PKG_*` or `CAT_*` code, or a `TEAM_WS_AGENT_*` code of this crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamAgentError {
    pub code: &'static str,
    pub retryable: bool,
}

/// Ended task runs, through the program over the Session Supervisor, the Package Catalogue and the
/// Package Inspector.
#[async_trait::async_trait]
pub trait TeamRuns: Send + Sync {
    /// None when the run is not one SA-TEAM-TW-03 names.
    async fn ended_task(&self, run_id: RunId) -> Result<Option<TeamRunFacts>, TeamRunsError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct TeamRunFacts {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub agent_name: String,
    pub task_title: Option<String>,
    pub state: RunState,
    pub ended_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRunsError {
    pub code: &'static str,
    pub retryable: bool,
}

/// What the service is built from (Public Interface; the team workspace amendment adds `agents`
/// and `runs`).
pub struct TeamKnowledgeDeps {
    pub journal: Arc<ordin_journal::Journal>,
    pub memory: Arc<dyn TeamMemory>,
    pub pages: Arc<dyn TeamPages>,
    pub push: Arc<dyn TeamPush>,
    pub workspaces: Arc<dyn TeamWorkspaces>,
    pub repos: Arc<dyn TeamRepos>,
    pub agents: Arc<dyn TeamAgents>,
    pub runs: Arc<dyn TeamRuns>,
    /// `<data root>/team-knowledge`
    pub clones_dir: PathBuf,
}
