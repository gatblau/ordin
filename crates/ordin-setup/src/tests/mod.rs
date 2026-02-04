//! Unit tests: the Setup Assistant over doubles of its five dependencies, its probes, the
//! program's updater and platform, and a real SQLite datastore in a temporary folder. HTTP goes to
//! `wiremock` on loopback.

mod acceptance;
mod choice;
mod error_table;
mod logic;
mod updates;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_catalogue::{RepositorySyncOutcome, RepositorySyncReport, RepositorySyncResult};
use ordin_craft_toolkit::CraftProgramCheck;
use ordin_identity::RefreshReport;
use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_organisation_profile::{
    AgentRepositoryEntry, HarnessEntry, HarnessName, InstallerSettings,
    OrganisationProfileDocument, ProfileSnapshot, ProfileState, ProfileStatus, RefreshOutcome,
    RepositoryFormat, SnapshotSource, UpdatesSection,
};
use ordin_package_catalogue::AgentFolderReport;
use ordin_types::domain::identity::HarnessKind;
use ordin_types::time::Timestamp;
use ordin_types::OrganisationProfileId;
use sqlx::SqlitePool;

use crate::assistant::{SetupAssistant, Tuning};
use crate::error::SetupError;
use crate::ports::{CataloguePort, CraftPort, IdentityPort, PackagesPort, Ports, ProfilePort};
use crate::types::{
    AppUpdater, BundledProgram, HarnessLocation, HarnessProbe, HarnessSignIn, InstallKind,
    ProbeError, ProbeRepair, ServerPlatform, ServerRunMode, SetupConfig, SetupProgram, UpdateOffer,
    UpdateState, UpdaterError,
};

/// A folder removed when dropped; canonical.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-setup-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(std::fs::canonicalize(&dir).expect("canonical"))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().expect("lock")
}

// ---------------------------------------------------------------- dependency doubles

pub(crate) struct FakeProfile {
    pub snapshot: Mutex<Arc<ProfileSnapshot>>,
    pub status: Mutex<ProfileState>,
    pub refresh: Mutex<Result<RefreshOutcome, ()>>,
    pub installer: Mutex<InstallerSettings>,
    pub refreshes: AtomicU32,
}

impl FakeProfile {
    fn new() -> FakeProfile {
        FakeProfile {
            snapshot: Mutex::new(Arc::new(verified(document()))),
            status: Mutex::new(ProfileState::Active),
            refresh: Mutex::new(Ok(RefreshOutcome::Unchanged)),
            installer: Mutex::new(InstallerSettings::default()),
            refreshes: AtomicU32::new(0),
        }
    }

    pub fn edit(&self, change: impl FnOnce(&mut OrganisationProfileDocument)) {
        let mut doc = lock(&self.snapshot).document.clone();
        change(&mut doc);
        *lock(&self.snapshot) = Arc::new(verified(doc));
    }
}

#[async_trait::async_trait]
impl ProfilePort for FakeProfile {
    fn current(&self) -> Arc<ProfileSnapshot> {
        lock(&self.snapshot).clone()
    }
    fn status(&self) -> ProfileStatus {
        ProfileStatus {
            state: *lock(&self.status),
            active_version: None,
            last_attempt_at: None,
            last_success_at: None,
            last_failure: None,
        }
    }
    async fn refresh(&self) -> Result<RefreshOutcome, ()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        lock(&self.refresh).clone()
    }
    fn installer_settings(&self) -> InstallerSettings {
        lock(&self.installer).clone()
    }
}

pub(crate) fn document() -> OrganisationProfileDocument {
    ordin_organisation_profile::ProfileSnapshot::defaults().document
}

pub(crate) fn verified(document: OrganisationProfileDocument) -> ProfileSnapshot {
    ProfileSnapshot {
        document,
        source: SnapshotSource::Verified {
            profile_id: OrganisationProfileId::new(),
            profile_version: "1.0.0".to_owned(),
            activated_at: Timestamp::now(),
        },
    }
}

pub(crate) fn repository(name: &str) -> AgentRepositoryEntry {
    AgentRepositoryEntry {
        key: name.to_lowercase(),
        display_name: name.to_owned(),
        host: "github.com".to_owned(),
        repository: format!("acme/{}", name.to_lowercase()),
        protected_branch: "main".to_owned(),
        format: RepositoryFormat::Ordin,
        signed_merges_required: false,
    }
}

