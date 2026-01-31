//! Parts and the reading of a whole repository tree (AgentModel.md *Internal Logic* 4).

use std::collections::{BTreeMap, BTreeSet};

use ordin_types::domain::agents::PartKind;
use ordin_types::num::Sha256Hex;

use crate::archive::{content_digest, ustar_split};
use crate::error::AmError;
use crate::manifest::{parse_manifest, Manifest};
use crate::vocabulary::VocabularySet;

/// The files of one part, keyed by '/'-separated path relative to the part's directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartFiles(pub BTreeMap<String, Vec<u8>>);

/// One part as the ordin format holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPart {
    pub kind: PartKind,
    pub manifest: Manifest,
    /// Repository-relative path of the manifest file (`models/@default/skills/x/SKILL.md`).
    pub manifest_path: String,
    pub files: PartFiles,
    pub content_digest: Sha256Hex,
}

/// Result of reading a whole tree.
#[derive(Debug, Clone, Default)]
pub struct TreeReading {
    pub parts: Vec<ModelPart>,
    /// A manifest that failed to parse: (repository path, error). The part is not offered.
    pub refused: Vec<(String, AmError)>,
    /// Files in a directory holding two manifests and no closer one: owned by no part.
    pub unowned_files: Vec<String>,
}

/// Step 4.1: absolute, a `..` segment, a `\` or a NUL.
pub(crate) fn path_is_invalid(path: &str) -> bool {
    path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path.split('/').any(|segment| segment == "..")
}

/// The directory of a repository path (`""` for a file at the root).
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

/// `path` relative to `dir`.
fn relative<'a>(path: &'a str, dir: &str) -> &'a str {
    if dir.is_empty() {
        path
    } else {
        path.strip_prefix(dir)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(path)
    }
}

pub fn read_tree(files: &BTreeMap<String, Vec<u8>>, vocabulary: &VocabularySet) -> TreeReading {
    let mut reading = TreeReading::default();
    // Steps 4.1–4.2: refuse bad paths, parse every other file.
    let mut manifests: BTreeMap<&str, Manifest> = BTreeMap::new();
    let mut others: Vec<&str> = Vec::new();
    for (path, bytes) in files {
        if path_is_invalid(path) {
            reading
                .refused
                .push((path.clone(), AmError::PathInvalid { path: path.clone() }));
            continue;
        }
        match parse_manifest(path, bytes) {
            Ok(Some(manifest)) => {
                manifests.insert(path, manifest);
            }
            Ok(None) => others.push(path),
            Err(error) => reading.refused.push((path.clone(), error)),
        }
    }
    let mut per_dir: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for path in manifests.keys() {
        per_dir.entry(parent(path)).or_default().push(path);
    }
    // Step 4.3: each other file belongs to the nearest directory holding exactly one manifest.
    let mut owned: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut unowned = BTreeSet::new();
    for path in others {
        let mut dir = parent(path);
        loop {
            match per_dir.get(dir).map(Vec::len) {
                Some(1) => {
                    if let Some(manifest_path) = per_dir.get(dir).and_then(|m| m.first()) {
                        owned.entry(manifest_path).or_default().push(path);
                    }
                    break;
                }
                Some(_) => {
                    unowned.insert(path.to_owned());
                    break;
                }
                None if dir.is_empty() => break,
                None => dir = parent(dir),
            }
        }
    }
    reading.unowned_files = unowned.into_iter().collect();
    // Steps 4.4–4.5: one part per manifest whose kind is known.
    for (manifest_path, manifest) in manifests {
        let Some(kind) = manifest.kind(vocabulary) else {
            reading.refused.push((
                manifest_path.to_owned(),
                AmError::KindUnknown {
                    path: manifest_path.to_owned(),
                    authored: manifest.authored_kind.clone(),
                },
            ));
            continue;
        };
        let dir = parent(manifest_path);
        let mut part_files = BTreeMap::new();
        part_files.insert(
            relative(manifest_path, dir).to_owned(),
            files.get(manifest_path).cloned().unwrap_or_default(),
        );
        for path in owned.get(manifest_path).into_iter().flatten() {
            part_files.insert(
                relative(path, dir).to_owned(),
                files.get(*path).cloned().unwrap_or_default(),
            );
        }
        // Step 5: a name the canonical archive cannot hold refuses the part.
        if let Some(bad) = part_files.keys().find(|name| ustar_split(name).is_none()) {
            let path = if dir.is_empty() {
                bad.clone()
            } else {
                format!("{dir}/{bad}")
            };
            reading
                .refused
                .push((manifest_path.to_owned(), AmError::PathInvalid { path }));
            continue;
        }
        let part_files = PartFiles(part_files);
        reading.parts.push(ModelPart {
            kind,
            content_digest: content_digest(&part_files),
            manifest,
            manifest_path: manifest_path.to_owned(),
            files: part_files,
        });
    }
    // Step 4.6.
    reading
        .parts
        .sort_by(|a, b| a.manifest_path.cmp(&b.manifest_path));
    reading
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULE: &[u8] = b"---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:x\n---\nBody\n";

    fn tree(entries: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(p, b)| ((*p).to_owned(), b.to_vec()))
            .collect()
    }

    #[test]
    fn ownership_walk() {
        let reading = read_tree(
            &tree(&[
                ("m/skills/s/SKILL.md", RULE),
                ("m/skills/s/scripts/run.sh", b"echo"),
                ("m/rules/a.md", RULE),
                ("m/rules/b.md", RULE),
                ("m/rules/deep/notes.txt", b"n"),
                ("README.md", b"# Top\n"),
                ("m/x.md", b"---\nkind: brand-overlay\napiVersion: ai.enterprise/v1\nmetadata: {id: urn:ordin:a:overlays:brand:b}\n---\n"),
                ("/abs.md", RULE),
            ]),
            &VocabularySet::default(),
        );
        assert_eq!(reading.parts.len(), 3);
        let skill = reading
            .parts
            .iter()
            .find(|p| p.manifest_path == "m/skills/s/SKILL.md")
            .unwrap();
        assert_eq!(
            skill.files.0.keys().cloned().collect::<Vec<_>>(),
            vec!["SKILL.md".to_owned(), "scripts/run.sh".to_owned()]
        );
        assert_eq!(
            reading.unowned_files,
            vec!["m/rules/deep/notes.txt".to_owned()]
        );
        let codes: Vec<(&str, &str)> = reading
            .refused
            .iter()
            .map(|(p, e)| (p.as_str(), e.code()))
            .collect();
        assert_eq!(
            codes,
            vec![
                ("/abs.md", "AM_PATH_INVALID"),
                ("m/x.md", "AM_KIND_UNKNOWN")
            ]
        );
    }

    #[test]
    fn unarchivable_name_refuses_the_part() {
        let long = format!("m/s/{}.txt", "n".repeat(120));
        let reading = read_tree(
            &tree(&[("m/s/a.md", RULE), (long.as_str(), b"x")]),
            &VocabularySet::default(),
        );
        assert!(reading.parts.is_empty());
        assert_eq!(
            reading.refused,
            vec![("m/s/a.md".to_owned(), AmError::PathInvalid { path: long })]
        );
        assert!(path_is_invalid("a\\b"));
        assert!(path_is_invalid("a\0b"));
        assert!(path_is_invalid(""));
    }
}
