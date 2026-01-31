//! The calls this crate makes on its five dependencies, as crate-private traits so that the check
//! runner is tested against doubles; `SetupAssistant::new` wraps the concrete services.

use std::sync::Arc;

use ordin_catalogue::{CatalogueService, RepositorySyncReport};
use ordin_craft_toolkit::{CraftProgramCheck, CraftToolkit};
use ordin_identity::{IdentityService, RefreshReport};
use ordin_organisation_profile::{
    InstallerSettings, OrganisationProfileService, ProfileSnapshot, ProfileStatus, RefreshOutcome,
};
use ordin_package_catalogue::{AgentFolderReport, PackageCatalogue};

use crate::error::SetupError;

#[async_trait::async_trait]
pub(crate) trait ProfilePort: Send + Sync {
    fn current(&self) -> Arc<ProfileSnapshot>;
    fn status(&self) -> ProfileStatus;
    /// `Err` carries nothing: every failure reads `SETUP_PROFILE_UNREACHABLE`.
    async fn refresh(&self) -> Result<RefreshOutcome, ()>;
    fn installer_settings(&self) -> InstallerSettings;
}

#[async_trait::async_trait]
pub(crate) trait IdentityPort: Send + Sync {
    async fn refresh_due(&self) -> Result<RefreshReport, SetupError>;
}

#[async_trait::async_trait]
pub(crate) trait CataloguePort: Send + Sync {
    async fn sync_profile_repositories(&self) -> Result<RepositorySyncReport, ()>;
}

#[async_trait::async_trait]
pub(crate) trait PackagesPort: Send + Sync {
    async fn verify_agent_folders(&self, repair: bool) -> Result<AgentFolderReport, ()>;
}

#[async_trait::async_trait]
pub(crate) trait CraftPort: Send + Sync {
    async fn check_apps(&self, repair: bool) -> Result<Vec<CraftProgramCheck>, ()>;
}

#[async_trait::async_trait]
impl ProfilePort for OrganisationProfileService {
    fn current(&self) -> Arc<ProfileSnapshot> {
        OrganisationProfileService::current(self)
    }
    fn status(&self) -> ProfileStatus {
        OrganisationProfileService::status(self)
    }
    async fn refresh(&self) -> Result<RefreshOutcome, ()> {
        OrganisationProfileService::refresh(self)
            .await
            .map_err(|_| ())
    }
    fn installer_settings(&self) -> InstallerSettings {
        OrganisationProfileService::installer_settings(self)
    }
}

#[async_trait::async_trait]
impl IdentityPort for IdentityService {
    async fn refresh_due(&self) -> Result<RefreshReport, SetupError> {
        IdentityService::refresh_due(self)
            .await
            .map_err(|e| SetupError::foreign(&e))
    }
}

#[async_trait::async_trait]
impl CataloguePort for CatalogueService {
    async fn sync_profile_repositories(&self) -> Result<RepositorySyncReport, ()> {
        CatalogueService::sync_profile_repositories(self)
            .await
            .map_err(|_| ())
    }
}

#[async_trait::async_trait]
impl PackagesPort for PackageCatalogue {
    async fn verify_agent_folders(&self, repair: bool) -> Result<AgentFolderReport, ()> {
        PackageCatalogue::verify_agent_folders(self, repair)
            .await
            .map_err(|_| ())
    }
}

#[async_trait::async_trait]
impl CraftPort for CraftToolkit {
    async fn check_apps(&self, repair: bool) -> Result<Vec<CraftProgramCheck>, ()> {
        CraftToolkit::check_apps(self, repair).await.map_err(|_| ())
    }
}

/// The dependencies as the logic reads them.
#[derive(Clone)]
pub(crate) struct Ports {
    pub profile: Arc<dyn ProfilePort>,
    pub identity: Arc<dyn IdentityPort>,
    pub catalogue: Arc<dyn CataloguePort>,
    pub packages: Arc<dyn PackagesPort>,
    pub craft: Arc<dyn CraftPort>,
}

/// Stand-ins that report nothing to check: no credential, no repository, no agent folder, no app.
/// Used by `SetupAssistant::with_profile_only` (feature `test-support`).
#[cfg(feature = "test-support")]
pub(crate) struct Inert;

#[cfg(feature = "test-support")]
#[async_trait::async_trait]
impl IdentityPort for Inert {
    async fn refresh_due(&self) -> Result<RefreshReport, SetupError> {
        Ok(RefreshReport::default())
    }
}

#[cfg(feature = "test-support")]
#[async_trait::async_trait]
impl CataloguePort for Inert {
    async fn sync_profile_repositories(&self) -> Result<RepositorySyncReport, ()> {
        Ok(RepositorySyncReport {
            results: Vec::new(),
        })
    }
}

#[cfg(feature = "test-support")]
#[async_trait::async_trait]
impl PackagesPort for Inert {
    async fn verify_agent_folders(&self, _repair: bool) -> Result<AgentFolderReport, ()> {
        Ok(AgentFolderReport {
            checked: 0,
            repaired: 0,
            failed: Vec::new(),
        })
    }
}

#[cfg(feature = "test-support")]
#[async_trait::async_trait]
impl CraftPort for Inert {
    async fn check_apps(&self, _repair: bool) -> Result<Vec<CraftProgramCheck>, ()> {
        Ok(Vec::new())
    }
}
