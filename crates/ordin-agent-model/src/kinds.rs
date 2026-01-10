//! The 25 kinds and the kind table (AgentModel.md *Kinds and the kind table*, *Internal Logic* 1–2).

use serde::{Deserialize, Serialize};

use ordin_types::domain::agents::PartKind;

use crate::vocabulary::VocabularySet;

/// The reference format's loading modes; serialised as the format spells them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadingMode {
    AlwaysOn,
    OnDemand,
    Event,
    ModeSwitch,
    RegisteredInvocation,
    Structural,
    None,
}

impl LoadingMode {
    /// The authored spelling (`always-on`, `registered-invocation`).
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            LoadingMode::AlwaysOn => "always-on",
            LoadingMode::OnDemand => "on-demand",
            LoadingMode::Event => "event",
            LoadingMode::ModeSwitch => "mode-switch",
            LoadingMode::RegisteredInvocation => "registered-invocation",
            LoadingMode::Structural => "structural",
            LoadingMode::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFamily {
    Constituent,
    Root,
    Ingredient,
    Overlay,
    ControlLoop,
    MetadataOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileConvention {
    /// `<base>/<slug>.md`, YAML front matter then Markdown body.
    MarkdownFile,
    /// `<base>/<slug>/SKILL.md`.
    SkillDirectory,
    /// `<base>/<slug>.yaml`, whole manifest as YAML, no body.
    YamlFile,
    /// `<base>/<slug>/<fixed file name>` (practice-pack.yaml, workspace-profile.yaml).
    YamlInDirectory { file_name: &'static str },
    /// `<base>/control-loop-<slug>.md`.
    ControlLoopFile,
}

impl FileConvention {
    /// True for the conventions whose file is front matter plus a Markdown body.
    pub(crate) fn is_markdown(self) -> bool {
        matches!(
            self,
            FileConvention::MarkdownFile
                | FileConvention::SkillDirectory
                | FileConvention::ControlLoopFile
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KindInfo {
    pub kind: PartKind,
    /// Authored `kind:` spellings that map to this kind (rule sub-variants come from the vocabulary).
    pub authored_spellings: &'static [&'static str],
    /// Directory under `models/@<publisher>/`; None where the table has no directory (SystemRole).
    pub kind_dir: Option<&'static str>,
    pub family: KindFamily,
    pub default_loading: LoadingMode,
    /// At most one selection per agent (overlay axes and the control-loop family).
    pub exactly_one: bool,
    pub forge_authorable: bool,
    /// Where a part draft writes this kind; None = not written as a part.
    pub file_convention: Option<FileConvention>,
    /// Offered to people (false for Journey-as-agent, AgentIdentity, CrewProfile, BrandOverlay, JurisdictionOverlay).
    pub offered: bool,
    /// Fluent message id of the kind in plain words (11 §5.27 table), e.g. "part-kind-ability".
    pub plain_message_id: &'static str,
}

use FileConvention::{ControlLoopFile, MarkdownFile, SkillDirectory, YamlFile, YamlInDirectory};
use KindFamily::{Constituent, ControlLoop as LoopFamily, Ingredient, MetadataOnly, Overlay, Root};
use LoadingMode::{AlwaysOn, ModeSwitch, OnDemand, RegisteredInvocation, Structural};

/// Builds one row; `offered` and `forge_authorable` are set from the two lists of the shared context.
#[allow(clippy::too_many_arguments)]
const fn row(
    kind: PartKind,
    authored_spellings: &'static [&'static str],
    kind_dir: Option<&'static str>,
    family: KindFamily,
    default_loading: LoadingMode,
    exactly_one: bool,
    forge_authorable: bool,
    file_convention: Option<FileConvention>,
    offered: bool,
    plain_message_id: &'static str,
) -> KindInfo {
    KindInfo {
        kind,
        authored_spellings,
        kind_dir,
        family,
        default_loading,
        exactly_one,
        forge_authorable,
        file_convention,
        offered,
        plain_message_id,
    }
}

// SPEC-GAP-1: the table gives no `family` column. Families are assigned here from the facts the spec
// states: WorkspaceProfile and CrewProfile are composition roots, PracticePack is an ingredient, the
// six overlay kinds are overlays, ControlLoop is its own family, the kinds whose default loading is
// `none` (Journey, Evaluation, Vocabulary, Guideline) are metadata only, and every other kind is a
// constituent.
// SPEC-GAP-2: `KindInfo.kind_dir`'s comment says None for overlays, while the table gives each overlay
// its directory (`overlays/brand`); the table's value is kept, and None is left for SystemRole only.
// SPEC-GAP-3: `all_kinds()` is documented as non-overlay kinds, then overlays, then the control loop,
// while the table lists Evaluation to Memory after the overlays. *Internal Logic* 1 fills the array in
// the table's order, which is the order kept here.
#[rustfmt::skip]
static TABLE: [KindInfo; 25] = [
    row(PartKind::SystemRole, &["system-role"], None, Constituent, AlwaysOn, false, false, Some(MarkdownFile), true, "part-kind-role"),
    row(PartKind::Rule, &["rule"], Some("rules"), Constituent, AlwaysOn, false, true, Some(MarkdownFile), true, "part-kind-rule"),
    row(PartKind::OperatingMode, &["operating-mode"], Some("rules"), Constituent, ModeSwitch, false, false, None, true, "part-kind-working-mode"),
    row(PartKind::Skill, &["skill"], Some("skills"), Constituent, OnDemand, false, true, Some(SkillDirectory), true, "part-kind-ability"),
    row(PartKind::Command, &["command"], Some("commands"), Constituent, RegisteredInvocation, false, false, Some(MarkdownFile), true, "part-kind-task"),
    row(PartKind::ContextResource, &["context-resource"], Some("context-resources"), Constituent, OnDemand, false, false, Some(MarkdownFile), true, "part-kind-reference-material"),
    row(PartKind::ToolBinding, &["tool-binding"], Some("tool-bindings"), Constituent, Structural, false, false, None, true, "part-kind-tool"),
    row(PartKind::AgentIdentity, &["agent-identity"], Some("agent-identities"), Constituent, Structural, false, false, None, false, "part-kind-not-offered"),
    row(PartKind::Capability, &["capability"], Some("capabilities"), Constituent, Structural, false, false, Some(YamlFile), true, "part-kind-group-of-abilities"),
    row(PartKind::WorkspaceProfile, &["workspace-profile"], Some("workspaces"), Root, Structural, false, false, Some(YamlInDirectory { file_name: "workspace-profile.yaml" }), true, "part-kind-agent"),
    row(PartKind::CrewProfile, &["crew-profile"], Some("crews"), Root, Structural, false, false, None, false, "part-kind-not-offered"),
    row(PartKind::Journey, &["journey"], Some("journeys"), MetadataOnly, LoadingMode::None, false, true, Some(YamlFile), false, "part-kind-lifecycle"),
    row(PartKind::PracticePack, &["practice-pack"], Some("practice-packs"), Ingredient, Structural, false, true, Some(YamlInDirectory { file_name: "practice-pack.yaml" }), true, "part-kind-working-practice"),
    row(PartKind::BrandOverlay, &[], Some("overlays/brand"), Overlay, AlwaysOn, false, false, None, false, "part-kind-not-offered"),
    row(PartKind::JurisdictionOverlay, &[], Some("overlays/jurisdiction"), Overlay, AlwaysOn, false, false, None, false, "part-kind-not-offered"),
    row(PartKind::LanguageOverlay, &["language-overlay"], Some("overlays/language"), Overlay, AlwaysOn, true, false, Some(MarkdownFile), true, "part-kind-language"),
    row(PartKind::SourceWorkflowOverlay, &["source-workflow-overlay"], Some("overlays/source-workflow"), Overlay, AlwaysOn, true, false, Some(MarkdownFile), true, "part-kind-code-workflow"),
    row(PartKind::StyleOverlay, &["style-overlay"], Some("overlays/style"), Overlay, AlwaysOn, true, false, Some(MarkdownFile), true, "part-kind-writing-style"),
    row(PartKind::HandoffOverlay, &["handoff-overlay"], Some("overlays/handoff"), Overlay, AlwaysOn, true, false, Some(MarkdownFile), true, "part-kind-hand-over"),
    row(PartKind::Evaluation, &["evaluation", "ci-gate"], Some("evals"), MetadataOnly, LoadingMode::None, false, false, Some(YamlFile), true, "part-kind-rehearsal-checks"),
    row(PartKind::OutputContract, &["output-contract"], Some("output-contracts"), Constituent, AlwaysOn, false, false, Some(YamlFile), true, "part-kind-result-must-hold"),
    row(PartKind::Vocabulary, &["vocabulary"], Some("vocabulary"), MetadataOnly, LoadingMode::None, false, false, Some(YamlFile), true, "part-kind-words"),
    row(PartKind::Guideline, &["guideline"], Some("guidelines"), MetadataOnly, LoadingMode::None, false, true, Some(YamlFile), true, "part-kind-writing-guide"),
    row(PartKind::Memory, &["memory"], Some("memories"), Constituent, AlwaysOn, false, true, Some(YamlFile), true, "part-kind-memory-rules"),
    row(PartKind::ControlLoop, &["control-loop"], Some("rules"), LoopFamily, AlwaysOn, true, true, Some(ControlLoopFile), true, "part-kind-way-of-working"),
];

static ALL_KINDS: [PartKind; 25] = {
    let mut kinds = [PartKind::SystemRole; 25];
    let mut i = 0;
    while i < 25 {
        kinds[i] = TABLE[i].kind;
        i += 1;
    }
    kinds
};

static OVERLAY_ORDER: [PartKind; 6] = [
    PartKind::BrandOverlay,
    PartKind::JurisdictionOverlay,
    PartKind::SourceWorkflowOverlay,
    PartKind::StyleOverlay,
    PartKind::LanguageOverlay,
    PartKind::HandoffOverlay,
];

/// Position of `kind` in `all_kinds()`.
pub(crate) fn kind_index(kind: PartKind) -> usize {
    // The table holds every variant once (unit test `kind_table_shape`), so the search always finds it;
    // the fallback is never reached.
    TABLE.iter().position(|row| row.kind == kind).unwrap_or(0)
}

pub fn kind_info(kind: PartKind) -> &'static KindInfo {
    &TABLE[kind_index(kind)]
}

/// The 25 kinds in the table's order.
pub fn all_kinds() -> &'static [PartKind; 25] {
    &ALL_KINDS
}

/// Overlay axes in composition order: Brand, Jurisdiction, SourceWorkflow, Style, Language, Handoff.
pub fn overlay_order() -> &'static [PartKind; 6] {
    &OVERLAY_ORDER
}

/// Maps an authored `kind:` value; rule sub-variants (`rule-layer`, `rule-orchestrator`) resolve
/// through the `rule-subvariant` vocabulary to `Rule`.
pub fn canonical_kind(authored: &str, vocabulary: &VocabularySet) -> Option<PartKind> {
    let authored = authored.trim();
    TABLE
        .iter()
        .find(|row| row.authored_spellings.contains(&authored))
        .map(|row| row.kind)
        .or_else(|| vocabulary.rule_subvariant(authored))
}

/// The enum spelling of a kind (`WorkspaceProfile`), as `PartKind`'s serde form writes it.
pub(crate) fn kind_name(kind: PartKind) -> &'static str {
    const NAMES: [&str; 25] = [
        "SystemRole",
        "Rule",
        "OperatingMode",
        "Skill",
        "Command",
        "ContextResource",
        "ToolBinding",
        "AgentIdentity",
        "Capability",
        "WorkspaceProfile",
        "CrewProfile",
        "Journey",
        "PracticePack",
        "BrandOverlay",
        "JurisdictionOverlay",
        "LanguageOverlay",
        "SourceWorkflowOverlay",
        "StyleOverlay",
        "HandoffOverlay",
        "Evaluation",
        "OutputContract",
        "Vocabulary",
        "Guideline",
        "Memory",
        "ControlLoop",
    ];
    NAMES[kind_index(kind)]
}

