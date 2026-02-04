//! `SetupAssistant`: construction, registration, the background timer and the read methods
//! (SetupAssistant.md *Public Interface*, *Internal Logic* C, D.4, E, F.1).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use futures::future::{BoxFuture, Shared};
use futures::FutureExt;
use ordin_journal::{Datastore, ModuleMigrations};
use ordin_organisation_profile::{HarnessName, LanguageServerEntry, ProfileState, SnapshotSource};
use ordin_types::domain::identity::{HarnessAttachment, HarnessKind};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::error::SetupError;
use crate::ports::Ports;
use crate::process::ClaudeCodeProbe;
use crate::store;
use crate::types::{
    AppUpdater, BundledProgram, CheckTrigger, HarnessChoice, HarnessProbe, ServerPlatform,
    SetupConfig, SetupDeps, SetupProgram, SetupProgress, SetupReport, UpdateOffer,
};
use crate::updates::CheckFailure;
use crate::TARGET;

/// Background check interval on the desktop (SA-SETUP-01).
const DESKTOP_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Background check interval on the server (2D-4).
const SERVER_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Desktop update checks after the first: every 24 hours (F.3).
const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// `close` waits at most this long for a running check.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// `{ module: "setup", sqlite: [(1, …), (2, "releases and updates", …), (3, "highest version started", …)], postgres: [] }`.
// spec: ordin-desktop/components/SetupAssistant#SA-RECON-02
pub fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: "setup",
        sqlite: &[
            (1, "create setup tables", include_str!("sql/sqlite_v1.sql")),
            (2, "releases and updates", include_str!("sql/sqlite_v2.sql")),
            (
                3,
                "highest version started",
                include_str!("sql/sqlite_v3.sql"),
            ),
        ],
        postgres: &[],
    }
}

pub(crate) type Flight<T> = Mutex<Option<Shared<BoxFuture<'static, T>>>>;

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub(crate) fn read<T>(m: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    m.read().unwrap_or_else(|p| p.into_inner())
}

pub(crate) fn write<T>(m: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    m.write().unwrap_or_else(|p| p.into_inner())
}

/// Runs `make()` unless a run is already in flight in `slot`, in which case the caller waits for
/// that run and receives its result.
pub(crate) async fn single<T, F>(slot: &Flight<T>, make: impl FnOnce() -> F) -> T
where
    T: Clone + Send + 'static,
    F: std::future::Future<Output = T> + Send + 'static,
{
    let flight = {
        let mut guard = lock(slot);
        match guard.as_ref() {
            Some(running) => running.clone(),
            None => {
                let f = make().boxed().shared();
                *guard = Some(f.clone());
                f
            }
        }
    };
    let out = flight.clone().await;
    let mut guard = lock(slot);
    if guard.as_ref().is_some_and(|f| f.ptr_eq(&flight)) {
        *guard = None;
    }
    out
}

/// Durations and values the tests shorten or replace.
/// One language-server install at a time per `(server, version)` (H.2).
pub(crate) type InstallLock = Arc<tokio::sync::Mutex<()>>;

pub(crate) struct Tuning {
    pub sign_in_poll: Duration,
    pub sign_in_wait: Duration,
    pub first_check_after: Duration,
    pub language_table: Vec<LanguageServerEntry>,
    /// The build's update public key (2D-2b), as Tauri's updater writes it.
    pub update_key: Option<String>,
    /// This computer's platform key; `None` reads `std::env::consts`.
    pub platform: Option<&'static str>,
}

/// The build's language-server table (SA-CLOSE-01).
// The table ships empty until the release build fills it (SetupAssistant.md, SA-CLOSE-01).
// spec: ordin-desktop/components/SetupAssistant#SA-CLOSE-01
const LANGUAGE_SERVERS_JSON: &str = include_str!("language-servers.json");

/// The build's language-server table (SA-CLOSE-01), parsed; empty when the embedded table is
/// empty or does not parse.
// spec: ordin-desktop/components/SetupAssistant#SET-REC-1
pub fn bundled_language_servers() -> Vec<LanguageServerEntry> {
    parse_language_servers(LANGUAGE_SERVERS_JSON)
}

