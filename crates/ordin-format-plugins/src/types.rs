//! The public value types and the plug-in trait (FormatPlugins.md *Public Interface*).

use std::collections::BTreeMap;

use ordin_agent_model::{ModelPart, PartKind, VocabularySet};
use ordin_types::domain::agents::AgentFormat;
use serde::Serialize;

use crate::error::FormatError;

/// Every format ordin knows. `Ordin` and `Apm` are repository formats (2C `AgentFormat`);
/// `ClaudeCode` and `Copilot` are read-only import formats (ADR-094).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatId {
    Ordin,
    Apm,
    ClaudeCode,
    Copilot,
}

impl FormatId {
    /// Ordin to Ordin, Apm to Apm, the import formats to None.
    pub fn as_repository_format(self) -> Option<AgentFormat> {
        match self {
            FormatId::Ordin => Some(AgentFormat::Ordin),
            FormatId::Apm => Some(AgentFormat::Apm),
            FormatId::ClaudeCode | FormatId::Copilot => None,
        }
    }

    /// True for Ordin and Apm.
    pub fn writable(self) -> bool {
        matches!(self, FormatId::Ordin | FormatId::Apm)
    }

    /// The snake_case spelling used in log lines.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FormatId::Ordin => "ordin",
            FormatId::Apm => "apm",
            FormatId::ClaudeCode => "claude_code",
            FormatId::Copilot => "copilot",
        }
    }
}

/// `AgentFormat::Ordin` gives `FormatId::Ordin`, `AgentFormat::Apm` gives `FormatId::Apm`.
impl From<AgentFormat> for FormatId {
    fn from(format: AgentFormat) -> FormatId {
        match format {
            AgentFormat::Ordin => FormatId::Ordin,
            AgentFormat::Apm => FormatId::Apm,
        }
    }
}

/// An in-memory tree: '/'-separated relative paths to bytes.
#[derive(Debug, Clone, Default)]
pub struct SourceTree(pub BTreeMap<String, Vec<u8>>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Detection {
    pub format: FormatId,
    /// The version the tree declares, or "unversioned".
    pub declared_version: String,
    /// True when `declared_version` is one this plug-in was tested with.
    pub version_tested: bool,
    /// Number of agents found, for "Found 12 agents in APM format" (ADR-086).
    pub agent_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FormatVersions {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
}

/// What a read needs besides the tree.
#[derive(Debug, Clone)]
pub struct ReadRequest {
    /// Publisher segment for identifiers minted on import (`urn:ordin:<publisher>:...`).
    pub publisher: String,
    /// The vocabulary of the target repository (rule sub-variants, status terms).
    pub vocabulary: VocabularySet,
    /// Plain words naming the source for "Imported from [source]" (11 section 5.30).
    pub source_label: String,
}

#[derive(Debug, Clone)]
pub struct ReadOutcome {
    pub parts: Vec<ModelPart>,
    pub mapping_report: MappingReport,
    /// For APM packages: `apm.yml` name and version (02:472); None for other formats.
    pub package: Option<PackageIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PackageIdentity {
    pub name: String,
    pub version: String,
}

/// Anything read that has no Agent Model equivalent (ADR-087).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MappingReport {
    pub format: Option<FormatId>,
    pub items: Vec<MappingItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MappingItem {
    /// Path in the source tree.
    pub source_path: String,
    /// The construct, in the format's own words ("hook", "apm.yml key scripts").
    pub construct: String,
    pub reason: UnmappedReason,
    /// The identifier of the part the item belonged to, when one was created.
    pub part_identifier: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmappedReason {
    /// No Agent Model kind holds it.
    NoEquivalentKind,
    /// A key or field the plug-in does not read.
    UnknownField,
    /// Something that would run code (hooks, scripts); never mapped (ADR-085, R-84).
    Executable,
    /// A file outside the paths the plug-in reads for this format.
    UnrecognisedPath,
    /// The file could not be parsed.
    Unreadable,
}

impl UnmappedReason {
    /// The snake_case spelling: "no_equivalent_kind", "unknown_field", "executable",
    /// "unrecognised_path", "unreadable".
    pub fn code(self) -> &'static str {
        match self {
            UnmappedReason::NoEquivalentKind => "no_equivalent_kind",
            UnmappedReason::UnknownField => "unknown_field",
            UnmappedReason::Executable => "executable",
            UnmappedReason::UnrecognisedPath => "unrecognised_path",
            UnmappedReason::Unreadable => "unreadable",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WriteRequest {
    /// The parts to write: the draft's changed and new parts.
    pub parts: Vec<ModelPart>,
    /// Parts the draft references but that live in another repository (ADR-090 section 2).
    pub borrowed: Vec<String>,
    /// Text bodies of new or changed Markdown parts, keyed by `metadata.id`.
    pub bodies: BTreeMap<String, String>,
    /// For APM: the package name and version to write into `apm.yml`.
    pub package: Option<PackageIdentity>,
    /// The tree as it is now on the target branch (for "starting files", ADR-088).
    pub existing: SourceTree,
    /// Ordin format only: the bundled catalogue's Vocabulary parts, one per axis.
    pub starting_vocabulary: Vec<ModelPart>,
}

#[derive(Debug, Clone)]
pub struct WriteOutcome {
    /// Files to create or replace, repository-relative.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Starting files the target needs that `existing` lacks (ADR-088).
    pub starting_files: BTreeMap<String, Vec<u8>>,
    pub loss_report: LossReport,
}

/// Anything the target format cannot hold (ADR-087).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LossReport {
    pub format: Option<FormatId>,
    pub items: Vec<LossItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LossItem {
    pub part_identifier: String,
    pub kind: PartKind,
    /// The field or whole part that cannot be written ("fixedPracticePacks", "whole part").
    pub what: String,
    pub reason: LossReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LossReason {
    NoPrimitive,
    FieldNotHeld,
    BorrowedPart,
    VersionUntested,
}

impl LossReason {
    /// "no_primitive", "field_not_held", "borrowed_part", "version_untested".
    pub fn code(self) -> &'static str {
        match self {
            LossReason::NoPrimitive => "no_primitive",
            LossReason::FieldNotHeld => "field_not_held",
            LossReason::BorrowedPart => "borrowed_part",
            LossReason::VersionUntested => "version_untested",
        }
    }
}

/// Proposed kind of one imported file (11 section 5.30 "This reads like an ability").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KindProposal {
    pub kind: PartKind,
    pub from_format: FormatId,
    pub rule: &'static str,
}

pub trait FormatPlugin: Send + Sync {
    fn id(&self) -> FormatId;
    fn versions(&self) -> FormatVersions;
    fn detect(&self, tree: &SourceTree) -> Option<Detection>;
    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError>;
    fn write(&self, request: &WriteRequest) -> Result<WriteOutcome, FormatError>;
}
