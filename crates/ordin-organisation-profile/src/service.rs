//! `OrganisationProfileService`: start, the refresh with its single flight, activation, the refresh
//! loop, the personal server's setup step and the remote agent status changes (Internal Logic 1–8).

use std::collections::HashMap;
use std::hash::{BuildHasher as _, Hasher as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use ordin_types::domain::agents::{RemoteAgent, RemoteAgentStatus};
use ordin_types::time::Timestamp;
use ordin_types::{CodedError as _, OrganisationProfileId, RemoteAgentId};
use sha2::{Digest as _, Sha256};
use sqlx::SqlitePool;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::document::{OrganisationProfileDocument, RemoteIdentity};
use crate::error::ProfileError;
use crate::fetch::{get_capped, Fetched, DOCUMENT_CAP, SIGNATURE_CAP};
use crate::installer::{
    check_address, read_installer, write_atomic, write_installer, InstallerRead,
};
use crate::jws::{decode_key, verify, InstallerKey};
use crate::snapshot::{
    ProfileFailure, ProfileSnapshot, ProfileState, ProfileStatus, RefreshOutcome, SnapshotSource,
};
use crate::{store, InstallerSettings, COMPONENT};

/// Retry while no profile is active: 5 s, doubling, at most 5 minutes, ±20 % jitter (Internal Logic 6.1).
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MAX: Duration = Duration::from_secs(300);
const RETRY_JITTER: f64 = 0.2;
/// An active profile with no successful check for this long is `stale` (Internal Logic 6.3).
const STALE_AFTER_MS: i64 = 3 * 60 * 60 * 1000;
/// The cache folder and its two fixed file names (*Configuration*).
const CACHE_FOLDER: &str = "profile";
const CACHE_DOCUMENT: &str = "active.json";
const CACHE_SIGNATURE: &str = "active.jws";

/// The time source, so that tests control `now`.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// How the program runs the component.
pub enum ProfileMode {
    /// Desktop and personal server.
    Read {
        /// The SQLite pool of the Journal's `Datastore`, handed over by the program (D4, R20).
        pool: sqlx::SqlitePool,
        data_root: std::path::PathBuf,
        http: reqwest::Client,
        /// Values a branded build carries (ADR-041); used when installer.json is absent.
        build_defaults: InstallerSettings,
    },
    /// Agent profile: no fetch, no tables, defaults only.
    NotUsed,
}

pub struct ProfileOptions {
    /// 60 minutes in the programs (2D-4).
    pub refresh_interval: std::time::Duration,
    pub clock: std::sync::Arc<dyn Clock>,
}

/// The cached, verified organisation profile and the remote agents' status (02:707–708).
pub struct OrganisationProfileService {
    inner: Arc<Inner>,
}

struct Inner {
    clock: Arc<dyn Clock>,
    refresh_interval: Duration,
    snapshot: watch::Sender<Arc<ProfileSnapshot>>,
    status: Mutex<StatusCell>,
    /// `None` in the agent profile.
    read: Option<ReadState>,
}

struct StatusCell {
    /// `NotUsed`, `NoAddress`, `NeverFetched` or `Active`; `Stale` is derived when read.
    base: ProfileState,
    active_version: Option<String>,
    last_attempt_at: Option<Timestamp>,
    last_success_at: Option<Timestamp>,
    last_failure: Option<ProfileFailure>,
}

struct ReadState {
    pool: SqlitePool,
    data_root: PathBuf,
    http: reqwest::Client,
    installer: Mutex<InstallerSettings>,
    /// The `ETag` of the active profile's response, held in memory only.
    etag: Mutex<Option<String>>,
    /// Single flight: one fetch at a time (Internal Logic 2.1).
    flight: tokio::sync::Mutex<()>,
    /// The number of finished attempts and the last one's result, handed to callers that waited.
    completed: Mutex<(u64, Option<Result<RefreshOutcome, ProfileError>>)>,
    /// Remote agent id → `identity_provider_ref`, for the synchronous `remote_agent_identity`.
    agent_refs: Mutex<HashMap<RemoteAgentId, Option<String>>>,
    refresh_loop: Mutex<Option<JoinHandle<()>>>,
}

/// A poisoned lock still holds consistent data here (every critical section is a plain assignment).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl OrganisationProfileService {
    /// Loads the installer settings and the cached profile, then starts the refresh loop.
    /// Never fails for network reasons; fails only on an unusable datastore.
    pub async fn start(
        mode: ProfileMode,
        options: ProfileOptions,
    ) -> Result<OrganisationProfileService, ProfileError> {
        let (pool, data_root, http, build_defaults) = match mode {
            ProfileMode::NotUsed => {
                // Internal Logic 1.1: defaults, no task.
                let (snapshot, _) = watch::channel(Arc::new(ProfileSnapshot::defaults()));
                let inner = Inner {
                    clock: options.clock,
                    refresh_interval: options.refresh_interval,
                    snapshot,
                    status: Mutex::new(StatusCell::new(ProfileState::NotUsed)),
                    read: None,
                };
                return Ok(OrganisationProfileService {
                    inner: Arc::new(inner),
                });
            }
            ProfileMode::Read {
                pool,
                data_root,
                http,
                build_defaults,
            } => (pool, data_root, http, build_defaults),
        };

        // 1.2 Installer settings.
        let installer = match read_installer(&data_root) {
            InstallerRead::Absent => build_defaults,
            InstallerRead::Valid(settings) => settings,
            InstallerRead::Invalid => {
                tracing::warn!(
                    component = COMPONENT,
                    code = ProfileError::InstallerInvalid.code(),
                    "profile.installer.invalid"
                );
                build_defaults
            }
        };
        let keys = installer.keys().unwrap_or_default();
        let has_address = installer.profile_url.is_some();

        // 1.3 Cached profile; 1.6 a datastore error stops the start.
        let mut snapshot = ProfileSnapshot::defaults();
        let mut status = StatusCell::new(if has_address {
            ProfileState::NeverFetched
        } else {
            ProfileState::NoAddress
        });
        if let Some(row) = store::active_row(&pool).await? {
            match load_cache(&row, &keys) {
                Ok(document) => {
                    status.base = ProfileState::Active;
                    status.active_version = Some(row.profile_version.clone());
                    status.last_success_at = Some(row.activated_at);
                    snapshot = ProfileSnapshot {
                        document,
                        source: SnapshotSource::Verified {
                            profile_id: row.id,
                            profile_version: row.profile_version.clone(),
                            activated_at: row.activated_at,
                        },
                    };
                }
                Err(cause) => tracing::warn!(
                    component = COMPONENT,
                    code = ProfileError::CacheInvalid.code(),
                    cause = cause.code(),
                    "profile.cache.invalid"
                ),
            }
        }
        let agent_refs = refs_of(&store::list_agents(&pool).await?);

        // 1.4 Publish.
        let (sender, _) = watch::channel(Arc::new(snapshot));
        let inner = Arc::new(Inner {
            clock: options.clock,
            refresh_interval: options.refresh_interval,
            snapshot: sender,
            status: Mutex::new(status),
            read: Some(ReadState {
                pool,
                data_root,
                http,
                installer: Mutex::new(installer),
                etag: Mutex::new(None),
                flight: tokio::sync::Mutex::new(()),
                completed: Mutex::new((0, None)),
                agent_refs: Mutex::new(agent_refs),
                refresh_loop: Mutex::new(None),
            }),
        });
        // 1.5 The refresh loop, first fetch at once.
        if has_address {
            spawn_loop(&inner);
        }
        Ok(OrganisationProfileService { inner })
    }

    /// The settings in force: the active verified profile, or the defaults.
    pub fn current(&self) -> std::sync::Arc<ProfileSnapshot> {
        self.inner.snapshot.borrow().clone()
    }

    /// Notified each time a newer verified profile becomes active (and once on subscribe).
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>> {
        let mut receiver = self.inner.snapshot.subscribe();
        receiver.mark_changed();
        receiver
    }

    /// Fetch now (the person's "Try again", Setup Assistant repair). Concurrent calls share one fetch.
    pub async fn refresh(&self) -> Result<RefreshOutcome, ProfileError> {
        self.inner.refresh().await
    }

    pub fn status(&self) -> ProfileStatus {
        self.inner.status()
    }

    pub fn installer_settings(&self) -> InstallerSettings {
        match &self.inner.read {
            Some(read) => lock(&read.installer).clone(),
            None => InstallerSettings::default(),
        }
    }

    /// Personal server: the setup command's `profile` step (2D-3; SA-NAME-05). The command takes
    /// the address and one or two signing keys (`--signing-key KEY`, repeatable up to twice);
    /// an address with no key is refused with PROF_INSTALLER_INVALID.
    pub async fn set_profile_address(
        &self,
        url: &str,
        signing_keys: Vec<String>,
    ) -> Result<(), ProfileError> {
        let Some(read) = &self.inner.read else {
            return Err(ProfileError::NotUsed);
        };
        // 8.1
        let parsed = check_address(url)?;
        if signing_keys.is_empty()
            || signing_keys.len() > 2
            || signing_keys.iter().any(|k| decode_key(k).is_err())
        {
            return Err(ProfileError::InstallerInvalid);
        }
        // 8.2 keeps an existing update_feed_url.
        let mut settings = lock(&read.installer).clone();
        let changed = settings.profile_url.as_deref() != Some(url);
        settings.profile_url = Some(url.to_owned());
        settings.profile_signing_keys = signing_keys;
        write_installer(&read.data_root, &settings)?;
        // 8.3
        *lock(&read.installer) = settings;
        if changed {
            *lock(&read.etag) = None;
            let mut status = lock(&self.inner.status);
            if status.base != ProfileState::Active {
                status.base = ProfileState::NeverFetched;
            }
        }
        tracing::info!(
            component = COMPONENT,
            host = parsed.host_str().unwrap_or_default(),
            "profile.address.set"
        );
        spawn_loop(&self.inner);
        // 8.4 A refresh failure does not fail this call.
        let _ = self.inner.refresh().await;
        Ok(())
    }

    pub async fn remote_agents(&self) -> Result<Vec<RemoteAgent>, ProfileError> {
        match &self.inner.read {
            Some(read) => store::list_agents(&read.pool).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn remote_agent(&self, id: RemoteAgentId) -> Result<RemoteAgent, ProfileError> {
        match &self.inner.read {
            Some(read) => store::get_agent(&read.pool, id).await,
            None => Err(ProfileError::RemoteAgentNotFound),
        }
    }

    /// The identity settings of a hosted agent's entry, from the active profile.
    pub fn remote_agent_identity(&self, id: RemoteAgentId) -> Option<RemoteIdentity> {
        let read = self.inner.read.as_ref()?;
        let name = lock(&read.agent_refs).get(&id).cloned().flatten()?;
        self.current()
            .document
            .remote_agents
            .iter()
            .find(|entry| entry.name == name)
            .and_then(|entry| entry.identity.clone())
    }

    /// `listed → enabled`; `enabled` and `verified` are left unchanged (Internal Logic 7.1).
    pub async fn mark_remote_agent_enabled(
        &self,
        id: RemoteAgentId,
    ) -> Result<RemoteAgent, ProfileError> {
        self.move_agent(id, RemoteAgentStatus::Enabled).await
    }

    /// `enabled → verified`; `listed` is refused; `verified` is left unchanged (Internal Logic 7.2).
    pub async fn mark_remote_agent_verified(
        &self,
        id: RemoteAgentId,
    ) -> Result<RemoteAgent, ProfileError> {
        self.move_agent(id, RemoteAgentStatus::Verified).await
    }

    /// Stops the refresh loop; an in-flight fetch is abandoned.
    pub async fn shutdown(&self) {
        let Some(read) = &self.inner.read else {
            return;
        };
        let handle = lock(&read.refresh_loop).take();
        if let Some(handle) = handle {
            handle.abort();
            // The task ends cancelled; its result carries nothing to report.
            let _ = handle.await;
        }
    }

    async fn move_agent(
        &self,
        id: RemoteAgentId,
        to: RemoteAgentStatus,
    ) -> Result<RemoteAgent, ProfileError> {
        let Some(read) = &self.inner.read else {
            return Err(ProfileError::RemoteAgentNotFound);
        };
        let mut agent = store::get_agent(&read.pool, id).await?;
        let next = match (agent.status, to) {
            (RemoteAgentStatus::Withdrawn, _) => return Err(ProfileError::RemoteAgentWithdrawn),
            (RemoteAgentStatus::Listed, RemoteAgentStatus::Verified) => {
                return Err(ProfileError::RemoteAgentNotEnabled)
            }
            (RemoteAgentStatus::Listed, _) => to,
            (RemoteAgentStatus::Enabled, RemoteAgentStatus::Verified) => to,
            (current, _) => current,
        };
        if next != agent.status {
            store::set_agent_status(&read.pool, id, next).await?;
            agent.status = next;
            tracing::info!(
                component = COMPONENT,
                remote_agent_id = id.to_string(),
                status = status_word(next),
                "profile.remote_agent.status"
            );
        }
        Ok(agent)
    }
}

fn status_word(status: RemoteAgentStatus) -> &'static str {
    match status {
        RemoteAgentStatus::Listed => "listed",
        RemoteAgentStatus::Enabled => "enabled",
        RemoteAgentStatus::Verified => "verified",
        RemoteAgentStatus::Withdrawn => "withdrawn",
    }
}

fn refs_of(agents: &[RemoteAgent]) -> HashMap<RemoteAgentId, Option<String>> {
    agents
        .iter()
        .map(|a| (a.id, a.identity_provider_ref.clone()))
        .collect()
}

impl StatusCell {
    fn new(base: ProfileState) -> Self {
        StatusCell {
            base,
            active_version: None,
            last_attempt_at: None,
            last_success_at: None,
            last_failure: None,
        }
    }
}

/// Reads the cached files of the active row and verifies them against the current keys, skipping the
/// version order (Internal Logic 1.3).
fn load_cache(
    row: &store::ActiveRow,
    keys: &[InstallerKey],
) -> Result<OrganisationProfileDocument, ProfileError> {
    let folder = Path::new(&row.cached_path);
    let document =
        std::fs::read(folder.join(CACHE_DOCUMENT)).map_err(|_| ProfileError::CacheInvalid)?;
    let jws = std::fs::read_to_string(folder.join(CACHE_SIGNATURE))
        .map_err(|_| ProfileError::CacheInvalid)?;
    // The files must be the active row's document, not a later write the row never recorded.
    if digest_of(&document) != row.document_digest {
        return Err(ProfileError::CacheInvalid);
    }
    verify(&document, &jws, keys, None)
        .map(|v| v.document)
        .map_err(|(e, _)| e)
}

/// SHA-256 of the document bytes, 64 lowercase hex characters.
fn digest_of(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    ordin_types::num::Sha256Hex::from_digest(&digest).0
}

/// A value in [-RETRY_JITTER, RETRY_JITTER], from the process's random hasher keys.
fn jitter() -> f64 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(0);
    let unit = (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64; // [0, 1)
    (unit * 2.0 - 1.0) * RETRY_JITTER
}

/// The wait after the `failures`-th failed attempt while nothing is active: 5 s doubling up to
/// 5 minutes, scaled by `1 + jitter` (Internal Logic 6.1).
pub(crate) fn retry_delay(failures: u32, jitter: f64) -> Duration {
    let base = RETRY_FIRST
        .checked_mul(2u32.saturating_pow(failures))
        .map_or(RETRY_MAX, |d| d.min(RETRY_MAX));
    base.mul_f64(1.0 + jitter.clamp(-RETRY_JITTER, RETRY_JITTER))
}

/// Starts the refresh loop unless it runs (Internal Logic 6). The loop holds only a weak reference
/// between attempts, so a dropped service ends it too.
fn spawn_loop(inner: &Arc<Inner>) {
    let Some(read) = &inner.read else {
        return;
    };
    let mut slot = lock(&read.refresh_loop);
    if slot.as_ref().is_some_and(|h| !h.is_finished()) {
        return;
    }
    let weak = Arc::downgrade(inner);
    *slot = Some(tokio::spawn(run_loop(weak)));
}

async fn run_loop(weak: Weak<Inner>) {
    let mut failures: u32 = 0;
    loop {
        let delay = {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // The outcome is recorded in the status; the loop only chooses the next wait.
            let _ = inner.refresh().await;
            if inner.has_active() {
                failures = 0;
                inner.refresh_interval
            } else {
                let delay = retry_delay(failures, jitter());
                failures = failures.saturating_add(1);
                delay
            }
        };
        tokio::time::sleep(delay).await;
    }
}

impl Inner {
    fn has_active(&self) -> bool {
        lock(&self.status).base == ProfileState::Active
    }

    fn status(&self) -> ProfileStatus {
        let now = self.clock.now();
        let cell = lock(&self.status);
        let stale = cell.base == ProfileState::Active
            && cell
                .last_success_at
                .is_none_or(|t| now.unix_millis() - t.unix_millis() > STALE_AFTER_MS);
        ProfileStatus {
            state: if stale {
                ProfileState::Stale
            } else {
                cell.base
            },
            active_version: cell.active_version.clone(),
            last_attempt_at: cell.last_attempt_at,
            last_success_at: cell.last_success_at,
            last_failure: cell.last_failure.clone(),
        }
    }

    /// Single flight (2.1) around one attempt, then the status of step 2.10.
    async fn refresh(&self) -> Result<RefreshOutcome, ProfileError> {
        let Some(read) = &self.read else {
            return Err(ProfileError::NotUsed);
        };
        let seen = lock(&read.completed).0;
        let _flight = read.flight.lock().await;
        {
            let completed = lock(&read.completed);
            if completed.0 != seen {
                if let Some(result) = &completed.1 {
                    return result.clone();
                }
            }
        }
        let result = self.attempt(read).await;
        let now = self.clock.now();
        {
            let mut status = lock(&self.status);
            status.last_attempt_at = Some(now);
            match &result {
                Ok(RefreshOutcome::Activated { .. } | RefreshOutcome::Unchanged) => {
                    status.last_success_at = Some(now);
                    status.last_failure = None;
                }
                Ok(RefreshOutcome::Refused { code }) => {
                    status.last_failure = Some(ProfileFailure {
                        code: code.clone(),
                        at: now,
                    });
                }
                Err(e) => {
                    status.last_failure = Some(ProfileFailure {
                        code: e.code().to_owned(),
                        at: now,
                    });
                }
            }
        }
        let mut completed = lock(&read.completed);
        completed.0 += 1;
        completed.1 = Some(result.clone());
        result
    }

    /// Internal Logic 2.2–2.9.
    async fn attempt(&self, read: &ReadState) -> Result<RefreshOutcome, ProfileError> {
        let installer = lock(&read.installer).clone();
        let Some(url) = installer.profile_url.clone() else {
            return Err(ProfileError::NoAddress);
        };
        check_address(&url)?;
        let keys = installer.keys().unwrap_or_default();

        // 2.3 the document.
        let etag = lock(&read.etag).clone();
        let (document, new_etag) =
            match get_capped(&read.http, &url, true, etag.as_deref(), DOCUMENT_CAP).await {
                Ok(Fetched::NotModified) => return Ok(RefreshOutcome::Unchanged),
                Ok(Fetched::Body { bytes, etag }) => (bytes, etag),
                Err(e) if e.is_refusal() => return Ok(refused(&e, None)),
                Err(e) => return Err(e),
            };
        // 2.4 the signature.
        let jws = match get_capped(
            &read.http,
            &format!("{url}.jws"),
            false,
            None,
            SIGNATURE_CAP,
        )
        .await
        {
            Ok(Fetched::Body { bytes, .. }) => match String::from_utf8(bytes) {
                Ok(text) if !text.trim().is_empty() => text,
                _ => return Ok(refused(&ProfileError::SignatureInvalid, None)),
            },
            // A 304 to a request without `If-None-Match` is a status the rule does not accept.
            Ok(Fetched::NotModified) => return Err(ProfileError::HttpStatus { status: 304 }),
            Err(ProfileError::HttpStatus { status: 404 }) => {
                return Ok(refused(&ProfileError::SignatureMissing, None))
            }
            Err(e) if e.is_refusal() => return Ok(refused(&e, None)),
            Err(e) => return Err(e),
        };

        // 2.5 the digest.
        let digest = digest_of(&document);
        let active = store::active_row(&read.pool).await?;
        if let Some(row) = active.as_ref().filter(|r| r.document_digest == digest) {
            if matches!(
                self.snapshot.borrow().source,
                SnapshotSource::Verified { .. }
            ) {
                return Ok(RefreshOutcome::Unchanged);
            }
            return self.reactivate(read, row, &document, &jws, &keys, new_etag);
        }
        // 2.6 a digest already recorded.
        if let Some(existing) = store::row_by_digest(&read.pool, &digest).await? {
            match existing.state.as_str() {
                "failed_verification" => {
                    let reason = existing.refusal_reason.unwrap_or_default();
                    let code = reason.split(' ').next().unwrap_or_default().to_owned();
                    tracing::warn!(
                        component = COMPONENT,
                        code = code.as_str(),
                        "profile.refused"
                    );
                    return Ok(RefreshOutcome::Refused { code });
                }
                "replaced" => {
                    // SPEC-GAP-6: the spec does not say what happens when the address serves a
                    // profile that was active once and has been replaced. `document_digest` is
                    // unique, so no new row can record it; it is checked without a row and, being
                    // older than the active profile, is refused (PROF_VERSION_NOT_NEWER) — a
                    // rollback is never served.
                    let active_version = active.as_ref().map(|r| r.profile_version.as_str());
                    let error = match verify(&document, &jws, &keys, active_version) {
                        Err((e, _)) => e,
                        Ok(_) => ProfileError::VersionNotNewer,
                    };
                    return Ok(refused(&error, None));
                }
                // `fetched` or `verified`: an attempt that never finished; it is recorded afresh.
                _ => store::delete_unfinished(&read.pool, &existing.id).await?,
            }
        }

        // 2.7 a new row.
        let id = OrganisationProfileId::new();
        let fetched_at = self.clock.now();
        let cached_path = read.data_root.join(CACHE_FOLDER);
        store::insert_fetched(
            &read.pool,
            id,
            &jws,
            &digest,
            &cached_path.to_string_lossy(),
            fetched_at,
        )
        .await?;

        // 2.8, 3 verify.
        let active_version = active.as_ref().map(|r| r.profile_version.clone());
        match verify(&document, &jws, &keys, active_version.as_deref()) {
            Err((error, version)) => {
                store::mark_failed(&read.pool, id, version.as_deref(), &error.refusal_reason())
                    .await?;
                Ok(refused(&error, version.as_deref()))
            }
            Ok(verified) => {
                let version = verified.document.profile_version.clone();
                store::mark_verified(&read.pool, id, &version, self.clock.now()).await?;
                // 2.9 activate.
                self.activate(
                    read,
                    id,
                    verified.document,
                    &document,
                    &jws,
                    new_etag,
                    active_version,
                )
                .await
            }
        }
    }

    /// Internal Logic 4.
    #[allow(clippy::too_many_arguments)]
    async fn activate(
        &self,
        read: &ReadState,
        id: OrganisationProfileId,
        parsed: OrganisationProfileDocument,
        document: &[u8],
        jws: &str,
        etag: Option<String>,
        previous_version: Option<String>,
    ) -> Result<RefreshOutcome, ProfileError> {
        // 4.1 the cache files.
        write_cache(&read.data_root, document, jws);
        // 4.2, 4.3 one transaction.
        let now = self.clock.now();
        let counts = match store::activate(&read.pool, id, &parsed, now).await {
            Ok(counts) => counts,
            Err(e) => {
                if matches!(
                    e,
                    ProfileError::UnverifiedActivation | ProfileError::MultipleActive
                ) {
                    // SPEC-GAP-8: step 4.2.2 says the defect is "logged at error" but the
                    // Observability list names no event for it; `profile.activation.failed` is used.
                    tracing::error!(
                        component = COMPONENT,
                        code = e.code(),
                        "profile.activation.failed"
                    );
                }
                return Err(e);
            }
        };
        tracing::info!(
            component = COMPONENT,
            listed_new = counts.listed_new,
            changed = counts.changed,
            withdrawn = counts.withdrawn,
            "profile.remote_agents.synced"
        );
        if let Ok(agents) = store::list_agents(&read.pool).await {
            *lock(&read.agent_refs) = refs_of(&agents);
        }
        // 4.4 the snapshot.
        let version = parsed.profile_version.clone();
        self.publish(id, parsed, now, read, etag);
        // 4.5
        match previous_version.as_deref() {
            Some(previous) => tracing::info!(
                component = COMPONENT,
                profile_version = version.as_str(),
                previous_version = previous,
                "profile.activated"
            ),
            None => tracing::info!(
                component = COMPONENT,
                profile_version = version.as_str(),
                "profile.activated"
            ),
        }
        Ok(RefreshOutcome::Activated {
            profile_version: version,
        })
    }

    /// SPEC-GAP-7: the active row's own document is served again while the snapshot holds the defaults
    /// (its cache failed at start — files missing, or the installer keys rotated). The spec's step 2.5
    /// would answer `Unchanged` and keep the defaults until a newer version is published; instead the
    /// document is verified again (without the version order, as at start) and, on success, served
    /// from the existing active row. No row is added and no state changes.
    fn reactivate(
        &self,
        read: &ReadState,
        row: &store::ActiveRow,
        document: &[u8],
        jws: &str,
        keys: &[InstallerKey],
        etag: Option<String>,
    ) -> Result<RefreshOutcome, ProfileError> {
        match verify(document, jws, keys, None) {
            Err((error, version)) => Ok(refused(&error, version.as_deref())),
            Ok(verified) => {
                write_cache(&read.data_root, document, jws);
                let version = verified.document.profile_version.clone();
                self.publish(row.id, verified.document, row.activated_at, read, etag);
                tracing::info!(
                    component = COMPONENT,
                    profile_version = version.as_str(),
                    "profile.activated"
                );
                Ok(RefreshOutcome::Activated {
                    profile_version: version,
                })
            }
        }
    }

    /// Replaces the snapshot, tells subscribers and records the active version and `ETag`.
    fn publish(
        &self,
        id: OrganisationProfileId,
        document: OrganisationProfileDocument,
        activated_at: Timestamp,
        read: &ReadState,
        etag: Option<String>,
    ) {
        let version = document.profile_version.clone();
        self.snapshot.send_replace(Arc::new(ProfileSnapshot {
            document,
            source: SnapshotSource::Verified {
                profile_id: id,
                profile_version: version.clone(),
                activated_at,
            },
        }));
        *lock(&read.etag) = etag;
        let mut status = lock(&self.status);
        status.base = ProfileState::Active;
        status.active_version = Some(version);
    }
}

/// Logs `profile.refused` and builds the outcome.
fn refused(error: &ProfileError, version: Option<&str>) -> RefreshOutcome {
    let code = error.code();
    let key_path = match error {
        ProfileError::ContentInvalid { key_path } => Some(key_path.as_str()),
        _ => None,
    };
    match (key_path, version) {
        (Some(path), Some(v)) => tracing::warn!(
            component = COMPONENT,
            code,
            key_path = path,
            profile_version = v,
            "profile.refused"
        ),
        (Some(path), None) => {
            tracing::warn!(
                component = COMPONENT,
                code,
                key_path = path,
                "profile.refused"
            )
        }
        (None, Some(v)) => {
            tracing::warn!(
                component = COMPONENT,
                code,
                profile_version = v,
                "profile.refused"
            )
        }
        (None, None) => tracing::warn!(component = COMPONENT, code, "profile.refused"),
    }
    RefreshOutcome::Refused {
        code: code.to_owned(),
    }
}

/// Internal Logic 4.1: `active.json` and `active.jws` in `<data_root>/profile/` (folder `0700`, files
/// `0600`), each through a `.tmp`, `fsync` and rename. A failure is logged and activation continues.
fn write_cache(data_root: &Path, document: &[u8], jws: &str) {
    let folder = data_root.join(CACHE_FOLDER);
    let written = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&folder)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700))?;
        }
        write_atomic(&folder.join(CACHE_DOCUMENT), document)?;
        write_atomic(&folder.join(CACHE_SIGNATURE), jws.as_bytes())
    })();
    if written.is_err() {
        tracing::warn!(component = COMPONENT, "profile.cache.write_failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_doubles_to_five_minutes_with_bounded_jitter() {
        assert_eq!(retry_delay(0, 0.0), Duration::from_secs(5));
        assert_eq!(retry_delay(3, 0.0), Duration::from_secs(40));
        assert_eq!(retry_delay(10, 0.0), Duration::from_secs(300));
        assert_eq!(retry_delay(u32::MAX, 0.0), Duration::from_secs(300));
        assert_eq!(retry_delay(0, 0.2), Duration::from_secs(6));
        assert_eq!(retry_delay(0, 9.0), Duration::from_secs(6));
        for _ in 0..100 {
            let j = jitter();
            assert!((-RETRY_JITTER..=RETRY_JITTER).contains(&j));
        }
    }
}
