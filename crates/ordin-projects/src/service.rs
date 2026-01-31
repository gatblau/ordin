//! The service itself: construction, migrations, start, shutdown and the background tasks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use ordin_files::{FileService, PlaceChange};
use ordin_identity::IdentityService;
use ordin_journal::{Journal, MigrationStep, ModuleMigrations, NewJournalEntry};
use ordin_mcp_front::{ServerName, ToolProvider, ToolRegistry};
use ordin_rules::RulesEngine;
use ordin_types::domain::ProjectState;
use ordin_types::{ChangeSetEntryId, CodedError, PlaceId, RunId, WorkspaceId};
use sqlx::SqlitePool;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::error::{file_failed, ProjectError};
use crate::ide::IdeCache;
use crate::lsp::LspState;
use crate::store;
use crate::terminal::Terminals;
use crate::types::{ProjectSettings, RepositoryOriginChange, ServerInstaller, StreamSink};

/// The broadcast channel of `subscribe_original_changes` holds this many changes.
const ORIGINAL_CHANNEL: usize = 256;
/// How long `shutdown` waits for a background task.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

const SQLITE_STEPS: &[MigrationStep] = &[
    (
        1,
        "create project tables",
        include_str!("sql/sqlite_v1.sql"),
    ),
    (
        2,
        "record the content hash of whole-file entries",
        include_str!("sql/sqlite_v2.sql"),
    ),
    (
        3,
        "record the person's answer on language servers per repository",
        include_str!("sql/sqlite_v3.sql"),
    ),
];

/// What the program hands over at construction.
pub struct ProjectDeps {
    /// Datastore owner (D4): this crate's tables through `journal.datastore()`; journal entries
    /// through `append_entry`.
    pub journal: Arc<Journal>,
    /// To call `tools_changed` for `code`, `github`, `gitlab`, `checks` (McpFront `ToolRegistry`).
    pub tool_registry: Arc<dyn ToolRegistry>,
    pub files: Arc<FileService>,
    pub rules: Arc<RulesEngine>,
    pub identity: Arc<IdentityService>,
    pub installer: Arc<dyn ServerInstaller>,
    pub streams: Arc<dyn StreamSink>,
    pub settings: watch::Receiver<ProjectSettings>,
    pub data_root: PathBuf,
}

/// The limits and programs of the service, as their constants state them. Tests shorten the times
/// and point the programs at stand-ins.
pub(crate) struct Tunables {
    pub check_timeout: Duration,
    pub gh_timeout: Duration,
    pub network_timeout: Duration,
    pub diagnostics_settle: Duration,
    pub lsp_start_timeout: Duration,
    pub lsp_request_timeout: Duration,
    // Started by name, from the `PATH` of 2D-1c (ProjectService.md SA-RECON-13).
    pub gh_program: PathBuf,
    pub glab_program: PathBuf,
    /// The person's shell when set; otherwise Internal Logic 13.1.
    pub shell: Option<String>,
    /// macOS `open`.
    pub open_program: PathBuf,
    /// The roots of the IDE detection table: the system `/Applications` and the person's home.
    pub system_applications: PathBuf,
    pub home: Option<PathBuf>,
    /// C-33: the languages confirmed to work in a worktree.
    pub worktree_confirmed: HashSet<String>,
    /// Test only: see `NetAuth::local_remote`.
    #[cfg(any(test, feature = "test-support"))]
    pub local_remote: Option<String>,
    /// Test only: the push host a test put in place of GitHub and GitLab (PROJ-SEAM-2).
    #[cfg(any(test, feature = "test-support"))]
    pub test_host: Option<TestHost>,
}

/// PROJ-SEAM-2: the address a test's stand-in host is reached at, ending in `/`, and the token it
/// is sent instead of Identity's.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
pub(crate) struct TestHost {
    pub address: String,
    pub token: String,
}

/// PROJ-SEAM-2: `https`, or plain `http` on a loopback host, with no query or fragment; the address
/// is kept with one trailing `/`, so `http://127.0.0.1:4123/` does not also match port 41234.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_host_address(address: &str) -> Option<String> {
    if address.contains(['?', '#']) {
        return None;
    }
    let allowed = if address.starts_with("https://") {
        crate::remote::https_host(address).is_some()
    } else {
        let rest = address.strip_prefix("http://")?;
        let authority = rest.split('/').next().unwrap_or_default();
        let host = match authority.split_once(':') {
            Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
                host
            }
            Some(_) => return None,
            None => authority,
        };
        matches!(host, "127.0.0.1" | "localhost")
    };
    allowed.then(|| format!("{}/", address.trim_end_matches('/')))
}

