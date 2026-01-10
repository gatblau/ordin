//! The withdrawn-list file `withdrawn.yaml` (AgentModel.md *The withdrawn-list file*, *Internal
//! Logic* 13). This crate reads and writes the file's shape; value rules (`WDL_*`) belong to the
//! callers. Keys keep camelCase (SA-RECON-05).

use std::collections::BTreeMap;

use ordin_types::domain::agents::PartKind;

use crate::error::AmError;
use crate::kinds::{kind_from_name, kind_name};
use crate::render::{emit_document, Node};
use crate::value::{read_value, ReadFailure, YamlValue, MAX_FILE_BYTES};
use crate::WITHDRAWN_LIST_SCHEMA;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnListFile {
    /// Always WITHDRAWN_LIST_SCHEMA when parsed successfully.
    pub list_schema: String,
    /// Oldest first, as in the file.
    pub entries: Vec<WithdrawnFileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnFileEntry {
    pub part: WithdrawnFilePart,
    pub reason: String,
    /// RFC 3339 text as written; the caller validates it (`WDL_*`).
    pub withdrawn_at: String,
    pub replacement: Option<WithdrawnFileReplacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnFilePart {
    pub kind: PartKind,
    pub id: String,
    pub version: Option<String>,
    /// 64 lowercase hex characters, written without the `sha256:` prefix (08).
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnFileReplacement {
    pub id: String,
    pub version: Option<String>,
}

fn invalid(reason: &'static str, entry: Option<u32>) -> AmError {
    AmError::WithdrawnListInvalid { reason, entry }
}

type Map = BTreeMap<String, YamlValue>;

/// The checks of one entry's map: every key known, every value of its shape.
struct EntryReader {
    n: u32,
}

impl EntryReader {
    fn shape(&self) -> AmError {
        invalid("entry_shape", Some(self.n))
    }

    fn map<'a>(&self, value: &'a YamlValue, known: &[&str]) -> Result<&'a Map, AmError> {
        let map = value.as_map().ok_or_else(|| self.shape())?;
        // SPEC-GAP-13: "an unknown key in an entry" is applied to the entry and to its `part` and
        // `replacement` maps, so no key is dropped silently on a read and render.
        if map.keys().any(|key| !known.contains(&key.as_str())) {
            return Err(invalid("entry_unknown_key", Some(self.n)));
        }
        Ok(map)
    }

    fn string(&self, map: &Map, key: &str) -> Result<String, AmError> {
        map.get(key)
            .and_then(YamlValue::as_str)
            .map(str::to_owned)
            .ok_or_else(|| self.shape())
    }

    fn opt_string(&self, map: &Map, key: &str) -> Result<Option<String>, AmError> {
        match map.get(key) {
            None => Ok(None),
            Some(YamlValue::Str(text)) => Ok(Some(text.clone())),
            Some(_) => Err(self.shape()),
        }
    }

    fn entry(&self, value: &YamlValue) -> Result<WithdrawnFileEntry, AmError> {
        let map = self.map(value, &["part", "reason", "withdrawnAt", "replacement"])?;
        let part = self.map(
            map.get("part").ok_or_else(|| self.shape())?,
            &["kind", "id", "version", "contentHash"],
        )?;
        let kind = part
            .get("kind")
            .and_then(YamlValue::as_str)
            .and_then(kind_from_name)
            .ok_or_else(|| self.shape())?;
        let replacement = match map.get("replacement") {
            None => None,
            Some(value) => {
                let replacement = self.map(value, &["id", "version"])?;
                Some(WithdrawnFileReplacement {
                    id: self.string(replacement, "id")?,
                    version: self.opt_string(replacement, "version")?,
                })
            }
        };
        Ok(WithdrawnFileEntry {
            part: WithdrawnFilePart {
                kind,
                id: self.string(part, "id")?,
                version: self.opt_string(part, "version")?,
                content_hash: self.opt_string(part, "contentHash")?,
            },
            reason: self.string(map, "reason")?,
            withdrawn_at: self.string(map, "withdrawnAt")?,
            replacement,
        })
    }
}

/// Parses `withdrawn.yaml` with the manifest YAML rules (anchors, aliases and custom tags refused;
/// 1 MiB limit). Called by: Catalogue Service, Checker.
pub fn parse_withdrawn_list(bytes: &[u8]) -> Result<WithdrawnListFile, AmError> {
    const PATH: &str = "withdrawn.yaml";
    if bytes.len() > MAX_FILE_BYTES {
        return Err(AmError::FileTooLarge {
            path: PATH.to_owned(),
            bytes: bytes.len() as u64,
        });
    }
    let root = read_value(bytes).map_err(|failure| match failure {
        ReadFailure::Yaml(error) => match error.code() {
            "YAML_FEATURE_REFUSED" => AmError::YamlFeatureRefused {
                path: PATH.to_owned(),
                feature: "anchor, alias or custom tag",
            },
            "YAML_TOO_LARGE" => AmError::FileTooLarge {
                path: PATH.to_owned(),
                bytes: bytes.len() as u64,
            },
            _ => AmError::ManifestMalformed {
                path: PATH.to_owned(),
                line: None,
            },
        },
        ReadFailure::NotUtf8 | ReadFailure::DuplicateKey => AmError::ManifestMalformed {
            path: PATH.to_owned(),
            line: None,
        },
    })?;
    let map = root.as_map().ok_or(invalid("not_a_mapping", None))?;
    if map.get("listSchema").and_then(YamlValue::as_str) != Some(WITHDRAWN_LIST_SCHEMA) {
        return Err(invalid("schema", None));
    }
    let items = match map.get("entries") {
        None => &[][..],
        Some(value) => value.as_seq().ok_or(invalid("entries_not_a_list", None))?,
    };
    let entries = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            EntryReader {
                n: u32::try_from(i + 1).unwrap_or(u32::MAX),
            }
            .entry(item)
        })
        .collect::<Result<Vec<_>, AmError>>()?;
    Ok(WithdrawnListFile {
        list_schema: WITHDRAWN_LIST_SCHEMA.to_owned(),
        entries,
    })
}

