//! Manifests: the parse of one file, the metadata, the references (AgentModel.md *Values, manifests
//! and parts*, *Internal Logic* 3 and 6).

use std::collections::BTreeMap;

use ordin_types::domain::agents::{PartKind, PartStatus};

use crate::error::AmError;
use crate::ident::{is_part_identifier, slug_from_id};
use crate::kinds::canonical_kind;
use crate::value::{read_value, ReadFailure, YamlValue, MAX_FILE_BYTES};
use crate::vocabulary::VocabularySet;
use crate::API_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Audience {
    pub domains: Vec<String>,
    pub roles: Vec<String>,
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseCase {
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub owner: String,
    pub loading: Option<String>,
    pub description: String,
    pub category: String,
    pub class: String,
    pub audience: Audience,
    pub use_cases: Vec<UseCase>,
    /// None = absent in the file (reads as Approved through `status_or_default`).
    pub status: Option<String>,
    pub supersedes: Option<String>,
    /// Metadata keys the format does not type; kept so a lossless write can re-emit them.
    pub extra: BTreeMap<String, YamlValue>,
}

impl Metadata {
    /// Metadata with `id` and every other typed key empty.
    pub(crate) fn with_id(id: &str) -> Metadata {
        Metadata {
            id: id.to_owned(),
            name: String::new(),
            version: String::new(),
            owner: String::new(),
            loading: None,
            description: String::new(),
            category: String::new(),
            class: String::new(),
            audience: Audience::default(),
            use_cases: Vec::new(),
            status: None,
            supersedes: None,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub api_version: String,
    /// The authored `kind:` value, unchanged (`workspace-profile`, `rule-layer`).
    pub authored_kind: String,
    pub metadata: Metadata,
    /// Every top-level key other than apiVersion, kind and metadata.
    pub body: BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRef {
    pub id: String,
    pub relation: String,
}

/// The status vocabulary; absent reads as approved.
pub(crate) fn parse_status(text: &str) -> Option<PartStatus> {
    match text {
        "draft" => Some(PartStatus::Draft),
        "approved" => Some(PartStatus::Approved),
        "deprecated" => Some(PartStatus::Deprecated),
        _ => None,
    }
}

impl Manifest {
    pub fn kind(&self, vocabulary: &VocabularySet) -> Option<PartKind> {
        canonical_kind(&self.authored_kind, vocabulary)
    }

    /// AM_API_VERSION_UNKNOWN unless `api_version == API_VERSION`.
    // SPEC-GAP-5: `AmError::ApiVersionUnknown` carries a `path`, which a `Manifest` does not hold; the
    // manifest's `metadata.id` is written there instead.
    pub fn require_api_version(&self) -> Result<(), AmError> {
        if self.api_version == API_VERSION {
            Ok(())
        } else {
            Err(AmError::ApiVersionUnknown {
                path: self.metadata.id.clone(),
                found: self.api_version.clone(),
            })
        }
    }

    pub fn status_or_default(&self) -> Result<PartStatus, AmError> {
        match &self.metadata.status {
            None => Ok(PartStatus::Approved),
            Some(status) => parse_status(status).ok_or_else(|| AmError::StatusUnknown {
                status: status.clone(),
            }),
        }
    }

    /// `slug_from_id(metadata.id)`.
    pub fn slug(&self) -> String {
        slug_from_id(&self.metadata.id)
    }

    /// Every identifier under a relation key, walking keys in sorted order; in nested maps the
    /// deepest relation key wins.
    pub fn references(&self) -> Vec<ManifestRef> {
        let mut out = Vec::new();
        for (key, value) in &self.body {
            let relation = relation_kind(key).map(|_| key.as_str());
            collect_refs(value, relation, &mut out);
        }
        out
    }
}

fn collect_refs(value: &YamlValue, relation: Option<&str>, out: &mut Vec<ManifestRef>) {
    match value {
        YamlValue::Map(map) => {
            for (key, item) in map {
                let deeper = if relation_kind(key).is_some() {
                    Some(key.as_str())
                } else {
                    relation
                };
                collect_refs(item, deeper, out);
            }
        }
        YamlValue::Seq(items) => {
            for item in items {
                collect_refs(item, relation, out);
            }
        }
        YamlValue::Str(text) => {
            if let Some(relation) = relation {
                let id = text.trim();
                if is_part_identifier(id) {
                    out.push(ManifestRef {
                        id: id.to_owned(),
                        relation: relation.to_owned(),
                    });
                }
            }
        }
        YamlValue::Null | YamlValue::Bool(_) | YamlValue::Int(_) | YamlValue::Decimal(_) => {}
    }
}

/// The kind a relation key points at, None for keys that are not relations.
pub fn relation_kind(relation: &str) -> Option<PartKind> {
    Some(match relation {
        "systemRole" => PartKind::SystemRole,
        "defaultRules" | "rules" | "packMembers" | "addRules" => PartKind::Rule,
        "controlLoop" => PartKind::ControlLoop,
        "allowedOperatingModes" | "defaultMode" | "modes" | "entry" => PartKind::OperatingMode,
        "capabilities" => PartKind::Capability,
        "commands" => PartKind::Command,
        "skills" => PartKind::Skill,
        "toolBindings" | "memoryService" => PartKind::ToolBinding,
        "contextResources" => PartKind::ContextResource,
        "outputContracts" | "outputContract" => PartKind::OutputContract,
        "workspace" => PartKind::WorkspaceProfile,
        "identity" => PartKind::AgentIdentity,
        "memory" => PartKind::Memory,
        _ => return None,
    })
}

/// The lower-cased extension of the last path segment.
fn extension(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// One line without its `\n` and an optional `\r`.
fn trim_eol(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn is_marker_line(line: &[u8]) -> bool {
    line.starts_with(b"apiVersion:") || line.starts_with(b"kind:")
}

fn malformed(path: &str) -> AmError {
    AmError::ManifestMalformed {
        path: path.to_owned(),
        line: None,
    }
}

/// The YAML document of a Markdown file: the lines between the opening `---` and the next line that
/// is exactly `---`. `Ok(None)` when the file holds no manifest.
fn front_matter<'a>(path: &str, bytes: &'a [u8]) -> Result<Option<&'a [u8]>, AmError> {
    if !bytes.starts_with(b"---") {
        return Ok(None);
    }
    let start = match bytes.iter().position(|b| *b == b'\n') {
        Some(at) => at + 1,
        None => return Ok(None),
    };
    let rest = &bytes[start..];
    let mut offset = 0;
    for line in rest.split(|b| *b == b'\n') {
        if trim_eol(line) == b"---" {
            return Ok(Some(&rest[..offset]));
        }
        offset += line.len() + 1;
    }
    if rest
        .split(|b| *b == b'\n')
        .any(|line| is_marker_line(trim_eol(line)))
    {
        Err(malformed(path))
    } else {
        Ok(None)
    }
}

/// Body text of a Markdown part (after the closing `---`), or None for YAML conventions and for a
/// Markdown file with no front matter.
pub fn markdown_body(path: &str, bytes: &[u8]) -> Option<String> {
    if !matches!(extension(path).as_str(), "md" | "markdown") || !bytes.starts_with(b"---") {
        return None;
    }
    let start = bytes.iter().position(|b| *b == b'\n')? + 1;
    let rest = &bytes[start..];
    let mut offset = 0;
    for line in rest.split(|b| *b == b'\n') {
        let end = offset + line.len();
        if trim_eol(line) == b"---" {
            let body_start = (end + 1).min(rest.len());
            return std::str::from_utf8(&rest[body_start..])
                .ok()
                .map(str::to_owned);
        }
        offset = end + 1;
    }
    None
}

/// Text of a scalar a typed string key accepts: a string, or a number kept as its source text.
pub(crate) fn scalar_text(value: &YamlValue) -> Option<String> {
    match value {
        YamlValue::Str(text) | YamlValue::Decimal(text) => Some(text.clone()),
        YamlValue::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

fn shape(key: &str, expected: &'static str) -> AmError {
    AmError::FieldShapeInvalid {
        key: key.to_owned(),
        expected,
    }
}

/// An optional string key: absent or null is None; another non-scalar is a shape error.
fn opt_string(
    map: &BTreeMap<String, YamlValue>,
    key: &str,
    full_key: &str,
) -> Result<Option<String>, AmError> {
    match map.get(key) {
        None | Some(YamlValue::Null) => Ok(None),
        Some(value) => scalar_text(value)
            .map(Some)
            .ok_or_else(|| shape(full_key, "string")),
    }
}

fn string_list(value: &YamlValue, full_key: &str) -> Result<Vec<String>, AmError> {
    match value {
        YamlValue::Null => Ok(Vec::new()),
        YamlValue::Seq(items) => items
            .iter()
            .map(|item| scalar_text(item).ok_or_else(|| shape(full_key, "list of strings")))
            .collect(),
        _ => Err(shape(full_key, "list of strings")),
    }
}

fn read_audience(value: &YamlValue) -> Result<Audience, AmError> {
    let map = match value {
        YamlValue::Null => return Ok(Audience::default()),
        YamlValue::Map(map) => map,
        _ => return Err(shape("metadata.audience", "map")),
    };
    let list = |key: &str| -> Result<Vec<String>, AmError> {
        map.get(key)
            .map(|v| string_list(v, &format!("metadata.audience.{key}")))
            .transpose()
            .map(Option::unwrap_or_default)
    };
    Ok(Audience {
        domains: list("domains")?,
        roles: list("roles")?,
        languages: list("languages")?,
    })
}

fn read_use_cases(value: &YamlValue) -> Result<Vec<UseCase>, AmError> {
    let items = match value {
        YamlValue::Null => return Ok(Vec::new()),
        YamlValue::Seq(items) => items,
        _ => return Err(shape("metadata.useCases", "list of use cases")),
    };
    items
        .iter()
        .map(|item| {
            let map = item
                .as_map()
                .ok_or_else(|| shape("metadata.useCases", "list of use cases"))?;
            Ok(UseCase {
                title: opt_string(map, "title", "metadata.useCases")?.unwrap_or_default(),
                summary: opt_string(map, "summary", "metadata.useCases")?.unwrap_or_default(),
            })
        })
        .collect()
}

/// The typed metadata keys, in the order of `Metadata`'s fields.
pub(crate) const TYPED_METADATA_KEYS: [&str; 12] = [
    "id",
    "name",
    "version",
    "owner",
    "loading",
    "description",
    "category",
    "class",
    "audience",
    "useCases",
    "status",
    "supersedes",
];

/// Step 3.6 for `metadata`: typed keys into `Metadata`, the rest into `extra`. `id` may be empty;
/// the caller reports it missing.
fn read_metadata(map: &BTreeMap<String, YamlValue>) -> Result<Metadata, AmError> {
    let text = |key: &str| -> Result<String, AmError> {
        Ok(opt_string(map, key, &format!("metadata.{key}"))?.unwrap_or_default())
    };
    let optional = |key: &str| opt_string(map, key, &format!("metadata.{key}"));
    let mut metadata = Metadata::with_id(&text("id")?);
    metadata.name = text("name")?;
    metadata.version = text("version")?;
    metadata.owner = text("owner")?;
    metadata.loading = optional("loading")?;
    metadata.description = text("description")?;
    metadata.category = text("category")?;
    metadata.class = text("class")?;
    if let Some(value) = map.get("audience") {
        metadata.audience = read_audience(value)?;
    }
    if let Some(value) = map.get("useCases") {
        metadata.use_cases = read_use_cases(value)?;
    }
    metadata.status = optional("status")?;
    metadata.supersedes = optional("supersedes")?;
    metadata.extra = map
        .iter()
        .filter(|(key, _)| !TYPED_METADATA_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Ok(metadata)
}

/// Reads one file of a parts tree. `Ok(None)` when the file holds no manifest.
pub fn parse_manifest(path: &str, bytes: &[u8]) -> Result<Option<Manifest>, AmError> {
    // Step 3.1: the size, before anything is read.
    if bytes.len() > MAX_FILE_BYTES {
        return Err(AmError::FileTooLarge {
            path: path.to_owned(),
            bytes: bytes.len() as u64,
        });
    }
    // Step 3.2: the document by extension.
    let document = match extension(path).as_str() {
        "md" | "markdown" => match front_matter(path, bytes)? {
            Some(document) => document,
            None => return Ok(None),
        },
        "yaml" | "yml" => bytes,
        _ => return Ok(None),
    };
    // Step 3.3: a document with no marker line is not a manifest.
    if !document
        .split(|b| *b == b'\n')
        .any(|line| is_marker_line(trim_eol(line)))
    {
        return Ok(None);
    }
    // Steps 3.4–3.5: the restricted reader and the scalar conversion.
    let root = read_value(document).map_err(|failure| match failure {
        ReadFailure::Yaml(error) => match error.code() {
            "YAML_FEATURE_REFUSED" => AmError::YamlFeatureRefused {
                path: path.to_owned(),
                feature: "anchor, alias or custom tag",
            },
            "YAML_TOO_LARGE" => AmError::FileTooLarge {
                path: path.to_owned(),
                bytes: bytes.len() as u64,
            },
            _ => malformed(path),
        },
        ReadFailure::NotUtf8 | ReadFailure::DuplicateKey => malformed(path),
    })?;
    let YamlValue::Map(mut top) = root else {
        return Err(malformed(path));
    };
    // Steps 3.6–3.7: the typed parts, then the required ones.
    let mut missing = Vec::new();
    let api_version = opt_string(&top, "apiVersion", "apiVersion")?;
    let authored_kind = opt_string(&top, "kind", "kind")?;
    let metadata = match top.get("metadata") {
        None | Some(YamlValue::Null) => None,
        Some(YamlValue::Map(map)) => Some(read_metadata(map)?),
        Some(_) => return Err(shape("metadata", "map")),
    };
    if api_version.is_none() {
        missing.push("apiVersion");
    }
    if authored_kind.is_none() {
        missing.push("kind");
    }
    if metadata.as_ref().is_none_or(|m| m.id.is_empty()) {
        missing.push("metadata.id");
    }
    let (Some(api_version), Some(authored_kind), Some(metadata)) =
        (api_version, authored_kind, metadata)
    else {
        return Err(AmError::ManifestFieldMissing {
            path: path.to_owned(),
            fields: missing,
        });
    };
    if !missing.is_empty() {
        return Err(AmError::ManifestFieldMissing {
            path: path.to_owned(),
            fields: missing,
        });
    }
    top.remove("apiVersion");
    top.remove("kind");
    top.remove("metadata");
    Ok(Some(Manifest {
        api_version,
        authored_kind,
        metadata,
        body: top,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_manifests() {
        assert_eq!(parse_manifest("a/README.md", b"# Title\n"), Ok(None));
        assert_eq!(parse_manifest("a/x.txt", b"kind: rule\n"), Ok(None));
        assert_eq!(parse_manifest("a/x.md", b"---"), Ok(None));
        assert_eq!(parse_manifest("a/x.md", b"---\ntitle: x\n"), Ok(None));
        assert_eq!(parse_manifest("a/x.yaml", b"title: x\n"), Ok(None));
        assert_eq!(
            parse_manifest("a/x.md", b"---\ntitle: x\n---\nkind: rule\n"),
            Ok(None)
        );
    }

    #[test]
    fn missing_and_shapes() {
        let error = parse_manifest("x.yaml", b"kind: rule\nmetadata:\n  name: x\n").unwrap_err();
        assert_eq!(
            error,
            AmError::ManifestFieldMissing {
                path: "x.yaml".into(),
                fields: vec!["apiVersion", "metadata.id"]
            }
        );
        let error = parse_manifest("x.yaml", b"kind: rule\nmetadata: [1]\n").unwrap_err();
        assert_eq!(error.code(), "AM_FIELD_SHAPE_INVALID");
        let error = parse_manifest(
            "x.yaml",
            b"apiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:x\n  audience:\n    roles: x\n",
        )
        .unwrap_err();
        assert_eq!(
            error,
            AmError::FieldShapeInvalid {
                key: "metadata.audience.roles".into(),
                expected: "list of strings"
            }
        );
        assert_eq!(
            parse_manifest("x.yaml", b"kind: [a]\n").unwrap_err().code(),
            "AM_FIELD_SHAPE_INVALID"
        );
        assert_eq!(
            parse_manifest("x.yaml", b"[a,\nkind: rule]\n")
                .unwrap_err()
                .code(),
            "AM_MANIFEST_MALFORMED"
        );
        assert_eq!(
            parse_manifest("x.yaml", b"kind: rule\n\xff: x\n")
                .unwrap_err()
                .code(),
            "AM_MANIFEST_MALFORMED"
        );
    }

    #[test]
    fn metadata_and_status() {
        let manifest = parse_manifest(
            "x.md",
            b"---\r\napiVersion: ai.enterprise/v1\r\nkind: rule\r\nmetadata:\r\n  id: urn:ordin:a:rules:x\r\n  version: 1.0\r\n  useCases:\r\n    - title: T\r\n      summary: S\r\n  tags: [a]\r\n  status: retired\r\n---\r\nBody\r\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(manifest.metadata.version, "1.0");
        assert_eq!(manifest.metadata.use_cases[0].title, "T");
        assert!(manifest.metadata.extra.contains_key("tags"));
        assert_eq!(
            manifest.status_or_default().unwrap_err(),
            AmError::StatusUnknown {
                status: "retired".into()
            }
        );
        assert_eq!(
            markdown_body("x.md", b"---\r\nkind: x\r\n---\r\nBody\r\n").as_deref(),
            Some("Body\r\n")
        );
        assert_eq!(
            markdown_body("x.md", b"---\nkind: x\n---").as_deref(),
            Some("")
        );
        assert_eq!(markdown_body("x.yaml", b"---\nkind: x\n---\n"), None);
        assert_eq!(markdown_body("x.md", b"---\nkind: x\n"), None);
    }

    #[test]
    fn references_deepest_relation_wins() {
        let manifest = parse_manifest(
            "w.yaml",
            b"apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:a:workspaces:w\nskills:\n  - urn:ordin:a:skills:s\n  - not an id\ncapabilities:\n  - id: urn:ordin:a:capabilities:c\n    commands: [urn:ordin:a:commands:k]\ngovernanceMetadata:\n  styleOverlays: [urn:ordin:a:overlays:style:p]\n",
        )
        .unwrap()
        .unwrap();
        let refs: Vec<(String, String)> = manifest
            .references()
            .into_iter()
            .map(|r| (r.relation, r.id))
            .collect();
        assert_eq!(
            refs,
            vec![
                ("commands".into(), "urn:ordin:a:commands:k".into()),
                ("capabilities".into(), "urn:ordin:a:capabilities:c".into()),
                ("skills".into(), "urn:ordin:a:skills:s".into()),
            ]
        );
        assert_eq!(relation_kind("memoryService"), Some(PartKind::ToolBinding));
        assert_eq!(relation_kind("requiredPracticePacks"), None);
    }
}
