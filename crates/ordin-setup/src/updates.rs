//! Releases and updates (SetupAssistant.md F.3, F.4 and the ADR-114 amendment SET-UP-1 to
//! SET-UP-26): the desktop's check, background download, install and **What's new**; the personal
//! server's feed read and its own update.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use ordin_types::time::Timestamp;
use ordin_types::CodedError;

use crate::assistant::{lock, read, single, Inner, SetupAssistant};
use crate::error::SetupError;
use crate::process::{self, Ran, COMMAND_TIMEOUT};
use crate::store;
use crate::types::{
    InstallKind, ServerRunMode, ServerUpdate, ServerUpdateOutcome, SetupProgram, UpdateFinish,
    UpdateOffer, UpdateState, UpdaterError, WhatsNew,
};
use crate::TARGET;

/// Release notes are cut beyond 4,000 characters.
const NOTES_MAX: usize = 4000;
/// A version is 1–64 characters.
const VERSION_MAX: usize = 64;
/// The server's feed read: 15 seconds (F.4).
const FEED_TIMEOUT: Duration = Duration::from_secs(15);
/// The server archive: at most 512 MiB and 10 minutes in all (SET-UP-17).
const ARCHIVE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The unpack limits of H.3.3, used again by SET-UP-19.
pub(crate) const UNPACK_MAX_ENTRIES: usize = 20_000;
pub(crate) const UNPACK_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// `setup.server_update.restart_failed`'s `reason`: at most 200 characters.
const REASON_MAX: usize = 200;
/// The two programs a server archive replaces, in replacement order (SET-UP-20).
const SERVER_PROGRAMS: [&str; 2] = ["ordin-hook", "ordin-server"];

/// The outcome of one shared update check, before the caller's trigger decides what is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckFailure {
    Unreachable,
    SignatureInvalid,
}

