//! The service itself: start, the in-memory view of the tables, and the Setup Assistant's check
//! (CraftToolkit.md *Internal Logic* 1 and 8).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_files::FileService;
use ordin_journal::{Datastore, Journal, ModuleMigrations, NewJournalEntry};
use ordin_mcp_front::{ToolProvider, ToolRegistry};
use ordin_organisation_profile::ProfileSnapshot;
use ordin_rules::{PathAccessRequest, RulesEngine};
use ordin_types::domain::engineering::{
    CraftInstallation, CraftInstallationStatus, FileTypeMapEntry,
};
use ordin_types::num::Sha256Hex;
use ordin_types::wire::rules::Verdict;
use ordin_types::wire::startup::{StartUpReportItem, UnavailableReason};
use ordin_types::{CodedError, RunId};
use sqlx::SqlitePool;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::definitions::{self, CraftAppDefinition, UsableDefinition};
use crate::error::CraftError;
use crate::model::{
    BlockedProgram, CraftAppPin, CraftDeps, CraftPinnedAsset, CraftPlatform, CraftProfile,
    CraftProgramCheck, ProgramCheckState,
};
use crate::opening::{Launcher, SystemLauncher};
use crate::profile::{self, display_name};
use crate::provider::CraftAppsProvider;
use crate::servers::{Owner, ServerPool};
use crate::store::{self, Row};

const SQLITE_V1: &str = include_str!("sql/sqlite_v1.sql");
const SQLITE_MIGRATIONS: &[ordin_journal::MigrationStep] = &[(
    1,
    "create craft_installations, file_type_map_entries and file_type_extensions",
    SQLITE_V1,
)];

/// `CALL_TIMEOUT`, 2D-4.
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// `START_TIMEOUT`.
pub(crate) const START_TIMEOUT: Duration = Duration::from_secs(15);
/// `SESSION_IDLE_STOP`.
pub(crate) const SESSION_IDLE_STOP: Duration = Duration::from_secs(600);
/// `MAX_SERVERS` (8 sessions × 2 apps, A-04).
pub(crate) const MAX_SERVERS: usize = 16;
/// `MAX_OUTPUT_BYTES`.
pub(crate) const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
/// `DOWNLOAD_TIMEOUT`.
pub(crate) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// The 10 s limit of the start check and of mode detection (Internal Logic 2.3.5, 2.3.6).
pub(crate) const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// How often idle app servers are looked for.
const IDLE_SWEEP: Duration = Duration::from_secs(30);
/// How long `offer` waits for app servers, inside the MCP Front's 2 s limit.
pub(crate) const OFFER_BUDGET: Duration = Duration::from_millis(1_600);
/// Backoff of the download retries: three tries, so two waits (Internal Logic 2.3.2).
const DOWNLOAD_BACKOFF: Duration = Duration::from_secs(1);

/// The limits above in one value, so that a test can run them short.
#[derive(Debug, Clone)]
pub(crate) struct Limits {
    pub(crate) call: Duration,
    pub(crate) start: Duration,
    pub(crate) idle: Duration,
    pub(crate) max_servers: usize,
    pub(crate) max_output: u64,
    pub(crate) download: Duration,
    pub(crate) backoff: Duration,
    pub(crate) check: Duration,
    pub(crate) offer: Duration,
    pub(crate) sweep: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            call: CALL_TIMEOUT,
            start: START_TIMEOUT,
            idle: SESSION_IDLE_STOP,
            max_servers: MAX_SERVERS,
            max_output: MAX_OUTPUT_BYTES,
            download: DOWNLOAD_TIMEOUT,
            backoff: DOWNLOAD_BACKOFF,
            check: CHECK_TIMEOUT,
            offer: OFFER_BUDGET,
            sweep: IDLE_SWEEP,
        }
    }
}

/// What the synchronous reads answer from: the tables as last read, and what no table keeps.
#[derive(Default)]
pub(crate) struct State {
    pub(crate) rows: BTreeMap<String, Row>,
    pub(crate) file_types: HashMap<String, FileTypeMapEntry>,
    /// The last start check's refusal, by app.
    pub(crate) blocked: BTreeMap<String, BlockedProgram>,
    /// The hash that failed verification, by app: "permanent until the profile pins another file".
    pub(crate) failed_pins: BTreeMap<String, Sha256Hex>,
    /// The app's own input schema, by (app, tool), from its last `tools/list`.
    pub(crate) schemas: HashMap<(String, String), serde_json::Value>,
}

