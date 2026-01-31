//! `ensure_language_server` (SetupAssistant.md *Internal Logic* H; SA-CLOSE-01): a pinned
//! language server installed under the data root on first use.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_organisation_profile::{LanguageServerAsset, LanguageServerEntry, ProfilePlatform};
use sha2::{Digest, Sha256};

use crate::assistant::{hex, lock, read, SetupAssistant};
use crate::error::SetupError;
use crate::types::LanguageServerLaunch;
use crate::updates::{safe_entry_path, set_executable, UNPACK_MAX_BYTES, UNPACK_MAX_ENTRIES};
use crate::TARGET;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DOWNLOAD_MAX_BYTES: u64 = 500 * 1024 * 1024;
const MARKER: &str = "ready";
const LANGUAGES: [&str; 4] = ["go", "rust", "java", "typescript"];

fn not_approved() -> SetupError {
    SetupError::permanent("SETUP_LANGUAGE_SERVER_NOT_APPROVED")
}

fn failed_retryable() -> SetupError {
    SetupError::of("SETUP_LANGUAGE_SERVER_INSTALL_FAILED")
}

fn failed_permanent() -> SetupError {
    SetupError::permanent("SETUP_LANGUAGE_SERVER_INSTALL_FAILED")
}

/// H.1: this computer's platform key.
pub(crate) fn this_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("macos_arm64"),
        ("macos", "x86_64") => Some("macos_x86_64"),
        ("windows", "x86_64") => Some("windows_x86_64"),
        ("linux", "x86_64") => Some("linux_x86_64"),
        _ => None,
    }
}

fn platform_key(p: ProfilePlatform) -> &'static str {
    match p {
        ProfilePlatform::MacosArm64 => "macos_arm64",
        ProfilePlatform::MacosX86_64 => "macos_x86_64",
        ProfilePlatform::WindowsX86_64 => "windows_x86_64",
        ProfilePlatform::LinuxX86_64 => "linux_x86_64",
    }
}

fn language_key(entry: &LanguageServerEntry) -> String {
    serde_json::to_value(entry.language)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A single path segment: no separator, no `..`, not empty.
fn segment(text: &str) -> bool {
    !text.is_empty() && text != "." && text != ".." && !text.contains(['/', '\\', '\0'])
}

/// H.3.3: unpack by file name into `tmp`; the archive is deleted afterwards.
fn unpack(file: &Path, file_name: &str, tmp: &Path) -> Result<(), SetupError> {
    if file_name.ends_with(".zip") {
        let reader = std::fs::File::open(file).map_err(|_| failed_retryable())?;
        let mut archive = zip::ZipArchive::new(reader).map_err(|_| failed_permanent())?;
        if archive.len() > UNPACK_MAX_ENTRIES {
            return Err(failed_permanent());
        }
        let mut total = 0u64;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|_| failed_permanent())?;
            let Some(path) = entry.enclosed_name() else {
                return Err(failed_permanent());
            };
            if !safe_entry_path(&path) {
                return Err(failed_permanent());
            }
            total = total.saturating_add(entry.size());
            if total > UNPACK_MAX_BYTES {
                return Err(failed_permanent());
            }
            let target = tmp.join(&path);
            if entry.is_dir() {
                std::fs::create_dir_all(&target).map_err(|_| failed_retryable())?;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|_| failed_retryable())?;
            }
            let mut out = std::fs::File::create(&target).map_err(|_| failed_retryable())?;
            std::io::copy(&mut (&mut entry).take(UNPACK_MAX_BYTES), &mut out)
                .map_err(|_| failed_retryable())?;
        }
    } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        let reader = std::fs::File::open(file).map_err(|_| failed_retryable())?;
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
        let mut count = 0usize;
        let mut total = 0u64;
        for entry in archive.entries().map_err(|_| failed_permanent())? {
            let mut entry = entry.map_err(|_| failed_permanent())?;
            count += 1;
            total = total.saturating_add(entry.header().size().map_err(|_| failed_permanent())?);
            if count > UNPACK_MAX_ENTRIES || total > UNPACK_MAX_BYTES {
                return Err(failed_permanent());
            }
            let path = entry.path().map_err(|_| failed_permanent())?.into_owned();
            if !safe_entry_path(&path) {
                return Err(failed_permanent());
            }
            let kind = entry.header().entry_type();
            let target = tmp.join(&path);
            if kind.is_dir() {
                std::fs::create_dir_all(&target).map_err(|_| failed_retryable())?;
            } else if kind.is_file() {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|_| failed_retryable())?;
                }
                let mut out = std::fs::File::create(&target).map_err(|_| failed_retryable())?;
                std::io::copy(&mut entry, &mut out).map_err(|_| failed_retryable())?;
            } else {
                // Links and special files are not unpacked.
                return Err(failed_permanent());
            }
        }
    } else if let Some(stem) = file_name.strip_suffix(".gz") {
        let reader = std::fs::File::open(file).map_err(|_| failed_retryable())?;
        let mut decoder = flate2::read::GzDecoder::new(reader).take(UNPACK_MAX_BYTES + 1);
        let mut out = std::fs::File::create(tmp.join(stem)).map_err(|_| failed_retryable())?;
        let written = std::io::copy(&mut decoder, &mut out).map_err(|_| failed_permanent())?;
        if written > UNPACK_MAX_BYTES {
            return Err(failed_permanent());
        }
    } else {
        // Any other suffix: the file is used as it is.
        return Ok(());
    }
    let _ = std::fs::remove_file(file);
    Ok(())
}

