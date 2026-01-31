//! The committed parity fixtures of `testdata/parity/workspaces/`: a catalogue of parts (written in the
//! ordin format), its withdrawn list, and one folder per case holding a request and the expected result.
//!
//! A missing or empty fixture folder fails the test; it never skips.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordin_agent_model::testkit::{ContractFixture, InMemorySource};
use ordin_agent_model::{
    parse_withdrawn_list, read_tree, ModelPart, PartKind, RepositoryRef, Sha256Hex, Timestamp,
    VocabularySet, WithdrawnListEntry, BUNDLED_REPOSITORY_ID,
};
use ordin_resolver::{
    PackAnswer, Replacement, ResolveError, ResolveRequest, ResolverAndCompiler, ResolverConfig,
};
use ordin_types::WithdrawnListEntryId;
use serde_json::{json, Value};

use super::scope;

pub fn root() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root");
    workspace.join("testdata").join("parity").join("workspaces")
}

/// Every file under `dir`, keyed by its '/'-separated path relative to `dir`.
pub fn read_dir(dir: &Path) -> BTreeMap<String, Vec<u8>> {
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
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

pub struct Catalogue {
    pub parts: Vec<ModelPart>,
    pub withdrawn: Vec<WithdrawnListEntry>,
}

impl Catalogue {
    /// The workspace profiles the catalogue holds, by identifier.
    pub fn workspaces(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .parts
            .iter()
            .filter(|p| p.kind == PartKind::WorkspaceProfile)
            .map(|p| p.manifest.metadata.id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn source(&self) -> InMemorySource {
        InMemorySource::new(&ContractFixture {
            repositories: vec![(
                RepositoryRef::Bundled,
                "bundled".to_owned(),
                self.parts.clone(),
            )],
            withdrawn: self.withdrawn.clone(),
            refused: Vec::new(),
        })
    }
}

pub fn catalogue() -> Catalogue {
    let base = root().join("catalogue");
    assert!(
        base.is_dir(),
        "the parity fixture folder {} is missing",
        base.display()
    );
    let files = read_dir(&base);
    assert!(
        !files.is_empty(),
        "the parity fixture folder {} is empty",
        base.display()
    );
    let tree: BTreeMap<String, Vec<u8>> = files
        .iter()
        .filter(|(p, _)| p.starts_with("models/"))
        .map(|(p, b)| (p.clone(), b.clone()))
        .collect();
    let reading = read_tree(&tree, &VocabularySet::default());
    assert!(reading.refused.is_empty(), "refused: {:?}", reading.refused);
    assert!(
        reading.unowned_files.is_empty(),
        "unowned: {:?}",
        reading.unowned_files
    );
    assert!(!reading.parts.is_empty(), "the catalogue holds no parts");
    let list = parse_withdrawn_list(
        files
            .get("withdrawn.yaml")
            .expect("withdrawn.yaml is committed"),
    )
    .expect("withdrawn.yaml parses");
    let stamp = Timestamp::from_unix_millis(1_768_000_000_000).expect("a timestamp");
    let withdrawn = list
        .entries
        .iter()
        .map(|entry| WithdrawnListEntry {
            id: WithdrawnListEntryId::new(),
            agent_repository_id: BUNDLED_REPOSITORY_ID,
            part_kind: entry.part.kind,
            part_identifier: entry.part.id.clone(),
            part_version: entry.part.version.clone(),
            part_content_digest: entry
                .part
                .content_hash
                .as_deref()
                .map(|h| Sha256Hex::parse(h).expect("a digest")),
            reason: entry.reason.clone(),
            withdrawn_at: stamp,
            replacement_identifier: entry.replacement.as_ref().map(|r| r.id.clone()),
            replacement_version: entry.replacement.as_ref().and_then(|r| r.version.clone()),
            removed_by_hand_seen_at: None,
            read_at: stamp,
        })
        .collect();
    Catalogue {
        parts: reading.parts,
        withdrawn,
    }
}

/// The folder names under `cases/`, sorted.
pub fn case_names() -> Vec<String> {
    let dir = root().join("cases");
    assert!(
        dir.is_dir(),
        "the parity fixture folder {} is missing",
        dir.display()
    );
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the cases folder is readable")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "the parity fixture folder {} is empty",
        dir.display()
    );
    names
}

fn text(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} is text"))
        .to_owned()
}

fn texts(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is a list"))
        .iter()
        .map(|v| v.as_str().expect("text").to_owned())
        .collect()
}

pub fn request_from(json: &Value) -> ResolveRequest {
    let mut request = super::request(&text(json, "root"));
    request.root_version = json["root_version"].as_str().map(str::to_owned);
    request.target_harness =
        serde_json::from_value(json["target_harness"].clone()).expect("a harness");
    request.workspace_languages = texts(json, "workspace_languages");
    request.interface_language = text(json, "interface_language");
    request.pack_answers = json["pack_answers"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|a| PackAnswer {
            rule_index: u32::try_from(a["rule_index"].as_u64().expect("an index")).expect("small"),
            pack_ids: texts(a, "pack_ids"),
        })
        .collect();
    request.pack_changes = texts(json, "pack_changes");
    request.overlay_changes = texts(json, "overlay_changes");
    request.replacements = json["replacements"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|r| Replacement {
            withdrawn_id: text(r, "withdrawn_id"),
            withdrawn_kind: serde_json::from_value(r["withdrawn_kind"].clone()).expect("a kind"),
            replacement_id: text(r, "replacement_id"),
            replacement_version: r["replacement_version"].as_str().map(str::to_owned),
        })
        .collect();
    request.scope = scope();
    request
}

/// What a case produced, in the shape the expected files hold.
pub enum Outcome {
    Built {
        plan: Value,
        lock: Vec<u8>,
        manifest: Vec<u8>,
        files: Value,
    },
    Refused(Value),
}

fn detail(error: &ResolveError) -> Value {
    match error {
        ResolveError::Conflict { conflicts } => json!({
            "slots": conflicts.iter().map(|c| json!({
                "slot": c.slot,
                "candidates": c.candidates.iter().map(|k| json!({
                    "id": k.part.id, "version": k.part.version,
                    "requested_by": k.requested_by.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
        ResolveError::RequiredPackUnsettled { rule_index, class } => {
            json!({ "rule_index": rule_index, "class": class })
        }
        ResolveError::PartWithdrawn { part, replacement } => {
            json!({ "part": part, "replacement": replacement })
        }
        ResolveError::RootNotApproved { id, status } => {
            json!({ "id": id, "status": format!("{status:?}") })
        }
        _ => json!({}),
    }
}

/// Runs one case against the catalogue with the bundle root `bundle_root`.
pub async fn run_case(catalogue: &Catalogue, name: &str, bundle_root: &Path) -> Outcome {
    let path = root().join("cases").join(name).join("request.json");
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()));
    let request = request_from(&serde_json::from_slice(&bytes).expect("request.json is JSON"));
    let resolver = ResolverAndCompiler::new(ResolverConfig {
        bundle_root: bundle_root.to_owned(),
        compiler_version: "0.1.0-fixture".to_owned(),
    })
    .expect("a resolver");
    resolver
        .install_part_source(Arc::new(catalogue.source()))
        .expect("the source binds");
    match resolver.build(&request).await {
        Err(error) => Outcome::Refused(json!({ "code": error.code(), "detail": detail(&error) })),
        Ok(bundle) => {
            let mut files = serde_json::Map::new();
            for part in &bundle.plan.resolved {
                let held = bundle
                    .read_part_files(part)
                    .expect("the bundle holds the part");
                files.insert(
                    format!("{}@{}", part.id, part.version),
                    Value::Array(held.0.keys().map(|k| Value::String(k.clone())).collect()),
                );
            }
            Outcome::Built {
                plan: serde_json::to_value(&bundle.plan).expect("a plan serialises"),
                lock: bundle.lock_file_bytes.clone(),
                manifest: std::fs::read(bundle.dir.join("bundle.manifest.json"))
                    .expect("a manifest"),
                files: Value::Object(files),
            }
        }
    }
}
