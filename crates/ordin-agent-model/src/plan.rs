//! Plan, bundle and lock-file types, produced by the Resolver and Compiler (AgentModel.md *Plan,
//! bundle and lock-file types*, *Internal Logic* 9–10).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use ordin_types::domain::agents::PartKind;
use ordin_types::num::DecimalString;

use crate::error::AmError;
use crate::kinds::LoadingMode;
use crate::readers::is_version;
use crate::render::{emit_document, Node};
use crate::value::{read_value, YamlValue};
use crate::LOCK_FILE_SCHEMA;

/// Sorted-key, no-whitespace JSON: the one definition in the workspace (Logging R-3).
pub use ordin_types::encoding::canonical_json;

/// A part as one plan, bundle or lock file names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartRef {
    pub kind: PartKind,
    /// `metadata.id`.
    pub id: String,
    /// slug_from_id(id).
    pub name: String,
    pub version: String,
    /// `sha256:<hex>`.
    pub content_hash: String,
    /// `<host>/<owner>/<name>`, or `bundled` for the catalogue shipped with ordin.
    pub repository: String,
    /// 40 lowercase hex characters; for `bundled`, the ordin release's catalogue commit.
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<LoadingMode>,
}

/// The resolved tool policy, widened by ADR-090 item 1.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedToolPolicy {
    pub forbidden_flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_floor: Option<DecimalString>,
    pub prohibited_verbs: Vec<String>,
    /// ADR-090 item 1: `defaultToolPolicy.requiresHumanApproval` of the agent's capabilities.
    pub requires_human_approval: Vec<String>,
    pub mcp_servers: Vec<McpServerDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub server_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    /// Identifier of the ToolBinding part that declared it.
    pub declared_by: String,
}

/// The pack and overlay choices one plan was built with.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selections {
    pub practice_packs: Vec<SelectedPart>,
    pub overlays: Vec<SelectedPart>,
    /// Required pack rules the maker left for the agent to ask and nobody has answered yet
    /// (decision 64): the bundle is built without them until the person answers.
    pub pending_choices: Vec<PendingPackChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPackChoice {
    /// 0-based index of the rule in `requiredPracticePacks`.
    pub rule_index: u32,
    pub at_least: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Identifiers of the packs the rule allows, sorted; the question's choices (ADR-089).
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPart {
    pub id: String,
    pub chosen_by: ChosenBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChosenBy {
    WorkspaceCode,
    MakerFixed,
    PersonAnswer,
    PersonChange,
    AgentDefault,
    InterfaceLanguage,
}

impl ChosenBy {
    fn as_str(self) -> &'static str {
        match self {
            ChosenBy::WorkspaceCode => "workspace-code",
            ChosenBy::MakerFixed => "maker-fixed",
            ChosenBy::PersonAnswer => "person-answer",
            ChosenBy::PersonChange => "person-change",
            ChosenBy::AgentDefault => "agent-default",
            ChosenBy::InterfaceLanguage => "interface-language",
        }
    }
}

/// The resolution plan, without tenant, inline root and crew fields (not ported, ADR-081).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionPlan {
    /// `sha256:<hex>` of the canonical JSON of this plan with planHash = "" and toolPolicy = None.
    pub plan_hash: String,
    pub root: PartRef,
    /// The serde spelling of 2C `HarnessKind` (`claude_code`, `copilot_cli`), kept as text
    /// because the plan is hashed and written to files (SA-RECON-02).
    pub target_harness: String,
    pub overlay_order: Vec<PartRef>,
    pub practice_packs: Vec<PartRef>,
    /// Sorted by (kind, name, version).
    pub resolved: Vec<PartRef>,
    pub selections: Selections,
    /// One plain warning per deprecated constituent, in `resolved` order.
    pub warnings: Vec<String>,
    /// Digest (`sha256:<hex>`) of the withdrawn entries consulted, sorted by entry id.
    pub withdrawn_snapshot: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_policy: Option<ResolvedToolPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleArtefact {
    pub part: PartRef,
    pub loading: LoadingMode,
}

/// `bundle.manifest.json`, without signing and time fields (ADR-084, R-71: no registry signature).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    /// BUNDLE_SCHEMA.
    pub bundle_schema: String,
    pub plan_hash: String,
    pub composition: PartRef,
    /// Sorted by (kind, name, version).
    pub artefacts: Vec<BundleArtefact>,
    /// Application order.
    pub overlays_applied: Vec<PartRef>,
    pub resolved_tool_policy: ResolvedToolPolicy,
    /// SHA-256 hex of the canonical lock-file bytes.
    pub lockfile_sha256: String,
    /// ordin's release version.
    pub compiler_version: String,
}

