//! Relative-path rules, confinement to a place, and the extension tables (FileService.md Internal
//! Logic 4.1, 4.4, 4.5).

use std::path::{Path, PathBuf};

use crate::error::FileError;
use crate::PATH_MAX_BYTES;

/// Internal Logic 4.1: `/`-separated, no leading `/`, no `..` segment, no NUL, at most 4,096
/// bytes. `.` segments and empty segments are dropped, so the result never starts or ends with
/// `/`; the empty string is the place root.
pub(crate) fn normalise_relative(path: &str) -> Result<String, FileError> {
    if path.len() > PATH_MAX_BYTES || path.contains('\0') {
        return Err(FileError::PathEscapesPlace);
    }
    let mut kept: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return Err(FileError::PathEscapesPlace),
            other => kept.push(other),
        }
    }
    Ok(kept.join("/"))
}

/// A path that must name a file: the normalised form must not be the root (`FVER_PATH_INVALID`).
pub(crate) fn normalise_file(path: &str) -> Result<String, FileError> {
    let normal = normalise_relative(path)?;
    if normal.is_empty() {
        return Err(FileError::Invalid("FVER_PATH_INVALID"));
    }
    Ok(normal)
}

/// The folder part of a normalised file path: `""` for the root, else without a trailing `/`.
pub(crate) fn parent_of(normal: &str) -> &str {
    match normal.rfind('/') {
        Some(at) => &normal[..at],
        None => "",
    }
}

pub(crate) fn name_of(normal: &str) -> &str {
    match normal.rfind('/') {
        Some(at) => &normal[at + 1..],
        None => normal,
    }
}

/// The lower-case extension without the dot; empty when there is none.
pub(crate) fn kind_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default()
}

/// Resolves symbolic links in the part of `path` that exists and appends the rest unchanged;
/// `None` when no ancestor can be resolved. The resolved part is in its plain form, so a place root
/// stored on Windows reads `C:\...`, never `\\?\C:\...`, and every path compared with it agrees.
// spec: ordin-desktop/components/FileService#FS-PATH-1
fn resolve_existing_part(path: &Path) -> Option<PathBuf> {
    let mut existing: &Path = path;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        match std::fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(_) => match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    tail.push(name);
                    existing = parent;
                }
                _ => return None,
            },
        }
    }
    let mut out = ordin_types::paths::plain_form(std::fs::canonicalize(existing).ok()?);
    for part in tail.iter().rev() {
        out.push(part);
    }
    Some(out)
}

/// The canonical form of `path`, or `path` itself when nothing of it can be resolved.
pub(crate) fn canonicalise_lenient(path: &Path) -> PathBuf {
    resolve_existing_part(path).unwrap_or_else(|| path.to_path_buf())
}

/// The canonical form of an absolute path, or `None` when it cannot be known. `..` is refused rather
/// than resolved: lexically and through a symbolic link it can name different files, and a harness
/// may take either.
pub(crate) fn canonicalise_strictly(path: &Path) -> Option<PathBuf> {
    let plain = path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir));
    plain.then(|| resolve_existing_part(path)).flatten()
}

/// The absolute location of a normalised relative path under a place root, after resolving
/// symbolic links in the part that exists; the result must lie under the (canonical) root.
pub(crate) fn confine(root: &Path, normal: &str) -> Result<PathBuf, FileError> {
    let joined = if normal.is_empty() {
        root.to_path_buf()
    } else {
        root.join(normal)
    };
    let resolved = resolve_existing_part(&joined).ok_or(FileError::PathEscapesPlace)?;
    if !resolved.starts_with(root) {
        return Err(FileError::PathEscapesPlace);
    }
    Ok(resolved)
}

/// The `/`-separated relative form of `absolute` under `root`, or `None` when it is outside.
pub(crate) fn relative_to(root: &Path, absolute: &Path) -> Option<String> {
    let rel = absolute.strip_prefix(root).ok()?;
    let mut parts: Vec<String> = Vec::new();
    for component in rel.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Text-like kinds the index reads as UTF-8 (Internal Logic 4.5 and 5.4); the source kinds are the
/// ones the index lists as text.
pub(crate) fn is_text_kind(kind: &str) -> bool {
    matches!(
        kind,
        "txt"
            | "md"
            | "markdown"
            | "csv"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "log"
            | "rs"
            | "go"
            | "py"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "java"
            | "kt"
            | "swift"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "sh"
            | "sql"
            | "html"
            | "css"
            | "svelte"
    )
}

pub(crate) fn is_markdown_kind(kind: &str) -> bool {
    matches!(kind, "md" | "markdown")
}

pub(crate) fn is_office_zip_kind(kind: &str) -> bool {
    matches!(kind, "docx" | "xlsx" | "pptx")
}

/// The IANA type of each extension the index knows (Internal Logic 4.4, chosen there).
pub fn media_type(kind: &str) -> &'static str {
    match kind {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "xml" => "application/xml",
        "html" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        "ppt" => "application/vnd.ms-powerpoint",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "rtf" => "application/rtf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Office kinds that get a platform thumbnail (Internal Logic 4.5).
pub(crate) fn is_office_kind(kind: &str) -> bool {
    matches!(
        kind,
        "docx" | "xlsx" | "pptx" | "doc" | "xls" | "ppt" | "odt" | "ods" | "odp" | "rtf"
    )
}

/// A name the File Service's own atomic writes use for temporary files; the watcher ignores them.
pub(crate) const TEMP_PREFIX: &str = ".ordin-tmp-";

pub(crate) fn is_temp_name(name: &str) -> bool {
    name.starts_with(TEMP_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_normalised() {
        assert_eq!(normalise_relative("/a//b/./c/").unwrap(), "a/b/c");
        assert_eq!(normalise_relative("").unwrap(), "");
        assert!(matches!(
            normalise_relative("a/../b"),
            Err(FileError::PathEscapesPlace)
        ));
        assert!(matches!(
            normalise_relative("a\0b"),
            Err(FileError::PathEscapesPlace)
        ));
        let long = "a".repeat(PATH_MAX_BYTES + 1);
        assert!(matches!(
            normalise_relative(&long),
            Err(FileError::PathEscapesPlace)
        ));
        assert_eq!(normalise_file("x").unwrap(), "x");
        assert_eq!(normalise_file("/").unwrap_err().code(), "FVER_PATH_INVALID");
    }

    #[test]
    fn name_parent_and_kind() {
        assert_eq!(parent_of("a/b/c.md"), "a/b");
        assert_eq!(parent_of("c.md"), "");
        assert_eq!(name_of("a/b/c.MD"), "c.MD");
        assert_eq!(kind_of("c.MD"), "md");
        assert_eq!(kind_of(".gitignore"), "");
        assert_eq!(kind_of("noext"), "");
        assert_eq!(media_type("pdf"), "application/pdf");
        assert_eq!(media_type("zzz"), "application/octet-stream");
        assert!(is_text_kind("rs") && !is_text_kind("pdf"));
    }

    #[test]
    fn relative_to_a_root() {
        let root = Path::new("/r");
        assert_eq!(relative_to(root, Path::new("/r/a/b")).unwrap(), "a/b");
        assert_eq!(relative_to(root, Path::new("/r")).unwrap(), "");
        assert!(relative_to(root, Path::new("/q/a")).is_none());
    }
}
