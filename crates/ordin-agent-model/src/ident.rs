//! Part identifiers (AgentModel.md *Identifiers*; the identifier facts of *Shared Context*).

use std::sync::LazyLock;

use regex::Regex;

use ordin_types::domain::agents::PartKind;

use crate::error::AmError;
use crate::kinds::kind_info;

/// The prefix of the URN spelling of an identifier.
pub(crate) const URN_PREFIX: &str = "urn:ordin:";

/// The identifier pattern of references, either spelling.
static IDENTIFIER: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^(@[a-z][a-z0-9-]*(/[A-Za-z0-9][A-Za-z0-9-]*)+|urn:ordin:[a-z][a-z0-9-]*(:[A-Za-z0-9][A-Za-z0-9._-]*)+)$",
    )
    .ok()
});

/// A publisher directory without `@`.
pub(crate) static PUBLISHER: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]*$").ok());

/// One category segment of a URN.
static SEGMENT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*$").ok());

/// True when `pattern` compiled and matches `text`. Every pattern of this crate is a literal that
/// compiles (unit test `patterns_compile`); a pattern that did not would match nothing rather than panic.
pub(crate) fn matches(pattern: &LazyLock<Option<Regex>>, text: &str) -> bool {
    pattern.as_ref().is_some_and(|re| re.is_match(text))
}

/// The most characters a minted slug keeps.
const MAX_MINTED_SLUG: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Urn {
    pub publisher: String,
    pub kind_dir: String,
    pub categories: Vec<String>,
    pub slug: String,
}

impl Urn {
    /// The `urn:ordin:` prefix, then at least three non-empty `:`-separated segments: publisher,
    /// kind directory, any categories, slug.
    pub fn parse(id: &str) -> Option<Urn> {
        let rest = id.strip_prefix(URN_PREFIX)?;
        let segments: Vec<&str> = rest.split(':').collect();
        if segments.len() < 3 || segments.iter().any(|s| s.is_empty()) {
            return None;
        }
        let last = segments.len() - 1;
        Some(Urn {
            publisher: segments[0].to_owned(),
            kind_dir: segments[1].to_owned(),
            categories: segments[2..last].iter().map(|s| (*s).to_owned()).collect(),
            slug: segments[last].to_owned(),
        })
    }

    pub fn to_id(&self) -> String {
        let mut id = format!("{URN_PREFIX}{}:{}", self.publisher, self.kind_dir);
        for category in &self.categories {
            id.push(':');
            id.push_str(category);
        }
        id.push(':');
        id.push_str(&self.slug);
        id
    }
}

/// True when `s` matches the identifier pattern (either spelling).
pub fn is_part_identifier(s: &str) -> bool {
    matches(&IDENTIFIER, s)
}

/// Removes the publisher and the kind directory from an identifier, leaving the categories and the
/// slug in their own separator (`urn:…:default:skills:quality:codegen` gives `quality:codegen`;
/// `@default/skills/quality/codegen` gives `quality/codegen`). Text in neither spelling is returned
/// trimmed and otherwise unchanged.
// SPEC-GAP-4: the function is named as a port, without its rules. The URN grammar of *Shared Context*
// (`<publisher>:<kind-dir>[:<category>]*:<slug>`) is applied: exactly two leading segments are the
// prefix, and an identifier with fewer than three segments keeps its last one.
pub fn strip_kind_dir_prefix(id: &str) -> String {
    let id = id.trim();
    let (rest, separator) = if let Some(rest) = id.strip_prefix(URN_PREFIX) {
        (rest, ':')
    } else if let Some(rest) = id.strip_prefix('@') {
        (rest, '/')
    } else {
        return id.to_owned();
    };
    let segments: Vec<&str> = rest.split(separator).collect();
    let kept = if segments.len() >= 3 {
        &segments[2..]
    } else {
        &segments[segments.len() - 1..]
    };
    kept.join(&separator.to_string())
}

/// Strip the prefix, collapse each run of `[^A-Za-z0-9-]` to `-`, trim `-`.
pub fn slug_from_id(id: &str) -> String {
    collapse(&strip_kind_dir_prefix(id), |c| {
        c.is_ascii_alphanumeric() || c == '-'
    })
    .trim_matches('-')
    .to_owned()
}

