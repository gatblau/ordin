//! H. `api-key` mode: prints the organisation key of one run.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ordin_types::RunId;

use crate::error::HookFailure;

/// The key file is read up to this size (H.2).
pub const KEY_FILE_MAX: u64 = 4096;

/// The data root: `--data-root`, else two levels above a Unix `ORDIN_HOOK_ENDPOINT`.
pub fn data_root(given: Option<&Path>, endpoint: Option<&str>) -> Result<PathBuf, HookFailure> {
    if let Some(root) = given {
        return Ok(root.to_path_buf());
    }
    let endpoint = endpoint.ok_or_else(HookFailure::key_unreadable)?;
    if !endpoint.starts_with('/') {
        return Err(HookFailure::key_unreadable());
    }
    Path::new(endpoint)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(HookFailure::key_unreadable)
}

pub fn key_path(root: &Path, run: RunId) -> PathBuf {
    root.join("run")
        .join("sessions")
        .join(run.to_string())
        .join("key")
}

#[cfg(unix)]
pub(crate) fn check_mode(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    // SAFETY: `geteuid` takes no arguments, touches no memory and cannot fail.
    let me = unsafe { libc::geteuid() };
    meta.is_file() && meta.uid() == me && meta.permissions().mode() & 0o077 == 0
}

#[cfg(not(unix))]
pub(crate) fn check_mode(meta: &std::fs::Metadata) -> bool {
    meta.is_file()
}

/// Reads and validates the key; the returned text has one trailing newline removed.
pub fn read_key(path: &Path) -> Result<String, HookFailure> {
    let fail = HookFailure::key_unreadable;
    let file = std::fs::File::open(path).map_err(|_| fail())?;
    let meta = file.metadata().map_err(|_| fail())?;
    if !check_mode(&meta) {
        return Err(fail());
    }
    let mut bytes = Vec::new();
    file.take(KEY_FILE_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| fail())?;
    if bytes.len() as u64 > KEY_FILE_MAX {
        return Err(fail());
    }
    let text = String::from_utf8(bytes).map_err(|_| fail())?;
    let key = text.strip_suffix('\n').unwrap_or(&text);
    let key = key.strip_suffix('\r').unwrap_or(key);
    if key.is_empty() {
        return Err(fail());
    }
    Ok(key.to_string())
}

pub fn run(
    run_id: RunId,
    given_root: Option<&Path>,
    endpoint: Option<&str>,
    out: &mut dyn Write,
) -> Result<(), HookFailure> {
    let root = data_root(given_root, endpoint)?;
    let key = read_key(&key_path(&root, run_id))?;
    out.write_all(key.as_bytes())
        .and_then(|_| out.write_all(b"\n"))
        .and_then(|_| out.flush())
        .map_err(|_| HookFailure::key_unreadable())
}
