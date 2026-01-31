//! The providers this component calls, held as private port traits (PackageCatalogue.md *Boundary
//! interfaces this spec uses*): every provider is a concrete type whose crate exports no trait for
//! these calls, so the trait below carries exactly the provider's own signatures and is implemented
//! for the concrete type. A test double implements the same trait.
//!
//! Three calls beyond the core ones are also used: `CatalogueService` as a `PartSource` (`withdrawn`,
//! for the entries a `RepositoryUpdated` notice names), `ResolverAndCompiler::open_bundle` (the
//! installed or candidate bundle, for the lock file's parts, the selections shown as "Uses" and the
//! candidate's folder) and `PackageInspector::model` (the tasks shown on an installed agent). Each is
//! called under the provider's own name and signature (SA-RECON-01).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use ordin_agent_folders::{AfmError, AgentFolder, AgentFolderManager};
use ordin_catalogue::{
    AgentRepositoryRef, CatalogueError, CatalogueNotice, CatalogueService, LibraryEntry,
    LibraryQuery, PartView, RepositorySummary,
};
use ordin_checker::{CheckError, Checker, ExecutablePart, InstallCheck, InstallCheckReport};
use ordin_installers::{
    InstallError, InstallOutcome, InstallRecord, InstallRequest, Installers, VerifyOutcome,
};
use ordin_package_inspector::{
    InspectError, InspectOutcome, InspectRequest, PackageInspector, PackageModel,
};
use ordin_resolver::model::{
    PartKind, PartSource, PartSourceError, RepositoryRef, ResolutionPlan, SourceScope,
    WithdrawnListEntry,
};
use ordin_resolver::{Bundle, PruneReport, ResolveError, ResolveRequest, ResolverAndCompiler};
use ordin_types::domain::agents::{AgentRepository, ExecutablePartDecision};
use ordin_types::{AgentInstanceId, AgentRepositoryId, Page, PartId, WorkspaceId};
use tokio::sync::broadcast;

// spec: ordin-desktop/components/PackageCatalogue#SA-RECON-01
#[async_trait]
pub(crate) trait CataloguePort: Send + Sync {
    fn subscribe(&self) -> broadcast::Receiver<CatalogueNotice>;
    async fn part(&self, id: PartId) -> Result<PartView, CatalogueError>;
    async fn find_part(
        &self,
        repository: AgentRepositoryId,
        kind: PartKind,
        identifier: &str,
        version: Option<&str>,
    ) -> Result<PartView, CatalogueError>;
    async fn search(&self, query: LibraryQuery) -> Result<Page<LibraryEntry>, CatalogueError>;
    async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, CatalogueError>;
    async fn source_scope(&self, home: RepositoryRef) -> Result<SourceScope, CatalogueError>;
    async fn repository_by_ref(
        &self,
        reference: &AgentRepositoryRef,
    ) -> Result<Option<AgentRepository>, CatalogueError>;
    async fn repository_ref(
        &self,
        id: AgentRepositoryId,
    ) -> Result<AgentRepositoryRef, CatalogueError>;
    async fn draft_source(
        &self,
        id: AgentRepositoryId,
        worktree: PathBuf,
    ) -> Result<Arc<dyn PartSource>, CatalogueError>;
    /// `PartSource::withdrawn` of the Catalogue Service.
    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError>;
}