/// One language-server table in the form of `language-servers.json`; empty when it does not parse.
pub(crate) fn parse_language_servers(json: &str) -> Vec<LanguageServerEntry> {
    serde_json::from_str(json).unwrap_or_default()
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            sign_in_poll: Duration::from_secs(3),
            sign_in_wait: Duration::from_secs(10 * 60),
            first_check_after: Duration::from_secs(60),
            language_table: bundled_language_servers(),
            // SPEC-GAP-4: no committed key file yet (SetupAssistant.md SET-UP-18); archives fail closed.
            update_key: None,
            platform: None,
        }
    }
}

pub(crate) struct Inner {
    pub ports: Ports,
    pub program: SetupProgram,
    pub bundled: Vec<BundledProgram>,
    pub app_version: String,
    pub http: reqwest::Client,
    pub pool: SqlitePool,
    pub data_root: PathBuf,
    pub probes: RwLock<Vec<Arc<dyn HarnessProbe>>>,
    /// The person's harness choice, registered by the desktop (SET-CHO-1).
    pub choice: RwLock<Option<Arc<dyn HarnessChoice>>>,
    /// The harnesses offered while a guided run waits at `HarnessChoice` (SET-CHO-3).
    pub pending_choice: Mutex<Option<Vec<HarnessKind>>>,
    /// The answer to the question, sent by `choose_harness` to the waiting run (SET-CHO-4).
    pub chose: watch::Sender<Option<HarnessKind>>,
    pub updater: RwLock<Option<Arc<dyn AppUpdater>>>,
    pub platform: RwLock<Option<Arc<dyn ServerPlatform>>>,
    pub progress: watch::Sender<SetupProgress>,
    pub guided_flight: Flight<Result<SetupReport, SetupError>>,
    pub checks_flight: Flight<Result<SetupReport, SetupError>>,
    pub run_lock: tokio::sync::Mutex<()>,
    /// Bumped each time `sign_in_harness` makes an attachment ready, so a waiting guided run
    /// resumes.
    pub signed_in: watch::Sender<u64>,
    pub offer: Mutex<Option<UpdateOffer>>,
    pub check_flight: Flight<Result<Option<UpdateOffer>, CheckFailure>>,
    pub download_lock: tokio::sync::Mutex<()>,
    pub download_task: Mutex<Option<JoinHandle<()>>>,
    pub server_task: Mutex<Option<JoinHandle<()>>>,
    pub background: Mutex<Option<JoinHandle<()>>>,
    pub background_started: AtomicBool,
    pub timers_started: AtomicU32,
    pub closed: watch::Sender<bool>,
    pub install_locks: Mutex<HashMap<(String, String), InstallLock>>,
    pub no_feed_logged: AtomicBool,
    pub program_versions: Mutex<BTreeMap<String, String>>,
    pub tuning: RwLock<Tuning>,
}

pub struct SetupAssistant {
    pub(crate) inner: Arc<Inner>,
}

/// The data root: the folder of the Journal's SQLite file.
// SetupAssistant.md, SA-CLOSE-02: the folder of the Journal's SQLite file.
// spec: ordin-desktop/components/SetupAssistant#SA-CLOSE-02
fn data_root_of(pool: &SqlitePool) -> PathBuf {
    let file = pool.connect_options().get_filename().to_path_buf();
    file.parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
}

impl SetupAssistant {
    /// `store` is the Journal's `Datastore`; a PostgreSQL store is refused with
    /// `SETUP_STORE_UNSUPPORTED` (the desktop and the personal server are SQLite only).
    pub fn new(deps: SetupDeps, config: SetupConfig, store: Datastore) -> Result<Self, SetupError> {
        let ports = Ports {
            profile: deps.profile,
            identity: deps.identity,
            catalogue: deps.catalogue,
            packages: deps.packages,
            craft: deps.craft,
        };
        Self::with_ports(ports, config, store, Tuning::default())
    }

