//! C19 — WorkDir: the per-run working folder (ADR-041).
//!
//! A run may carry input files. The host creates a fresh folder for that run only, writes
//! the inputs into it, preopens it in the guest at `/work` (the guest's only directory),
//! and, after the run, reports every file added, changed or removed by comparing SHA-256
//! snapshots taken before and after. The folder is deleted when the `WorkDir` is dropped.
//!
//! Invariants:
//!   - the caller supplies file *content*, never a host path; the person's own folders are
//!     never mounted;
//!   - an input path is relative, made only of normal components, and stays inside the
//!     folder (no `..`, no absolute path, no empty segment);
//!   - symbolic links in the folder are never followed and never reported;
//!   - file count, total bytes and single-file size are bounded, both for inputs and for
//!     the folder's state after the run.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use sha2::{Digest, Sha256};

/// The guest path at which the working folder is preopened.
pub const GUEST_MOUNT: &str = "/work";

/// Bounds on the working folder (inputs and post-run state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkLimits {
    pub max_files: usize,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
}

impl Default for WorkLimits {
    fn default() -> Self {
        WorkLimits {
            max_files: 1_000,
            max_total_bytes: 64 * 1024 * 1024,
            max_file_bytes: 16 * 1024 * 1024,
        }
    }
}

/// One input file: a relative path under `/work` and its content (base64 on the wire).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkFile {
    pub path: String,
    pub content_base64: String,
}

/// How a file differs between the start and the end of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Changed,
    Removed,
}

/// One entry of the change record. `content_base64` is present for added and changed files.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_base64: Option<String>,
}

/// Why a working folder could not be prepared or reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkDirError {
    /// An input path is absolute, empty, or leaves the folder. Carries the offending path.
    InvalidPath(String),
    /// Two inputs name the same path.
    DuplicatePath(String),
    /// An input's content is not valid base64.
    InvalidContent(String),
    /// The inputs exceed a `WorkLimits` bound.
    InputsTooLarge,
    /// The folder exceeds a `WorkLimits` bound after the run.
    LimitExceeded,
    /// The host could not create, write or read the folder.
    Io,
}

impl WorkDirError {
    /// The wire code: request-shape faults are `VAL_ERR`; an over-limit folder after a run
    /// is `RUNTIME_LIMIT`; a host fault is `INTERNAL`.
    pub fn code(&self) -> &'static str {
        match self {
            WorkDirError::InvalidPath(_)
            | WorkDirError::DuplicatePath(_)
            | WorkDirError::InvalidContent(_)
            | WorkDirError::InputsTooLarge => "VAL_ERR",
            WorkDirError::LimitExceeded => "RUNTIME_LIMIT",
            WorkDirError::Io => "INTERNAL",
        }
    }
}

/// A file's SHA-256 digest, keyed by its `/`-separated path relative to the folder.
pub type Snapshot = BTreeMap<String, [u8; 32]>;

/// A per-run working folder. Dropping it deletes the folder and everything in it.
pub struct WorkDir {
    root: PathBuf,
    limits: WorkLimits,
    before: Snapshot,
}

impl WorkDir {
    /// Create a fresh folder under the system temporary directory, write `inputs` into it
    /// after validating every path and the limits, and take the starting snapshot.
    pub fn create(inputs: &[WorkFile], limits: WorkLimits) -> Result<WorkDir, WorkDirError> {
        let decoded = validate_inputs(inputs, &limits)?;
        let root = fresh_dir()?;
        // From here on the folder exists: construct the guard first so any failure below
        // still deletes it.
        let mut work = WorkDir {
            root,
            limits,
            before: Snapshot::new(),
        };
        for (rel, bytes) in &decoded {
            let host = work.root.join(rel);
            if let Some(parent) = host.parent() {
                std::fs::create_dir_all(parent).map_err(|_| WorkDirError::Io)?;
            }
            std::fs::write(&host, bytes).map_err(|_| WorkDirError::Io)?;
        }
        work.before = work.snapshot()?;
        Ok(work)
    }

    /// The host path to preopen in the guest.
    pub fn host_path(&self) -> &Path {
        &self.root
    }

    /// The folder's current total size in bytes (regular files only, links not followed).
    /// Used by the runtime to stop a run that fills the folder past `max_total_bytes`.
    pub fn current_bytes(&self) -> u64 {
        dir_bytes(&self.root)
    }

    /// The configured bounds.
    pub fn limits(&self) -> WorkLimits {
        self.limits
    }