/// `ordin.lock`: one entry per part with repository, commit and digest (02:453, 04:42). Keys keep
/// camelCase (SA-RECON-05).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockFile {
    /// LOCK_FILE_SCHEMA.
    pub lockfile_schema: String,
    pub plan_hash: String,
    /// 2C `HarnessKind` spelling (SA-RECON-02).
    pub target_harness: String,
    pub composition: PartRef,
    /// Sorted by (kind, name, version).
    pub artefacts: Vec<PartRef>,
    /// Application order.
    pub overlays_applied: Vec<PartRef>,
    pub selections: Selections,
}

fn part_ref_node(part: &PartRef) -> Node {
    let mut entries = vec![
        (
            "kind".to_owned(),
            Node::str(crate::kinds::kind_name(part.kind)),
        ),
        ("id".to_owned(), Node::str(&part.id)),
        ("name".to_owned(), Node::str(&part.name)),
        ("version".to_owned(), Node::str(&part.version)),
        ("contentHash".to_owned(), Node::str(&part.content_hash)),
        ("repository".to_owned(), Node::str(&part.repository)),
        ("commit".to_owned(), Node::str(&part.commit)),
    ];
    if let Some(loading) = part.loading {
        entries.push(("loading".to_owned(), Node::str(loading.as_str())));
    }
    Node::Map(entries)
}

fn selected_node(selected: &SelectedPart) -> Node {
    Node::Map(vec![
        ("id".to_owned(), Node::str(&selected.id)),
        (
            "chosenBy".to_owned(),
            Node::str(selected.chosen_by.as_str()),
        ),
    ])
}

fn pending_node(pending: &PendingPackChoice) -> Node {
    let mut entries = vec![
        (
            "ruleIndex".to_owned(),
            Node::Int(i64::from(pending.rule_index)),
        ),
        ("atLeast".to_owned(), Node::Int(i64::from(pending.at_least))),
    ];
    if let Some(class) = &pending.class {
        entries.push(("class".to_owned(), Node::str(class)));
    }
    entries.push(("choices".to_owned(), Node::strs(&pending.choices)));
    Node::Map(entries)
}

/// Sorts by (kind, name, version), the order the lock file and the bundle manifest list parts in.
fn by_kind_name_version(a: &PartRef, b: &PartRef) -> Ordering {
    a.kind
        .cmp(&b.kind)
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.version.cmp(&b.version))
}

/// The JSON a `YamlValue` reads as for serde; a decimal stays text, so it fails a numeric field.
fn json_of(value: &YamlValue) -> serde_json::Value {
    use serde_json::Value;
    match value {
        YamlValue::Null => Value::Null,
        YamlValue::Bool(b) => Value::Bool(*b),
        YamlValue::Int(n) => Value::from(*n),
        YamlValue::Decimal(text) | YamlValue::Str(text) => Value::String(text.clone()),
        YamlValue::Seq(items) => Value::Array(items.iter().map(json_of).collect()),
        YamlValue::Map(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), json_of(v))).collect())
        }
    }
}

impl LockFile {
    /// Deterministic YAML: keys in the struct's declared order, `artefacts` sorted by (kind, name,
    /// version), two-space indent, LF, one trailing newline.
    pub fn to_canonical_yaml(&self) -> String {
        let mut artefacts = self.artefacts.clone();
        artefacts.sort_by(by_kind_name_version);
        let selections = Node::Map(vec![
            (
                "practicePacks".to_owned(),
                Node::Seq(
                    self.selections
                        .practice_packs
                        .iter()
                        .map(selected_node)
                        .collect(),
                ),
            ),
            (
                "overlays".to_owned(),
                Node::Seq(self.selections.overlays.iter().map(selected_node).collect()),
            ),
            (
                "pendingChoices".to_owned(),
                Node::Seq(
                    self.selections
                        .pending_choices
                        .iter()
                        .map(pending_node)
                        .collect(),
                ),
            ),
        ]);
        emit_document(&[
            (
                "lockfileSchema".to_owned(),
                Node::str(&self.lockfile_schema),
            ),
            ("planHash".to_owned(), Node::str(&self.plan_hash)),
            ("targetHarness".to_owned(), Node::str(&self.target_harness)),
            ("composition".to_owned(), part_ref_node(&self.composition)),
            (
                "artefacts".to_owned(),
                Node::Seq(artefacts.iter().map(part_ref_node).collect()),
            ),
            (
                "overlaysApplied".to_owned(),
                Node::Seq(self.overlays_applied.iter().map(part_ref_node).collect()),
            ),
            ("selections".to_owned(), selections),
        ])
    }