    /// The Organisation Profile service and nothing else: no credential, repository, agent
    /// folder or app to check. For integration tests of the update path.
    #[cfg(feature = "test-support")]
    pub fn with_profile_only(
        profile: Arc<ordin_organisation_profile::OrganisationProfileService>,
        config: SetupConfig,
        store: Datastore,
    ) -> Result<Self, SetupError> {
        let inert = Arc::new(crate::ports::Inert);
        let ports = Ports {
            profile,
            identity: inert.clone(),
            catalogue: inert.clone(),
            packages: inert.clone(),
            craft: inert,
        };
        Self::with_ports(ports, config, store, Tuning::default())
    }

    pub(crate) fn with_ports(
        ports: Ports,
        config: SetupConfig,
        store: Datastore,
        tuning: Tuning,
    ) -> Result<Self, SetupError> {
        let Some(pool) = store.sqlite().cloned() else {
            tracing::error!(
                target: TARGET,
                code = "SETUP_STORE_UNSUPPORTED",
                "setup.store_unsupported"
            );
            return Err(SetupError::permanent("SETUP_STORE_UNSUPPORTED"));
        };
        let data_root = data_root_of(&pool);
        let inner = Inner {
            ports,
            program: config.program,
            bundled: config.bundled_programs,
            app_version: config.application_version,
            http: config.http,
            pool,
            data_root,
            probes: RwLock::new(vec![Arc::new(ClaudeCodeProbe::new())]),
            choice: RwLock::new(None),
            pending_choice: Mutex::new(None),
            chose: watch::channel(None).0,
            updater: RwLock::new(None),
            platform: RwLock::new(None),
            progress: watch::channel(SetupProgress {
                running: false,
                steps: Vec::new(),
                done: 0,
                total: 0,
            })
            .0,
            guided_flight: Mutex::new(None),
            checks_flight: Mutex::new(None),
            run_lock: tokio::sync::Mutex::new(()),
            signed_in: watch::channel(0).0,
            offer: Mutex::new(None),
            check_flight: Mutex::new(None),
            download_lock: tokio::sync::Mutex::new(()),
            download_task: Mutex::new(None),
            server_task: Mutex::new(None),
            background: Mutex::new(None),
            background_started: AtomicBool::new(false),
            timers_started: AtomicU32::new(0),
            closed: watch::channel(false).0,
            install_locks: Mutex::new(HashMap::new()),
            no_feed_logged: AtomicBool::new(false),
            program_versions: Mutex::new(BTreeMap::new()),
            tuning: RwLock::new(tuning),
        };
        Ok(SetupAssistant {
            inner: Arc::new(inner),
        })
    }

    /// A second probe for the same harness replaces the first.
    pub fn register_probe(&self, probe: Arc<dyn HarnessProbe>) {
        let mut probes = write(&self.inner.probes);
        probes.retain(|p| p.harness() != probe.harness());
        probes.push(probe);
    }

    /// The person's harness choice; replaces any earlier one (SET-CHO-1). The desktop only.
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-1
    pub fn set_harness_choice(&self, choice: Arc<dyn HarnessChoice>) {
        *write(&self.inner.choice) = Some(choice);
    }

    /// The person's answer to the first-start question (SET-CHO-4).
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-4
    pub async fn choose_harness(&self, harness: HarnessKind) -> Result<(), SetupError> {
        self.inner.choose_harness(harness).await
    }

    /// True on the first start, or when the profile's setup requirements changed since the last
    /// completed run (C).
    pub async fn needs_guided_setup(&self) -> Result<bool, SetupError> {
        let digest = self.inner.requirements_digest();
        Ok(match store::setup_state(&self.inner.pool).await? {
            None => true,
            Some((None, _)) => true,
            Some((Some(_), stored)) => stored.as_deref() != Some(digest.as_str()),
        })
    }

    /// The guided sequence. Returns when every applicable step has ended; asks nothing but a
    /// sign-in.
    pub async fn run_guided(&self) -> Result<SetupReport, SetupError> {
        let inner = self.inner.clone();
        single(&self.inner.guided_flight, move || async move {
            inner.guided().await
        })
        .await
    }

    /// The same checks without the guided screen; repairs once each; never starts a sign-in.
    pub async fn run_checks(&self, trigger: CheckTrigger) -> Result<SetupReport, SetupError> {
        self.inner.clone().run_checks(trigger).await
    }