    /// Compare the folder now with its starting snapshot. Enforces the limits on the
    /// folder's post-run state (`LimitExceeded` → the run is reported as `RUNTIME_LIMIT`).
    pub fn changes(&self) -> Result<Vec<FileChange>, WorkDirError> {
        let after = self.snapshot()?;
        let mut out = Vec::new();
        for (path, digest) in &after {
            let kind = match self.before.get(path) {
                None => ChangeKind::Added,
                Some(prev) if prev != digest => ChangeKind::Changed,
                Some(_) => continue,
            };
            let bytes = std::fs::read(self.root.join(path)).map_err(|_| WorkDirError::Io)?;
            out.push(FileChange {
                path: path.clone(),
                kind,
                content_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            });
        }
        for path in self.before.keys() {
            if !after.contains_key(path) {
                out.push(FileChange {
                    path: path.clone(),
                    kind: ChangeKind::Removed,
                    content_base64: None,
                });
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Hash every regular file in the folder, enforcing the limits as it goes.
    fn snapshot(&self) -> Result<Snapshot, WorkDirError> {
        let mut snap = Snapshot::new();
        let mut total = 0u64;
        let limits = self.limits;
        let root = self.root.clone();
        walk(&root, &mut |path, meta| {
            if meta.len() > limits.max_file_bytes {
                return Err(WorkDirError::LimitExceeded);
            }
            total = total.saturating_add(meta.len());
            if total > limits.max_total_bytes || snap.len() + 1 > limits.max_files {
                return Err(WorkDirError::LimitExceeded);
            }
            let bytes = std::fs::read(path).map_err(|_| WorkDirError::Io)?;
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            snap.insert(relative_key(&root, path)?, digest);
            Ok(())
        })?;
        Ok(snap)
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Total size in bytes of the regular files under `root` (links not followed).
pub fn dir_bytes(root: &Path) -> u64 {
    let mut total = 0u64;
    let _ = walk(root, &mut |_, meta| {
        total = total.saturating_add(meta.len());
        Ok(())
    });
    total
}

/// Validate every input path and decode its content, enforcing the limits on the inputs.
fn validate_inputs(
    inputs: &[WorkFile],
    limits: &WorkLimits,
) -> Result<Vec<(PathBuf, Vec<u8>)>, WorkDirError> {
    if inputs.len() > limits.max_files {
        return Err(WorkDirError::InputsTooLarge);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0u64;
    let mut out = Vec::with_capacity(inputs.len());
    for f in inputs {
        let rel = safe_relative(&f.path)?;
        if !seen.insert(rel.clone()) {
            return Err(WorkDirError::DuplicatePath(f.path.clone()));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(f.content_base64.as_bytes())
            .map_err(|_| WorkDirError::InvalidContent(f.path.clone()))?;
        let len = bytes.len() as u64;
        if len > limits.max_file_bytes {
            return Err(WorkDirError::InputsTooLarge);
        }
        total = total.saturating_add(len);
        if total > limits.max_total_bytes {
            return Err(WorkDirError::InputsTooLarge);
        }
        out.push((rel, bytes));
    }
    Ok(out)
}

/// Accept `a/b.txt` or `/work/a/b.txt`; refuse absolute paths elsewhere, `..`, `.`-only,
/// empty segments, and anything that is not a plain relative path.
fn safe_relative(raw: &str) -> Result<PathBuf, WorkDirError> {
    let invalid = || WorkDirError::InvalidPath(raw.to_string());
    let trimmed = raw
        .strip_prefix(GUEST_MOUNT)
        .map(|r| r.strip_prefix('/').unwrap_or(""))
        .unwrap_or(raw);
    if trimmed.is_empty() || trimmed.contains('\\') || trimmed.contains('\0') {
        return Err(invalid());
    }
    if trimmed
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(invalid());
    }
    let path = PathBuf::from(trimmed);
    if !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(invalid());
    }
    Ok(path)
}

/// A unique folder under the system temporary directory, readable only by this user.
fn fresh_dir() -> Result<PathBuf, WorkDirError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| WorkDirError::Io)?;
    let name: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let root = std::env::temp_dir().join(format!("sandboxd-work-{name}"));
    std::fs::create_dir(&root).map_err(|_| WorkDirError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| WorkDirError::Io)?;
    }
    Ok(root)
}

/// Visit every regular file under `dir`, depth first. Symbolic links are skipped, never
/// followed.
fn walk(
    dir: &Path,
    visit: &mut dyn FnMut(&Path, &std::fs::Metadata) -> Result<(), WorkDirError>,
) -> Result<(), WorkDirError> {
    let entries = std::fs::read_dir(dir).map_err(|_| WorkDirError::Io)?;
    for entry in entries {
        let entry = entry.map_err(|_| WorkDirError::Io)?;
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|_| WorkDirError::Io)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(&path, visit)?;
        } else if meta.is_file() {
            visit(&path, &meta)?;
        }
    }
    Ok(())
}