#[async_trait]
impl CataloguePort for CatalogueService {
    fn subscribe(&self) -> broadcast::Receiver<CatalogueNotice> {
        CatalogueService::subscribe(self)
    }
    async fn part(&self, id: PartId) -> Result<PartView, CatalogueError> {
        CatalogueService::part(self, id).await
    }
    async fn find_part(
        &self,
        repository: AgentRepositoryId,
        kind: PartKind,
        identifier: &str,
        version: Option<&str>,
    ) -> Result<PartView, CatalogueError> {
        CatalogueService::find_part(self, repository, kind, identifier, version).await
    }
    async fn search(&self, query: LibraryQuery) -> Result<Page<LibraryEntry>, CatalogueError> {
        CatalogueService::search(self, query).await
    }
    async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, CatalogueError> {
        CatalogueService::list_repositories(self).await
    }
    async fn source_scope(&self, home: RepositoryRef) -> Result<SourceScope, CatalogueError> {
        CatalogueService::source_scope(self, home).await
    }
    async fn repository_by_ref(
        &self,
        reference: &AgentRepositoryRef,
    ) -> Result<Option<AgentRepository>, CatalogueError> {
        CatalogueService::repository_by_ref(self, reference).await
    }
    async fn repository_ref(
        &self,
        id: AgentRepositoryId,
    ) -> Result<AgentRepositoryRef, CatalogueError> {
        CatalogueService::repository_ref(self, id).await
    }
    async fn draft_source(
        &self,
        id: AgentRepositoryId,
        worktree: PathBuf,
    ) -> Result<Arc<dyn PartSource>, CatalogueError> {
        CatalogueService::draft_source(self, id, worktree).await
    }
    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        PartSource::withdrawn(self, scope).await
    }
}

#[async_trait]
pub(crate) trait ResolverPort: Send + Sync {
    async fn resolve(&self, request: &ResolveRequest) -> Result<ResolutionPlan, ResolveError>;
    async fn build(&self, request: &ResolveRequest) -> Result<Bundle, ResolveError>;
    fn prune(&self, keep: &BTreeSet<String>) -> Result<PruneReport, ResolveError>;
    /// The installed or candidate bundle.
    fn open_bundle(&self, plan_hash: &str) -> Result<Bundle, ResolveError>;
}

#[async_trait]
impl ResolverPort for ResolverAndCompiler {
    async fn resolve(&self, request: &ResolveRequest) -> Result<ResolutionPlan, ResolveError> {
        ResolverAndCompiler::resolve(self, request).await
    }
    async fn build(&self, request: &ResolveRequest) -> Result<Bundle, ResolveError> {
        ResolverAndCompiler::build(self, request).await
    }
    fn prune(&self, keep: &BTreeSet<String>) -> Result<PruneReport, ResolveError> {
        ResolverAndCompiler::prune(self, keep)
    }
    fn open_bundle(&self, plan_hash: &str) -> Result<Bundle, ResolveError> {
        ResolverAndCompiler::open_bundle(self, plan_hash)
    }
}

#[async_trait]
pub(crate) trait CheckerPort: Send + Sync {
    async fn check_for_install(
        &self,
        request: &InstallCheck,
    ) -> Result<InstallCheckReport, CheckError>;
    async fn executable_parts(
        &self,
        instance: AgentInstanceId,
    ) -> Result<Vec<ExecutablePart>, CheckError>;
    async fn decide_executable_parts(
        &self,
        id: AgentInstanceId,
        parts: &[ExecutablePart],
    ) -> Result<Vec<ExecutablePartDecision>, CheckError>;
    async fn forget_instance(&self, id: AgentInstanceId) -> Result<(), CheckError>;
}

#[async_trait]
impl CheckerPort for Checker {
    async fn check_for_install(
        &self,
        request: &InstallCheck,
    ) -> Result<InstallCheckReport, CheckError> {
        Checker::check_for_install(self, request).await
    }
    async fn executable_parts(
        &self,
        instance: AgentInstanceId,
    ) -> Result<Vec<ExecutablePart>, CheckError> {
        Checker::executable_parts(self, instance).await
    }
    async fn decide_executable_parts(
        &self,
        id: AgentInstanceId,
        parts: &[ExecutablePart],
    ) -> Result<Vec<ExecutablePartDecision>, CheckError> {
        Checker::decide_executable_parts(self, id, parts).await
    }
    async fn forget_instance(&self, id: AgentInstanceId) -> Result<(), CheckError> {
        Checker::forget_instance(self, id).await
    }
}

