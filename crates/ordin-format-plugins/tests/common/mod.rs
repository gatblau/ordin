//! Fixture loading and small builders shared by the test binaries. A missing fixture folder fails the
//! test; it never skips.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ordin_agent_model::{parse_manifest, read_tree, ModelPart, PartKind, VocabularySet};
use ordin_format_plugins::{FormatPlugins, ReadOutcome, ReadRequest, SourceTree, WriteRequest};

pub fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root")
        .join("testdata")
}

fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("fixture folder {} cannot be read: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            walk(base, &path, out);
        } else {
            let relative = path
                .strip_prefix(base)
                .expect("under the base")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.insert(
                relative,
                std::fs::read(&path).expect("a readable fixture file"),
            );
        }
    }
}

/// Every file under `testdata/<relative>`, keyed by its '/'-separated path.
pub fn load_at(relative: &str) -> SourceTree {
    let dir = testdata().join(relative);
    assert!(
        dir.is_dir(),
        "the fixture folder {} is missing",
        dir.display()
    );
    let mut out = BTreeMap::new();
    walk(&dir, &dir, &mut out);
    assert!(
        !out.is_empty(),
        "the fixture folder {} is empty",
        dir.display()
    );
    SourceTree(out)
}

/// A fixture tree of this crate: `testdata/parity/format-plugins/<name>`.
pub fn load(name: &str) -> SourceTree {
    load_at(&format!("parity/format-plugins/{name}"))
}

pub fn tree(entries: &[(&str, &str)]) -> SourceTree {
    SourceTree(
        entries
            .iter()
            .map(|(p, b)| ((*p).to_owned(), b.as_bytes().to_vec()))
            .collect(),
    )
}

pub fn request(publisher: &str, label: &str) -> ReadRequest {
    ReadRequest {
        publisher: publisher.to_owned(),
        vocabulary: VocabularySet::default(),
        source_label: label.to_owned(),
    }
}

/// The vocabulary set of the Vocabulary parts a tree holds.
pub fn vocabulary_of(tree: &SourceTree) -> VocabularySet {
    let reading = read_tree(&tree.0, &VocabularySet::default());
    let vocabularies: Vec<&ModelPart> = reading
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Vocabulary)
        .collect();
    VocabularySet::from_parts(&vocabularies).expect("one vocabulary per axis")
}

pub fn write_request(parts: Vec<ModelPart>) -> WriteRequest {
    WriteRequest {
        parts,
        borrowed: Vec::new(),
        bodies: BTreeMap::new(),
        package: None,
        existing: SourceTree::default(),
        starting_vocabulary: Vec::new(),
    }
}

/// Reads a tree through the ordin plug-in (the fixture's own vocabulary).
pub fn read_ordin(tree: &SourceTree) -> ReadOutcome {
    let plugins = FormatPlugins::builtin();
    let mut request = request("acme", "an ordin tree");
    request.vocabulary = vocabulary_of(tree);
    plugins
        .get(ordin_format_plugins::FormatId::Ordin)
        .read(tree, &request)
        .expect("the ordin tree reads")
}

/// The parts of a small ordin tree given as (path, text).
pub fn parts_of(entries: &[(&str, &str)]) -> Vec<ModelPart> {
    let tree = tree(entries);
    read_ordin(&tree).parts
}

/// Every part's manifest file parses back to the part's manifest and the digest matches: the parts a
/// plug-in creates are consistent with the Agent Model's own reader.
pub fn assert_parts_consistent(parts: &[ModelPart]) {
    for part in parts {
        let name = part.manifest_path.rsplit('/').next().expect("a file name");
        let bytes = part
            .files
            .0
            .get(name)
            .unwrap_or_else(|| panic!("{} holds no manifest file", part.manifest_path));
        let parsed = parse_manifest(&part.manifest_path, bytes)
            .unwrap_or_else(|e| panic!("{} does not parse: {e}", part.manifest_path))
            .unwrap_or_else(|| panic!("{} holds no manifest", part.manifest_path));
        assert_eq!(parsed, part.manifest, "{}", part.manifest_path);
        assert_eq!(
            ordin_agent_model::content_digest(&part.files),
            part.content_digest,
            "{}",
            part.manifest_path
        );
    }
}

pub fn ids(parts: &[ModelPart]) -> Vec<(PartKind, String)> {
    parts
        .iter()
        .map(|p| (p.kind, p.manifest.metadata.id.clone()))
        .collect()
}
