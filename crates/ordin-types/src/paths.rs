//! The plain form of a resolved folder path (SharedTypes.md, Amendment January 2026, ST-PATH-1).
//!
//! On Windows the operating system's resolution of a path (`std::fs::canonicalize`) returns the
//! verbatim form `\\?\C:\...`. Programs that do not expect it refuse it: the command interpreter
//! starts in `C:\Windows`, and a `file:///C:/...` address never lies under it. `plain_form` turns
//! such a path back into the plain `C:\...` form whenever that form names the same file, so a
//! folder keeps its resolved meaning (links followed, `..` gone) and every program reads it alike.
//! Elsewhere, and for any path the plain form cannot express, the path is returned unchanged.
//! The function is pure: it reads no file system.

use std::path::PathBuf;

/// The longest plain path Windows opens without the verbatim prefix, in UTF-16 units (`MAX_PATH`,
/// counting the terminating NUL).
const MAX_PATH: usize = 260;

/// `path` in its plain form on Windows when it is a verbatim drive path the plain form can express;
/// otherwise `path` unchanged.
// spec: ordin-desktop/components/SharedTypes#ST-PATH-1
pub fn plain_form(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    match path.to_str().and_then(simplified) {
        Some(plain) => PathBuf::from(plain),
        None => path,
    }
}

/// The text of a verbatim drive path without its prefix, or `None` when it is not one or the plain
/// form would name another file or none: a UNC share, a relative drive, a path of `MAX_PATH` or
/// more, or a name Windows reads differently without the prefix.
fn simplified(text: &str) -> Option<&str> {
    let rest = text.strip_prefix(r"\\?\")?;
    let bytes = rest.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return None;
    }
    if rest.encode_utf16().count() >= MAX_PATH {
        return None;
    }
    let names = &rest[3..];
    let names = names.strip_suffix('\\').unwrap_or(names);
    if !names.is_empty() && !names.split('\\').all(plain_name) {
        return None;
    }
    Some(rest)
}

/// Whether one name means the same with and without the verbatim prefix.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '|' | '?' | '*'))
        && !reserved_device(name)
}

/// A device name Windows opens instead of a file of that name, with or without an extension.
fn reserved_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let Some(number) = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    else {
        return false;
    };
    matches!(
        number,
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_drive_path_loses_its_prefix() {
        let cases = [
            (r"\\?\C:\Users\me\project", Some(r"C:\Users\me\project")),
            (r"\\?\d:\", Some(r"d:\")),
            (r"\\?\C:\a b\c.rs", Some(r"C:\a b\c.rs")),
            (r"\\?\C:\a\", Some(r"C:\a\")),
        ];
        for (given, expected) in cases {
            assert_eq!(simplified(given), expected, "{given}");
        }
    }

    #[test]
    fn a_path_the_plain_form_cannot_express_is_kept() {
        let long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH));
        let cases = [
            r"C:\already\plain",
            "/home/me/project",
            r"\\?\UNC\server\share\x",
            r"\\?\C:",
            r"\\?\C:relative",
            r"\\?\C:\a\..\b",
            r"\\?\C:\a\.\b",
            r"\\?\C:\a\\b",
            r"\\?\C:\name.",
            r"\\?\C:\name ",
            r"\\?\C:\a\con",
            r"\\?\C:\a\NUL.txt",
            r"\\?\C:\a\com1",
            r"\\?\C:\a\LPT9.log",
            r"\\?\C:\a\COM¹",
            r"\\?\C:\a\b?c",
            r"\\?\C:\a\b:c",
            "\\\\?\\C:\\a\u{1}b",
            long.as_str(),
        ];
        for given in cases {
            assert_eq!(simplified(given), None, "{given}");
        }
    }

    #[test]
    fn names_near_a_device_name_are_plain() {
        for name in ["console", "com10", "lpt", "auxiliary", "nul-x", "a.con"] {
            assert!(plain_name(name), "{name}");
        }
    }

    #[test]
    fn plain_form_changes_a_path_only_on_windows() {
        let verbatim = PathBuf::from(r"\\?\C:\Users\me");
        let expected = if cfg!(windows) {
            PathBuf::from(r"C:\Users\me")
        } else {
            verbatim.clone()
        };
        assert_eq!(plain_form(verbatim), expected);
        assert_eq!(
            plain_form(PathBuf::from("/home/me")),
            PathBuf::from("/home/me")
        );
    }
}
