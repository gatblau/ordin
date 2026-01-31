//! The Team Knowledge Service's traits over their owners (OrdinDesktop.md, hub-trait table row 3.41a,
//! and DSK-TW-2): each method delegates one for one, so the program adds no rule of its own.

use std::sync::Arc;

use ordin_catalogue::CatalogueService;
use ordin_identity::{CredError, IdentityService, PersonProfile};
use ordin_knowledge::{
    KnowledgeError, KnowledgeService, TeamPageFile, TeamPageImport, TeamPageImported,
};
use ordin_localisation::Localisation;
use ordin_memory_service::{MemoryError, MemoryService};
use ordin_package_catalogue::{
    AddAgentInput, AddOutcome, AgentMatch, PackageCatalogue, RemoveAgentInput,
};
use ordin_package_inspector::PackageInspector;
use ordin_projects::{
    BranchReviewRules, GitCredential, GithubAccess, GithubError, ProjectService, RepoPermission,
};
use ordin_push::{PublishCopyRequest, PushError, PushNotice, PushService, TeamShareProposal};
use ordin_session_supervisor::SessionSupervisor;
use ordin_team_knowledge::{
    TeamAgentError, TeamAgentFind, TeamAgentHere, TeamAgents, TeamExportPage, TeamImportBatch,
    TeamImportOutcome, TeamMemory, TeamPages, TeamPush, TeamRepos, TeamRunFacts, TeamRuns,
    TeamRunsError, TeamWorkspaces, TEAM_WS_AGENT_NEEDS_IT, TEAM_WS_AGENT_PACK_MISSING,
    TEAM_WS_AGENT_UNSAFE,
};
use ordin_types::common::AgentRef;
use ordin_types::domain::{
    FolderAccess, FolderGrant, Grantee, PartKind, SourceLink, SourceLinkKind, WorkingCopyItem,
    Workspace,
};
use ordin_types::num::Sha256Hex;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::{
    AgentInstanceId, ApprovalId, CodedError, KnowledgePageId, MemoryFactId, PlaceId,
    PublishedCopyId, RunId, SourceLinkId, TeamGrantId, TeamShareId, WorkspaceId,
};
use ordin_workspaces::{
    GrantDecision, LinkLocation, NewPendingGrant, OriginReach, PendingGrant, Place, TeamResultFile,
    WorkspaceError, WorkspaceService,
};

pub struct DesktopTeamMemory {
    pub memory: Arc<MemoryService>,
}

