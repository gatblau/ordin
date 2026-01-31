//! Names, keys and the field rules of B-1 and its amendments: name keys (SA-KLINK-02), file names
//! of titles (SA-KLINK-04, SA-OKF-08), tags and aliases (SA-KLINK-03), page types and descriptions
//! (SA-OKF-01, SA-OKF-02).

use ordin_types::domain::places::KnowledgeSource;
use unicode_normalization::UnicodeNormalization;

use crate::error::KnowledgeError;
use crate::okf::PAGE_TYPES;

pub(crate) const TITLE_MAX: usize = 200;
pub(crate) const BODY_MAX_BYTES: usize = 1_048_576;
pub(crate) const SOURCES_MAX: usize = 100;
pub(crate) const TAG_MAX: usize = 64;
pub(crate) const TAGS_MAX: usize = 32;
pub(crate) const ALIAS_MAX: usize = 200;
pub(crate) const ALIASES_MAX: usize = 16;
pub(crate) const TYPE_MAX: usize = 64;
pub(crate) const DESCRIPTION_MAX: usize = 200;
const FILE_NAME_MAX_BYTES: usize = 120;

/// The name key of a title, alias or link target: NFC, lower-cased, white-space runs made one
/// space, both ends trimmed.
// spec: ordin-desktop/components/KnowledgeService#SA-KLINK-02
pub(crate) fn name_key(raw: &str) -> String {
    let nfc: String = raw.nfc().collect();
    let lower = nfc.to_lowercase();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The name key with every character that is not a letter or a digit removed.
pub(crate) fn folded_key(raw: &str) -> String {
    name_key(raw)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// B-1: trimmed, 1–200 characters, no control characters.
pub(crate) fn check_title(raw: &str) -> Result<String, KnowledgeError> {
    let title = raw.trim();
    let n = title.chars().count();
    if n == 0 || n > TITLE_MAX || has_control(title) {
        return Err(KnowledgeError::TitleInvalid);
    }
    Ok(title.to_owned())
}

pub(crate) fn check_body(body: &str) -> Result<(), KnowledgeError> {
    if body.len() > BODY_MAX_BYTES {
        return Err(KnowledgeError::BodyTooLarge);
    }
    Ok(())
}

/// B-1: at most 100 sources; origin `place`, `sharepoint` or `confluence` (and `okf` when read
/// from a file, SA-OKF-05); location 1–2048 characters; version read 1–256 characters.
pub(crate) fn check_sources(
    sources: &[KnowledgeSource],
    from_file: bool,
) -> Result<(), KnowledgeError> {
    if sources.len() > SOURCES_MAX {
        return Err(KnowledgeError::SourcesTooMany);
    }
    for s in sources {
        let origin_ok = matches!(s.origin.as_str(), "place" | "sharepoint" | "confluence")
            || (from_file && s.origin == "okf");
        let location = s.location.chars().count();
        let version = s.version_read.chars().count();
        if !origin_ok
            || !(1..=2048).contains(&location)
            || !(1..=256).contains(&version)
            || (s.origin == "place" && place_location(&s.location).is_none())
        {
            return Err(KnowledgeError::SourceInvalid);
        }
    }
    Ok(())
}

/// `place:<place_id>:<relative path>` → (place id text, relative path).
pub(crate) fn place_location(location: &str) -> Option<(ordin_types::PlaceId, String)> {
    let rest = location.strip_prefix("place:")?;
    let (id, path) = rest.split_once(':')?;
    let place = ordin_types::PlaceId::parse(id).ok()?;
    if path.is_empty() {
        return None;
    }
    Some((place, path.to_owned()))
}

/// One tag's value: a leading `#` dropped, 1–64 characters, no white space.
fn tag_value(raw: &str) -> Option<String> {
    let tag = raw.strip_prefix('#').unwrap_or(raw);
    let n = tag.chars().count();
    if n == 0 || n > TAG_MAX || tag.chars().any(char::is_whitespace) || has_control(tag) {
        return None;
    }
    Some(tag.to_owned())
}

fn alias_value(raw: &str) -> Option<String> {
    let alias = raw.trim();
    let n = alias.chars().count();
    if n == 0 || n > ALIAS_MAX || has_control(alias) {
        return None;
    }
    Some(alias.to_owned())
}

/// Through the Service API the rules refuse (SA-KLINK-03).
pub(crate) fn check_tags(tags: &[String]) -> Result<Vec<String>, KnowledgeError> {
    if tags.len() > TAGS_MAX {
        return Err(KnowledgeError::TagInvalid);
    }
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let v = tag_value(t).ok_or(KnowledgeError::TagInvalid)?;
        if !out.iter().any(|o| name_key(o) == name_key(&v)) {
            out.push(v);
        }
    }
    Ok(out)
}

pub(crate) fn check_aliases(aliases: &[String]) -> Result<Vec<String>, KnowledgeError> {
    if aliases.len() > ALIASES_MAX {
        return Err(KnowledgeError::AliasInvalid);
    }
    let mut out: Vec<String> = Vec::new();
    for a in aliases {
        let v = alias_value(a).ok_or(KnowledgeError::AliasInvalid)?;
        if !out.iter().any(|o| name_key(o) == name_key(&v)) {
            out.push(v);
        }
    }
    Ok(out)
}

/// In a file the rules drop: returns the kept values and how many were dropped.
pub(crate) fn file_tags(raw: &[String]) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut dropped = 0;
    for t in raw {
        match tag_value(t) {
            Some(v) if out.len() < TAGS_MAX && !out.iter().any(|o| name_key(o) == name_key(&v)) => {
                out.push(v)
            }
            _ => dropped += 1,
        }
    }
    (out, dropped)
}

