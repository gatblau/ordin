//! The installer-carried settings (2D-2b): read at start, written only by the personal server's setup
//! command (Internal Logic 1.2 and 8).

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ProfileError;
use crate::jws::{decode_key, InstallerKey};

/// `installer.json` is at most 16 KiB (Internal Logic 1.2).
const MAX_INSTALLER_BYTES: u64 = 16 * 1024;
/// At most two signing keys (rotation; 2D-2b).
const MAX_KEYS: usize = 2;

/// The three values of `<application data root>/installer.json`, or the branded build's defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct InstallerSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_url: Option<String>,
    /// Each a base64url (no padding) raw 32-byte Ed25519 public key; at most two.
    #[serde(default)]
    pub profile_signing_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_feed_url: Option<String>,
}

impl InstallerSettings {
    /// The checks of Internal Logic 1.2: `https` URLs, at most two keys, each 32 bytes of base64url.
    pub(crate) fn check(&self) -> Result<(), ProfileError> {
        for url in [&self.profile_url, &self.update_feed_url]
            .into_iter()
            .flatten()
        {
            check_address(url).map_err(|_| ProfileError::InstallerInvalid)?;
        }
        if self.profile_signing_keys.len() > MAX_KEYS {
            return Err(ProfileError::InstallerInvalid);
        }
        self.keys().map(|_| ())
    }

    /// The decoded signing keys.
    pub(crate) fn keys(&self) -> Result<Vec<InstallerKey>, ProfileError> {
        self.profile_signing_keys
            .iter()
            .map(|k| decode_key(k))
            .collect()
    }
}

/// An absolute `https` URL of at most 2,048 bytes, else `PROF_ADDRESS_INVALID` (Internal Logic 2.2, 8.1).
pub(crate) fn check_address(url: &str) -> Result<url::Url, ProfileError> {
    let codes = ordin_types::text::AreaCodes {
        name_empty: "PROF_ADDRESS_INVALID",
        name_too_long: "PROF_ADDRESS_INVALID",
        path_invalid: "PROF_ADDRESS_INVALID",
        url_invalid: "PROF_ADDRESS_INVALID",
    };
    ordin_types::text::check_https_url(url, crate::ALLOW_LOOPBACK_HTTP, codes)
        .map_err(|_| ProfileError::AddressInvalid)
}

/// `<data_root>/installer.json`.
pub(crate) fn installer_path(data_root: &Path) -> PathBuf {
    data_root.join("installer.json")
}

/// What reading `installer.json` found.
pub(crate) enum InstallerRead {
    Absent,
    Valid(InstallerSettings),
    Invalid,
}

/// Reads and checks `installer.json` (Internal Logic 1.2).
pub(crate) fn read_installer(data_root: &Path) -> InstallerRead {
    let path = installer_path(data_root);
    let metadata = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return InstallerRead::Absent,
        Err(_) => return InstallerRead::Invalid,
    };
    if metadata.len() > MAX_INSTALLER_BYTES {
        return InstallerRead::Invalid;
    }
    let Ok(bytes) = std::fs::read(&path) else {
        return InstallerRead::Invalid;
    };
    match serde_json::from_slice::<InstallerSettings>(&bytes) {
        Ok(settings) if settings.check().is_ok() => InstallerRead::Valid(settings),
        _ => InstallerRead::Invalid,
    }
}

/// Writes `installer.json` atomically: `.tmp`, `fsync`, rename; mode `0600` on macOS and Linux
/// (Internal Logic 8.2).
pub(crate) fn write_installer(
    data_root: &Path,
    settings: &InstallerSettings,
) -> Result<(), ProfileError> {
    let bytes =
        serde_json::to_vec_pretty(settings).map_err(|_| ProfileError::InstallerWriteFailed)?;
    std::fs::create_dir_all(data_root).map_err(|_| ProfileError::InstallerWriteFailed)?;
    write_atomic(&installer_path(data_root), &bytes).map_err(|_| ProfileError::InstallerWriteFailed)
}

/// `<path>.tmp` with mode `0600`, written and `fsync`ed, then renamed over `path`.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    #[cfg(unix)]
    {
        // An existing `.tmp` keeps its old mode through `open`; set it again.
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}
