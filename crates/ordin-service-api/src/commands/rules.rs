//! The small checks every `CommandInput::check` is written with. Each refusal carries the code and
//! the JSON Pointer from the envelope root: a field pointer `"/query"` becomes `"/input/query"`, the
//! whole input `""` becomes `"/input"`.
//!
//! Every failure is `SVC_INPUT_INVALID` (this crate refuses before dispatch, so the owner's own code
//! does not apply; *Command input and result types*), except a cursor over 1,024 characters
//! (`SRV_BAD_CURSOR`).

use ordin_types::Cursor;

use crate::api::InputRejection;

pub(crate) const INPUT_INVALID: &str = "SVC_INPUT_INVALID";
pub(crate) const BAD_CURSOR: &str = "SRV_BAD_CURSOR";

/// The longest cursor accepted, in characters (*any `Cursor`* row).
const CURSOR_MAX_CHARS: usize = 1024;

/// `SVC_INPUT_INVALID` at `/input<at>`.
pub(crate) fn reject(at: &str) -> InputRejection {
    InputRejection {
        code: INPUT_INVALID,
        at: format!("/input{at}"),
    }
}

/// `min..=max` Unicode scalar values.
pub(crate) fn text(value: &str, min: usize, max: usize, at: &str) -> Result<(), InputRejection> {
    let count = value.chars().count();
    if count < min || count > max {
        return Err(reject(at));
    }
    Ok(())
}

/// `min..=max` Unicode scalar values when present.
pub(crate) fn opt_text(
    value: &Option<String>,
    min: usize,
    max: usize,
    at: &str,
) -> Result<(), InputRejection> {
    match value {
        Some(v) => text(v, min, max, at),
        None => Ok(()),
    }
}

/// At most `max` bytes of UTF-8 (and at least `min`).
pub(crate) fn bytes(value: &str, min: usize, max: usize, at: &str) -> Result<(), InputRejection> {
    if value.len() < min || value.len() > max {
        return Err(reject(at));
    }
    Ok(())
}

/// `min..=max` items.
pub(crate) fn count<T>(
    items: &[T],
    min: usize,
    max: usize,
    at: &str,
) -> Result<(), InputRejection> {
    if items.len() < min || items.len() > max {
        return Err(reject(at));
    }
    Ok(())
}

/// A number within `min..=max`.
pub(crate) fn range<N: PartialOrd>(
    value: N,
    min: N,
    max: N,
    at: &str,
) -> Result<(), InputRejection> {
    if value < min || value > max {
        return Err(reject(at));
    }
    Ok(())
}

/// A number within `min..=max` when present.
pub(crate) fn opt_range<N: PartialOrd + Copy>(
    value: Option<N>,
    min: N,
    max: N,
    at: &str,
) -> Result<(), InputRejection> {
    match value {
        Some(v) => range(v, min, max, at),
        None => Ok(()),
    }
}

/// No two items equal.
pub(crate) fn unique<T: PartialEq>(items: &[T], at: &str) -> Result<(), InputRejection> {
    for (i, item) in items.iter().enumerate() {
        if items[..i].contains(item) {
            return Err(reject(&format!("{at}/{i}")));
        }
    }
    Ok(())
}

/// Exactly one of two optional keys (for example `workspace_id | scope_id`); refused at `at`.
pub(crate) fn exactly_one<A, B>(
    a: &Option<A>,
    b: &Option<B>,
    at: &str,
) -> Result<(), InputRejection> {
    if a.is_some() == b.is_some() {
        return Err(reject(at));
    }
    Ok(())
}

/// A cursor of at most 1,024 characters, else `SRV_BAD_CURSOR`.
pub(crate) fn cursor(value: &Option<Cursor>, at: &str) -> Result<(), InputRejection> {
    let Some(c) = value else {
        return Ok(());
    };
    // `Cursor` keeps its text private; its serde form is the text itself.
    let chars = match serde_json::to_value(c) {
        Ok(serde_json::Value::String(s)) => s.chars().count(),
        _ => usize::MAX,
    };
    cursor_len(chars, at)
}

/// A cursor carried as plain text (owners that type it `Option<String>`), at most 1,024 characters.
pub(crate) fn cursor_text(value: &Option<String>, at: &str) -> Result<(), InputRejection> {
    match value {
        Some(s) => cursor_len(s.chars().count(), at),
        None => Ok(()),
    }
}

fn cursor_len(chars: usize, at: &str) -> Result<(), InputRejection> {
    if chars > CURSOR_MAX_CHARS {
        return Err(InputRejection {
            code: BAD_CURSOR,
            at: format!("/input{at}"),
        });
    }
    Ok(())
}

/// An absolute `https` address of at most 2,048 bytes.
pub(crate) fn https_url(value: &str, at: &str) -> Result<(), InputRejection> {
    const CODES: ordin_types::text::AreaCodes = ordin_types::text::AreaCodes {
        name_empty: INPUT_INVALID,
        name_too_long: INPUT_INVALID,
        path_invalid: INPUT_INVALID,
        url_invalid: INPUT_INVALID,
    };
    ordin_types::text::check_https_url(value, false, CODES)
        .map(|_| ())
        .map_err(|_| reject(at))
}