pub(crate) fn file_aliases(raw: &[String]) -> (Vec<String>, usize) {
    let mut out: Vec<String> = Vec::new();
    let mut dropped = 0;
    for a in raw {
        match alias_value(a) {
            Some(v)
                if out.len() < ALIASES_MAX && !out.iter().any(|o| name_key(o) == name_key(&v)) =>
            {
                out.push(v)
            }
            _ => dropped += 1,
        }
    }
    (out, dropped)
}

/// SA-OKF-01 through the Service API and `write_page`: one of the five.
pub(crate) fn check_page_type(raw: &str) -> Result<String, KnowledgeError> {
    if PAGE_TYPES.contains(&raw) {
        Ok(raw.to_owned())
    } else {
        Err(KnowledgeError::TypeInvalid)
    }
}

/// A type read from a file: 1–64 characters, no control characters, kept as written.
pub(crate) fn file_page_type(raw: &str) -> Option<String> {
    let n = raw.chars().count();
    ((1..=TYPE_MAX).contains(&n) && !has_control(raw)).then(|| raw.to_owned())
}

/// SA-OKF-02: 1–200 characters, one line, no control characters.
pub(crate) fn check_description(raw: &str) -> Result<String, KnowledgeError> {
    let n = raw.chars().count();
    if n == 0 || n > DESCRIPTION_MAX || has_control(raw) {
        return Err(KnowledgeError::DescriptionInvalid);
    }
    Ok(raw.to_owned())
}

/// A description usable on a page row (read from a file, at most 2,000 characters kept).
pub(crate) fn file_description(raw: &str) -> Option<String> {
    let one_line: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = one_line.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(2_000).collect())
}

const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// The file stem of a title, before the ` (n)` suffix (SA-KLINK-04, SA-OKF-08).
// spec: ordin-desktop/components/KnowledgeService#SA-KLINK-04
pub(crate) fn file_stem_of_title(title: &str) -> String {
    let replaced: String = title
        .chars()
        .map(|c| {
            if matches!(
                c,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']'
            ) || c.is_control()
            {
                '-'
            } else {
                c
            }
        })
        .collect();
    let spaced = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = spaced.trim_matches(|c: char| c == ' ' || c == '.');
    let mut cut = String::new();
    for c in trimmed.chars() {
        if cut.len() + c.len_utf8() > FILE_NAME_MAX_BYTES {
            break;
        }
        cut.push(c);
    }
    let cut = cut.trim_end_matches([' ', '.']).to_owned();
    let mut stem = if cut.is_empty() {
        "page".to_owned()
    } else {
        cut
    };
    let upper = stem.to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&upper.as_str()) {
        stem = format!("{stem} page");
    }
    // spec: ordin-desktop/components/KnowledgeService#SA-OKF-08
    let lower = stem.to_lowercase();
    if lower == "index" || lower == "log" {
        stem = format!("{stem} page");
    }
    stem
}

