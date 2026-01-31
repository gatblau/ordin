//! `TauriUpdater` (OrdinDesktop.md *Internal Logic* step 8, amendment January 2026 DSK-UP-11, and
//! amendment January 2026 DSK-UP-15, DSK-UP-16): the Setup Assistant's `AppUpdater` over Tauri's
//! updater plug-in. It keeps at most one downloaded update, in memory. Built for macOS and Windows.
//
// `tauri.conf.json` keeps `plugins.updater.pubkey` empty, so a development build refuses every
// downloaded update (`DSK_UPDATE_INSTALL_FAILED`) and nothing unsigned is ever installed. The release
// build passes the key in `release/ordin-update.pub` to the build (Playbook PB-REL-4).

use std::path::{Path, PathBuf};
use std::time::Duration;

use ordin_setup::{AppUpdater, InstallKind, UpdateOffer, UpdateState, UpdaterError};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

use crate::error::{UPDATE_CHECK_FAILED, UPDATE_INSTALL_FAILED};
use crate::install_kind::{detect_install_kind, install_folder, other_install, InstallFolders};

/// A download whose announced or received size passes this is stopped (DSK-UP-11.7).
pub const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
/// A download not finished this long after it started is abandoned (DSK-UP-11.7).
pub const DOWNLOAD_TIME_LIMIT: Duration = Duration::from_secs(30 * 60);

/// The pending update and, once verified, its bytes.
#[derive(Default)]
struct Kept {
    pending: Option<Update>,
    bytes: Option<(String, Vec<u8>)>,
}

pub struct TauriUpdater<R: Runtime> {
    app: AppHandle<R>,
    kept: Mutex<Kept>,
    kind: InstallKind,
    other: Option<PathBuf>,
}

impl<R: Runtime> TauriUpdater<R> {
    /// Works out the install kind once, before `register_updater` (DSK-UP-15), and the other install
    /// (DSK-UP-16).
    pub fn new(app: AppHandle<R>) -> TauriUpdater<R> {
        let exe = std::env::current_exe().unwrap_or_default();
        let folder = install_folder(&exe).unwrap_or_default();
        let kind = detect_install_kind(&folder);
        let product = app
            .config()
            .product_name
            .clone()
            .unwrap_or_else(|| "ordin".to_owned());
        let running = running_entry(&exe, &folder, &product);
        let other = install_folders(&app)
            .and_then(|folders| other_install(kind, &folders, &product, &running));
        TauriUpdater {
            app,
            kept: Mutex::new(Kept::default()),
            kind,
            other,
        }
    }
}

/// The running copy's entry in its install folder: the `.app` bundle on macOS, the executable's
/// folder entry elsewhere.
fn running_entry(exe: &Path, folder: &Path, product: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        folder.join(format!("{product}.app"))
    } else {
        exe.parent().map(Path::to_path_buf).unwrap_or_default()
    }
}

/// macOS: `~/Applications` and `/Applications`; Windows: `%LOCALAPPDATA%\Programs` and
/// `%ProgramFiles%` (DSK-UP-16).
fn install_folders<R: Runtime>(app: &AppHandle<R>) -> Option<InstallFolders> {
    if cfg!(target_os = "macos") {
        let home = app.path().home_dir().ok()?;
        Some(InstallFolders {
            per_user: home.join("Applications"),
            whole_computer: PathBuf::from("/Applications"),
        })
    } else {
        let local = std::env::var_os("LOCALAPPDATA")?;
        let program_files = std::env::var_os("ProgramFiles")?;
        Some(InstallFolders {
            per_user: PathBuf::from(local).join("Programs"),
            whole_computer: PathBuf::from(program_files),
        })
    }
}

fn unreachable(reason: &'static str) -> UpdaterError {
    tracing::warn!(
        component = "desktop",
        code = UPDATE_CHECK_FAILED,
        reason,
        "update_check_failed"
    );
    UpdaterError::Unreachable
}