/// A relative path inside a place: 1–1,024 bytes, no `..` segment, no leading `/`.
pub(crate) fn relative_path(value: &str, at: &str) -> Result<(), InputRejection> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('/')
        || value.split(['/', '\\']).any(|segment| segment == "..")
    {
        return Err(reject(at));
    }
    Ok(())
}

/// A file name: 1–255 characters, no path separator.
pub(crate) fn file_name(value: &str, at: &str) -> Result<(), InputRejection> {
    text(value, 1, 255, at)?;
    if value.contains(['/', '\\']) {
        return Err(reject(at));
    }
    Ok(())
}

/// A BCP 47 language tag of 2–35 characters: letters, digits and hyphens, starting with a letter.
pub(crate) fn locale(value: &str, at: &str) -> Result<(), InputRejection> {
    let ok = (2..=35).contains(&value.len())
        && value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value
            .split('-')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric()));
    if !ok {
        return Err(reject(at));
    }
    Ok(())
}

/// A host name of at most 253 characters (labels of letters, digits and hyphens) or an IP literal.
pub(crate) fn host(value: &str, at: &str) -> Result<(), InputRejection> {
    if is_host(value) {
        Ok(())
    } else {
        Err(reject(at))
    }
}

/// True for a host name of at most 253 characters or an IP literal (IPv6 without brackets).
pub(crate) fn is_host(value: &str) -> bool {
    if value.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// An email address of `min..=max` characters with one `@` and text on both sides.
pub(crate) fn email(value: &str, min: usize, max: usize, at: &str) -> Result<(), InputRejection> {
    text(value, min, max, at)?;
    let mut parts = value.split('@');
    let ok = matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty() && !value.contains(char::is_whitespace)
    );
    if !ok {
        return Err(reject(at));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_counts_scalar_values() {
        assert!(text("é", 1, 1, "/a").is_ok());
        assert_eq!(text("", 1, 2, "/a").unwrap_err().at, "/input/a");
        assert_eq!(text("abc", 1, 2, "").unwrap_err().at, "/input");
        assert!(opt_text(&None, 1, 2, "/a").is_ok());
        assert!(opt_text(&Some("abc".into()), 1, 2, "/a").is_err());
    }

    #[test]
    fn counts_ranges_and_uniqueness() {
        assert!(count(&[1, 2], 1, 2, "/x").is_ok());
        assert!(count::<u8>(&[], 1, 2, "/x").is_err());
        assert!(range(5u32, 1, 5, "/n").is_ok());
        assert!(range(6u32, 1, 5, "/n").is_err());
        assert!(opt_range(Some(0u32), 1, 5, "/n").is_err());
        assert!(opt_range::<u32>(None, 1, 5, "/n").is_ok());
        assert_eq!(unique(&["a", "b", "a"], "/c").unwrap_err().at, "/input/c/2");
        assert!(exactly_one(&Some(1), &None::<u8>, "").is_ok());
        assert!(exactly_one(&Some(1), &Some(2), "").is_err());
        assert!(exactly_one(&None::<u8>, &None::<u8>, "").is_err());
        assert!(bytes("ab", 1, 1, "/b").is_err());
    }

    #[test]
    fn cursors_paths_names_and_addresses() {
        let long = Cursor::encode(&"x".repeat(900));
        assert_eq!(cursor(&Some(long), "/cursor").unwrap_err().code, BAD_CURSOR);
        assert!(cursor(&Some(Cursor::encode(&1)), "/cursor").is_ok());
        assert!(cursor(&None, "/cursor").is_ok());
        assert_eq!(
            cursor_text(&Some("x".repeat(1025)), "/c").unwrap_err().code,
            BAD_CURSOR
        );
        assert!(cursor_text(&None, "/c").is_ok());
        assert!(https_url("https://example.org/a", "/url").is_ok());
        assert!(https_url("http://example.org", "/url").is_err());
        assert!(relative_path("a/b.txt", "/p").is_ok());
        for bad in ["", "/a", "a/../b", &"x".repeat(1025)] {
            assert!(relative_path(bad, "/p").is_err());
        }
        assert!(file_name("a.txt", "/f").is_ok());
        assert!(file_name("a/b", "/f").is_err());
        assert!(locale("fr-FR", "/l").is_ok());
        assert!(locale("f", "/l").is_err());
        assert!(locale("fr--FR", "/l").is_err());
        assert!(host("ordin.example.org", "/h").is_ok());
        assert!(host("10.0.0.1", "/h").is_ok());
        assert!(host("::1", "/h").is_ok());
        assert!(host("-bad.example", "/h").is_err());
        assert!(email("a@b.c", 3, 254, "/e").is_ok());
        assert!(email("a.b.c", 3, 254, "/e").is_err());
        assert!(email("a@b@c", 3, 254, "/e").is_err());
    }
}