    pub fn from_yaml(bytes: &[u8]) -> Result<LockFile, AmError> {
        let value = read_value(bytes).map_err(|_| AmError::LockFileInvalid { reason: "yaml" })?;
        let schema = value
            .as_map()
            .and_then(|map| map.get("lockfileSchema"))
            .and_then(YamlValue::as_str);
        if schema != Some(LOCK_FILE_SCHEMA) {
            return Err(AmError::LockFileInvalid { reason: "schema" });
        }
        serde_json::from_value(json_of(&value))
            .map_err(|_| AmError::LockFileInvalid { reason: "key" })
    }
}

/// One pre-release identifier: numeric ones rank below alphanumeric ones.
fn compare_identifier(a: &str, b: &str) -> Ordering {
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    match (numeric(a), numeric(b)) {
        (true, true) => {
            // Compare by length first so numbers of any size compare numerically.
            let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
            a.len().cmp(&b.len()).then_with(|| a.cmp(b))
        }
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

/// Semantic-version precedence (SemVer 2.0.0 §11), for "latest" choices.
pub fn compare_versions(a: &str, b: &str) -> Result<Ordering, AmError> {
    for version in [a, b] {
        if !is_version(version) {
            return Err(AmError::VersionInvalid {
                version: version.to_owned(),
            });
        }
    }
    let split = |v: &str| -> (Vec<String>, Option<String>) {
        let without_build = v.split_once('+').map(|(h, _)| h).unwrap_or(v);
        match without_build.split_once('-') {
            Some((core, pre)) => (
                core.split('.').map(str::to_owned).collect(),
                Some(pre.to_owned()),
            ),
            None => (without_build.split('.').map(str::to_owned).collect(), None),
        }
    };
    let ((core_a, pre_a), (core_b, pre_b)) = (split(a), split(b));
    for (x, y) in core_a.iter().zip(core_b.iter()) {
        let order = compare_identifier(x, y);
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    Ok(match (pre_a, pre_b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let (xs, ys): (Vec<&str>, Vec<&str>) = (x.split('.').collect(), y.split('.').collect());
            xs.iter()
                .zip(ys.iter())
                .map(|(p, q)| compare_identifier(p, q))
                .find(|o| *o != Ordering::Equal)
                .unwrap_or_else(|| xs.len().cmp(&ys.len()))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_precedence() {
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.2.0",
            "1.10.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert_eq!(
                compare_versions(pair[0], pair[1]),
                Ok(Ordering::Less),
                "{pair:?}"
            );
            assert_eq!(compare_versions(pair[1], pair[0]), Ok(Ordering::Greater));
        }
        assert_eq!(compare_versions("1.0.0+a", "1.0.0+b"), Ok(Ordering::Equal));
        assert_eq!(
            compare_versions("1.0", "1.0.0"),
            Err(AmError::VersionInvalid {
                version: "1.0".into()
            })
        );
    }

    #[test]
    fn part_ref_order() {
        let base = PartRef {
            kind: PartKind::Rule,
            id: "b".into(),
            name: "b".into(),
            version: "1.0.0".into(),
            content_hash: String::new(),
            repository: String::new(),
            commit: String::new(),
            loading: None,
        };
        let mut skill = base.clone();
        skill.kind = PartKind::Skill;
        let mut loaded = base.clone();
        loaded.loading = Some(LoadingMode::AlwaysOn);
        assert!(base < skill);
        assert!(base < loaded);
        assert_eq!(base.partial_cmp(&base), Some(Ordering::Equal));
    }

    #[test]
    fn lock_file_refusals() {
        assert_eq!(
            LockFile::from_yaml(b"a: &x 1\n"),
            Err(AmError::LockFileInvalid { reason: "yaml" })
        );
        assert_eq!(
            LockFile::from_yaml(b"lockfileSchema: other/v1\n"),
            Err(AmError::LockFileInvalid { reason: "schema" })
        );
        assert_eq!(
            LockFile::from_yaml(b"lockfileSchema: ordin.lock/v1\nplanHash: x\n"),
            Err(AmError::LockFileInvalid { reason: "key" })
        );
    }
}