pub(crate) fn harness(name: HarnessName, tested: &[&str]) -> HarnessEntry {
    HarnessEntry {
        name,
        tested_versions: tested.iter().map(|s| (*s).to_owned()).collect(),
    }
}

pub(crate) fn feed(url: &str) -> UpdatesSection {
    UpdatesSection {
        feed_url: url.to_owned(),
    }
}

pub(crate) struct FakeIdentity {
    pub report: Mutex<Result<RefreshReport, SetupError>>,
    pub refreshes: AtomicU32,
}

#[async_trait::async_trait]
impl IdentityPort for FakeIdentity {
    async fn refresh_due(&self) -> Result<RefreshReport, SetupError> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        lock(&self.report).clone()
    }
}

pub(crate) struct FakeCatalogue(pub Mutex<Result<RepositorySyncReport, ()>>);

#[async_trait::async_trait]
impl CataloguePort for FakeCatalogue {
    async fn sync_profile_repositories(&self) -> Result<RepositorySyncReport, ()> {
        lock(&self.0).clone()
    }
}

pub(crate) fn synced(outcomes: &[RepositorySyncOutcome]) -> RepositorySyncReport {
    RepositorySyncReport {
        results: outcomes
            .iter()
            .enumerate()
            .map(|(i, o)| RepositorySyncResult {
                display_name: format!("Repository {i}"),
                outcome: o.clone(),
            })
            .collect(),
    }
}

pub(crate) struct FakePackages(pub Mutex<Result<AgentFolderReport, ()>>);

#[async_trait::async_trait]
impl PackagesPort for FakePackages {
    async fn verify_agent_folders(&self, _repair: bool) -> Result<AgentFolderReport, ()> {
        lock(&self.0).clone()
    }
}

pub(crate) struct FakeCraft(pub Mutex<Result<Vec<CraftProgramCheck>, ()>>);

#[async_trait::async_trait]
impl CraftPort for FakeCraft {
    async fn check_apps(&self, _repair: bool) -> Result<Vec<CraftProgramCheck>, ()> {
        lock(&self.0).clone()
    }
}

// ---------------------------------------------------------------- program-side doubles

/// Where the stand-in harness says Claude Code is: an absolute path on this platform.
#[cfg(unix)]
pub(crate) const CLAUDE_AT: &str = "/usr/local/bin/claude";
#[cfg(windows)]
pub(crate) const CLAUDE_AT: &str = "C:/Program Files/claude/claude.exe";

pub(crate) struct FakeProbe {
    pub kind: HarnessKind,
    pub location: Mutex<Result<Option<HarnessLocation>, ProbeError>>,
    pub status: Mutex<Result<HarnessSignIn, ProbeError>>,
    /// The status the probe reports once a sign-in has started.
    pub after_sign_in: Mutex<Option<HarnessSignIn>>,
    pub start_result: Mutex<Result<(), ProbeError>>,
    pub repair: Mutex<Result<ProbeRepair, ProbeError>>,
    pub sign_ins: AtomicU32,
}

impl FakeProbe {
    pub fn new(kind: HarnessKind, version: &str, status: HarnessSignIn) -> Arc<FakeProbe> {
        Arc::new(FakeProbe {
            kind,
            location: Mutex::new(Ok(Some(HarnessLocation {
                path: PathBuf::from(CLAUDE_AT),
                version: version.to_owned(),
            }))),
            status: Mutex::new(Ok(status)),
            after_sign_in: Mutex::new(None),
            start_result: Mutex::new(Ok(())),
            repair: Mutex::new(Ok(ProbeRepair::NothingToDo)),
            sign_ins: AtomicU32::new(0),
        })
    }
}

#[async_trait::async_trait]
impl HarnessProbe for FakeProbe {
    fn harness(&self) -> HarnessKind {
        self.kind
    }
    async fn locate(&self) -> Result<Option<HarnessLocation>, ProbeError> {
        lock(&self.location).clone()
    }
    async fn sign_in_status(&self) -> Result<HarnessSignIn, ProbeError> {
        lock(&self.status).clone()
    }
    async fn start_sign_in(&self) -> Result<(), ProbeError> {
        self.sign_ins.fetch_add(1, Ordering::SeqCst);
        if let Some(next) = lock(&self.after_sign_in).clone() {
            *lock(&self.status) = Ok(next);
        }
        lock(&self.start_result).clone()
    }
    async fn repair(&self) -> Result<ProbeRepair, ProbeError> {
        lock(&self.repair).clone()
    }
}