/// The feed in Tauri's updater format (C-99).
#[derive(Debug, Clone)]
pub(crate) struct Feed {
    pub version: semver::Version,
    pub notes: String,
    pub platforms: BTreeMap<String, FeedEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct FeedEntry {
    pub url: String,
    pub signature: String,
}

/// Unit tests reach the `wiremock` stand-in over plain HTTP on loopback; every build that ships
/// accepts `https` addresses only.
const ALLOW_LOOPBACK_HTTP: bool = cfg!(any(test, feature = "test-support"));

pub(crate) fn https_or_loopback(address: &str) -> Option<url::Url> {
    let parsed = url::Url::parse(address).ok()?;
    match parsed.scheme() {
        "https" => Some(parsed),
        "http"
            if ALLOW_LOOPBACK_HTTP
                && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")) =>
        {
            Some(parsed)
        }
        _ => None,
    }
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn newer_than(version: &str, current: &str) -> bool {
    match (
        semver::Version::parse(version),
        semver::Version::parse(current),
    ) {
        (Ok(v), Ok(c)) => v > c,
        _ => false,
    }
}

impl Inner {
    fn updater(&self) -> Option<Arc<dyn crate::types::AppUpdater>> {
        read(&self.updater).clone()
    }

    /// SET-UP-1 to SET-UP-3 (and SET-UP-25): one read of the feed, shared by every caller.
    async fn check_once(self: Arc<Self>) -> Result<Option<UpdateOffer>, CheckFailure> {
        let Some(updater) = self.updater() else {
            return Ok(None);
        };
        let Some(feed) = self
            .effective_update_feed()
            .as_deref()
            .and_then(https_or_loopback)
        else {
            return Ok(None);
        };
        match updater.check(&feed).await {
            Ok(Some(found))
                if !found.version.is_empty()
                    && found.version.chars().count() <= VERSION_MAX
                    && newer_than(&found.version, &self.app_version) =>
            {
                let notes = cut(&found.notes, NOTES_MAX);
                // SET-UP-25: a whole-computer install keeps the notes and downloads nothing.
                if updater.install_kind() == InstallKind::WholeComputer {
                    let offer = UpdateOffer {
                        version: found.version.clone(),
                        notes: notes.clone(),
                        state: UpdateState::AdministratorNeeded,
                    };
                    let same = lock(&self.offer).as_ref() == Some(&offer);
                    if !same {
                        if let Err(e) = store::keep_whats_new(
                            &self.pool,
                            &offer.version,
                            &notes,
                            Timestamp::now(),
                        )
                        .await
                        {
                            tracing::warn!(target: TARGET, code = ordin_types::CodedError::code(&e), "setup.update_store_failed");
                        }
                        *lock(&self.offer) = Some(offer.clone());
                        tracing::info!(target: TARGET, version = offer.version.as_str(), "setup.update_offered");
                    }
                    return Ok(Some(offer));
                }
                // SET-UP-3: the same version kept is kept unchanged (no second download).
                if let Some(kept) = lock(&self.offer).as_ref() {
                    if kept.version == found.version {
                        return Ok(Some(kept.clone()));
                    }
                }
                let offer = UpdateOffer {
                    version: found.version,
                    notes,
                    state: UpdateState::Downloading,
                };
                *lock(&self.offer) = Some(offer.clone());
                tracing::info!(target: TARGET, version = offer.version.as_str(), "setup.update_offered");
                let inner = self.clone();
                let download = offer.clone();
                let task = tokio::spawn(async move { inner.download(download).await });
                *lock(&self.download_task) = Some(task);
                Ok(Some(offer))
            }
            Ok(_) => {
                let mut kept = lock(&self.offer);
                if kept
                    .as_ref()
                    .is_some_and(|o| !newer_than(&o.version, &self.app_version))
                {
                    *kept = None;
                }
                Ok(None)
            }
            Err(UpdaterError::SignatureInvalid) => {
                *lock(&self.offer) = None;
                tracing::warn!(
                    target: TARGET,
                    code = "SETUP_UPDATE_SIGNATURE_INVALID",
                    "setup.update_signature_invalid"
                );
                Err(CheckFailure::SignatureInvalid)
            }
            Err(_) => {
                tracing::info!(
                    target: TARGET,
                    code = "SETUP_UPDATE_UNREACHABLE",
                    "setup.update_unreachable"
                );
                Err(CheckFailure::Unreachable)
            }
        }
    }

    async fn shared_check(self: Arc<Self>) -> Result<Option<UpdateOffer>, CheckFailure> {
        let inner = self.clone();
        single(&self.check_flight, move || inner.check_once()).await
    }

    /// F.3: the timer's check; failures stay in the log.
    pub(crate) async fn timer_update_check(self: Arc<Self>) {
        let _ = self.shared_check().await;
    }

    /// SET-UP-4.
    async fn download(self: Arc<Self>, offer: UpdateOffer) {
        let _one = self.download_lock.lock().await;
        let Some(updater) = self.updater() else {
            return;
        };
        let still = |inner: &Inner| {
            lock(&inner.offer)
                .as_ref()
                .is_some_and(|o| o.version == offer.version)
        };
        match updater.download(&offer).await {
            Ok(()) => {
                match store::keep_whats_new(
                    &self.pool,
                    &offer.version,
                    &offer.notes,
                    Timestamp::now(),
                )
                .await
                {
                    Ok(()) => {
                        let mut kept = lock(&self.offer);
                        if let Some(o) = kept.as_mut().filter(|o| o.version == offer.version) {
                            o.state = UpdateState::Ready;
                            tracing::info!(target: TARGET, version = offer.version.as_str(), "setup.update_ready");
                        }
                    }
                    // SET-UP-9: the offer stays downloading and is retried at the next check.
                    Err(_) => tracing::warn!(
                        target: TARGET,
                        code = "SETUP_STORE_FAILED",
                        "setup.update_store_failed"
                    ),
                }
            }
            Err(error) => {
                if still(&self) {
                    *lock(&self.offer) = None;
                }
                let code = match error {
                    UpdaterError::SignatureInvalid => "SETUP_UPDATE_SIGNATURE_INVALID",
                    UpdaterError::Unreachable => "SETUP_UPDATE_UNREACHABLE",
                    UpdaterError::InstallFailed | UpdaterError::NothingDownloaded => {
                        "SETUP_UPDATE_UNREACHABLE"
                    }
                };
                tracing::warn!(target: TARGET, code, "setup.update_download_failed");
            }
        }
    }

    // ---------------------------------------------------------------- personal server

    /// F.4 and SET-UP-11: the effective feed read once; `None` when there is no feed.
    pub(crate) async fn read_feed(&self) -> Result<Option<Feed>, SetupError> {
        let Some(address) = self.effective_update_feed() else {
            return Ok(None);
        };
        let invalid = || SetupError::permanent("SETUP_UPDATE_FEED_INVALID");
        let feed_url = https_or_loopback(&address).ok_or_else(invalid)?;
        let unreachable = || SetupError::of("SETUP_UPDATE_UNREACHABLE");
        let reply = self
            .http
            .get(feed_url)
            .timeout(FEED_TIMEOUT)
            .send()
            .await
            .map_err(|_| unreachable())?;
        if !reply.status().is_success() {
            return Err(unreachable());
        }
        let body: serde_json::Value = reply.json().await.map_err(|_| invalid())?;
        let version = body
            .get("version")
            .and_then(serde_json::Value::as_str)
            .filter(|v| !v.is_empty() && v.chars().count() <= VERSION_MAX)
            .and_then(|v| semver::Version::parse(v).ok())
            .ok_or_else(invalid)?;
        let notes = cut(
            body.get("notes")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
            NOTES_MAX,
        );
        let mut platforms = BTreeMap::new();
        if let Some(entries) = body.get("platforms").and_then(serde_json::Value::as_object) {
            for (key, entry) in entries {
                let url = entry
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(invalid)?;
                https_or_loopback(url).ok_or_else(invalid)?;
                let signature = entry
                    .get("signature")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(invalid)?;
                platforms.insert(
                    key.clone(),
                    FeedEntry {
                        url: url.to_owned(),
                        signature: signature.to_owned(),
                    },
                );
            }
        }
        Ok(Some(Feed {
            version,
            notes,
            platforms,
        }))
    }

    fn newer_feed(&self, feed: Option<Feed>) -> Option<Feed> {
        let feed = feed?;
        let current = semver::Version::parse(&self.app_version).ok()?;
        (feed.version > current).then_some(feed)
    }

    fn server_only(&self) -> Result<(), SetupError> {
        if self.program == SetupProgram::Desktop {
            return Err(SetupError::permanent("SETUP_SELF_UPDATE_UNAVAILABLE"));
        }
        Ok(())
    }

    /// SET-UP-14.
    fn may_replace_itself(&self) -> Result<Arc<dyn crate::types::ServerPlatform>, SetupError> {
        let platform = read(&self.platform).clone();
        match platform {
            Some(p) if p.run_mode() == ServerRunMode::UserUnit => Ok(p),
            _ => Err(SetupError::permanent("SETUP_SELF_UPDATE_UNAVAILABLE")),
        }
    }

    /// SET-UP-23: after the restart, in `run_checks(Start)` on the personal server.
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-23
    pub(crate) async fn after_server_restart(&self) -> Result<(), SetupError> {
        let Some((version, state, _)) = store::server_update_row(&self.pool).await? else {
            return Ok(());
        };
        if state != "restarting" {
            return Ok(());
        }
        if version == self.app_version {
            store::set_server_update(&self.pool, "done", None, Timestamp::now()).await?;
            tracing::info!(target: TARGET, version = version.as_str(), "setup.server_update.done");
        } else {
            store::set_server_update(
                &self.pool,
                "failed",
                Some("SETUP_UPDATE_REPLACE_FAILED"),
                Timestamp::now(),
            )
            .await?;
            tracing::warn!(
                target: TARGET,
                version = version.as_str(),
                code = "SETUP_UPDATE_REPLACE_FAILED",
                "setup.server_update.failed"
            );
        }
        Ok(())
    }

    /// SET-UP-16 to SET-UP-22 after the claim (SET-UP-15).
    async fn replace_programs(
        &self,
        feed: &Feed,
        platform: &Arc<dyn crate::types::ServerPlatform>,
    ) -> Result<ServerUpdateOutcome, SetupError> {
        let version = feed.version.to_string();
        let outcome = self.replace_inner(feed, platform, &version).await;
        if let Err(error) = &outcome {
            let code = error.code();
            // SET-UP-21 records its own failure; every earlier one is recorded here (SET-UP-22).
            if code != "SETUP_UPDATE_REPLACE_FAILED"
                || store::server_update_row(&self.pool)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|(_, s, _)| s == "running")
            {
                let _ =
                    store::set_server_update(&self.pool, "failed", Some(code), Timestamp::now())
                        .await;
            }
        }
        outcome
    }

    async fn replace_inner(
        &self,
        feed: &Feed,
        platform: &Arc<dyn crate::types::ServerPlatform>,
        version: &str,
    ) -> Result<ServerUpdateOutcome, SetupError> {
        // SET-UP-16.
        let entry = feed
            .platforms
            .get(platform.feed_platform())
            .ok_or_else(|| SetupError::permanent("SETUP_UPDATE_NO_ARCHIVE"))?;
        let program_dir = platform.program_dir();
        let folder = program_dir.join(format!(
            ".update-{version}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&folder)
            .map_err(|_| SetupError::of("SETUP_UPDATE_REPLACE_FAILED"))?;
        let result = self
            .download_verify_unpack(entry, &folder, version)
            .await
            .and_then(|()| replace_files(&program_dir, &folder));
        if let Err(error) = result {
            let _ = std::fs::remove_dir_all(&folder);
            if error.code() == "SETUP_UPDATE_SIGNATURE_INVALID" {
                tracing::warn!(target: TARGET, code = "SETUP_UPDATE_SIGNATURE_INVALID", "setup.server_update.signature_invalid");
            }
            return Err(error);
        }
        // SET-UP-21.
        store::set_server_update(&self.pool, "restarting", None, Timestamp::now()).await?;
        let _ = std::fs::remove_dir_all(&folder);
        tracing::info!(target: TARGET, version, "setup.server_update.installed");
        match platform.restart_unit().await {
            Ok(()) => Ok(ServerUpdateOutcome::Restarting {
                version: version.to_owned(),
            }),
            Err(reason) => {
                tracing::warn!(
                    target: TARGET,
                    reason = %cut(&reason, REASON_MAX),
                    "setup.server_update.restart_failed"
                );
                for name in SERVER_PROGRAMS.iter().rev() {
                    let previous = program_dir.join(format!("{name}.previous"));
                    if std::fs::rename(&previous, program_dir.join(name)).is_err() {
                        tracing::error!(
                            target: TARGET,
                            file = name,
                            code = "SETUP_UPDATE_REPLACE_FAILED",
                            "setup.server_update.restore_failed"
                        );
                    }
                }
                store::set_server_update(
                    &self.pool,
                    "failed",
                    Some("SETUP_UPDATE_REPLACE_FAILED"),
                    Timestamp::now(),
                )
                .await?;
                Err(SetupError::of("SETUP_UPDATE_REPLACE_FAILED"))
            }
        }
    }

    /// SET-UP-17 to SET-UP-19.
    async fn download_verify_unpack(
        &self,
        entry: &FeedEntry,
        folder: &Path,
        version: &str,
    ) -> Result<(), SetupError> {
        let invalid = || SetupError::permanent("SETUP_UPDATE_FEED_INVALID");
        let unreachable = || SetupError::of("SETUP_UPDATE_UNREACHABLE");
        let url = https_or_loopback(&entry.url).ok_or_else(invalid)?;
        let archive = folder.join("archive.tar.gz");
        let bytes = tokio::time::timeout(ARCHIVE_TIMEOUT, async {
            let mut reply = self.http.get(url).send().await.map_err(|_| unreachable())?;
            // A redirect to any other scheme is refused.
            if https_or_loopback(reply.url().as_str()).is_none() {
                return Err(invalid());
            }
            if !reply.status().is_success() {
                return Err(unreachable());
            }
            let mut bytes: Vec<u8> = Vec::new();
            while let Some(chunk) = reply.chunk().await.map_err(|_| unreachable())? {
                bytes.extend_from_slice(&chunk);
                if bytes.len() as u64 > ARCHIVE_MAX_BYTES {
                    return Err(unreachable());
                }
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| unreachable())??;
        std::fs::write(&archive, &bytes).map_err(|_| unreachable())?;
        // SET-UP-18.
        let key = read(&self.tuning).update_key.clone();
        if !verify_signature(key.as_deref(), &bytes, &entry.signature) {
            return Err(SetupError::permanent("SETUP_UPDATE_SIGNATURE_INVALID"));
        }
        // SET-UP-19.
        unpack_server_archive(&archive, folder)?;
        let _ = std::fs::remove_file(&archive);
        let server = folder.join("bin").join("ordin-server");
        let ran = process::run(&server, &["--version".to_owned()], None, COMMAND_TIMEOUT).await;
        match ran {
            Ran::Exited {
                code: Some(0),
                stdout,
                ..
            } if stdout.split_whitespace().next() == Some(version) => Ok(()),
            _ => Err(invalid()),
        }
    }
}

/// SET-UP-18: Tauri writes both the key and the signature as base64 of minisign's text.
pub(crate) fn verify_signature(key: Option<&str>, bytes: &[u8], signature: &str) -> bool {
    let Some(key) = key else {
        return false;
    };
    let decode = |text: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    };
    let (Some(key_text), Some(signature_text)) = (decode(key), decode(signature)) else {
        return false;
    };
    let (Ok(public), Ok(signature)) = (
        minisign_verify::PublicKey::decode(&key_text),
        minisign_verify::Signature::decode(&signature_text),
    ) else {
        return false;
    };
    // Legacy (non-prehashed) signatures are accepted as Tauri's updater accepts them.
    public.verify(bytes, &signature, true).is_ok()
}

/// A path inside an archive: relative, no `..`, no root (H.3.3).
pub(crate) fn safe_entry_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path.components().all(|c| {
            matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn normal(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// SET-UP-19: every entry checked before anything is written; only `bin/ordin-server` and
/// `bin/ordin-hook` extracted, mode 0755.
pub(crate) fn unpack_server_archive(archive: &Path, folder: &Path) -> Result<(), SetupError> {
    let invalid = || SetupError::permanent("SETUP_UPDATE_FEED_INVALID");
    let open = || -> Result<tar::Archive<flate2::read::GzDecoder<std::fs::File>>, SetupError> {
        let file = std::fs::File::open(archive).map_err(|_| invalid())?;
        Ok(tar::Archive::new(flate2::read::GzDecoder::new(file)))
    };
    // Pass 1: the headers.
    let mut count = 0usize;
    let mut total = 0u64;
    let mut reader = open()?;
    for entry in reader.entries().map_err(|_| invalid())? {
        let entry = entry.map_err(|_| invalid())?;
        count += 1;
        total = total.saturating_add(entry.header().size().map_err(|_| invalid())?);
        if count > UNPACK_MAX_ENTRIES || total > UNPACK_MAX_BYTES {
            return Err(invalid());
        }
        let path = entry.path().map_err(|_| invalid())?.into_owned();
        if !safe_entry_path(&path) {
            return Err(invalid());
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            if normal(&path) != "bin" {
                return Err(invalid());
            }
        } else if !kind.is_file() {
            return Err(invalid());
        }
    }
    // Pass 2: the two programs.
    let bin = folder.join("bin");
    std::fs::create_dir_all(&bin).map_err(|_| invalid())?;
    let mut written = 0;
    let mut reader = open()?;
    for entry in reader.entries().map_err(|_| invalid())? {
        let mut entry = entry.map_err(|_| invalid())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = normal(&entry.path().map_err(|_| invalid())?);
        let Some(program) = SERVER_PROGRAMS.iter().find(|p| name == format!("bin/{p}")) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|_| invalid())?;
        let target = bin.join(program);
        std::fs::write(&target, bytes).map_err(|_| invalid())?;
        set_executable(&target).map_err(|_| invalid())?;
        written += 1;
    }
    if written != SERVER_PROGRAMS.len() || !SERVER_PROGRAMS.iter().all(|p| bin.join(p).is_file()) {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn set_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// SET-UP-20: `ordin-hook`, then `ordin-server`; each kept as `<name>.previous`; any failure undoes
/// the renames already made, in reverse order.
fn replace_files(program_dir: &Path, folder: &Path) -> Result<(), SetupError> {
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    let undo = |done: &[(PathBuf, PathBuf)]| {
        for (from, to) in done.iter().rev() {
            let _ = std::fs::rename(to, from);
        }
    };
    for name in SERVER_PROGRAMS {
        let current = program_dir.join(name);
        let previous = program_dir.join(format!("{name}.previous"));
        let new = folder.join("bin").join(name);
        if current.exists() {
            if std::fs::rename(&current, &previous).is_err() {
                undo(&done);
                return Err(SetupError::of("SETUP_UPDATE_REPLACE_FAILED"));
            }
            done.push((current.clone(), previous));
        }
        if std::fs::rename(&new, &current).is_err() {
            undo(&done);
            return Err(SetupError::of("SETUP_UPDATE_REPLACE_FAILED"));
        }
        done.push((new, current));
    }
    Ok(())
}

impl SetupAssistant {
    /// The update the desktop found and has not installed; `None` when none.
    pub fn update_offer(&self) -> Option<UpdateOffer> {
        lock(&self.inner.offer).clone()
    }

    /// `setup.check_update`: an on-demand check; one at a time (SET-UP-2).
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-2
    pub async fn check_update(&self) -> Result<Option<UpdateOffer>, SetupError> {
        match self.inner.clone().shared_check().await {
            Ok(offer) => Ok(offer),
            Err(CheckFailure::Unreachable) => Err(SetupError::of("SETUP_UPDATE_UNREACHABLE")),
            Err(CheckFailure::SignatureInvalid) => {
                Err(SetupError::permanent("SETUP_UPDATE_SIGNATURE_INVALID"))
            }
        }
    }

    /// SET-UP-5, SET-UP-26: with an offer in state `Ready` returns `Ok(())`; the program then runs
    /// its quit flow and calls `finish_update(UpdateFinish::Restart)`.
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-5
    pub async fn install_update(&self) -> Result<(), SetupError> {
        let offer = lock(&self.inner.offer).clone();
        match offer {
            Some(o) if o.state == UpdateState::Ready => {
                tracing::info!(target: TARGET, version = o.version.as_str(), "setup.update_install_requested");
                Ok(())
            }
            _ => Err(SetupError::permanent("SETUP_UPDATE_NONE")),
        }
    }

    /// SET-UP-6: the last step of the program's quit flow. Reads and writes no store.
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-6
    pub async fn finish_update(&self, how: UpdateFinish) -> Result<bool, SetupError> {
        let ready = lock(&self.inner.offer)
            .clone()
            .filter(|o| o.state == UpdateState::Ready);
        let (Some(offer), Some(updater)) = (ready, self.inner.updater()) else {
            return Ok(false);
        };
        let result = match how {
            UpdateFinish::Restart => updater.install_and_restart().await,
            UpdateFinish::OnExit => updater.install_on_exit().await,
        };
        match result {
            Ok(()) => {
                tracing::info!(target: TARGET, version = offer.version.as_str(), how = match how {
                    UpdateFinish::Restart => "restart",
                    UpdateFinish::OnExit => "on_exit",
                }, "setup.update_installed");
                Ok(true)
            }
            Err(UpdaterError::NothingDownloaded) => {
                *lock(&self.inner.offer) = None;
                Ok(false)
            }
            Err(_) => {
                tracing::warn!(target: TARGET, code = "SETUP_UPDATE_INSTALL_FAILED", "setup.update_install_failed");
                Err(SetupError::of("SETUP_UPDATE_INSTALL_FAILED"))
            }
        }
    }

    /// `setup.whats_new`: the running version's kept notes (SET-UP-8).
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-8
    pub async fn whats_new(&self) -> Result<Option<WhatsNew>, SetupError> {
        // SET-REC-2.2: nothing is new for a version lower than the highest one started.
        if let Some(highest) = store::highest_started(&self.inner.pool).await? {
            if newer_than(&highest, &self.inner.app_version) {
                return Ok(None);
            }
        }
        let row = store::whats_new(&self.inner.pool, &self.inner.app_version).await?;
        Ok(row.map(|(notes, seen)| WhatsNew {
            version: self.inner.app_version.clone(),
            notes,
            seen,
        }))
    }

    /// Called by the desktop program once at start (OrdinDesktop.md DSK-REC-44): the highest version
    /// recorded as started before this call, `None` on the first start; the running version is
    /// recorded when it is higher (SET-REC-2.1).
    // spec: ordin-desktop/components/SetupAssistant#SET-REC-2
    pub async fn record_started(&self) -> Result<Option<String>, SetupError> {
        let pool = &self.inner.pool;
        let running = &self.inner.app_version;
        let stored = store::highest_started(pool)
            .await?
            .filter(|v| semver::Version::parse(v).is_ok());
        match &stored {
            Some(v) if !newer_than(running, v) => {}
            _ => store::record_started(pool, running, Timestamp::now()).await?,
        }
        Ok(stored)
    }

    /// `setup.whats_new_seen { version }`.
    pub async fn whats_new_seen(&self, version: &str) -> Result<(), SetupError> {
        store::mark_seen(&self.inner.pool, version, Timestamp::now()).await
    }

    /// Server: reads the effective feed once and returns the newer version it names (F.4).
    pub async fn check_update_feed(&self) -> Result<Option<String>, SetupError> {
        let feed = self.inner.read_feed().await?;
        let newer = self.inner.newer_feed(feed).map(|f| f.version.to_string());
        if let Some(v) = &newer {
            tracing::info!(target: TARGET, version = v.as_str(), "setup.update_announced");
        }
        Ok(newer)
    }

    /// Registers the server program's platform parts. Without one, self-update is unavailable.
    pub fn register_server_platform(&self, platform: Arc<dyn crate::types::ServerPlatform>) {
        *crate::assistant::write(&self.inner.platform) = Some(platform);
    }

    /// `setup.server_update` (SET-UP-12).
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-12
    pub async fn server_update(&self) -> Result<Option<ServerUpdate>, SetupError> {
        self.inner.server_only()?;
        let feed = self.inner.read_feed().await?;
        let Some(feed) = self.inner.newer_feed(feed) else {
            return Ok(None);
        };
        Ok(Some(ServerUpdate {
            version: feed.version.to_string(),
            notes: feed.notes,
            self_update: self.inner.may_replace_itself().is_ok(),
        }))
    }

    /// `setup.update_server { version }` (SET-UP-13): validates, then replaces on a task.
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-13
    pub async fn update_server(&self, version: &str) -> Result<(), SetupError> {
        self.inner.server_only()?;
        let platform = self.inner.may_replace_itself()?;
        store::claim_server_update(&self.inner.pool, version, Timestamp::now()).await?;
        let feed = match self.inner.read_feed().await {
            Ok(feed) => self.inner.newer_feed(feed),
            Err(e) => {
                let _ = store::release_server_update(&self.inner.pool).await;
                return Err(e);
            }
        };
        let Some(feed) = feed else {
            let _ = store::release_server_update(&self.inner.pool).await;
            return Err(SetupError::permanent("SETUP_UPDATE_NONE"));
        };
        if feed.version.to_string() != version {
            let _ = store::release_server_update(&self.inner.pool).await;
            return Err(SetupError::permanent("SETUP_UPDATE_VERSION_CHANGED"));
        }
        let inner = self.inner.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = inner.replace_programs(&feed, &platform).await {
                tracing::warn!(target: TARGET, code = ordin_types::CodedError::code(&e), "setup.server_update.failed");
            }
        });
        *lock(&self.inner.server_task) = Some(task);
        Ok(())
    }

    /// `ordin-server update`, in the foreground (SET-UP-16 to SET-UP-22).
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-16
    pub async fn run_server_update(&self) -> Result<ServerUpdateOutcome, SetupError> {
        self.inner.server_only()?;
        let platform = self.inner.may_replace_itself()?;
        let feed = self.inner.read_feed().await?;
        let Some(feed) = self.inner.newer_feed(feed) else {
            return Ok(ServerUpdateOutcome::UpToDate);
        };
        store::claim_server_update(
            &self.inner.pool,
            &feed.version.to_string(),
            Timestamp::now(),
        )
        .await?;
        self.inner.replace_programs(&feed, &platform).await
    }
}
