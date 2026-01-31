//! File-system helpers: private directories, link-safe removal and the path rules of 2C (LD-03).

use std::io;
use std::path::Path;

/// Creates `path` and any missing parent with mode `0700` on macOS and Linux. The directory modes
/// do not apply on Windows, where the per-user profile directory's ACL protects the data root
/// (*Security*).
pub(crate) fn make_private_dir_all(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Deletes `path` and everything under it without following symbolic links: a link is removed,
/// its target untouched (step 5). A path that is already gone is not an error.
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if meta.file_type().is_dir() {
        for entry in std::fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        std::fs::remove_dir(path)
    } else {
        // A directory link on Windows is removed with `remove_dir`.
        std::fs::remove_file(path).or_else(|error| std::fs::remove_dir(path).map_err(|_| error))
    }
}

/// 2C shared rule LD-03: an absolute local path of 1–4096 bytes, UTF-8, no NUL.
pub(crate) fn valid_local_path(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    path.is_absolute() && (1..=4096).contains(&text.len()) && !text.contains('\0')
}

/// True when `path` is one of `roots` or lies inside one, compared by path component.
pub(crate) fn is_inside_any(path: &Path, roots: &[&Path]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_path_rules() {
        assert!(valid_local_path(Path::new(if cfg!(windows) {
            "C:\\a"
        } else {
            "/a"
        })));
        assert!(!valid_local_path(Path::new("relative/dir")));
        assert!(!valid_local_path(Path::new("")));
        let long = format!("/{}", "a".repeat(4096));
        assert!(!valid_local_path(Path::new(&long)));
        assert!(!valid_local_path(Path::new("/a\0b")));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_path_is_invalid() {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"/a/\xff"));
        assert!(!valid_local_path(path));
    }

    #[test]
    fn containment_is_by_component() {
        let root = Path::new("/data/ordin");
        assert!(is_inside_any(Path::new("/data/ordin"), &[root]));
        assert!(is_inside_any(Path::new("/data/ordin/agents"), &[root]));
        assert!(!is_inside_any(Path::new("/data/ordin-other"), &[root]));
    }
}