#[async_trait::async_trait]
impl TeamMemory for DesktopTeamMemory {
    async fn export_team(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<String>,
    ) -> Result<TeamExportPage, MemoryError> {
        self.memory.export_team(workspace_id, cursor).await
    }
    async fn import_team(
        &self,
        workspace_id: WorkspaceId,
        batch: TeamImportBatch,
    ) -> Result<Vec<TeamImportOutcome>, MemoryError> {
        self.memory.import_team(workspace_id, batch).await
    }
    async fn forget_team_fact(
        &self,
        workspace_id: WorkspaceId,
        id: MemoryFactId,
    ) -> Result<(), MemoryError> {
        self.memory.forget_team_fact(workspace_id, id).await
    }
    async fn held_for_check(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<MemoryFactId>, MemoryError> {
        self.memory.held_for_check(workspace_id).await
    }
}

pub struct DesktopTeamPages {
    pub knowledge: Arc<KnowledgeService>,
}

#[async_trait::async_trait]
impl TeamPages for DesktopTeamPages {
    async fn export_team_pages(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<TeamPageFile>, KnowledgeError> {
        self.knowledge.export_team_pages(workspace_id).await
    }
    async fn import_team_page(
        &self,
        workspace_id: WorkspaceId,
        page: TeamPageImport,
    ) -> Result<TeamPageImported, KnowledgeError> {
        self.knowledge.import_team_page(workspace_id, page).await
    }
    async fn remove_team_page(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    ) -> Result<(), KnowledgeError> {
        self.knowledge.remove_team_page(workspace_id, page_id).await
    }
}

pub struct DesktopTeamPush {
    pub push: Arc<PushService>,
}

#[async_trait::async_trait]
impl TeamPush for DesktopTeamPush {
    async fn propose_team_share(&self, p: TeamShareProposal) -> Result<ApprovalId, PushError> {
        self.push.propose_team_share(p).await
    }
    async fn resend_team_share(
        &self,
        share_id: TeamShareId,
        commit: String,
    ) -> Result<(), PushError> {
        self.push.resend_team_share(share_id, commit).await
    }
    async fn withdraw_team_share(&self, share_id: TeamShareId) -> Result<(), PushError> {
        self.push.withdraw_team_share(share_id).await
    }
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PushNotice> {
        self.push.subscribe()
    }
    // spec: ordin-desktop/components/OrdinDesktop#DSK-TW-2
    async fn publish_result(&self, req: PublishCopyRequest) -> Result<PublishedCopyId, PushError> {
        self.push.publish_copy(req).await
    }
}

pub struct DesktopTeamWorkspaces {
    pub workspaces: Arc<WorkspaceService>,
}

#[async_trait::async_trait]
impl TeamWorkspaces for DesktopTeamWorkspaces {
    async fn get(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        self.workspaces.get(id).await
    }
    async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError> {
        self.workspaces.source_links(id).await
    }
    async fn add_link(
        &self,
        id: WorkspaceId,
        location: LinkLocation,
        display_name: Option<String>,
    ) -> Result<SourceLink, WorkspaceError> {
        self.workspaces.add_link(id, location, display_name).await
    }
    fn opened(&self) -> tokio::sync::broadcast::Receiver<WorkspaceId> {
        self.workspaces.opened()
    }
    fn deleted(&self) -> tokio::sync::broadcast::Receiver<WorkspaceId> {
        self.workspaces.deleted()
    }
    fn links_removed(&self) -> tokio::sync::broadcast::Receiver<(WorkspaceId, SourceLinkId)> {
        self.workspaces.links_removed()
    }
    async fn create(&self, name: String) -> Result<Workspace, WorkspaceError> {
        self.workspaces.create(name).await
    }
    async fn rename(&self, id: WorkspaceId, name: String) -> Result<Workspace, WorkspaceError> {
        self.workspaces.rename(id, name).await
    }
    async fn move_to_bin(&self, id: WorkspaceId) -> Result<Workspace, WorkspaceError> {
        self.workspaces.move_to_bin(id).await
    }
    async fn delete_from_bin(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        self.workspaces.delete_from_bin(id).await
    }
    async fn remove_link(&self, link: SourceLinkId) -> Result<(), WorkspaceError> {
        self.workspaces.remove_link(link).await
    }
    async fn working_copy_items(
        &self,
        link: SourceLinkId,
    ) -> Result<Vec<WorkingCopyItem>, WorkspaceError> {
        self.workspaces.working_copy_items(link).await
    }
    async fn grants(&self, id: WorkspaceId) -> Result<Vec<FolderGrant>, WorkspaceError> {
        self.workspaces.grants(id).await
    }
    async fn set_grant(
        &self,
        id: WorkspaceId,
        grantee: Grantee,
        place_id: PlaceId,
        access: Option<FolderAccess>,
    ) -> Result<Option<FolderGrant>, WorkspaceError> {
        self.workspaces
            .set_grant(id, grantee, place_id, access)
            .await
    }
    async fn check_origin(
        &self,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<OriginReach, WorkspaceError> {
        self.workspaces.check_origin(kind, origin_location).await
    }
    async fn add_link_from_origin(
        &self,
        id: WorkspaceId,
        kind: SourceLinkKind,
        origin_location: &str,
        display_name: &str,
    ) -> Result<SourceLink, WorkspaceError> {
        self.workspaces
            .add_link_from_origin(id, kind, origin_location, display_name)
            .await
    }
    async fn make_results_folder(&self, id: WorkspaceId) -> Result<Place, WorkspaceError> {
        self.workspaces.make_results_folder(id).await
    }
    async fn results_folder(&self, id: WorkspaceId) -> Result<Option<PlaceId>, WorkspaceError> {
        self.workspaces.results_folder(id).await
    }
    async fn results_of_run(
        &self,
        id: WorkspaceId,
        run_id: RunId,
    ) -> Result<Vec<String>, WorkspaceError> {
        self.workspaces.results_of_run(id, run_id).await
    }
    async fn unsent_results(&self, id: WorkspaceId) -> Result<Vec<TeamResultFile>, WorkspaceError> {
        self.workspaces.unsent_results(id).await
    }
    async fn read_result(&self, id: WorkspaceId, path: &str) -> Result<Vec<u8>, WorkspaceError> {
        self.workspaces.read_result(id, path).await
    }
    async fn store_team_result(
        &self,
        id: WorkspaceId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Sha256Hex, WorkspaceError> {
        self.workspaces.store_team_result(id, path, bytes).await
    }
    async fn remove_team_result(&self, id: WorkspaceId, path: &str) -> Result<(), WorkspaceError> {
        self.workspaces.remove_team_result(id, path).await
    }
    async fn mark_results_sent(
        &self,
        id: WorkspaceId,
        sent: Vec<(String, Sha256Hex)>,
    ) -> Result<(), WorkspaceError> {
        self.workspaces.mark_results_sent(id, sent).await
    }
    async fn keep_both_results(
        &self,
        id: WorkspaceId,
        path: &str,
        theirs: Vec<u8>,
        mine_to: &str,
    ) -> Result<Sha256Hex, WorkspaceError> {
        self.workspaces
            .keep_both_results(id, path, theirs, mine_to)
            .await
    }
    async fn record_pending_grant(
        &self,
        g: NewPendingGrant,
    ) -> Result<PendingGrant, WorkspaceError> {
        self.workspaces.record_pending_grant(g).await
    }
    async fn pending_grants(&self, id: WorkspaceId) -> Result<Vec<PendingGrant>, WorkspaceError> {
        self.workspaces.pending_grants(id).await
    }
    async fn decide_pending_grant(
        &self,
        grant: TeamGrantId,
        decision: GrantDecision,
    ) -> Result<PendingGrant, WorkspaceError> {
        self.workspaces.decide_pending_grant(grant, decision).await
    }
    async fn end_team(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        self.workspaces.end_team(id).await
    }
    // spec: ordin-desktop/components/OrdinDesktop#DSK-TW-2
    async fn resolve_origin(
        &self,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<LinkLocation, WorkspaceError> {
        self.workspaces.resolve_origin(kind, origin_location).await
    }
}

pub struct DesktopTeamRepos {
    pub projects: Arc<ProjectService>,
    pub identity: Arc<IdentityService>,
}

#[async_trait::async_trait]
impl TeamRepos for DesktopTeamRepos {
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError> {
        GithubAccess::git_credentials(self.projects.as_ref(), host).await
    }
    async fn repository_permission(
        &self,
        host: &str,
        repository: &str,
    ) -> Result<RepoPermission, GithubError> {
        GithubAccess::repository_permission(self.projects.as_ref(), host, repository).await
    }
    async fn branch_review_rules(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewRules, GithubError> {
        GithubAccess::branch_review_rules(self.projects.as_ref(), host, repository, branch).await
    }
    async fn person(&self) -> Result<PersonProfile, CredError> {
        self.identity.person().await
    }
}

/// `TeamAgents` over the Package Catalogue and the Catalogue Service (DSK-TW-2).
pub struct DesktopTeamAgents {
    pub packages: Arc<PackageCatalogue>,
    pub catalogue: Arc<CatalogueService>,
    pub projects: Arc<ProjectService>,
    pub localisation: Arc<Localisation>,
}

fn agent_error<E: CodedError>(e: E) -> TeamAgentError {
    TeamAgentError {
        code: e.code(),
        retryable: e.retryable(),
    }
}

impl DesktopTeamAgents {
    /// The WorkspaceProfile part: at `version`, else at any version.
    async fn part(
        &self,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<Option<ordin_types::PartId>, TeamAgentError> {
        let Some(repo) = self
            .catalogue
            .repository_by_ref(repository)
            .await
            .map_err(agent_error)?
        else {
            return Ok(None);
        };
        if let Ok(view) = self
            .catalogue
            .find_part(repo.id, PartKind::WorkspaceProfile, id, Some(version))
            .await
        {
            return Ok(Some(view.part.id));
        }
        Ok(self
            .catalogue
            .find_part(repo.id, PartKind::WorkspaceProfile, id, None)
            .await
            .ok()
            .map(|v| v.part.id))
    }
}

#[async_trait::async_trait]
impl TeamAgents for DesktopTeamAgents {
    async fn agents_in_workspace(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<TeamAgentHere>, TeamAgentError> {
        let agents = self
            .packages
            .agents_in_workspace(ws)
            .await
            .map_err(agent_error)?;
        let mut out = Vec::with_capacity(agents.len());
        for a in agents {
            let identity = match self.packages.agent_identity(a.agent_instance_id).await {
                Ok((repository, id, version, _)) => Some((repository, id, version)),
                Err(e) if e.code() == "PKG_TRIAL_NOT_NAMEABLE" => None,
                Err(e) => return Err(agent_error(e)),
            };
            out.push(TeamAgentHere {
                agent_instance_id: a.agent_instance_id,
                display_name: a.display_name,
                identity,
            });
        }
        Ok(out)
    }

    async fn find_agent(
        &self,
        ws: WorkspaceId,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<TeamAgentFind, TeamAgentError> {
        match self
            .packages
            .match_workflow_agent(ws, repository, id, version)
            .await
            .map_err(agent_error)?
        {
            AgentMatch::Present {
                agent_instance_id, ..
            } => Ok(TeamAgentFind::Present { agent_instance_id }),
            AgentMatch::OtherVersion {
                agent_instance_id,
                installed_version,
                ..
            } => Ok(TeamAgentFind::OtherVersion {
                agent_instance_id,
                installed_version,
            }),
            AgentMatch::Missing => Ok(match self.part(repository, id, version).await? {
                Some(_) => TeamAgentFind::Available,
                None => TeamAgentFind::NotInYourRepositories,
            }),
        }
    }

    async fn add_agent(
        &self,
        ws: WorkspaceId,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<AgentInstanceId, TeamAgentError> {
        let part = self
            .part(repository, id, version)
            .await?
            .ok_or(TeamAgentError {
                code: "CAT_PART_UNKNOWN",
                retryable: false,
            })?;
        // As the Service API fills `agents.add`: the workspace's languages and the interface locale.
        let workspace_languages = self
            .projects
            .workspace_languages(ws)
            .await
            .map_err(agent_error)?;
        let input = AddAgentInput {
            workspace_id: ws,
            agent: part,
            workspace_languages,
            interface_locale: self.localisation.active().locale,
            choices: Vec::new(),
        };
        match self.packages.add(input).await.map_err(agent_error)? {
            AddOutcome::Ready { agent } => Ok(agent.instance.id),
            AddOutcome::Unsafe => Err(TeamAgentError {
                code: TEAM_WS_AGENT_UNSAFE,
                retryable: false,
            }),
            AddOutcome::NeedsItApproval { .. } => Err(TeamAgentError {
                code: TEAM_WS_AGENT_NEEDS_IT,
                retryable: false,
            }),
            AddOutcome::PackMissing { .. } => Err(TeamAgentError {
                code: TEAM_WS_AGENT_PACK_MISSING,
                retryable: false,
            }),
        }
    }

    async fn remove_agent(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<(), TeamAgentError> {
        self.packages
            .remove(RemoveAgentInput {
                agent_instance_id: agent,
                workspace_id: Some(ws),
            })
            .await
            .map_err(agent_error)
    }
}

/// `TeamRuns` over the Session Supervisor, the Package Catalogue and the Package Inspector
/// (DSK-TW-2; SA-TEAM-TW-03 and SA-TEAM-TW-04).
pub struct DesktopTeamRuns {
    pub supervisor: Arc<SessionSupervisor>,
    pub packages: Arc<PackageCatalogue>,
    pub inspector: Arc<PackageInspector>,
}

#[async_trait::async_trait]
impl TeamRuns for DesktopTeamRuns {
    async fn ended_task(&self, run_id: RunId) -> Result<Option<TeamRunFacts>, TeamRunsError> {
        let view = self
            .supervisor
            .get(run_id)
            .await
            .map_err(|e| TeamRunsError {
                code: e.code(),
                retryable: e.retryable(),
            })?;
        let (Some(task), AgentRef::AgentInstance { agent_instance_id }) =
            (view.task.as_ref(), view.run.agent)
        else {
            return Ok(None);
        };
        let Some(ended_at) = view.run.ended_at else {
            return Ok(None);
        };
        let agent = self
            .packages
            .get(agent_instance_id)
            .await
            .map_err(|e| TeamRunsError {
                code: e.code(),
                retryable: e.retryable(),
            })?;
        let task_title = match &task.command {
            Some(command) => self
                .inspector
                .directory(view.workspace_id)
                .await
                .ok()
                .and_then(|entries| {
                    entries
                        .into_iter()
                        .find(|e| e.agent_instance_id == agent_instance_id)
                        .and_then(|e| e.tasks.into_iter().find(|t| &t.name == command))
                        .and_then(|t| t.title)
                }),
            None => None,
        };
        Ok(Some(TeamRunFacts {
            workspace_id: view.workspace_id,
            agent_instance_id,
            agent_name: agent.instance.display_name,
            task_title,
            state: view.run.state,
            ended_at,
        }))
    }
}