pub struct CraftToolkit {
    pub(crate) journal: Journal,
    pub(crate) registry: Arc<dyn ToolRegistry>,
    pub(crate) rules: Arc<RulesEngine>,
    pub(crate) files: Arc<FileService>,
    pub(crate) http: reqwest::Client,
    pub(crate) data_root: PathBuf,
    pub(crate) platform: CraftPlatform,
    pub(crate) definitions: Vec<UsableDefinition>,
    pub(crate) pool: SqlitePool,
    pub(crate) limits: Limits,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) state: Mutex<State>,
    pub(crate) servers: ServerPool,
    pub(crate) sync_lock: tokio::sync::Mutex<()>,
    profile_feed: watch::Receiver<Arc<ProfileSnapshot>>,
    cancel: CancellationToken,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

/// A version is a folder name: only a SemVer version, which holds no separator, no `..` and no
/// drive, may become part of a path (Internal Logic 2.1).
pub(crate) fn version_ok(version: &str) -> bool {
    semver::Version::parse(version).is_ok()
}

/// Whether `row` was installed from the file `asset` pins. An app with no file for this platform has
/// no hash to compare.
pub(crate) fn hash_current(row: &Row, asset: Option<&CraftPinnedAsset>) -> bool {
    asset.is_none_or(|asset| {
        row.installed_sha256
            .as_deref()
            .is_some_and(|installed| installed.eq_ignore_ascii_case(&asset.sha256.0))
    })
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl CraftToolkit {
    pub(crate) const MODULE: &'static str = "craft";

    /// Module `craft`, for the Journal's registry (ruling R3).
    // spec: ordin-desktop/components/CraftToolkit#SA-RECON-06
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: SQLITE_MIGRATIONS,
            postgres: &[],
        }
    }

    pub async fn new(deps: CraftDeps) -> Result<Arc<CraftToolkit>, CraftError> {
        let feed = deps.profile.subscribe();
        Self::build(deps, feed, Limits::default(), Arc::new(SystemLauncher))
    }

    /// The tables are SQLite only: another datastore is `CRAFT_DATASTORE_UNAVAILABLE`.
    pub(crate) fn build(
        deps: CraftDeps,
        profile_feed: watch::Receiver<Arc<ProfileSnapshot>>,
        limits: Limits,
        launcher: Arc<dyn Launcher>,
    ) -> Result<Arc<CraftToolkit>, CraftError> {
        let Datastore::Sqlite(pool) = deps.journal.datastore().clone() else {
            return Err(CraftError::DatastoreUnavailable);
        };
        Ok(Arc::new(CraftToolkit {
            definitions: definitions::usable(&deps.definitions),
            journal: deps.journal,
            registry: deps.registry,
            rules: deps.rules,
            files: deps.files,
            http: deps.http,
            data_root: deps.data_root,
            platform: deps.platform,
            pool,
            limits,
            launcher,
            state: Mutex::new(State::default()),
            servers: ServerPool::default(),
            sync_lock: tokio::sync::Mutex::new(()),
            profile_feed,
            cancel: CancellationToken::new(),
            tasks: Mutex::new(Vec::new()),
        }))
    }

    /// Loads rows, applies the current profile's file-type map, and watches the profile for changes.
    /// Does not register with the MCP Front (SA-RECON-01).
    // spec: ordin-desktop/components/CraftToolkit#SA-RECON-01
    pub async fn start(self: &Arc<Self>) -> Result<(), CraftError> {
        self.reload_rows().await?;
        self.reload_entries().await?;
        self.apply_file_types(&self.craft_profile()).await?;
        // Taken here, so that a profile arriving before the task first runs is not missed.
        let feed = self.profile_feed.clone();
        let applied = self.craft_profile().profile_version;
        let watcher = tokio::spawn(Arc::clone(self).watch_profile(feed, applied));
        let sweeper = tokio::spawn(Arc::clone(self).sweep_idle());
        lock(&self.tasks).extend([watcher, sweeper]);
        Ok(())
    }

    /// Internal Logic 1.4. The toolkit does not download on its own: a new profile changes the map,
    /// and apps follow at the Setup Assistant's next run.
    async fn watch_profile(
        self: Arc<Self>,
        mut feed: watch::Receiver<Arc<ProfileSnapshot>>,
        mut applied: String,
    ) {
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => return,
                changed = feed.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
            }
            let current = self.craft_profile();
            if current.profile_version == applied {
                continue;
            }
            applied.clone_from(&current.profile_version);
            if let Err(error) = self.apply_file_types(&current).await {
                tracing::warn!(
                    component = "craft",
                    code = error.code(),
                    "craft_map_not_applied"
                );
            }
        }
    }

    /// Stops app servers that have had no call for `SESSION_IDLE_STOP`.
    async fn sweep_idle(self: Arc<Self>) {
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => return,
                () = tokio::time::sleep(self.limits.sweep) => {}
            }
            self.stop_idle().await;
        }
    }

    /// The one `craft_*` provider, for the program to register with the MCP Front before its `start`.
    // spec: ordin-desktop/components/CraftToolkit#SA-RECON-01
    pub fn tool_provider(self: &Arc<Self>) -> Arc<dyn ToolProvider> {
        Arc::new(CraftAppsProvider::new(Arc::clone(self)))
    }

    pub async fn shutdown(&self) {
        self.cancel.cancel();
        let tasks = std::mem::take(&mut *lock(&self.tasks));
        for task in tasks {
            task.abort();
        }
        self.stop_all_servers().await;
    }

    pub async fn installations(&self) -> Result<Vec<CraftInstallation>, CraftError> {
        let rows = store::load_rows(&self.pool).await?;
        Ok(rows.iter().map(Row::to_public).collect())
    }

    /// Setup Assistant: each pinned app's state; with `repair`, runs `sync_installations` first
    /// (SA-RECON-02).
    // spec: ordin-desktop/components/CraftToolkit#SA-RECON-02
    pub async fn check_apps(&self, repair: bool) -> Result<Vec<CraftProgramCheck>, CraftError> {
        let profile = self.craft_profile();
        let ready_before = |rows: &[Row], pin: &CraftAppPin| {
            rows.iter().any(|r| {
                r.app_name == pin.name
                    && r.is_installed()
                    && r.installed_version.as_deref() == Some(pin.version.as_str())
                    && hash_current(r, pin.platforms.get(&self.platform))
            })
        };
        let before = store::load_rows(&self.pool).await?;
        if repair {
            // Internal Logic 8.1: each app's state shows the outcome.
            if let Err(error) = self.sync_installations().await {
                tracing::debug!(
                    component = "craft",
                    code = error.code(),
                    "craft_repair_failed"
                );
            }
        }
        let rows = store::load_rows(&self.pool).await?;
        let blocked = lock(&self.state).blocked.clone();
        let mut checks = Vec::with_capacity(profile.apps.len());
        for pin in &profile.apps {
            let state = if ready_before(&rows, pin) {
                if ready_before(&before, pin) {
                    ProgramCheckState::Ready
                } else {
                    ProgramCheckState::Repaired
                }
            } else {
                self.not_ready_state(&rows, pin, blocked.get(&pin.name))
            };
            checks.push(CraftProgramCheck {
                app_name: pin.name.clone(),
                version: pin.version.clone(),
                state,
            });
        }
        Ok(checks)
    }

    /// A row left `downloaded` with its folder in place has passed everything before the start
    /// check, so its last start check failed even when this process did not see it.
    fn not_ready_state(
        &self,
        rows: &[Row],
        pin: &CraftAppPin,
        blocked: Option<&BlockedProgram>,
    ) -> ProgramCheckState {
        if let Some(found) = blocked {
            return ProgramCheckState::Refused {
                publisher: found
                    .publisher
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
                sha256: found.sha256.clone(),
            };
        }
        let stuck = rows.iter().any(|r| {
            r.app_name == pin.name
                && r.status == CraftInstallationStatus::Downloaded
                && self
                    .version_folder(&pin.name, &pin.version)
                    .is_some_and(|folder| folder.is_dir())
        });
        match pin.platforms.get(&self.platform) {
            Some(asset) if stuck => ProgramCheckState::Refused {
                publisher: "unknown".to_owned(),
                sha256: asset.sha256.clone(),
            },
            _ => ProgramCheckState::Missing,
        }
    }

    /// Start-up report items for a session: one `UnavailablePart` per approved app agents cannot use.
    // spec: ordin-desktop/components/CraftToolkit#C-07
    pub async fn start_up_items(
        &self,
        _grant: &ordin_mcp_front::SessionGrant,
    ) -> Vec<StartUpReportItem> {
        let profile = self.craft_profile();
        let served: Vec<String> = self.served_apps().into_iter().map(|(app, _)| app).collect();
        profile
            .apps
            .iter()
            .filter(|pin| !served.contains(&pin.name))
            .map(|pin| StartUpReportItem::UnavailablePart {
                part: format!("craft_{}", pin.name),
                plain: format!(
                    "{} can't be used by agents on this computer yet.",
                    display_name(&pin.name)
                ),
                reason: UnavailableReason::MissingDependency,
            })
            .collect()
    }

    // ---- shared by the other modules ----

    /// `<app data root>/craft`. The spec's `INSTALL_ROOT` row spells the whole
    /// `<app data root>/craft/<app>/<version>/`, and Internal Logic writes `INSTALL_ROOT/<app>/`.
    pub(crate) fn install_root(&self) -> PathBuf {
        self.data_root.join("craft")
    }

    /// `None` for anything but a version, so that no caller can build a path out of the install root.
    pub(crate) fn version_folder(&self, app: &str, version: &str) -> Option<PathBuf> {
        version_ok(version).then(|| self.install_root().join(app).join(version))
    }

    /// `SCRATCH_ROOT`.
    pub(crate) fn scratch_root(&self) -> PathBuf {
        self.data_root.join("craft-scratch")
    }

    pub(crate) fn conversions_folder(&self) -> PathBuf {
        self.scratch_root().join("conversions")
    }

    pub(crate) fn owner_folder(&self, owner: Owner) -> PathBuf {
        match owner {
            Owner::Run(run_id) => self.scratch_root().join(run_id.to_string()),
            Owner::Conversion => self.conversions_folder(),
        }
    }

    /// The Rules Engine's answer to a path request as this crate's result.
    pub(crate) fn path_access(&self, request: &PathAccessRequest) -> Result<(), CraftError> {
        match self.rules.check_path_access(request).verdict {
            Verdict::Allow => Ok(()),
            _ => Err(CraftError::PathNotAllowed),
        }
    }

    /// The profile in force, as this crate reads it.
    pub(crate) fn craft_profile(&self) -> CraftProfile {
        let snapshot = Arc::clone(&self.profile_feed.borrow());
        profile::build(&snapshot)
    }

    pub(crate) async fn reload_rows(&self) -> Result<(), CraftError> {
        let rows = store::load_rows(&self.pool).await?;
        lock(&self.state).rows = rows
            .into_iter()
            .map(|row| (row.app_name.clone(), row))
            .collect();
        Ok(())
    }

    pub(crate) async fn reload_entries(&self) -> Result<(), CraftError> {
        let entries = store::load_entries(&self.pool).await?;
        self.set_file_types(entries);
        Ok(())
    }

    pub(crate) fn set_file_types(&self, entries: Vec<FileTypeMapEntry>) {
        let mut by_extension = HashMap::new();
        for entry in entries {
            for extension in &entry.extensions {
                by_extension.insert(extension.clone(), entry.clone());
            }
        }
        lock(&self.state).file_types = by_extension;
    }

    pub(crate) fn row(&self, app: &str) -> Option<Row> {
        lock(&self.state).rows.get(app).cloned()
    }

    pub(crate) fn definition(&self, app: &str, version: &str) -> Option<&CraftAppDefinition> {
        definitions::matching(&self.definitions, app, version)
    }

    /// The definition of an installed app, at its installed version.
    pub(crate) fn installed_definition(&self, row: &Row) -> Option<&CraftAppDefinition> {
        self.definition(&row.app_name, row.installed_version.as_deref()?)
    }

    /// The path of an app's command-line tool.
    // With no definition the tool is `<app>-cli` at the top of the unpacked folder.
    pub(crate) fn cli_path(&self, app: &str, version: &str) -> Option<PathBuf> {
        let name = self
            .definition(app, version)
            .map_or_else(|| format!("{app}-cli"), |d| d.cli.clone());
        let folder = self.version_folder(app, version)?;
        Some(match self.platform {
            CraftPlatform::WindowsX86_64 if Path::new(&name).extension().is_none() => {
                folder.join(format!("{name}.exe"))
            }
            _ => folder.join(name),
        })
    }

    /// One journal entry; a failed write is logged, and never fails the work already done.
    pub(crate) async fn record(
        &self,
        kind: &str,
        run_id: Option<RunId>,
        summary: String,
        data: serde_json::Value,
    ) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: Self::MODULE.to_owned(),
            run_id,
            crew_run_id: None,
            workspace_id: None,
            summary,
            data,
        };
        if let Err(error) = self.journal.append_entry(entry).await {
            tracing::warn!(
                component = "craft",
                kind,
                code = CodedError::code(&error),
                "craft_journal_failed"
            );
        }
    }
}
