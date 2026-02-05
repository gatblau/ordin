//! The component value, its construction and the helpers every flow shares: the SQLite pool, the
//! journal, the notices, the per-instance lock, the bundle prune (§A9) and the validation of an
//! instance record against the 2C `AGI_` rules.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use ordin_agent_folders::AgentFolderManager;
use ordin_catalogue::CatalogueService;
use ordin_checker::Checker;
use ordin_installers::Installers;
use ordin_journal::{Datastore, Journal, ModuleMigrations, NewJournalEntry};
use ordin_package_inspector::PackageInspector;
use ordin_resolver::model::BUNDLED_REPOSITORY_ID;
use ordin_resolver::ResolverAndCompiler;
use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::domain::HarnessKind;
use ordin_types::{AgentInstanceId, AgentRepositoryId, WorkspaceId};
use sqlx::SqlitePool;
use tokio::sync::broadcast;

use crate::error::PackageError;
use crate::ports::Ports;
use crate::store::{self, InstanceRow};
use crate::types::AgentNotice;

/// Owned here; implemented by the program over the Session Supervisor (ADR-100 pattern).
pub trait InstanceActivity: Send + Sync {
    fn running(&self, id: AgentInstanceId) -> bool;
    /// Fired when an instance's last run ends.
    fn idle(&self) -> broadcast::Receiver<AgentInstanceId>;
}

/// Owned here; implemented by the program over the Organisation Profile: `ProfileSnapshot::default_harness()`
/// mapped `HarnessName::ClaudeCode` to `HarnessKind::ClaudeCode` and `HarnessName::Copilot` to
/// `HarnessKind::CopilotCli` (OrganisationProfile.md, consumer table).
pub trait HarnessSettings: Send + Sync {
    fn default_harness(&self) -> HarnessKind;
}

/// What the program hands over (PackageCatalogue.md *Public Interface*).
pub struct PackageCatalogueDeps {
    pub datastore: Datastore,
    pub journal: Arc<Journal>,
    pub catalogue: Arc<CatalogueService>,
    pub resolver: Arc<ResolverAndCompiler>,
    pub checker: Arc<Checker>,
    pub installers: Arc<Installers>,
    pub folders: Arc<AgentFolderManager>,
    pub inspector: Arc<PackageInspector>,
    pub harness: Arc<dyn HarnessSettings>,
    pub activity: Arc<dyn InstanceActivity>,
}

/// Module `package_catalogue`; empty `postgres` (not in the agent profile). Registered by the
/// program (R3).
// spec: ordin-desktop/components/PackageCatalogue#SA-RECON-04
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: "package_catalogue",
        sqlite: &[
            (
                1,
                "package catalogue tables",
                include_str!("sql/sqlite_v1.sql"),
            ),
            (2, "starter agent offers", include_str!("sql/sqlite_v2.sql")),
        ],
        postgres: &[],
    }
}

pub(crate) struct Inner {
    pub ports: Ports,
    pub pool: Option<SqlitePool>,
    pub journal: Arc<Journal>,
    pub harness: Arc<dyn HarnessSettings>,
    pub activity: Arc<dyn InstanceActivity>,
    pub notices: broadcast::Sender<AgentNotice>,
    /// Serialises `add` and `add_trial`, so two adds of one agent cannot both pass the §A2 check.
    pub add_lock: tokio::sync::Mutex<()>,
    /// Serialises `bundled_instance`, so one bundled instance is never installed twice at once.
    pub bundled_lock: tokio::sync::Mutex<()>,
    pub instance_locks: Mutex<HashMap<AgentInstanceId, Arc<tokio::sync::Mutex<()>>>>,
    /// Plan hashes of bundles built but not yet named by a stored row, kept out of every prune.
    pub inflight: Mutex<Vec<String>>,
    /// Repositories the company dropped: no update is offered from them again (§D3).
    pub dropped: Mutex<HashSet<AgentRepositoryId>>,
}

/// The Package Catalogue. Cloning is cheap; every clone is the same component.
#[derive(Clone)]
pub struct PackageCatalogue {
    pub(crate) inner: Arc<Inner>,
}

/// A plan hash kept out of the prune until dropped.
pub(crate) struct HashGuard {
    inner: Arc<Inner>,
    hash: String,
}

impl Drop for HashGuard {
    fn drop(&mut self) {
        let mut held = self
            .inner
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(at) = held.iter().position(|h| *h == self.hash) {
            held.remove(at);
        }
    }
}

impl PackageCatalogue {
    /// Bundles are written by the Resolver and Compiler under its own `bundle_root`; this component
    /// keeps no staging folder.
    pub fn new(deps: PackageCatalogueDeps) -> Self {
        let pool = deps.datastore.sqlite().cloned();
        let ports = Ports {
            catalogue: deps.catalogue,
            resolver: deps.resolver,
            checker: deps.checker,
            installers: deps.installers,
            folders: deps.folders,
            inspector: deps.inspector,
        };
        Self::from_parts(ports, pool, deps.journal, deps.harness, deps.activity)
    }