impl Default for Tunables {
    fn default() -> Self {
        Tunables {
            check_timeout: crate::CHECK_TIMEOUT,
            gh_timeout: crate::GH_TIMEOUT,
            network_timeout: crate::GIT_NETWORK_TIMEOUT,
            diagnostics_settle: crate::DIAGNOSTICS_SETTLE,
            lsp_start_timeout: crate::LSP_START_TIMEOUT,
            lsp_request_timeout: crate::LSP_REQUEST_TIMEOUT,
            gh_program: PathBuf::from("gh"),
            glab_program: PathBuf::from("glab"),
            shell: None,
            open_program: PathBuf::from("open"),
            system_applications: PathBuf::from("/Applications"),
            home: std::env::var_os("HOME").map(PathBuf::from),
            worktree_confirmed: crate::WORKTREE_CONFIRMED_LANGUAGES
                .iter()
                .map(|language| (*language).to_owned())
                .collect(),
            #[cfg(any(test, feature = "test-support"))]
            local_remote: None,
            #[cfg(any(test, feature = "test-support"))]
            test_host: None,
        }
    }
}

pub struct ProjectService {
    pub(crate) journal: Arc<Journal>,
    pub(crate) pool: SqlitePool,
    pub(crate) tool_registry: Arc<dyn ToolRegistry>,
    pub(crate) files: Arc<FileService>,
    pub(crate) rules: Arc<RulesEngine>,
    pub(crate) identity: Arc<IdentityService>,
    pub(crate) installer: Arc<dyn ServerInstaller>,
    pub(crate) streams: Arc<dyn StreamSink>,
    pub(crate) settings: watch::Receiver<ProjectSettings>,
    pub(crate) data_root: PathBuf,
    pub(crate) me: Weak<ProjectService>,
    pub(crate) originals: broadcast::Sender<RepositoryOriginChange>,
    pub(crate) cancel: CancellationToken,
    pub(crate) background: Mutex<Vec<JoinHandle<()>>>,
    pub(crate) tunables: Mutex<Tunables>,
    pub(crate) lsp: LspState,
    pub(crate) terminals: Terminals,
    pub(crate) ide_cache: Mutex<IdeCache>,
    /// Pending entries the engineer's own edit touched (Internal Logic 2.2).
    pub(crate) marked_entries: Mutex<HashSet<ChangeSetEntryId>>,
    pub(crate) coverage_checked: Mutex<HashMap<PlaceId, Instant>>,
}