/// The candidate name for the n-th try: `<stem>.md`, then `<stem> (2).md`, ….
pub(crate) fn numbered_file_name(stem: &str, n: u32) -> String {
    if n <= 1 {
        format!("{stem}.md")
    } else {
        format!("{stem} ({n}).md")
    }
}

/// The stem of a relative path's file name, without `.md` (any case).
pub(crate) fn path_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    strip_md(name).to_owned()
}

pub(crate) fn strip_md(name: &str) -> &str {
    if name.len() >= 3 && name[name.len() - 3..].eq_ignore_ascii_case(".md") {
        &name[..name.len() - 3]
    } else {
        name
    }
}

/// The folder of a relative path (`knowledge/a/b.md` → `knowledge/a`).
pub(crate) fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(f, _)| f).unwrap_or("")
}

/// The path below `knowledge/` without `.md` (`knowledge/a/B.md` → `a/B`).
pub(crate) fn below_knowledge(path: &str) -> String {
    let rest = path.strip_prefix("knowledge/").unwrap_or(path);
    strip_md(rest).to_owned()
}

/// SA-KLINK-06: a page file is a `.md` file under `knowledge/`, at most 4 folder levels below it,
/// with no path segment beginning with `.`.
// spec: ordin-desktop/components/KnowledgeService#SA-KLINK-06
pub(crate) fn is_page_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("knowledge/") else {
        return false;
    };
    if rest.is_empty() || !rest.to_ascii_lowercase().ends_with(".md") {
        return false;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.len() > 5 {
        return false;
    }
    segments
        .iter()
        .all(|s| !s.is_empty() && !s.starts_with('.') && *s != "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_keys_fold_case_and_space() {
        assert_eq!(name_key("  Supplier   Rates "), "supplier rates");
        assert_eq!(folded_key("Supplier-rates"), "supplierrates");
        assert_eq!(folded_key("Supplier rates"), "supplierrates");
    }

    #[test]
    fn file_names_follow_titles() {
        assert_eq!(file_stem_of_title("Supplier rates"), "Supplier rates");
        assert_eq!(file_stem_of_title("a/b: c?"), "a-b- c-");
        assert_eq!(file_stem_of_title("CON"), "CON page");
        assert_eq!(file_stem_of_title("Index"), "Index page");
        assert_eq!(file_stem_of_title(" ... "), "page");
        assert_eq!(numbered_file_name("x", 2), "x (2).md");
        let long = "é".repeat(100);
        assert!(file_stem_of_title(&long).len() <= 120);
    }

    #[test]
    fn page_paths_are_markdown_under_knowledge() {
        assert!(is_page_path("knowledge/a.md"));
        assert!(is_page_path("knowledge/a/b/c/d/e.MD"));
        assert!(!is_page_path("knowledge/a/b/c/d/e/f.md"));
        assert!(!is_page_path("knowledge/.obsidian/app.json"));
        assert!(!is_page_path("knowledge/.hidden/a.md"));
        assert!(!is_page_path("other/a.md"));
    }

    #[test]
    fn tags_and_aliases() {
        assert_eq!(
            check_tags(&["#suppliers".into()]).unwrap(),
            vec!["suppliers"]
        );
        assert!(check_tags(&["supplier rates".into()]).is_err());
        assert!(check_aliases(&["".into()]).is_err());
        let (kept, dropped) = file_tags(&["ok".into(), "bad tag".into()]);
        assert_eq!((kept.len(), dropped), (1, 1));
    }
}
