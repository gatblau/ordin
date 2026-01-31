//! The tree guard of every operation (FormatPlugins.md *Internal Logic* 1) and the limits.

use crate::error::FormatError;
use crate::types::SourceTree;

/// Files a tree may hold (assumed here, *Configuration*).
pub const MAX_TREE_FILES: usize = 20_000;
/// Bytes a tree may hold: 256 MiB.
pub const MAX_TREE_BYTES: u64 = 256 * 1024 * 1024;
/// The Agent Model's per-file limit: 1 MiB.
pub const MAX_FILE_BYTES: usize = 1_048_576;

/// A path that is empty, absolute, holds a `..` segment, a `\` or a NUL.
pub(crate) fn path_is_invalid(path: &str) -> bool {
    path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.split('/').any(|segment| segment == "..")
}

/// Step 1: the size first, then every path. Logs nothing.
pub(crate) fn guard(tree: &SourceTree) -> Result<(), FormatError> {
    let files = tree.0.len();
    let bytes: u64 = tree.0.values().map(|b| b.len() as u64).sum();
    if files > MAX_TREE_FILES || bytes > MAX_TREE_BYTES {
        return Err(FormatError::TreeTooLarge {
            files: u32::try_from(files).unwrap_or(u32::MAX),
            bytes,
        });
    }
    match tree.0.keys().find(|path| path_is_invalid(path)) {
        Some(path) => Err(FormatError::PathInvalid { path: path.clone() }),
        None => Ok(()),
    }
}
