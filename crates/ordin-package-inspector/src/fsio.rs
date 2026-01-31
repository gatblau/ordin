//! File-system helpers: bounded walks that never follow symbolic links, atomic writes, digests and
//! the text clean-up applied to everything read (PackageInspector.md *Internal Logic* 1, 7, 10, 11).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use ordin_agent_model::{parse_manifest, Manifest};
use sha2::{Digest, Sha256};

use crate::error::InspectError;

/// Files (and links) a walk accepts before it refuses the folder (step 4, step 10).
pub(crate) const MAX_ENTRIES: usize = 20_000;
/// Bytes an APM-era folder may hold (step 4).
pub(crate) const MAX_FOLDER_BYTES: u64 = 256 * 1024 * 1024;
/// Files a bundle part may hold before it is treated as unreadable.
const MAX_PART_FILES: usize = 2_000;

/// What a walk found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    File,
    Link,
}

#[derive(Debug, Clone)]
pub(crate) struct Walked {
    /// `/`-separated path relative to the walk's root.
    pub rel: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub size: u64,
}

/// Step 1: a name that holds `/`, `\` or NUL, or that is not UTF-8, is never joined into a path.
pub(crate) fn name_is_safe(name: &std::ffi::OsStr) -> bool {
    match name.to_str() {
        Some(text) => !text.is_empty() && !text.contains(['/', '\\', '\0']),
        None => false,
    }
}

/// Every file and link under `root`, links not followed. More than `MAX_ENTRIES` entries or more
/// than `max_bytes` bytes refuses the folder (`PKI_FOLDER_UNREADABLE`, permanent).
pub(crate) fn walk(root: &Path, max_bytes: u64) -> Result<Vec<Walked>, InspectError> {
    let mut out = Vec::new();
    let mut total: u64 = 0;
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let listing = fs::read_dir(&dir).map_err(|e| InspectError::folder(&e))?;
        for entry in listing {
            let entry = entry.map_err(|e| InspectError::folder(&e))?;
            let file_name = entry.file_name();
            if !name_is_safe(&file_name) {
                tracing::warn!(reason = "unsafe_name", "package_inspector.name_skipped");
                continue;
            }
            let name = file_name.to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|e| InspectError::folder(&e))?;
            let file_type = meta.file_type();
            if file_type.is_dir() {
                stack.push((path, rel));
                continue;
            }
            let kind = if file_type.is_symlink() {
                EntryKind::Link
            } else if file_type.is_file() {
                EntryKind::File
            } else {
                continue;
            };
            total = total.saturating_add(meta.len());
            if out.len() >= MAX_ENTRIES || total > max_bytes {
                return Err(InspectError::FolderUnreadable { retryable: false });
            }
            out.push(Walked {
                rel,
                path,
                kind,
                size: meta.len(),
            });
        }
    }
    out.sort_by(|a, b| a.rel.as_bytes().cmp(b.rel.as_bytes()));
    Ok(out)
}

/// True when `root/name` is a regular file (a symbolic link is not followed, so it is not one).
pub(crate) fn is_regular_file(root: &Path, name: &str) -> bool {
    fs::symlink_metadata(root.join(name))
        .map(|meta| meta.file_type().is_file())
        .unwrap_or(false)
}

/// Writes `bytes` to a temporary file beside `path`, flushes it to disk and renames it into place.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", ordin_types::AgentInstanceId::new()));
    let temporary = PathBuf::from(temporary);
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The one file of a bundle part that `parse_manifest` accepts: its relative path, bytes and
/// manifest. None when the directory is unreadable, holds none or holds two.
pub(crate) fn primary_manifest(dir: &Path) -> Option<(String, Vec<u8>, Manifest)> {
    let files = walk(dir, u64::MAX).ok()?;
    if files.len() > MAX_PART_FILES {
        return None;
    }
    let mut found: Option<(String, Vec<u8>, Manifest)> = None;
    for file in files.iter().filter(|f| f.kind == EntryKind::File) {
        let Ok(bytes) = fs::read(&file.path) else {
            continue;
        };
        if let Ok(Some(manifest)) = parse_manifest(&file.rel, &bytes) {
            if found.is_some() {
                return None;
            }
            found = Some((file.rel.clone(), bytes, manifest));
        }
    }
    found
}

/// Removes every control character (used for the argument hint).
pub(crate) fn strip_controls(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Removes control characters other than a line feed and a tab (used for page text).
pub(crate) fn strip_controls_keep_lines(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// Cuts `text` to at most `max` characters, on a character boundary.
pub(crate) fn cut_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Step 3: the text up to the first `. `, `.\n` or the end, cut at 300 characters.
pub(crate) fn first_sentence(description: &str) -> String {
    let text = description.trim();
    let end = [". ", ".\n"]
        .iter()
        .filter_map(|marker| text.find(marker))
        .min()
        .map(|at| at + 1)
        .unwrap_or(text.len());
    cut_chars(strip_controls_keep_lines(&text[..end]).trim(), 300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sentence_cuts_at_the_first_full_stop_followed_by_space_or_line_end() {
        assert_eq!(first_sentence("One. Two."), "One.");
        assert_eq!(first_sentence("One.\nTwo."), "One.");
        assert_eq!(first_sentence("No stop"), "No stop");
        assert_eq!(first_sentence("Ends here."), "Ends here.");
        assert_eq!(first_sentence("  padded  "), "padded");
        assert_eq!(first_sentence(""), "");
        assert_eq!(first_sentence(&"é".repeat(400)).chars().count(), 300);
    }

    #[test]
    fn control_characters_are_removed() {
        assert_eq!(strip_controls("a\tb\nc\u{7}"), "abc");
        assert_eq!(strip_controls_keep_lines("a\tb\nc\u{7}\r"), "a\tb\nc");
    }

    #[test]
    fn unsafe_names_are_refused() {
        use std::ffi::OsStr;
        assert!(name_is_safe(OsStr::new("ordin.lock")));
        assert!(!name_is_safe(OsStr::new("a\\b")));
        assert!(!name_is_safe(OsStr::new("a/b")));
        assert!(!name_is_safe(OsStr::new("")));
        assert!(!name_is_safe(OsStr::new("a\0b")));
    }

    #[test]
    fn a_walk_over_the_entry_limit_or_the_byte_limit_is_refused() {
        let dir =
            std::env::temp_dir().join(format!("pki-walk-{}", ordin_types::AgentInstanceId::new()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("a"), [0u8; 10]).expect("file");
        std::fs::write(dir.join("b"), [0u8; 10]).expect("file");
        assert_eq!(walk(&dir, u64::MAX).expect("walk").len(), 2);
        assert!(matches!(
            walk(&dir, 15),
            Err(InspectError::FolderUnreadable { retryable: false })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn digests_are_lowercase_hex_of_64_characters() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
