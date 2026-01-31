//! Installing the approved apps (CraftToolkit.md *Internal Logic* 2; ADR-016, ADR-072).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

use ordin_types::domain::engineering::{CraftCliMode, CraftInstallationStatus, VerificationResult};
use ordin_types::num::Sha256Hex;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt as _;
use tokio::process::Command;

use crate::error::CraftError;
use crate::model::{BlockedProgram, CraftAppPin, CraftPinnedAsset, CraftPlatform, SyncReport};
use crate::servers::KEPT_ENV;
use crate::store::{self, Row};
use crate::toolkit::{hash_current, lock, version_ok, CraftToolkit};

/// Tries of one download (Internal Logic 2.3.2).
const DOWNLOAD_TRIES: u32 = 3;

/// The app's name is its folder name and part of its server name.
fn app_name_ok(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// What the start check found (Internal Logic 2.3.5).
enum StartCheck {
    Passed,
    Refused(String),
}

/// `/usr/bin/codesign` output, read for Diagnostics (C-47: recorded, not gating).
struct Signature {
    check: &'static str,
    publisher: Option<String>,
}

impl CraftToolkit {
    /// Installs, updates and removes apps to match the profile. Reached through `check_apps(true)`,
    /// which the Setup Assistant calls in its guided and check runs (02:752 "With the Setup
    /// Assistant"). A profile change starts no download.
    // Every app is worked on, then the first failure in profile order is returned; a refused start check goes in `blocked`.
    // spec: ordin-desktop/components/CraftToolkit#ADR-072
    pub async fn sync_installations(&self) -> Result<SyncReport, CraftError> {
        let _guard = self.sync_lock.lock().await;
        let before = self.served_tools();
        self.reload_rows().await?;
        let profile = self.craft_profile();
        let mut pins: Vec<&CraftAppPin> = Vec::new();
        for pin in &profile.apps {
            if app_name_ok(&pin.name) {
                pins.push(pin);
            } else {
                tracing::warn!(
                    component = "craft",
                    code = CraftError::NotApproved.code(),
                    "craft_app_skipped"
                );
            }
        }
        self.remove_withdrawn(&pins).await?;
        let mut blocked = Vec::new();
        let mut first_failure = None;
        for pin in pins {
            // Still pinned, so not withdrawn above: skipped, and what is installed stays.
            if !version_ok(&pin.version) {
                tracing::warn!(
                    component = "craft",
                    app = %pin.name,
                    code = CraftError::NotApproved.code(),
                    "craft_app_skipped"
                );
                continue;
            }
            match self.sync_app(pin, &mut blocked).await {
                Ok(()) => {}
                Err(error) => {
                    tracing::warn!(
                        component = "craft",
                        app = %pin.name,
                        code = error.code(),
                        "craft_app_failed"
                    );
                    first_failure.get_or_insert(error);
                }
            }
        }
        self.reload_rows().await?;
        self.announce_changes(&before);
        if let Some(error) = first_failure {
            return Err(error);
        }
        Ok(SyncReport {
            installations: self.installations().await?,
            blocked,
        })
    }

    fn served_tools(&self) -> BTreeMap<String, Vec<String>> {
        self.server_specs()
            .into_iter()
            .map(|spec| {
                let name = spec.name.as_str().to_owned();
                (name, spec.tools.into_iter().map(|t| t.name).collect())
            })
            .collect()
    }

    /// Internal Logic 2.4.
    fn announce_changes(&self, before: &BTreeMap<String, Vec<String>>) {
        let specs = self.server_specs();
        let after = self.served_tools();
        for spec in &specs {
            if before.get(spec.name.as_str()) != after.get(spec.name.as_str()) {
                self.registry.tools_changed(&spec.name);
            }
        }
        for name in before.keys().filter(|name| !after.contains_key(*name)) {
            if let Ok(server) = ordin_mcp_front::ServerName::new(name.clone()) {
                self.registry.tools_changed(&server);
            }
        }
    }

    /// Internal Logic 2.2.
    async fn remove_withdrawn(&self, pins: &[&CraftAppPin]) -> Result<(), CraftError> {
        let rows: Vec<Row> = lock(&self.state).rows.values().cloned().collect();
        for mut row in rows {
            let pinned = pins.iter().any(|pin| pin.name == row.app_name);
            if pinned || row.status == CraftInstallationStatus::Removed {
                continue;
            }
            row.status = CraftInstallationStatus::Removed;
            store::update_row(&self.pool, &row).await?;
            lock(&self.state)
                .rows
                .insert(row.app_name.clone(), row.clone());
            self.stop_app_servers(&row.app_name).await;
            let _ = tokio::fs::remove_dir_all(self.install_root().join(&row.app_name)).await;
        }
        Ok(())
    }

    fn save_in_memory(&self, row: &Row) {
        lock(&self.state)
            .rows
            .insert(row.app_name.clone(), row.clone());
    }

    /// Internal Logic 2.1 and 2.3 for one pin.
    async fn sync_app(
        &self,
        pin: &CraftAppPin,
        blocked: &mut Vec<BlockedProgram>,
    ) -> Result<(), CraftError> {
        let mut row = match self.row(&pin.name) {
            Some(row) => row,
            None => {
                let row = Row::approved(&pin.name, &pin.version);
                store::insert_row(&self.pool, &row).await?;
                row
            }
        };
        let asset = pin.platforms.get(&self.platform);
        let needs_work = self.needs_work(&row, pin, asset);
        if row.pinned_version != pin.version || row.status == CraftInstallationStatus::Removed {
            row.pinned_version.clone_from(&pin.version);
            if row.status == CraftInstallationStatus::Removed {
                // `→ approved (approved in the profile)`: an app the profile brings back.
                row.status = CraftInstallationStatus::Approved;
            }
            store::update_row(&self.pool, &row).await?;
        }
        self.save_in_memory(&row);
        if !needs_work {
            return Ok(());
        }
        let Some(asset) = asset else {
            tracing::warn!(component = "craft", app = %pin.name, "craft_app_not_available_here");
            return Err(CraftError::PlatformUnsupported);
        };
        self.install(row, pin, asset, blocked).await
    }

    /// Internal Logic 2.3. A row left `downloaded` has not reached `installed`: whatever stopped it
    /// (a refused start, an unpacking failure, a closed program) gets another try.
    fn needs_work(&self, row: &Row, pin: &CraftAppPin, asset: Option<&CraftPinnedAsset>) -> bool {
        match row.status {
            CraftInstallationStatus::Approved
            | CraftInstallationStatus::Removed
            | CraftInstallationStatus::Downloaded => true,
            CraftInstallationStatus::VerificationFailed => {
                let failed = lock(&self.state).failed_pins.get(&pin.name).cloned();
                row.pinned_version != pin.version || failed.as_ref() != asset.map(|a| &a.sha256)
            }
            CraftInstallationStatus::Installed => {
                row.installed_version.as_deref() != Some(pin.version.as_str())
                    || !hash_current(row, asset)
            }
        }
    }

    async fn install(
        &self,
        mut row: Row,
        pin: &CraftAppPin,
        asset: &CraftPinnedAsset,
        blocked: &mut Vec<BlockedProgram>,
    ) -> Result<(), CraftError> {
        let app = pin.name.clone();
        let folder = self
            .version_folder(&app, &pin.version)
            .ok_or(CraftError::InstallFailed)?;
        let part = self.download(&app, &pin.version, asset).await?;
        let (actual, part) = part;

        // 2.3.3
        if !actual.0.eq_ignore_ascii_case(&asset.sha256.0) {
            let _ = tokio::fs::remove_file(&part).await;
            row.verification_result = Some(VerificationResult::Failed);
            row.status = CraftInstallationStatus::VerificationFailed;
            store::update_row(&self.pool, &row).await?;
            self.save_in_memory(&row);
            lock(&self.state)
                .failed_pins
                .insert(app.clone(), asset.sha256.clone());
            tracing::warn!(component = "craft", app = %app, version = %pin.version, "craft_verification_failed");
            self.record(
                "craft_verification_failed",
                None,
                format!("The download of {app} failed its safety check."),
                json!({ "app": app, "version": pin.version }),
            )
            .await;
            return Err(CraftError::HashMismatch);
        }

        // 2.3.4
        row.status = CraftInstallationStatus::Downloaded;
        store::update_row(&self.pool, &row).await?;
        self.save_in_memory(&row);
        // The same version under a new hash replaces the folder its servers run from.
        if row.installed_version.as_deref() == Some(pin.version.as_str()) {
            self.stop_app_servers(&app).await;
        }
        let root = self.install_root().join(&app);
        let version = pin.version.clone();
        let archive = part.clone();
        let unpacked = tokio::task::spawn_blocking(move || unpack(&archive, &root, &version)).await;
        let _ = tokio::fs::remove_file(&part).await;
        match unpacked {
            Ok(Ok(())) => {}
            _ => return Err(CraftError::InstallFailed),
        }
        let cli = self
            .cli_path(&app, &pin.version)
            .ok_or(CraftError::InstallFailed)?;
        let bundle = self
            .definition(&app, &pin.version)
            .and_then(|d| d.window.macos_bundle.as_ref())
            .map(|name| folder.join(name))
            .filter(|path| path.exists());
        let signature = self.read_signature(&cli, bundle).await;
        row.signature_check = Some(signature.check.to_owned());

        // 2.3.5
        let program = cli
            .file_name()
            .map_or_else(|| app.clone(), |n| n.to_string_lossy().into_owned());
        match self.start_check(&cli).await? {
            StartCheck::Passed => {
                lock(&self.state).blocked.remove(&app);
            }
            StartCheck::Refused(reason) => {
                store::update_row(&self.pool, &row).await?;
                self.save_in_memory(&row);
                tracing::warn!(component = "craft", program = %program, "craft_start_blocked");
                let refused = BlockedProgram {
                    program,
                    publisher: signature.publisher,
                    sha256: asset.sha256.clone(),
                    reason,
                };
                lock(&self.state).blocked.insert(app, refused.clone());
                blocked.push(refused);
                return Ok(());
            }
        }

        // 2.3.6 and 2.3.7
        let previous = row.installed_version.clone();
        row.cli_modes = self.detect_modes(&cli).await;
        row.verification_result = Some(VerificationResult::Verified);
        row.installed_version = Some(pin.version.clone());
        row.installed_sha256 = Some(asset.sha256.0.clone());
        row.status = CraftInstallationStatus::Installed;
        store::update_row(&self.pool, &row).await?;
        self.save_in_memory(&row);
        if let Some(previous) = previous.filter(|p| p != &pin.version) {
            self.stop_app_servers(&app).await;
            if let Some(old) = self.version_folder(&app, &previous) {
                let _ = tokio::fs::remove_dir_all(old).await;
            }
        }
        let modes = serde_json::to_value(&row.cli_modes).unwrap_or_default();
        tracing::info!(component = "craft", app = %app, version = %pin.version, modes = %modes, "craft_installed");
        self.record(
            "craft_installed",
            None,
            format!("{app} was installed."),
            json!({ "app": app, "version": pin.version, "modes": modes }),
        )
        .await;
        Ok(())
    }

    /// Internal Logic 2.3.2: streams the asset to `<file_name>.part`, hashing as it goes. Returns the
    /// hash and the file.
    async fn download(
        &self,
        app: &str,
        version: &str,
        asset: &CraftPinnedAsset,
    ) -> Result<(Sha256Hex, PathBuf), CraftError> {
        let folder = self.scratch_root().join("downloads");
        tokio::fs::create_dir_all(&folder)
            .await
            .map_err(|_| CraftError::DownloadFailed)?;
        let part = folder.join(format!("{}.part", asset.file_name));
        let started = Instant::now();
        let mut outcome = Err(CraftError::DownloadFailed);
        for attempt in 0..DOWNLOAD_TRIES {
            if attempt > 0 {
                tokio::time::sleep(self.limits.backoff * 2u32.pow(attempt - 1)).await;
            }
            outcome = self.download_once(asset, &part).await;
            if outcome.is_ok() {
                break;
            }
            let _ = tokio::fs::remove_file(&part).await;
        }
        let (result, bytes) = match &outcome {
            Ok((_, bytes)) => ("ok", *bytes),
            Err(error) => (error.code(), 0),
        };
        tracing::info!(
            component = "craft",
            app,
            version,
            bytes,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            result,
            "craft_download"
        );
        outcome.map(|(hash, _)| (hash, part))
    }

    async fn download_once(
        &self,
        asset: &CraftPinnedAsset,
        part: &Path,
    ) -> Result<(Sha256Hex, u64), CraftError> {
        let failed = |_| CraftError::DownloadFailed;
        let mut response = self
            .http
            .get(asset.asset_url.clone())
            .timeout(self.limits.download)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(failed)?;
        let mut file = tokio::fs::File::create(part)
            .await
            .map_err(|_| CraftError::DownloadFailed)?;
        let mut hasher = Sha256::new();
        let mut bytes = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(failed)? {
            hasher.update(&chunk);
            bytes += chunk.len() as u64;
            file.write_all(&chunk)
                .await
                .map_err(|_| CraftError::DownloadFailed)?;
        }
        file.flush().await.map_err(|_| CraftError::DownloadFailed)?;
        let digest: [u8; 32] = hasher.finalize().into();
        Ok((Sha256Hex::from_digest(&digest), bytes))
    }

    /// Internal Logic 2.3.4, on macOS: the result is recorded (C-47), and quarantine is never
    /// cleared (ADR-072).
    // spec: ordin-desktop/components/CraftToolkit#C-47
    async fn read_signature(&self, cli: &Path, bundle: Option<PathBuf>) -> Signature {
        if !matches!(
            self.platform,
            CraftPlatform::MacosArm64 | CraftPlatform::MacosX86_64
        ) {
            return Signature {
                check: "not_checked",
                publisher: None,
            };
        }
        let mut check = "valid";
        for target in std::iter::once(cli.to_path_buf()).chain(bundle) {
            let verify = Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(&target)
                .output()
                .await;
            let Ok(verify) = verify else {
                return Signature {
                    check: "not_checked",
                    publisher: None,
                };
            };
            if verify.status.success() {
                continue;
            }
            check = if String::from_utf8_lossy(&verify.stderr).contains("not signed") {
                "unsigned"
            } else {
                "invalid"
            };
        }
        let shown = Command::new("/usr/bin/codesign")
            .args(["-dvv"])
            .arg(cli)
            .output()
            .await;
        let publisher = shown.ok().and_then(|out| {
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .find_map(|line| line.strip_prefix("Authority="))
                .map(str::to_owned)
        });
        Signature { check, publisher }
    }

    fn check_command(&self, cli: &Path, arg: &str) -> Command {
        let mut command = Command::new(cli);
        command
            .arg(arg)
            .env_clear()
            .envs(
                KEPT_ENV.iter().filter_map(|name| {
                    std::env::var_os(name).map(|value| ((*name).to_owned(), value))
                }),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    /// Internal Logic 2.3.5. A program missing from the archive never started and is an install
    /// failure, not a refusal.
    async fn start_check(&self, cli: &Path) -> Result<StartCheck, CraftError> {
        let run = self.check_command(cli, "--version").output();
        let refused = |reason: &str| Ok(StartCheck::Refused(reason.to_owned()));
        match tokio::time::timeout(self.limits.check, run).await {
            Err(_) => refused("no output before the limit"),
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(CraftError::InstallFailed)
            }
            Ok(Err(_)) => refused("the operating system refused to start it"),
            Ok(Ok(out)) if out.status.code().is_none() => {
                refused("the operating system stopped it")
            }
            Ok(Ok(out)) if out.stdout.is_empty() && out.stderr.is_empty() => {
                refused("no output before the limit")
            }
            Ok(Ok(_)) => Ok(StartCheck::Passed),
        }
    }

    /// Internal Logic 2.3.6 (C-07).
    async fn detect_modes(&self, cli: &Path) -> Vec<CraftCliMode> {
        let run = self.check_command(cli, "--help").output();
        let Ok(Ok(out)) = tokio::time::timeout(self.limits.check, run).await else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&out.stdout);
        let words: Vec<&str> = text.split(|c: char| !c.is_ascii_alphanumeric()).collect();
        [
            ("mcp", CraftCliMode::Mcp),
            ("run", CraftCliMode::Run),
            ("batch", CraftCliMode::Batch),
        ]
        .into_iter()
        .filter(|(word, _)| words.contains(word))
        .map(|(_, mode)| mode)
        .collect()
    }
}

/// Unpacks the archive into `<root>/<version>.tmp/`, then renames it to `<root>/<version>/`. Entry
/// paths are checked before anything is written, so a refused archive leaves nothing behind.
fn unpack(archive: &Path, root: &Path, version: &str) -> Result<(), CraftError> {
    let failed = |_| CraftError::InstallFailed;
    let file = std::fs::File::open(archive).map_err(failed)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| CraftError::InstallFailed)?;
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|_| CraftError::InstallFailed)?;
        let name = entry.name().map_err(|_| CraftError::InstallFailed)?;
        if !entry_path_ok(&name) {
            return Err(CraftError::InstallFailed);
        }
    }
    let temporary = root.join(format!("{version}.tmp"));
    let target = root.join(version);
    let _ = std::fs::remove_dir_all(&temporary);
    let unpacked = unpack_entries(&mut zip, &temporary);
    if unpacked.is_err() {
        let _ = std::fs::remove_dir_all(&temporary);
        return unpacked;
    }
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&temporary, &target).map_err(failed)
}