/// The `/`-separated key of `path` relative to `root`.
fn relative_key(root: &Path, path: &Path) -> Result<String, WorkDirError> {
    let rel = path.strip_prefix(root).map_err(|_| WorkDirError::Io)?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, content: &[u8]) -> WorkFile {
        WorkFile {
            path: path.to_string(),
            content_base64: base64::engine::general_purpose::STANDARD.encode(content),
        }
    }

    #[test]
    fn inputs_are_written_and_an_untouched_folder_reports_no_changes() {
        let w = WorkDir::create(
            &[file("a.txt", b"one"), file("d/b.txt", b"two")],
            WorkLimits::default(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(w.host_path().join("d/b.txt")).unwrap(),
            b"two"
        );
        assert!(w.changes().unwrap().is_empty());
    }

    #[test]
    fn added_changed_and_removed_files_are_reported() {
        let w = WorkDir::create(
            &[
                file("keep.txt", b"k"),
                file("edit.txt", b"1"),
                file("gone.txt", b"g"),
            ],
            WorkLimits::default(),
        )
        .unwrap();
        std::fs::write(w.host_path().join("edit.txt"), b"2").unwrap();
        std::fs::remove_file(w.host_path().join("gone.txt")).unwrap();
        std::fs::write(w.host_path().join("new.txt"), b"n").unwrap();
        let c = w.changes().unwrap();
        let summary: Vec<(&str, ChangeKind)> =
            c.iter().map(|f| (f.path.as_str(), f.kind)).collect();
        assert_eq!(
            summary,
            vec![
                ("edit.txt", ChangeKind::Changed),
                ("gone.txt", ChangeKind::Removed),
                ("new.txt", ChangeKind::Added),
            ]
        );
        assert_eq!(c[0].content_base64.as_deref(), Some("Mg=="));
        assert_eq!(c[1].content_base64, None);
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        for bad in [
            "../x",
            "/etc/passwd",
            "a/../../x",
            "",
            "a//b",
            "./a",
            "a\\b",
            "/work/../x",
            "/work",
        ] {
            let r = WorkDir::create(&[file(bad, b"x")], WorkLimits::default());
            assert!(
                matches!(r, Err(WorkDirError::InvalidPath(_))),
                "{bad:?} must be refused"
            );
        }
    }

    #[test]
    fn a_work_prefixed_path_is_accepted_as_relative() {
        let w = WorkDir::create(&[file("/work/sub/a.txt", b"x")], WorkLimits::default()).unwrap();
        assert!(w.host_path().join("sub/a.txt").is_file());
    }

    #[test]
    fn duplicate_paths_and_bad_base64_are_refused() {
        let dup = WorkDir::create(&[file("a", b"1"), file("a", b"2")], WorkLimits::default());
        assert!(matches!(dup, Err(WorkDirError::DuplicatePath(_))));
        let bad = WorkDir::create(
            &[WorkFile {
                path: "a".into(),
                content_base64: "***".into(),
            }],
            WorkLimits::default(),
        );
        assert!(matches!(bad, Err(WorkDirError::InvalidContent(_))));
    }

    #[test]
    fn input_limits_are_enforced() {
        let small = WorkLimits {
            max_files: 1,
            max_total_bytes: 4,
            max_file_bytes: 3,
        };
        assert_eq!(
            WorkDir::create(&[file("a", b"1"), file("b", b"2")], small).err(),
            Some(WorkDirError::InputsTooLarge)
        );
        assert_eq!(
            WorkDir::create(&[file("a", b"1234")], small).err(),
            Some(WorkDirError::InputsTooLarge)
        );
    }

    #[test]
    fn post_run_limits_are_enforced() {
        let w = WorkDir::create(
            &[],
            WorkLimits {
                max_files: 2,
                max_total_bytes: 10,
                max_file_bytes: 10,
            },
        )
        .unwrap();
        std::fs::write(w.host_path().join("a"), b"12345678901").unwrap();
        assert_eq!(w.changes().err(), Some(WorkDirError::LimitExceeded));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_not_followed_or_reported() {
        let w = WorkDir::create(&[], WorkLimits::default()).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", w.host_path().join("link")).unwrap();
        assert!(w.changes().unwrap().is_empty());
    }

    #[test]
    fn the_folder_is_deleted_on_drop() {
        let w = WorkDir::create(&[file("a", b"1")], WorkLimits::default()).unwrap();
        let p = w.host_path().to_path_buf();
        drop(w);
        assert!(!p.exists());
    }
}