/// The kind whose enum spelling is `name` exactly.
pub(crate) fn kind_from_name(name: &str) -> Option<PartKind> {
    ALL_KINDS
        .iter()
        .copied()
        .find(|kind| kind_name(*kind) == name)
}

/// The kebab-case kind name (`brand-overlay`), the authored kind of a kind the table spells no way.
pub(crate) fn kebab_kind_name(kind: PartKind) -> String {
    let mut out = String::new();
    for (i, c) in kind_name(kind).chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Internal Logic 1: the table's shape.
    #[test]
    fn kind_table_shape() {
        assert_eq!(TABLE.len(), 25);
        for (i, kind) in ALL_KINDS.iter().enumerate() {
            assert_eq!(ALL_KINDS.iter().filter(|k| *k == kind).count(), 1);
            assert_eq!(kind_info(*kind).kind, *kind);
            assert_eq!(kind_index(*kind), i);
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::String(kind_name(*kind).to_owned())
            );
        }
        let mut spellings: Vec<&str> = TABLE
            .iter()
            .flat_map(|row| row.authored_spellings.iter().copied())
            .collect();
        let total = spellings.len();
        spellings.sort_unstable();
        spellings.dedup();
        assert_eq!(spellings.len(), total, "authored spellings are disjoint");

        let forge: Vec<PartKind> = TABLE
            .iter()
            .filter(|r| r.forge_authorable)
            .map(|r| r.kind)
            .collect();
        assert_eq!(
            forge,
            vec![
                PartKind::Rule,
                PartKind::Skill,
                PartKind::Journey,
                PartKind::PracticePack,
                PartKind::Guideline,
                PartKind::Memory,
                PartKind::ControlLoop
            ]
        );
        let one: Vec<PartKind> = TABLE
            .iter()
            .filter(|r| r.exactly_one)
            .map(|r| r.kind)
            .collect();
        assert_eq!(
            one,
            vec![
                PartKind::LanguageOverlay,
                PartKind::SourceWorkflowOverlay,
                PartKind::StyleOverlay,
                PartKind::HandoffOverlay,
                PartKind::ControlLoop
            ]
        );
        let not_offered: Vec<PartKind> = TABLE
            .iter()
            .filter(|r| !r.offered)
            .map(|r| r.kind)
            .collect();
        assert_eq!(
            not_offered,
            vec![
                PartKind::AgentIdentity,
                PartKind::CrewProfile,
                PartKind::Journey,
                PartKind::BrandOverlay,
                PartKind::JurisdictionOverlay
            ]
        );
    }

    #[test]
    fn canonical_kind_maps_spellings() {
        let empty = VocabularySet::default();
        assert_eq!(canonical_kind(" skill ", &empty), Some(PartKind::Skill));
        assert_eq!(
            canonical_kind("ci-gate", &empty),
            Some(PartKind::Evaluation)
        );
        assert_eq!(canonical_kind("brand-overlay", &empty), None);
        assert_eq!(canonical_kind("rule-layer", &empty), None);
        assert_eq!(kebab_kind_name(PartKind::BrandOverlay), "brand-overlay");
        assert_eq!(kind_from_name("ControlLoop"), Some(PartKind::ControlLoop));
        assert_eq!(kind_from_name("control-loop"), None);
        for mode in [
            LoadingMode::AlwaysOn,
            LoadingMode::RegisteredInvocation,
            LoadingMode::None,
        ] {
            assert_eq!(
                serde_json::to_value(mode).unwrap(),
                serde_json::Value::String(mode.as_str().to_owned())
            );
        }
    }
}
