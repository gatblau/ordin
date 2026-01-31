//! File operations of the writer (Installers.md *Internal Logic* step 11): atomic writes that never
//! follow a link, digests, link-safe removal, and the install record.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use ordin_types::encoding::canonical_json;
use ordin_types::num::Sha256Hex;

use crate::error::InstallError;
use crate::types::{InstallRecord, RECORD_FILE, RECORD_SCHEMA};

/// Lowercase SHA-256 hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    Sha256Hex::from_digest(&digest).0
}

/// `root` joined with a '/'-separated relative path.
pub(crate) fn join(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        path.push(segment);
    }
    path
}

/// What is at a path, without following a link.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OnDisk {
    Missing,
    /// A regular file with its digest.
    File(String),
    /// A link, a directory or anything else that is not a regular file.
    Other,
}

pub(crate) fn on_disk(root: &Path, relative: &str) -> io::Result<OnDisk> {
    let path = join(root, relative);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OnDisk::Missing),
        Err(error) => Err(error),
        Ok(meta) if meta.file_type().is_file() => Ok(OnDisk::File(sha256_hex(&fs::read(&path)?))),
        Ok(_) => Ok(OnDisk::Other),
    }
}

/// Creates `dir` (relative to `root`) with mode `0700`, one component at a time; a link met on the
/// way is removed (its target untouched) and replaced by a directory, so a write never leaves `root`.
fn ensure_dir(root: &Path, dir: &str) -> io::Result<()> {
    let mut path = root.to_path_buf();
    for segment in dir.split('/').filter(|s| !s.is_empty()) {
        path.push(segment);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_dir() => continue,
            Ok(meta) if meta.file_type().is_symlink() => remove_entry(&path)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "a file holds a directory's place",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        make_private_dir(&path)?;
    }
    Ok(())
}

/// Creates one directory with mode `0700` on macOS and Linux.
pub(crate) fn make_private_dir(path: &Path) -> io::Result<()> {
    // Only Unix sets a mode, so on Windows the builder is never changed.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        other => other,
    }
}

/// Step 11: parents created (`0700`), the bytes written to `<dir>/.ordin-tmp-<UUIDv7>`, synced, and
/// renamed over the target; an existing link at the target is removed first, its target untouched.
pub(crate) fn write_atomic(root: &Path, relative: &str, bytes: &[u8]) -> io::Result<()> {
    let (dir, _) = relative.rsplit_once('/').unwrap_or(("", relative));
    ensure_dir(root, dir)?;
    let target = join(root, relative);
    let parent = target.parent().unwrap_or(root).to_path_buf();
    let temporary = parent.join(format!(".ordin-tmp-{}", uuid::Uuid::now_v7()));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
            remove_entry(&target)?;
        }
        fs::rename(&temporary, &target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Removes a file or a link (never what a link points to). A path already gone is not an error.
pub(crate) fn remove_entry(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(meta) if meta.file_type().is_dir() => fs::remove_dir(path),
        Ok(_) => fs::remove_file(path).or_else(|error| fs::remove_dir(path).map_err(|_| error)),
    }
}

/// Deletes `path` and everything under it without following links.
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(meta) if meta.file_type().is_dir() => {
            for entry in fs::read_dir(path)? {
                remove_tree(&entry?.path())?;
            }
            fs::remove_dir(path)
        }
        Ok(_) => remove_entry(path),
    }
}

/// Empties `dir` (link-safe), keeping `dir` itself.
pub(crate) fn empty_dir(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        remove_tree(&entry?.path())?;
    }
    Ok(())
}

/// Removes the directories above `relative` that are now empty, deepest first, stopping at `root`.
pub(crate) fn prune_empty_parents(root: &Path, relative: &str) {
    let segments: Vec<&str> = relative.split('/').collect();
    for end in (1..segments.len()).rev() {
        let dir = join(root, &segments[..end].join("/"));
        let is_real_dir = fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_dir());
        if !is_real_dir || fs::remove_dir(&dir).is_err() {
            return;
        }
    }
}

/// True when `dir` holds any entry.
pub(crate) fn has_entries(dir: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(dir)?.next().is_some())
}

/// Reads `meta/install-record.json`: missing → `INST_NOT_INSTALLED`; unparseable or another schema
/// → `INST_RECORD_INVALID`.
pub(crate) fn read_record(meta: &Path) -> Result<InstallRecord, InstallError> {
    let bytes = match fs::read(meta.join(RECORD_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(InstallError::NotInstalled)
        }
        Err(_) => return Err(InstallError::io()),
    };
    let record: InstallRecord =
        serde_json::from_slice(&bytes).map_err(|_| InstallError::RecordInvalid)?;
    if record.record_schema != RECORD_SCHEMA {
        return Err(InstallError::RecordInvalid);
    }
    Ok(record)
}

/// Writes the record atomically as canonical JSON.
pub(crate) fn write_record(meta: &Path, record: &InstallRecord) -> Result<(), InstallError> {
    let value =
        serde_json::to_value(record).map_err(|_| InstallError::WriteFailed { retryable: false })?;
    write_atomic(meta, RECORD_FILE, &canonical_json(&value)).map_err(|_| InstallError::io())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ordin-inst-fsio-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn writes_never_follow_links() {
        let root = temp();
        let outside = temp();
        fs::write(outside.join("victim"), b"keep").unwrap();
        std::os::unix::fs::symlink(outside.join("victim"), root.join("file")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("dir")).unwrap();
        write_atomic(&root, "file", b"new").unwrap();
        write_atomic(&root, "dir/inner", b"x").unwrap();
        assert_eq!(fs::read(outside.join("victim")).unwrap(), b"keep");
        assert!(!outside.join("inner").exists());
        assert_eq!(fs::read(root.join("file")).unwrap(), b"new");
        assert_eq!(
            on_disk(&root, "dir/inner").unwrap(),
            OnDisk::File(sha256_hex(b"x"))
        );
        std::os::unix::fs::symlink(outside.join("victim"), root.join("link")).unwrap();
        assert_eq!(on_disk(&root, "link").unwrap(), OnDisk::Other);
        remove_tree(&root.join("link")).unwrap();
        assert!(outside.join("victim").exists());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_file_in_a_directory_place_fails_and_leaves_no_temporary() {
        let root = temp();
        fs::write(root.join("a"), b"file").unwrap();
        assert!(write_atomic(&root, "a/b", b"x").is_err());
        fs::create_dir_all(root.join("d/e")).unwrap();
        assert!(write_atomic(&root, "d", b"x").is_err());
        let names: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.iter().all(|n| !n.starts_with(".ordin-tmp-")),
            "{names:?}"
        );
        prune_empty_parents(&root, "d/e/f");
        assert!(!root.join("d").exists());
        assert!(has_entries(&root).unwrap());
        empty_dir(&root).unwrap();
        assert!(!has_entries(&root).unwrap());
        let _ = fs::remove_dir_all(&root);
    }
}