#[async_trait]
pub(crate) trait InstallersPort: Send + Sync {
    async fn install(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    async fn update(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    async fn update_in_session(
        &self,
        request: &InstallRequest,
    ) -> Result<InstallOutcome, InstallError>;
    async fn verify(&self, id: AgentInstanceId) -> Result<VerifyOutcome, InstallError>;
    async fn verify_and_repair(
        &self,
        id: AgentInstanceId,
        scope: &SourceScope,
    ) -> Result<InstallOutcome, InstallError>;
    async fn uninstall(&self, id: AgentInstanceId) -> Result<(), InstallError>;
    async fn record(&self, id: AgentInstanceId) -> Result<InstallRecord, InstallError>;
}

#[async_trait]
impl InstallersPort for Installers {
    async fn install(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError> {
        Installers::install(self, request).await
    }
    async fn update(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError> {
        Installers::update(self, request).await
    }
    async fn update_in_session(
        &self,
        request: &InstallRequest,
    ) -> Result<InstallOutcome, InstallError> {
        Installers::update_in_session(self, request).await
    }
    async fn verify(&self, id: AgentInstanceId) -> Result<VerifyOutcome, InstallError> {
        Installers::verify(self, id).await
    }
    async fn verify_and_repair(
        &self,
        id: AgentInstanceId,
        scope: &SourceScope,
    ) -> Result<InstallOutcome, InstallError> {
        Installers::verify_and_repair(self, id, scope).await
    }
    async fn uninstall(&self, id: AgentInstanceId) -> Result<(), InstallError> {
        Installers::uninstall(self, id).await
    }
    async fn record(&self, id: AgentInstanceId) -> Result<InstallRecord, InstallError> {
        Installers::record(self, id).await
    }
}

#[async_trait]
pub(crate) trait FoldersPort: Send + Sync {
    async fn create(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError>;
    async fn remove(&self, id: AgentInstanceId) -> Result<(), AfmError>;
}

#[async_trait]
impl FoldersPort for AgentFolderManager {
    async fn create(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError> {
        AgentFolderManager::create(self, id).await
    }
    async fn remove(&self, id: AgentInstanceId) -> Result<(), AfmError> {
        AgentFolderManager::remove(self, id).await
    }
}

#[async_trait]
pub(crate) trait InspectorPort: Send + Sync {
    async fn inspect(&self, request: &InspectRequest) -> Result<InspectOutcome, InspectError>;
    async fn inspect_candidate(
        &self,
        request: &InspectRequest,
    ) -> Result<PackageModel, InspectError>;
    async fn forget(&self, id: AgentInstanceId) -> Result<(), InspectError>;
    async fn set_workspaces(
        &self,
        id: AgentInstanceId,
        workspace_ids: Vec<WorkspaceId>,
    ) -> Result<(), InspectError>;
    /// The tasks shown on an installed agent.
    async fn model(&self, id: AgentInstanceId) -> Result<PackageModel, InspectError>;
}

#[async_trait]
impl InspectorPort for PackageInspector {
    async fn inspect(&self, request: &InspectRequest) -> Result<InspectOutcome, InspectError> {
        PackageInspector::inspect(self, request).await
    }
    async fn inspect_candidate(
        &self,
        request: &InspectRequest,
    ) -> Result<PackageModel, InspectError> {
        PackageInspector::inspect_candidate(self, request).await
    }
    async fn forget(&self, id: AgentInstanceId) -> Result<(), InspectError> {
        PackageInspector::forget(self, id).await
    }
    async fn set_workspaces(
        &self,
        id: AgentInstanceId,
        workspace_ids: Vec<WorkspaceId>,
    ) -> Result<(), InspectError> {
        PackageInspector::set_workspaces(self, id, workspace_ids).await
    }
    async fn model(&self, id: AgentInstanceId) -> Result<PackageModel, InspectError> {
        PackageInspector::model(self, id).await
    }
}

/// The six providers as the component holds them.
#[derive(Clone)]
pub(crate) struct Ports {
    pub catalogue: Arc<dyn CataloguePort>,
    pub resolver: Arc<dyn ResolverPort>,
    pub checker: Arc<dyn CheckerPort>,
    pub installers: Arc<dyn InstallersPort>,
    pub folders: Arc<dyn FoldersPort>,
    pub inspector: Arc<dyn InspectorPort>,
}
