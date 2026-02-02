//! Work sessions: one `/work` folder kept for the length of an agent session (ADR-040 F5;
//! Sandbox.md *Internal Logic* C.13 and F).

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};

/// At most this many work sessions at once (Sandbox.md *Configuration and constants*).
pub(crate) const MAX_SESSIONS: usize = 16;

/// One work session's state.
pub(crate) struct WorkSession {
    pub dir: PathBuf,
    /// False until the first run of the session finished and wrote the folder.
    pub created: bool,
    /// Path → SHA-256 of the files the first run was given (C.13).
    pub baseline: BTreeMap<String, [u8; 32]>,
    pub last_used: Instant,
    pub busy: bool,
}

impl WorkSession {
    pub fn reserved(dir: PathBuf) -> WorkSession {
        WorkSession {
            dir,
            created: false,
            baseline: BTreeMap::new(),
            last_used: Instant::now(),
            busy: true,
        }
    }
}

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// A relative, '/'-separated path of normal components only.
pub(crate) fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
        return None;
    }
    let p = Path::new(path);
    if p.components().all(|c| matches!(c, Component::Normal(_))) {
        Some(p.to_path_buf())
    } else {
        None
    }
}

/// Creates `dir` (and its parents), owner-only on macOS and Linux.
pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Every regular file under `dir` as ('/'-separated relative path, bytes); symbolic links are
/// skipped.
pub(crate) fn read_files(dir: &Path) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(dir)
                    .map_err(|_| io::Error::other("outside the folder"))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push((relative, std::fs::read(&path)?));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Writes or removes one file of the session folder; a path that is not a safe relative path is
/// skipped.
pub(crate) fn apply(dir: &Path, path: &str, content: Option<&[u8]>) -> io::Result<()> {
    let Some(relative) = safe_relative(path) else {
        return Ok(());
    };
    let target = dir.join(relative);
    match content {
        Some(bytes) => {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(target, bytes)
        }
        None => match std::fs::remove_file(target) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

/// The session's cumulative changes against its baseline, sorted by path.
pub(crate) fn session_changes(
    baseline: &BTreeMap<String, [u8; 32]>,
    now: &BTreeMap<String, [u8; 32]>,
) -> Vec<(String, &'static str)> {
    let mut out = Vec::new();
    for (path, hash) in now {
        match baseline.get(path) {
            None => out.push((path.clone(), "added")),
            Some(before) if before != hash => out.push((path.clone(), "changed")),
            Some(_) => {}
        }
    }
    for path in baseline.keys() {
        if !now.contains_key(path) {
            out.push((path.clone(), "removed"));
        }
    }
    out.sort();
    out
}

/// Deletes a session folder; returns (files, bytes) it held.
pub(crate) fn remove(dir: &Path) -> io::Result<(usize, u64)> {
    let (files, bytes) = match read_files(dir) {
        Ok(all) => (all.len(), all.iter().map(|(_, b)| b.len() as u64).sum()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(_) => (0, 0),
    };
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok((files, bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let mut r = [0u8; 8];
        getrandom::fill(&mut r).unwrap();
        std::env::temp_dir().join(format!("ordin-sbx-work-{}", hex(&r)))
    }

    #[test]
    fn files_are_written_read_and_removed() {
        let dir = temp();
        create_private_dir(&dir).unwrap();
        apply(&dir, "a.txt", Some(b"one")).unwrap();
        apply(&dir, "sub/b.txt", Some(b"two")).unwrap();
        apply(&dir, "../escape.txt", Some(b"x")).unwrap();
        let files = read_files(&dir).unwrap();
        assert_eq!(
            files,
            vec![
                ("a.txt".to_owned(), b"one".to_vec()),
                ("sub/b.txt".to_owned(), b"two".to_vec())
            ]
        );
        apply(&dir, "a.txt", None).unwrap();
        apply(&dir, "never.txt", None).unwrap();
        assert_eq!(read_files(&dir).unwrap().len(), 1);
        assert_eq!(remove(&dir).unwrap(), (1, 3));
        assert!(!dir.exists());
        assert_eq!(remove(&dir).unwrap(), (0, 0));
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_is_owner_only_and_links_are_skipped() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp();
        create_private_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        std::os::unix::fs::symlink("/etc/hosts", dir.join("link")).unwrap();
        assert!(read_files(&dir).unwrap().is_empty());
        remove(&dir).unwrap();
    }

    #[test]
    fn changes_are_measured_against_the_baseline() {
        let base = BTreeMap::from([
            ("same".to_owned(), sha256(b"1")),
            ("edit".to_owned(), sha256(b"1")),
            ("gone".to_owned(), sha256(b"1")),
        ]);
        let now = BTreeMap::from([
            ("same".to_owned(), sha256(b"1")),
            ("edit".to_owned(), sha256(b"2")),
            ("new".to_owned(), sha256(b"3")),
        ]);
        assert_eq!(
            session_changes(&base, &now),
            vec![
                ("edit".to_owned(), "changed"),
                ("gone".to_owned(), "removed"),
                ("new".to_owned(), "added")
            ]
        );
    }

    #[test]
    fn only_safe_relative_paths_pass() {
        assert!(safe_relative("a/b.txt").is_some());
        for bad in ["", "/etc/x", "../x", "a/../b", "a\\b", "./a"] {
            assert!(safe_relative(bad).is_none(), "{bad}");
        }
    }
}