pub(crate) struct FakeUpdater {
    pub offer: Mutex<Result<Option<(String, String)>, UpdaterError>>,
    pub download: Mutex<Result<(), UpdaterError>>,
    pub install: Mutex<Result<(), UpdaterError>>,
    pub kind: Mutex<InstallKind>,
    pub other: Mutex<Option<PathBuf>>,
    pub check_delay: Mutex<Duration>,
    pub download_delay: Mutex<Duration>,
    pub checks: AtomicU32,
    pub downloads: AtomicU32,
    pub restarts: AtomicU32,
    pub on_exit: AtomicU32,
}

impl FakeUpdater {
    pub fn offering(version: &str) -> Arc<FakeUpdater> {
        Arc::new(FakeUpdater {
            offer: Mutex::new(Ok(Some((
                version.to_owned(),
                format!("Notes for {version}"),
            )))),
            download: Mutex::new(Ok(())),
            install: Mutex::new(Ok(())),
            kind: Mutex::new(InstallKind::PerUser),
            other: Mutex::new(None),
            check_delay: Mutex::new(Duration::ZERO),
            download_delay: Mutex::new(Duration::ZERO),
            checks: AtomicU32::new(0),
            downloads: AtomicU32::new(0),
            restarts: AtomicU32::new(0),
            on_exit: AtomicU32::new(0),
        })
    }
}

#[async_trait::async_trait]
impl AppUpdater for FakeUpdater {
    async fn check(&self, _feed_url: &url::Url) -> Result<Option<UpdateOffer>, UpdaterError> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        let delay = *lock(&self.check_delay);
        tokio::time::sleep(delay).await;
        lock(&self.offer).clone().map(|o| {
            o.map(|(version, notes)| UpdateOffer {
                version,
                notes,
                state: UpdateState::Downloading,
            })
        })
    }
    async fn download(&self, _offer: &UpdateOffer) -> Result<(), UpdaterError> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        let delay = *lock(&self.download_delay);
        tokio::time::sleep(delay).await;
        lock(&self.download).clone()
    }
    async fn install_and_restart(&self) -> Result<(), UpdaterError> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        lock(&self.install).clone()
    }
    async fn install_on_exit(&self) -> Result<(), UpdaterError> {
        self.on_exit.fetch_add(1, Ordering::SeqCst);
        lock(&self.install).clone()
    }
    fn install_kind(&self) -> InstallKind {
        *lock(&self.kind)
    }
    fn other_install(&self) -> Option<PathBuf> {
        lock(&self.other).clone()
    }
}

pub(crate) struct FakePlatform {
    pub mode: ServerRunMode,
    pub dir: PathBuf,
    pub key: &'static str,
    pub restart: Mutex<Result<(), String>>,
    pub restarts: AtomicU32,
}

#[async_trait::async_trait]
impl ServerPlatform for FakePlatform {
    fn run_mode(&self) -> ServerRunMode {
        self.mode
    }
    fn program_dir(&self) -> PathBuf {
        self.dir.clone()
    }
    fn feed_platform(&self) -> &'static str {
        self.key
    }
    async fn restart_unit(&self) -> Result<(), String> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        lock(&self.restart).clone()
    }
}

// ---------------------------------------------------------------- fixture

pub(crate) struct Env {
    pub dir: TempDir,
    pub store: Datastore,
    pub pool: SqlitePool,
    pub profile: Arc<FakeProfile>,
    pub identity: Arc<FakeIdentity>,
    pub catalogue: Arc<FakeCatalogue>,
    pub packages: Arc<FakePackages>,
    pub craft: Arc<FakeCraft>,
    pub claude: Arc<FakeProbe>,
    pub setup: SetupAssistant,
}

pub(crate) struct Options {
    pub program: SetupProgram,
    pub version: &'static str,
    pub bundled: Vec<BundledProgram>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            program: SetupProgram::Desktop,
            version: "0.1.0",
            bundled: Vec::new(),
        }
    }
}