fn unpack_entries(
    zip: &mut zip::ZipArchive<std::fs::File>,
    folder: &Path,
) -> Result<(), CraftError> {
    let failed = |_| CraftError::InstallFailed;
    std::fs::create_dir_all(folder).map_err(failed)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| CraftError::InstallFailed)?;
        let name = entry.name().map_err(|_| CraftError::InstallFailed)?;
        let path = folder.join(name.as_ref());
        drop(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(failed)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        let mut out = std::fs::File::create(&path).map_err(failed)?;
        std::io::copy(&mut entry, &mut out).map_err(failed)?;
        out.flush().map_err(failed)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt as _;
            // Permission bits only: an archive sets no setuid or sticky bit.
            let permissions = std::fs::Permissions::from_mode(mode & 0o777);
            std::fs::set_permissions(&path, permissions).map_err(failed)?;
        }
    }
    Ok(())
}

/// Refuses an absolute path and any `..`, on either separator (Internal Logic 2.3.4).
fn entry_path_ok(name: &str) -> bool {
    let unified = name.replace('\\', "/");
    !unified.starts_with('/')
        && !unified.contains(':')
        && Path::new(&unified)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_entry_paths() {
        for good in [
            "photocraft-cli",
            "PhotoCraft.app/Contents/Info.plist",
            "./a",
        ] {
            assert!(entry_path_ok(good), "{good}");
        }
        for bad in [
            "../../evil",
            "/etc/passwd",
            "a/../../b",
            "..\\evil",
            "C:\\x",
            "\\x",
        ] {
            assert!(!entry_path_ok(bad), "{bad}");
        }
    }

    #[test]
    fn app_names() {
        assert!(app_name_ok("photocraft"));
        assert!(app_name_ok("photo-craft-2"));
        assert!(!app_name_ok(""));
        assert!(!app_name_ok("Photo"));
        assert!(!app_name_ok("a/b"));
        assert!(!app_name_ok(&"a".repeat(41)));
    }
}
