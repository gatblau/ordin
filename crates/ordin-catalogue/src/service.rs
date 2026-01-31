//! The service value, its construction, and the helpers every other module uses.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ordin_agent_model::RepositoryRef;
use ordin_journal::{Datastore, ModuleMigrations};
use ordin_types::AgentRepositoryId;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

use crate::db::{repo_by_id, RepoRow};
use crate::error::CatalogueError;
use crate::types::{CatalogueConfig, CatalogueNotice};

/// What the program hands over: the datastore, the profile, GitHub access, the format plug-ins
/// and the Checker.
// spec: ordin-desktop/components/CatalogueService#SA-RECON-04
pub struct CatalogueDeps {
    /// Desktop and personal server: the Journal's datastore (SQLite). Agent profile: SA-RECON-06.
    pub datastore: Datastore,
    pub profile: Arc<ordin_organisation_profile::OrganisationProfileService>,
    pub github: Arc<dyn ordin_projects::GithubAccess>,
    pub formats: Arc<ordin_format_plugins::FormatPlugins>,
    pub checker: Arc<ordin_checker::Checker>,
}

/// Module `catalogue`: migration 1 is the Data Model, migration 2 the Jira place type (ADR-106),
/// migration 3 the bundled catalogue's digest (CAT-BND-1).
/// `postgres` is empty (SA-RECON-06).
// spec: ordin-desktop/components/CatalogueService#SA-RECON-05
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: "catalogue",
        sqlite: &[
            (1, "catalogue tables", include_str!("sql/sqlite_v1.sql")),
            (
                2,
                "jira lifecycle places",
                include_str!("sql/sqlite_v2.sql"),
            ),
            (
                3,
                "bundled catalogue digest",
                include_str!("sql/sqlite_v3.sql"),
            ),
        ],
        postgres: &[],
    }
}

/// A probe kept for ten minutes (CatalogueService.md §K1).
#[derive(Debug, Clone)]
pub(crate) struct Probe {
    pub host: String,
    pub repository: String,
    pub branch: String,
    pub format: ordin_types::domain::agents::AgentFormat,
    pub path: PathBuf,
    pub can_publish: bool,
    pub expires: Instant,
}

/// Growth of the fetch interval after repeated `Unreachable` results (§C6).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Backoff {
    pub streak: u32,
    pub skip_ticks: u32,
}

pub(crate) struct Inner {
    pub deps: CatalogueDeps,
    pub config: CatalogueConfig,
    pub pool: Option<SqlitePool>,
    pub notices: broadcast::Sender<CatalogueNotice>,
    pub repo_locks: Mutex<HashMap<AgentRepositoryId, Arc<tokio::sync::Mutex<()>>>>,
    pub reconcile_lock: tokio::sync::Mutex<()>,
    pub probes: Mutex<HashMap<uuid::Uuid, Probe>>,
    pub backoff: Mutex<HashMap<AgentRepositoryId, Backoff>>,
    /// Health lines an ingest found, waiting for the background report that stores them.
    pub pending_health: Mutex<HashMap<AgentRepositoryId, Vec<crate::types::HealthLine>>>,
}

/// The Catalogue Service. Cloning is cheap; every clone is the same service, so one clone can be
/// bound as the Checker's and the Resolver's `PartSource` while the program keeps another.
#[derive(Clone)]
pub struct CatalogueService {
    pub(crate) inner: Arc<Inner>,
}

impl CatalogueService {
    // spec: ordin-desktop/components/CatalogueService#SA-RECON-06
    // spec: ordin-desktop/components/CatalogueService#SA-CLOSE-01
    pub fn new(deps: CatalogueDeps, config: CatalogueConfig) -> Self {
        let pool = deps.datastore.sqlite().cloned();
        let (notices, _) = broadcast::channel(64);
        CatalogueService {
            inner: Arc::new(Inner {
                deps,
                config,
                pool,
                notices,
                repo_locks: Mutex::new(HashMap::new()),
                reconcile_lock: tokio::sync::Mutex::new(()),
                probes: Mutex::new(HashMap::new()),
                backoff: Mutex::new(HashMap::new()),
                pending_health: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CatalogueNotice> {
        self.inner.notices.subscribe()
    }

    pub(crate) fn notify(&self, notice: CatalogueNotice) {
        // No subscriber is not an error: the notice is for whoever listens.
        let _ = self.inner.notices.send(notice);
    }

    /// The SQLite pool; the postgres slice is empty, so any other datastore is a store failure.
    pub(crate) fn pool(&self) -> Result<&SqlitePool, CatalogueError> {
        self.inner
            .pool
            .as_ref()
            .ok_or_else(|| CatalogueError::store("the catalogue's tables are in SQLite only"))
    }

    pub(crate) fn lock_for(&self, id: AgentRepositoryId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .inner
            .repo_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(locks.entry(id).or_default())
    }

    pub(crate) async fn repo(&self, id: AgentRepositoryId) -> Result<RepoRow, CatalogueError> {
        repo_by_id(self.pool()?, id)
            .await?
            .ok_or_else(CatalogueError::repository_unknown)
    }

    pub(crate) async fn repo_of(
        &self,
        reference: &RepositoryRef,
    ) -> Result<RepoRow, CatalogueError> {
        let id = match reference {
            RepositoryRef::Repository(id) => *id,
            RepositoryRef::Bundled => ordin_agent_model::BUNDLED_REPOSITORY_ID,
        };
        self.repo(id).await
    }

    pub(crate) fn clones_root(&self) -> PathBuf {
        self.inner.config.root.join("clones")
    }

    pub(crate) fn clone_path(&self, host: &str, repository: &str) -> PathBuf {
        let mut path = self.clones_root().join(host);
        for segment in repository.split('/') {
            path = path.join(segment);
        }
        path
    }
}

pub(crate) fn repo_ref_of(row: &RepoRow) -> RepositoryRef {
    if row.is_bundled() {
        RepositoryRef::Bundled
    } else {
        RepositoryRef::Repository(row.id)
    }
}