    /// Starts the harness's own sign-in.
    pub async fn sign_in_harness(
        &self,
        harness: HarnessKind,
    ) -> Result<HarnessAttachment, SetupError> {
        self.inner.sign_in_harness(harness).await
    }

    pub async fn harness_attachments(&self) -> Result<Vec<HarnessAttachment>, SetupError> {
        store::attachments(&self.inner.pool).await
    }

    /// The harness a session should use: first in profile order whose attachment is `ready` (E).
    pub async fn ready_harness(&self) -> Result<HarnessAttachment, SetupError> {
        self.inner.clone().ready_harness().await
    }

    pub fn progress(&self) -> watch::Receiver<SetupProgress> {
        self.inner.progress.subscribe()
    }

    /// The installer's `update_feed_url`, replaced by a verified profile's `updates.feed_url` (F.1).
    // spec: ordin-desktop/components/SetupAssistant#SA-RECON-01
    pub fn effective_update_feed(&self) -> Option<String> {
        self.inner.effective_update_feed()
    }

    /// Desktop: registers the program's updater. Without one, update checks on the desktop are
    /// off.
    pub fn register_updater(&self, updater: Arc<dyn AppUpdater>) {
        *write(&self.inner.updater) = Some(updater);
    }

    /// Spawns the background timer (D.4 checks, F.3 update checks). A second call does nothing.
    pub async fn start_background_checks(&self) {
        if self.inner.background_started.swap(true, Ordering::SeqCst) {
            return;
        }
        // A call after `close` starts no timer: the program's start checks may end after quitting
        // began. Subscribed first, so a `close` after this look still ends the timer.
        // spec: ordin-desktop/components/SetupAssistant#SET-SC-1
        let mut closed = self.inner.closed.subscribe();
        if *closed.borrow_and_update() {
            return;
        }
        self.inner.timers_started.fetch_add(1, Ordering::SeqCst);
        let inner = self.inner.clone();
        let first = read(&self.inner.tuning).first_check_after;
        let interval = match self.inner.program {
            SetupProgram::Desktop => DESKTOP_INTERVAL,
            SetupProgram::PersonalServer => SERVER_INTERVAL,
        };
        let task = tokio::spawn(async move {
            let start = tokio::time::Instant::now();
            let mut next_checks = start + first;
            let mut next_update = start + first;
            loop {
                let due = next_checks.min(next_update);
                tokio::select! {
                    _ = closed.changed() => break,
                    _ = tokio::time::sleep_until(due) => {}
                }
                let now = tokio::time::Instant::now();
                if now >= next_update {
                    next_update = now + UPDATE_INTERVAL;
                    if inner.program == SetupProgram::Desktop {
                        inner.clone().timer_update_check().await;
                    }
                }
                if now >= next_checks {
                    next_checks = now + interval;
                    let _ = inner.clone().run_checks(CheckTrigger::Background).await;
                }
            }
        });
        *lock(&self.inner.background) = Some(task);
    }

    /// Stops the timer; waits at most 2 s for a running check.
    pub async fn close(&self) {
        // Kept even with no timer listening, so a later `start_background_checks` sees it.
        self.inner.closed.send_replace(true);
        let task = lock(&self.inner.background).take();
        if let Some(mut task) = task {
            if tokio::time::timeout(CLOSE_WAIT, &mut task).await.is_err() {
                task.abort();
            }
        }
    }
}

impl Inner {
    pub(crate) fn probe(&self, harness: HarnessKind) -> Option<Arc<dyn HarnessProbe>> {
        read(&self.probes)
            .iter()
            .find(|p| p.harness() == harness)
            .cloned()
    }