/// A plug-in error of a download: a refused signature, or anything else as unreachable.
fn download_error(error: &tauri_plugin_updater::Error) -> UpdaterError {
    use tauri_plugin_updater::Error as E;
    match error {
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => {
            tracing::warn!(
                component = "desktop",
                code = UPDATE_INSTALL_FAILED,
                reason = "signature",
                "update_download_refused"
            );
            UpdaterError::SignatureInvalid
        }
        _ => unreachable("unreachable"),
    }
}

#[async_trait::async_trait]
impl<R: Runtime> AppUpdater for TauriUpdater<R> {
    // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-11
    async fn check(&self, feed_url: &url::Url) -> Result<Option<UpdateOffer>, UpdaterError> {
        let updater = self
            .app
            .updater_builder()
            .endpoints(vec![feed_url.clone()])
            .and_then(|b| b.build())
            .map_err(|_| unreachable("feed"))?;
        let found = updater.check().await.map_err(|_| unreachable("feed"))?;
        let mut kept = self.kept.lock().await;
        let Some(update) = found else {
            kept.pending = None;
            return Ok(None);
        };
        let offer = UpdateOffer {
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
            state: UpdateState::Downloading,
        };
        kept.pending = Some(update);
        Ok(Some(offer))
    }

    async fn download(&self, offer: &UpdateOffer) -> Result<(), UpdaterError> {
        let pending = {
            let kept = self.kept.lock().await;
            if kept
                .bytes
                .as_ref()
                .is_some_and(|(version, _)| *version == offer.version)
            {
                return Ok(());
            }
            match &kept.pending {
                Some(update) if update.version == offer.version => update.clone(),
                _ => return Err(unreachable("no_pending_update")),
            }
        };
        // DSK-UP-11.7: the size is watched chunk by chunk; past the bound the download is dropped.
        let (too_large_tx, mut too_large_rx) = tokio::sync::watch::channel(false);
        let mut received: u64 = 0;
        let download = pending.download(
            move |chunk, announced| {
                received = received.saturating_add(chunk as u64);
                if received > MAX_DOWNLOAD_BYTES
                    || announced.is_some_and(|n| n > MAX_DOWNLOAD_BYTES)
                {
                    let _ = too_large_tx.send(true);
                }
            },
            || {},
        );
        let bytes = tokio::select! {
            out = tokio::time::timeout(DOWNLOAD_TIME_LIMIT, download) => match out {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(error)) => return Err(download_error(&error)),
                Err(_) => return Err(unreachable("timed_out")),
            },
            _ = too_large_rx.wait_for(|too_large| *too_large) => {
                return Err(unreachable("too_large"));
            }
        };
        if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
            return Err(unreachable("too_large"));
        }
        // An offer for a different version replaces the kept bytes.
        self.kept.lock().await.bytes = Some((offer.version.clone(), bytes));
        Ok(())
    }

    async fn install_and_restart(&self) -> Result<(), UpdaterError> {
        self.install().await?;
        self.app.restart();
    }

    async fn install_on_exit(&self) -> Result<(), UpdaterError> {
        self.install().await
    }

    fn install_kind(&self) -> InstallKind {
        self.kind
    }

    fn other_install(&self) -> Option<PathBuf> {
        self.other.clone()
    }
}

impl<R: Runtime> TauriUpdater<R> {
    /// Installs the kept bytes with the plug-in's install (which checks the signature again).
    async fn install(&self) -> Result<(), UpdaterError> {
        let kept = self.kept.lock().await;
        let (Some(update), Some((version, bytes))) = (&kept.pending, &kept.bytes) else {
            return Err(UpdaterError::NothingDownloaded);
        };
        if update.version != *version {
            return Err(UpdaterError::NothingDownloaded);
        }
        update.install(bytes).map_err(|_| {
            tracing::error!(
                component = "desktop",
                code = UPDATE_INSTALL_FAILED,
                "update_install_failed"
            );
            UpdaterError::InstallFailed
        })
    }
}