    pub(crate) fn from_parts(
        ports: Ports,
        pool: Option<SqlitePool>,
        journal: Arc<Journal>,
        harness: Arc<dyn HarnessSettings>,
        activity: Arc<dyn InstanceActivity>,
    ) -> Self {
        let (notices, _) = broadcast::channel(64);
        PackageCatalogue {
            inner: Arc::new(Inner {
                ports,
                pool,
                journal,
                harness,
                activity,
                notices,
                add_lock: tokio::sync::Mutex::new(()),
                bundled_lock: tokio::sync::Mutex::new(()),
                instance_locks: Mutex::new(HashMap::new()),
                inflight: Mutex::new(Vec::new()),
                dropped: Mutex::new(HashSet::new()),
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentNotice> {
        self.inner.notices.subscribe()
    }

    pub(crate) fn notify(&self, notice: AgentNotice) {
        // No subscriber is not an error: the notice is for whoever listens.
        let _ = self.inner.notices.send(notice);
    }

    /// The SQLite pool; the `postgres` slice is empty, so any other datastore is a store failure.
    pub(crate) fn pool(&self) -> Result<&SqlitePool, PackageError> {
        self.inner
            .pool
            .as_ref()
            .ok_or_else(|| PackageError::store(false, "the instance tables are in SQLite only"))
    }

    pub(crate) async fn conn(
        &self,
    ) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, PackageError> {
        Ok(self.pool()?.acquire().await?)
    }

    pub(crate) fn lock_for(&self, id: AgentInstanceId) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .inner
            .instance_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(locks.entry(id).or_default())
    }

    pub(crate) fn hold_hash(&self, hash: &str) -> HashGuard {
        self.inner
            .inflight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(hash.to_owned());
        HashGuard {
            inner: Arc::clone(&self.inner),
            hash: hash.to_owned(),
        }
    }

    /// Validates the record (the 2C `AGI_` rules) and writes it.
    pub(crate) async fn save(
        &self,
        conn: &mut sqlx::SqliteConnection,
        row: &InstanceRow,
    ) -> Result<(), PackageError> {
        validate_instance(row)?;
        store::save_instance(conn, row).await
    }

    /// A journal entry of kind `kind`; a journal failure is logged and does not undo the step (§A0.6).
    pub(crate) async fn journal(
        &self,
        kind: &str,
        workspace_id: Option<WorkspaceId>,
        summary: &str,
        data: serde_json::Value,
    ) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: "package_catalogue".to_owned(),
            run_id: None,
            crew_run_id: None,
            workspace_id,
            summary: summary.to_owned(),
            data,
        };
        if let Err(error) = self.inner.journal.append_entry(entry).await {
            tracing::warn!(
                component = "package_catalogue",
                code = ordin_types::CodedError::code(&error),
                kind,
                "agent.journal_failed"
            );
        }
    }

    /// The first workspace an instance serves, for its journal entries.
    pub(crate) async fn first_workspace(&self, id: AgentInstanceId) -> Option<WorkspaceId> {
        let mut conn = self.conn().await.ok()?;
        store::workspaces_of(&mut conn, id)
            .await
            .ok()
            .and_then(|all| all.first().copied())
    }

    /// §A9: after every install, update and removal, `resolver.prune(keep)`; a failure is logged and
    /// never fails the step.
    pub(crate) async fn prune(&self) {
        let keep = match self.keep_set().await {
            Ok(keep) => keep,
            Err(error) => {
                tracing::warn!(
                    component = "package_catalogue",
                    code = error.code,
                    "agent.prune_failed"
                );
                return;
            }
        };
        if let Err(error) = self.inner.ports.resolver.prune(&keep) {
            tracing::warn!(
                component = "package_catalogue",
                code = error.code(),
                "agent.prune_failed"
            );
        }
    }

    async fn keep_set(&self) -> Result<BTreeSet<String>, PackageError> {
        let mut conn = self.conn().await?;
        let mut keep: BTreeSet<String> = store::live_plan_hashes(&mut conn)
            .await?
            .into_iter()
            .collect();
        keep.extend(
            self.inner
                .inflight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .cloned(),
        );
        Ok(keep)
    }
}

/// The 2C `AGI_` record rules (PackageCatalogue.md *Shared Context*, `AgentInstance`).
pub(crate) fn validate_instance(row: &InstanceRow) -> Result<(), PackageError> {
    let invalid = |code: &'static str, detail: &str| Err(PackageError::permanent(code, detail));
    let name_len = row.display_name.chars().count();
    if !(1..=120).contains(&name_len) {
        return invalid(
            "AGI_NAME_INVALID",
            "display_name is not 1 to 120 characters",
        );
    }
    let package_ok = (1..=214).contains(&row.package_name.len())
        && row.package_name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        });
    if !package_ok {
        return invalid(
            "AGI_PACKAGE_NAME_INVALID",
            "package_name is not [a-z0-9._-]{1,214}",
        );
    }
    if !(1..=64).contains(&row.package_version.chars().count()) {
        return invalid(
            "AGI_PACKAGE_VERSION_INVALID",
            "package_version is not 1 to 64 characters",
        );
    }
    if row.agent_repository_id.is_some() != row.commit.is_some() {
        return invalid(
            "AGI_REPOSITORY_MISMATCH",
            "a commit is present exactly when a repository is",
        );
    }
    if let Some(commit) = &row.commit {
        // The bundled catalogue's commit value is `ordin-<version>` (CatalogueService), not 40 hex;
        // the 40-hex rule is applied to every other repository.
        let bundled = row.agent_repository_id == Some(BUNDLED_REPOSITORY_ID);
        let hex = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
        if !bundled && !hex {
            return invalid("AGI_COMMIT_INVALID", "commit is not 40 hex characters");
        }
    }
    if row.status == AgentInstanceStatus::Ready && !Path::new(&row.agent_folder_path).is_absolute()
    {
        return invalid("AGI_FOLDER_INVALID", "agent_folder_path is not absolute");
    }
    let refused_without_reason = row.status == AgentInstanceStatus::Refused
        && row.audit_result != Some(ordin_types::domain::agents::AuditResult::Failed)
        && row.stopped_reason.is_none();
    if refused_without_reason || row.stopped_reason.is_some() != row.stopped_by.is_some() {
        return invalid(
            "AGI_STATE_INCONSISTENT",
            "refused needs a failed audit or a stopped reason, and the reason and its cause go together",
        );
    }
    Ok(())
}