pub(crate) fn short_tuning() -> Tuning {
    Tuning {
        sign_in_poll: Duration::from_millis(10),
        sign_in_wait: Duration::from_millis(800),
        first_check_after: Duration::from_millis(50),
        ..Tuning::default()
    }
}

impl Env {
    pub async fn new() -> Env {
        Env::with(Options::default()).await
    }

    pub async fn with(options: Options) -> Env {
        let dir = TempDir::new();
        let store = open_store(dir.path()).await;
        Env::on(dir, store, options).await
    }

    /// A second assistant over the same folder and datastore (a simulated restart).
    pub async fn on(dir: TempDir, store: Datastore, options: Options) -> Env {
        let pool = store.sqlite().expect("sqlite").clone();
        let profile = Arc::new(FakeProfile::new());
        let identity = Arc::new(FakeIdentity {
            report: Mutex::new(Ok(RefreshReport::default())),
            refreshes: AtomicU32::new(0),
        });
        let catalogue = Arc::new(FakeCatalogue(Mutex::new(Ok(synced(&[
            RepositorySyncOutcome::Cloned,
        ])))));
        let packages = Arc::new(FakePackages(Mutex::new(Ok(AgentFolderReport {
            checked: 1,
            repaired: 0,
            failed: Vec::new(),
        }))));
        let craft = Arc::new(FakeCraft(Mutex::new(Ok(Vec::new()))));
        let ports = Ports {
            profile: profile.clone(),
            identity: identity.clone(),
            catalogue: catalogue.clone(),
            packages: packages.clone(),
            craft: craft.clone(),
        };
        let setup = SetupAssistant::with_ports(
            ports,
            SetupConfig {
                program: options.program,
                bundled_programs: options.bundled,
                application_version: options.version.to_owned(),
                http: reqwest::Client::new(),
            },
            store.clone(),
            short_tuning(),
        )
        .expect("setup");
        let claude = FakeProbe::new(
            HarnessKind::ClaudeCode,
            "2.1.302",
            HarnessSignIn::SignedIn {
                method: Some("claude.ai".to_owned()),
            },
        );
        setup.register_probe(claude.clone());
        Env {
            dir,
            store,
            pool,
            profile,
            identity,
            catalogue,
            packages,
            craft,
            claude,
            setup,
        }
    }

    pub fn tuning(&self) -> std::sync::RwLockWriteGuard<'_, Tuning> {
        crate::assistant::write(&self.setup.inner.tuning)
    }

    /// A bundled program: a shell script at `<dir>/<name>` with `body` after the shebang.
    pub fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        write_script(&path, body);
        path
    }
}

pub(crate) async fn open_store(dir: &Path) -> Datastore {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.join("journal.db"),
        backup_dir: dir.join("backups"),
    })
    .await
    .expect("datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry.register(crate::migrations()).expect("setup");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("migrations");
    store
}

pub(crate) fn write_script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("script");
    crate::updates::set_executable(path).expect("mode");
    wait_until_executable(path);
}

/// A file this process has just written cannot be executed while another test thread still holds
/// an inherited copy of its write handle (`ETXTBSY`, "text file busy", errno 26): the other thread's
/// child has been forked but has not yet executed. The handle closes when that child executes, within
/// milliseconds, so the script is run until the operating system lets it start. Every fake script
/// here only prints, exits or kills itself, so the extra run changes nothing.
#[cfg(unix)]
fn wait_until_executable(path: &Path) {
    const TEXT_FILE_BUSY: i32 = 26;
    for _ in 0..500 {
        match std::process::Command::new(path)
            .arg("--exec-probe")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Err(error) if error.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return,
        }
    }
    panic!("the fake program stayed busy for 5 s");
}

#[cfg(not(unix))]
fn wait_until_executable(_path: &Path) {}

pub(crate) fn bundled(path: PathBuf, name: &str) -> BundledProgram {
    BundledProgram {
        name: name.to_owned(),
        publisher: "gatblau".to_owned(),
        path,
        version_args: vec!["--version".to_owned()],
        expected_sha256: None,
        restore_from: None,
        needed_for: "agent sessions".to_owned(),
    }
}

pub(crate) fn code<T: std::fmt::Debug>(result: Result<T, SetupError>) -> &'static str {
    use ordin_types::CodedError;
    result.expect_err("an error").code()
}

pub(crate) fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}