/// Replaces each run of characters for which `keep` is false with one `-`.
fn collapse(text: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for c in text.chars() {
        if keep(c) {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    out
}

/// The slug a plain name mints: lowercase, each run outside `[a-z0-9]` becomes `-`, trimmed, at most 64
/// characters.
pub(crate) fn mint_slug(name: &str) -> String {
    let lower = name.to_lowercase();
    let collapsed = collapse(&lower, |c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let trimmed = collapsed.trim_matches('-');
    let cut: String = trimmed.chars().take(MAX_MINTED_SLUG).collect();
    cut.trim_end_matches('-').to_owned()
}

/// Mints `urn:ordin:<publisher>:<kind-dir>[:<category>]:<slug>` from a plain name.
///
/// The kind directory is the table's, with `/` written as `:` (`overlays/brand` gives
/// `overlays:brand`); ControlLoop adds its `control-loops` category.
pub fn mint_identifier(
    kind: PartKind,
    publisher: &str,
    category: Option<&str>,
    name: &str,
) -> Result<String, AmError> {
    if !matches(&PUBLISHER, publisher) {
        return Err(AmError::IdInvalid {
            id: publisher.to_owned(),
        });
    }
    // SystemRole has no kind directory: its identifier is placed beside its workspace by the caller.
    let Some(kind_dir) = kind_info(kind).kind_dir else {
        return Err(AmError::IdInvalid {
            id: name.to_owned(),
        });
    };
    let slug = mint_slug(name);
    if slug.is_empty() {
        return Err(AmError::IdInvalid {
            id: name.to_owned(),
        });
    }
    let mut id = format!("{URN_PREFIX}{publisher}:{}", kind_dir.replace('/', ":"));
    if kind == PartKind::ControlLoop {
        id.push_str(":control-loops");
    }
    if let Some(category) = category {
        if !matches(&SEGMENT, category) {
            return Err(AmError::IdInvalid {
                id: category.to_owned(),
            });
        }
        id.push(':');
        id.push_str(category);
    }
    id.push(':');
    id.push_str(&slug);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_compile() {
        assert!(IDENTIFIER.is_some() && PUBLISHER.is_some() && SEGMENT.is_some());
    }

    #[test]
    fn urn_parse_and_print() {
        let urn = Urn::parse("urn:ordin:default:skills:quality:codegen").unwrap();
        assert_eq!(urn.publisher, "default");
        assert_eq!(urn.kind_dir, "skills");
        assert_eq!(urn.categories, vec!["quality".to_owned()]);
        assert_eq!(urn.slug, "codegen");
        assert_eq!(urn.to_id(), "urn:ordin:default:skills:quality:codegen");
        assert_eq!(Urn::parse("urn:ordin:default:skills"), None);
        assert_eq!(Urn::parse("urn:ordin:default::x"), None);
        assert_eq!(Urn::parse("@default/skills/x"), None);
    }

    #[test]
    fn identifier_pattern_and_slugs() {
        assert!(is_part_identifier(
            "urn:ordin:default:skills:quality:codegen"
        ));
        assert!(is_part_identifier("@default/skills/codegen"));
        assert!(!is_part_identifier("urn:ordin:Default:skills"));
        assert!(!is_part_identifier("skills/codegen"));
        assert_eq!(
            slug_from_id("urn:ordin:default:skills:quality:codegen"),
            "quality-codegen"
        );
        assert_eq!(
            slug_from_id("@default/skills/quality/codegen"),
            "quality-codegen"
        );
        assert_eq!(slug_from_id("urn:ordin:x:y"), "y");
        assert_eq!(slug_from_id("plain text"), "plain-text");
        assert_eq!(strip_kind_dir_prefix("@x/y"), "y");
    }

    #[test]
    fn minting() {
        assert_eq!(
            mint_identifier(
                PartKind::WorkspaceProfile,
                "acme",
                None,
                "Contract reviewer"
            )
            .unwrap(),
            "urn:ordin:acme:workspaces:contract-reviewer"
        );
        assert_eq!(
            mint_identifier(PartKind::StyleOverlay, "acme", None, "Plain  English!").unwrap(),
            "urn:ordin:acme:overlays:style:plain-english"
        );
        assert_eq!(
            mint_identifier(PartKind::ControlLoop, "acme", Some("cspa"), "Loop").unwrap(),
            "urn:ordin:acme:rules:control-loops:cspa:loop"
        );
        let long = "a".repeat(80);
        let minted = mint_identifier(PartKind::Rule, "acme", None, &long).unwrap();
        assert_eq!(minted.rsplit(':').next().unwrap().len(), 64);
        for (publisher, category, name) in [
            ("Acme", None, "x"),
            ("acme", None, "!!!"),
            ("acme", Some("bad category"), "x"),
        ] {
            assert_eq!(
                mint_identifier(PartKind::Rule, publisher, category, name)
                    .unwrap_err()
                    .code(),
                "AM_ID_INVALID"
            );
        }
        assert!(mint_identifier(PartKind::SystemRole, "acme", None, "x").is_err());
    }
}