fn entry_node(entry: &WithdrawnFileEntry) -> Node {
    let mut part = vec![
        ("kind".to_owned(), Node::str(kind_name(entry.part.kind))),
        ("id".to_owned(), Node::str(&entry.part.id)),
    ];
    if let Some(version) = &entry.part.version {
        part.push(("version".to_owned(), Node::str(version)));
    }
    if let Some(hash) = &entry.part.content_hash {
        part.push(("contentHash".to_owned(), Node::str(hash)));
    }
    let mut entries = vec![
        ("part".to_owned(), Node::Map(part)),
        ("reason".to_owned(), Node::str(&entry.reason)),
        ("withdrawnAt".to_owned(), Node::str(&entry.withdrawn_at)),
    ];
    if let Some(replacement) = &entry.replacement {
        let mut map = vec![("id".to_owned(), Node::str(&replacement.id))];
        if let Some(version) = &replacement.version {
            map.push(("version".to_owned(), Node::str(version)));
        }
        entries.push(("replacement".to_owned(), Node::Map(map)));
    }
    Node::Map(entries)
}

/// Deterministic YAML through the fixed emitter. Called by: Authoring Service (withdrawal draft).
pub fn render_withdrawn_list(list: &WithdrawnListFile) -> String {
    emit_document(&[
        ("listSchema".to_owned(), Node::str(&list.list_schema)),
        (
            "entries".to_owned(),
            Node::Seq(list.entries.iter().map(entry_node).collect()),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(bytes: &[u8]) -> (&'static str, Option<u32>) {
        match parse_withdrawn_list(bytes).unwrap_err() {
            AmError::WithdrawnListInvalid { reason, entry } => (reason, entry),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn refusals() {
        assert_eq!(reason(b"- a\n"), ("not_a_mapping", None));
        assert_eq!(reason(b"entries: []\n"), ("schema", None));
        assert_eq!(
            reason(b"listSchema: ordin.withdrawn/v1\nentries: x\n"),
            ("entries_not_a_list", None)
        );
        let entry = "listSchema: ordin.withdrawn/v1\nentries:\n  - part: {kind: Skill, id: x}\n    reason: r\n    withdrawnAt: t\n";
        assert!(parse_withdrawn_list(entry.as_bytes()).is_ok());
        for (bad, expected) in [
            ("    extra: 1\n", ("entry_unknown_key", Some(2))),
            (
                "    replacement: {id: y, other: z}\n",
                ("entry_unknown_key", Some(2)),
            ),
            (
                "    replacement: {version: '1'}\n",
                ("entry_shape", Some(2)),
            ),
        ] {
            // A second entry, like the first, with one more line.
            let text = format!(
                "{entry}  - part: {{kind: Skill, id: x}}\n    reason: r\n    withdrawnAt: t\n{bad}"
            );
            assert_eq!(reason(text.as_bytes()), expected, "{bad}");
        }
        for bad_part in [
            "{kind: skill, id: x}",
            "{kind: Skill}",
            "{kind: Skill, id: x, version: 1.0}",
            "x",
        ] {
            let text = format!("listSchema: ordin.withdrawn/v1\nentries:\n  - part: {bad_part}\n    reason: r\n    withdrawnAt: t\n");
            assert_eq!(
                reason(text.as_bytes()),
                ("entry_shape", Some(1)),
                "{bad_part}"
            );
        }
        assert_eq!(
            parse_withdrawn_list(b"listSchema: ordin.withdrawn/v1\n")
                .unwrap()
                .entries,
            vec![]
        );
        assert_eq!(
            parse_withdrawn_list(b"listSchema: !!binary x\n")
                .unwrap_err()
                .code(),
            "AM_YAML_FEATURE_REFUSED"
        );
        assert_eq!(
            parse_withdrawn_list(b"listSchema: [\n").unwrap_err().code(),
            "AM_MANIFEST_MALFORMED"
        );
        let big = vec![b' '; MAX_FILE_BYTES + 1];
        assert_eq!(
            parse_withdrawn_list(&big).unwrap_err().code(),
            "AM_FILE_TOO_LARGE"
        );
    }
}