    /// `document.harnesses` in order as harness kinds, without `copilot` on the server (C-48). With no
    /// company profile, `[claude_code, copilot]` with no tested versions (SET-CPX-6); a harness with no
    /// registered probe is skipped by the callers, so a build without `copilot` checks Claude Code only.
    // spec: ordin-desktop/components/SetupAssistant#SET-CPX-6
    pub(crate) fn listed_harnesses(&self) -> Vec<(HarnessKind, Vec<String>)> {
        if matches!(
            self.ports.profile.status().state,
            ProfileState::NotUsed | ProfileState::NoAddress
        ) {
            let mut kinds = vec![(HarnessKind::ClaudeCode, Vec::new())];
            if self.program != SetupProgram::PersonalServer {
                kinds.push((HarnessKind::CopilotCli, Vec::new()));
                // SET-CHO-2: the person's choice is walked first.
                // spec: ordin-desktop/components/SetupAssistant#SET-CHO-2
                if let Some(chosen) = self.chosen_harness() {
                    if let Some(at) = kinds.iter().position(|(k, _)| *k == chosen) {
                        let first = kinds.remove(at);
                        kinds.insert(0, first);
                    }
                }
            }
            return kinds;
        }
        let snapshot = self.ports.profile.current();
        snapshot
            .document
            .harnesses
            .iter()
            .filter_map(|entry| {
                let kind = match entry.name {
                    HarnessName::ClaudeCode => HarnessKind::ClaudeCode,
                    HarnessName::Copilot => HarnessKind::CopilotCli,
                };
                if kind == HarnessKind::CopilotCli && self.program == SetupProgram::PersonalServer {
                    return None;
                }
                Some((kind, entry.tested_versions.clone()))
            })
            .collect()
    }

    /// The registered choice's answer; `None` with no choice registered (SET-CHO-1).
    pub(crate) fn chosen_harness(&self) -> Option<HarnessKind> {
        let choice = read(&self.choice).clone();
        choice.and_then(|c| c.chosen())
    }

    /// C.1: SHA-256 of `{"harnesses": …, "repositories": [display names, sorted]}`.
    pub(crate) fn requirements_digest(&self) -> String {
        let snapshot = self.ports.profile.current();
        let mut repositories: Vec<&str> = snapshot
            .document
            .agent_catalogue
            .repositories
            .iter()
            .map(|r| r.display_name.as_str())
            .collect();
        repositories.sort_unstable();
        let canonical = serde_json::json!({
            "harnesses": snapshot.document.harnesses,
            "repositories": repositories,
        });
        let text = serde_json::to_string(&canonical).unwrap_or_default();
        hex(&Sha256::digest(text.as_bytes()))
    }

    /// F.1.
    pub(crate) fn effective_update_feed(&self) -> Option<String> {
        let snapshot = self.ports.profile.current();
        if let (SnapshotSource::Verified { .. }, Some(updates)) =
            (&snapshot.source, &snapshot.document.updates)
        {
            return Some(updates.feed_url.clone());
        }
        let feed = self.ports.profile.installer_settings().update_feed_url;
        if feed.is_none() && !self.no_feed_logged.swap(true, Ordering::SeqCst) {
            tracing::info!(target: TARGET, "setup.no_update_feed");
        }
        feed
    }

    pub(crate) fn interval(&self) -> Duration {
        match self.program {
            SetupProgram::Desktop => DESKTOP_INTERVAL,
            SetupProgram::PersonalServer => SERVER_INTERVAL,
        }
    }

    pub(crate) async fn run_checks(
        self: Arc<Self>,
        trigger: CheckTrigger,
    ) -> Result<SetupReport, SetupError> {
        let inner = self.clone();
        single(&self.checks_flight, move || async move {
            inner.checks(trigger).await
        })
        .await
    }

    /// E.
    pub(crate) async fn ready_harness(self: Arc<Self>) -> Result<HarnessAttachment, SetupError> {
        let listed = self.listed_harnesses();
        let interval = chrono::Duration::from_std(self.interval()).unwrap_or_default();
        for (kind, _) in &listed {
            let Some(found) = store::attachment(&self.pool, *kind).await? else {
                continue;
            };
            if found.state != ordin_types::domain::identity::HarnessAttachmentState::Ready {
                continue;
            }
            let now = ordin_types::time::Timestamp::now();
            if now.0 - found.last_checked_at.0 <= interval {
                return Ok(found);
            }
            self.clone().run_checks(CheckTrigger::BeforeSession).await?;
            if let Some(again) = store::attachment(&self.pool, *kind).await? {
                if again.state == ordin_types::domain::identity::HarnessAttachmentState::Ready {
                    return Ok(again);
                }
            }
        }
        Err(SetupError::permanent("SETUP_NO_READY_HARNESS"))
    }
}

/// Lowercase hexadecimal.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