/// A poisoned lock still holds consistent data: every critical section is one assignment.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl ProjectService {
    pub const MODULE: &'static str = "projects";

    /// Data Model as `MigrationStep` tuples, versions 1 and 2; the program registers it (SA-RECON-01).
    /// Module `projects` runs on programs D and P, so it has no PostgreSQL steps.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-01
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: SQLITE_STEPS,
            postgres: &[],
        }
    }

    /// Test support (feature `test-support`, off in every shipped build): names the one `file://`
    /// address that stands in for the repository host, so a test can use a local bare repository.
    #[cfg(feature = "test-support")]
    pub fn allow_local_remote(&self, url: impl Into<String>) {
        lock(&self.tunables).local_remote = Some(url.into());
    }

    /// Test support (feature `test-support`, off in every shipped build): a remote under `address`
    /// is pushed to and fetched from as a GitHub remote, with `token` in place of Identity's
    /// (PROJ-SEAM-2). An address that is neither `https` nor loopback `http` is
    /// `PROJ_REMOTE_UNREACHABLE`, and nothing changes.
    // spec: ordin-desktop/components/ProjectService#PROJ-SEAM-2
    #[cfg(any(test, feature = "test-support"))]
    pub fn use_test_host(&self, address: &str, token: &str) -> Result<(), ProjectError> {
        let address = test_host_address(address).ok_or(ProjectError::RemoteUnreachable)?;
        lock(&self.tunables).test_host = Some(TestHost {
            address,
            token: token.to_owned(),
        });
        Ok(())
    }

    pub async fn new(deps: ProjectDeps) -> Result<Arc<ProjectService>, ProjectError> {
        let pool = deps
            .journal
            .datastore()
            .sqlite()
            .cloned()
            .ok_or(ProjectError::DatastoreUnavailable)?;
        let (originals, _) = broadcast::channel(ORIGINAL_CHANNEL);
        Ok(Arc::new_cyclic(|me| ProjectService {
            journal: deps.journal,
            pool,
            tool_registry: deps.tool_registry,
            files: deps.files,
            rules: deps.rules,
            identity: deps.identity,
            installer: deps.installer,
            streams: deps.streams,
            settings: deps.settings,
            data_root: deps.data_root,
            me: me.clone(),
            originals,
            cancel: CancellationToken::new(),
            background: Mutex::new(Vec::new()),
            tunables: Mutex::new(Tunables::default()),
            lsp: LspState::default(),
            terminals: Terminals::default(),
            ide_cache: Mutex::new(IdeCache::default()),
            marked_entries: Mutex::new(HashSet::new()),
            coverage_checked: Mutex::new(HashMap::new()),
        }))
    }

    /// The provider serving `code`, `github`, `gitlab`, `checks`; the program registers it with the
    /// MCP Front.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-02
    pub fn tool_provider(self: &Arc<Self>) -> Arc<dyn ToolProvider> {
        Arc::new(crate::tools::ProjectTools::new(self.clone()))
    }

    /// Starts the fetch timer; reconciles settings; removes worktrees of tasks already settled or
    /// set aside (crash recovery).
    pub async fn start(self: &Arc<Self>) -> Result<(), ProjectError> {
        self.reconcile_settings().await?;
        self.recover_worktrees().await;
        self.spawn(Self::watch_settings);
        self.spawn(Self::watch_places);
        self.spawn(Self::fetch_timer);
        Ok(())
    }

    /// Stops the timers, every language server and every terminal.
    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let handles: Vec<_> = lock(&self.background).drain(..).collect();
        for handle in handles {
            let _ = tokio::time::timeout(SHUTDOWN_WAIT, handle).await;
        }
        self.stop_all_instances().await;
        self.close_all_terminals();
    }

    fn spawn<F, Fut>(self: &Arc<Self>, task: F)
    where
        F: FnOnce(Arc<ProjectService>) -> Fut,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(task(self.clone()));
        lock(&self.background).push(handle);
    }

    /// Step 1.2: a profile change reconciles the plug-ins and the git-host settings.
    async fn watch_settings(self: Arc<Self>) {
        let mut changes = self.settings.clone();
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                changed = changes.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    if let Err(error) = self.reconcile_settings().await {
                        crate::log::background_failed("reconcile_settings", error.code());
                    }
                }
            }
        }
    }

    /// Internal Logic 2.1 and 2.2: follows the File Service's changes for open projects.
    async fn watch_places(self: Arc<Self>) {
        let mut changes = self.files.subscribe_changes();
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                change = changes.recv() => match change {
                    Ok(change) => {
                        if let Err(error) = self.place_changed(&change).await {
                            crate::log::background_failed("place_changed", error.code());
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
            }
        }
    }

    pub(crate) async fn place_changed(&self, change: &PlaceChange) -> Result<(), ProjectError> {
        let Some(row) = store::open_project_on(&self.pool, change.place_id).await? else {
            return Ok(());
        };
        if !change.by_ordin {
            let marked =
                store::pending_entries_for_file(&self.pool, row.project.id, &change.path).await?;
            lock(&self.marked_entries).extend(marked);
        }
        if row.project.state == ProjectState::Opened {
            let due = {
                let mut checked = lock(&self.coverage_checked);
                let due = checked
                    .get(&change.place_id)
                    .is_none_or(|at| at.elapsed() >= crate::COVERAGE_CHECK_EVERY);
                if due {
                    checked.insert(change.place_id, Instant::now());
                }
                due
            };
            if due {
                self.mark_indexed_when_covered(row.project.id, change.place_id)
                    .await?;
            }
        }
        Ok(())
    }

    /// `opened → indexed` once the File Service has indexed the place (Internal Logic 2.1).
    pub(crate) async fn mark_indexed_when_covered(
        &self,
        project: ordin_types::ProjectId,
        place: PlaceId,
    ) -> Result<(), ProjectError> {
        let coverage = self.files.coverage(&[place]).await.map_err(file_failed)?;
        if coverage.indexed == coverage.total {
            store::set_project_state(&self.pool, project, ProjectState::Indexed).await?;
        }
        Ok(())
    }

    /// One journal entry. A journal failure is logged; the operation it records stands.
    pub(crate) async fn record(
        &self,
        kind: &'static str,
        run_id: Option<RunId>,
        workspace_id: Option<WorkspaceId>,
        summary: &str,
        data: serde_json::Value,
    ) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: Self::MODULE.to_owned(),
            run_id,
            crew_run_id: None,
            workspace_id,
            summary: summary.to_owned(),
            data,
        };
        if let Err(error) = self.journal.append_entry(entry).await {
            crate::log::journal_failed(kind, error.code());
        }
    }

    pub(crate) fn current_settings(&self) -> ProjectSettings {
        self.settings.borrow().clone()
    }

    pub(crate) fn tell_registry(&self, servers: &[&str]) {
        for server in servers {
            if let Ok(name) = ServerName::new(*server) {
                self.tool_registry.tools_changed(&name);
            }
        }
    }

    /// The folder of a place.
    pub(crate) async fn place_folder(&self, place: PlaceId) -> Result<PathBuf, ProjectError> {
        Ok(PathBuf::from(
            self.files.place(place).await.map_err(file_failed)?.path,
        ))
    }

    pub(crate) fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// `FETCH_INTERVAL` timer: the start fetch first, then each repository as it falls due.
    async fn fetch_timer(self: Arc<Self>) {
        self.fetch_due(true).await;
        let mut tick = tokio::time::interval(crate::FETCH_TICK);
        tick.tick().await;
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                _ = tick.tick() => self.fetch_due(false).await,
            }
        }
    }
}