impl SetupAssistant {
    /// H.1: the approved entry for `language`, from the profile or else the build's table.
    // spec: ordin-desktop/components/SetupAssistant#SA-CLOSE-01
    fn language_entry(&self, language: &str) -> Option<LanguageServerEntry> {
        let snapshot = self.inner.ports.profile.current();
        if let Some(entry) = snapshot
            .document
            .language_servers
            .iter()
            .find(|e| language_key(e) == language)
        {
            return Some(entry.clone());
        }
        read(&self.inner.tuning)
            .language_table
            .iter()
            .find(|e| language_key(e) == language)
            .cloned()
    }

    /// Installs a pinned language server under the data root on first use and returns how to
    /// start it (H).
    pub async fn ensure_language_server(
        &self,
        language: &str,
        server_name: &str,
        version: &str,
    ) -> Result<LanguageServerLaunch, SetupError> {
        if !LANGUAGES.contains(&language) || !segment(server_name) || !segment(version) {
            return Err(not_approved());
        }
        let entry = self.language_entry(language).ok_or_else(not_approved)?;
        if entry.name != server_name || entry.version != version {
            return Err(not_approved());
        }
        let platform = read(&self.inner.tuning).platform.or_else(this_platform);
        let asset = entry
            .platforms
            .iter()
            .find(|(p, _)| Some(platform_key(**p)) == platform)
            .map(|(_, a)| a.clone())
            .ok_or_else(|| SetupError::permanent("SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED"))?;
        let base = self
            .inner
            .data_root
            .join("language-servers")
            .join(server_name);
        let folder = base.join(version);
        // H.2: one install per (name, version) at a time.
        let gate = {
            let mut locks = lock(&self.inner.install_locks);
            locks
                .entry((server_name.to_owned(), version.to_owned()))
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _one = gate.lock().await;
        if let Some(launch) = read_marker(&folder) {
            return Ok(launch);
        }
        let started = Instant::now();
        let tmp = base.join(format!(".{version}.tmp-{}", uuid::Uuid::new_v4().simple()));
        let result = self
            .install(&asset, &tmp, &folder, server_name, version)
            .await;
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
            return result;
        }
        tracing::info!(
            target: TARGET,
            server = server_name,
            version,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "setup.language_server.installed"
        );
        result
    }

    /// H.3.
    async fn install(
        &self,
        asset: &LanguageServerAsset,
        tmp: &Path,
        folder: &Path,
        server: &str,
        version: &str,
    ) -> Result<LanguageServerLaunch, SetupError> {
        if !segment(&asset.file_name) {
            return Err(failed_permanent());
        }
        let binary_path = PathBuf::from(&asset.binary_path);
        if !safe_entry_path(&binary_path) {
            return Err(failed_permanent());
        }
        std::fs::create_dir_all(tmp).map_err(|_| failed_retryable())?;
        let file = tmp.join(&asset.file_name);
        // H.3.1.
        let bytes = tokio::time::timeout(DOWNLOAD_TIMEOUT, async {
            let mut reply = self
                .inner
                .http
                .get(&asset.asset_url)
                .send()
                .await
                .map_err(|_| failed_retryable())?;
            if !reply.status().is_success() {
                return Err(failed_retryable());
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = reply.chunk().await.map_err(|_| failed_retryable())? {
                bytes.extend_from_slice(&chunk);
                if bytes.len() as u64 > DOWNLOAD_MAX_BYTES {
                    return Err(failed_retryable());
                }
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| failed_retryable())??;
        // H.3.2.
        if hex(&Sha256::digest(&bytes)) != asset.sha256.0 {
            tracing::warn!(
                target: TARGET,
                server,
                version,
                "setup.language_server.digest_mismatch"
            );
            return Err(SetupError::permanent(
                "SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH",
            ));
        }
        std::fs::write(&file, &bytes).map_err(|_| failed_retryable())?;
        // H.3.3.
        unpack(&file, &asset.file_name, tmp)?;
        // H.3.4.
        let binary = tmp.join(&binary_path);
        if !std::fs::symlink_metadata(&binary).is_ok_and(|m| m.is_file()) {
            return Err(failed_permanent());
        }
        set_executable(&binary).map_err(|_| failed_retryable())?;
        // H.3.5.
        let program = folder.join(&binary_path);
        let marker = serde_json::json!({
            "program": program.to_string_lossy(),
            "args": [],
            "env": [],
        });
        std::fs::write(tmp.join(MARKER), marker.to_string()).map_err(|_| failed_retryable())?;
        std::fs::rename(tmp, folder).map_err(|_| failed_retryable())?;
        Ok(LanguageServerLaunch {
            program,
            args: Vec::new(),
            env: Vec::new(),
        })
    }
}

/// H.2: the launch kept in the `ready` marker, if the folder holds one.
fn read_marker(folder: &Path) -> Option<LanguageServerLaunch> {
    let text = std::fs::read_to_string(folder.join(MARKER)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let program = PathBuf::from(value.get("program")?.as_str()?);
    let args = value
        .get("args")?
        .as_array()?
        .iter()
        .filter_map(|a| a.as_str().map(str::to_owned))
        .collect();
    let env = value
        .get("env")?
        .as_array()?
        .iter()
        .filter_map(|pair| {
            let pair = pair.as_array()?;
            Some((
                pair.first()?.as_str()?.to_owned(),
                pair.get(1)?.as_str()?.to_owned(),
            ))
        })
        .collect();
    Some(LanguageServerLaunch { program, args, env })
}
