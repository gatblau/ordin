//! Front matter and small YAML helpers shared by the plug-ins (FormatPlugins.md *Internal Logic* 5, 10).
//!
//! Front matter is read only through `ordin_types::yaml::read_restricted_node` (R-84): nothing in it
//! is executed, evaluated or followed.

use ordin_types::yaml::{read_restricted_node, YamlNode};

use crate::guard::MAX_FILE_BYTES;

/// The outcome of looking for front matter in a Markdown text.
pub(crate) enum Front {
    /// The text opens with no `---` line, or the block never closes.
    Absent,
    /// Keys in source order.
    Parsed(Vec<(String, YamlNode)>),
    /// The reader refused it: the code of the refusal, or "front matter not a mapping".
    Failed(String),
}

/// The text between a first line `---` and the next line `---`, and the text after that second line.
pub(crate) fn split(text: &str) -> Option<(&str, &str)> {
    let first_end = text.find('\n')?;
    if text[..first_end].trim_end_matches('\r') != "---" {
        return None;
    }
    let rest = &text[first_end + 1..];
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Reads the front matter of `text`. The second value is the Markdown body: the text after the
/// front matter, or the whole text when there is none or it could not be read ("as if the file had
/// no front matter").
pub(crate) fn parse(text: &str) -> (Front, &str) {
    let Some((block, body)) = split(text) else {
        return (Front::Absent, text);
    };
    if block.trim().is_empty() {
        return (Front::Parsed(Vec::new()), body);
    }
    match read_restricted_node(block, MAX_FILE_BYTES) {
        Ok(YamlNode::Map(entries)) => (Front::Parsed(entries), body),
        Ok(_) => (Front::Failed("front matter not a mapping".to_owned()), text),
        Err(error) => (Front::Failed(error.code().to_owned()), text),
    }
}

/// The text of a scalar node.
pub(crate) fn scalar(node: &YamlNode) -> Option<&str> {
    match node {
        YamlNode::Scalar { text, .. } => Some(text.as_str()),
        _ => None,
    }
}

/// The value of `key` in a mapping's entries.
pub(crate) fn lookup<'a>(entries: &'a [(String, YamlNode)], key: &str) -> Option<&'a YamlNode> {
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// A non-empty scalar value of `key`.
pub(crate) fn text_of(entries: &[(String, YamlNode)], key: &str) -> Option<String> {
    lookup(entries, key)
        .and_then(scalar)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// A list of names: a sequence of scalars, or one scalar of comma-separated names.
pub(crate) fn name_list(node: &YamlNode) -> Vec<String> {
    let raw: Vec<&str> = match node {
        YamlNode::Seq(items) => items.iter().filter_map(scalar).collect(),
        YamlNode::Scalar { text, .. } => text.split(',').collect(),
        _ => Vec::new(),
    };
    raw.into_iter()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The slug with `-` replaced by spaces and the first letter capitalised ("compare-quotes" gives
/// "Compare quotes").
pub(crate) fn default_name(slug: &str) -> String {
    let spaced = slug.replace('-', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A YAML scalar for text this crate writes: plain when that reads back as the same string under any
/// YAML 1.2 reader, double-quoted (JSON escaping) otherwise.
pub(crate) fn yaml_scalar(text: &str) -> String {
    let plain = text
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '/' | '-' | '+'))
        && !text.ends_with(' ')
        && text.parse::<f64>().is_err()
        && !["true", "false", "yes", "no", "on", "off", "null"]
            .iter()
            .any(|reserved| reserved.eq_ignore_ascii_case(text));
    if plain {
        text.to_owned()
    } else {
        serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
    }
}

/// The slug of `path` when it is `<dir>/<slug><suffix>` with no further '/'.
pub(crate) fn child_slug<'a>(path: &'a str, dir: &str, suffix: &str) -> Option<&'a str> {
    path.strip_prefix(dir)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.strip_suffix(suffix))
        .filter(|slug| !slug.is_empty() && !slug.contains('/'))
}

/// The final segment of a '/'-separated path.
pub(crate) fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The directory of a path ("" at the root).
pub(crate) fn dir_name(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `dir/name`, or `name` when `dir` is empty.
pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_finds_the_block() {
        assert_eq!(split("---\na: 1\n---\nBody\n"), Some(("a: 1\n", "Body\n")));
        assert_eq!(split("---\r\na: 1\r\n---\r\nB"), Some(("a: 1\r\n", "B")));
        assert_eq!(split("no front matter"), None);
        assert_eq!(split("---\nnever closed\n"), None);
        assert_eq!(split("---"), None);
    }

    #[test]
    fn parse_classifies() {
        assert!(matches!(parse("plain").0, Front::Absent));
        assert!(matches!(parse("---\n---\nB").0, Front::Parsed(ref v) if v.is_empty()));
        assert!(matches!(parse("---\nname: x\n---\nB").0, Front::Parsed(ref v) if v.len() == 1));
        match parse("---\n- a\n---\nB") {
            (Front::Failed(reason), body) => {
                assert_eq!(reason, "front matter not a mapping");
                assert!(body.starts_with("---"));
            }
            _ => panic!("a list is not a mapping"),
        }
        assert!(
            matches!(parse("---\nname: &a x\n---\nB").0, Front::Failed(ref c) if c == "YAML_FEATURE_REFUSED")
        );
    }

    #[test]
    fn scalars_and_names() {
        assert_eq!(default_name("compare-quotes"), "Compare quotes");
        assert_eq!(default_name(""), "");
        assert_eq!(yaml_scalar("1.3.0"), "1.3.0");
        assert_eq!(yaml_scalar("1.10"), "\"1.10\"");
        assert_eq!(yaml_scalar("yes"), "\"yes\"");
        assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
        assert_eq!(yaml_scalar(""), "\"\"");
        let list = YamlNode::Scalar {
            text: "Read, Grep ,".to_owned(),
            quoted: false,
        };
        assert_eq!(name_list(&list), vec!["Read".to_owned(), "Grep".to_owned()]);
        assert!(name_list(&YamlNode::Null).is_empty());
        assert_eq!(file_name("a/b/c.md"), "c.md");
        assert_eq!(dir_name("a/b/c.md"), "a/b");
        assert_eq!(dir_name("c.md"), "");
        assert_eq!(join("", "x"), "x");
        assert_eq!(join("d", "x"), "d/x");
    }
}
