//! The request and result types of ResolverAndCompiler.md *Public Interface*.

use std::path::PathBuf;

use ordin_agent_model::{LockFile, PartKind, SourceOverride, SourceScope, SourcedPart, StagedPart};
use ordin_types::domain::HarnessKind;

#[derive(Debug, Clone)]
pub struct ResolverConfig {
    pub bundle_root: PathBuf,
    pub compiler_version: String,
}

#[derive(Debug, Clone)]
pub struct PackAnswer {
    pub rule_index: u32,
    pub pack_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Replacement {
    pub withdrawn_id: String,
    pub withdrawn_kind: PartKind,
    pub replacement_id: String,
    pub replacement_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolveRequest {
    pub scope: SourceScope,
    /// `metadata.id` of the WorkspaceProfile.
    pub root_id: String,
    /// None = highest non-draft version on the followed commit.
    pub root_version: Option<String>,
    /// The harness the bundle is built for (2C `HarnessKind`); written into the plan and the lock file
    /// as its `snake_case` serialisation (`claude_code`, `copilot_cli`).
    pub target_harness: HarnessKind,
    /// Lower-case language names of the workspace's linked projects (Project Service, ADR-024); empty
    /// for a workspace with no code.
    pub workspace_languages: Vec<String>,
    /// BCP 47 tag of the person's interface language (ADR-042), e.g. "en-GB".
    pub interface_language: String,
    pub pack_answers: Vec<PackAnswer>,
    /// Packs the person chose on **Change** (ADR-089), identifiers.
    pub pack_changes: Vec<String>,
    /// Overlays the person chose on **Change** (ADR-090 §5), identifiers.
    pub overlay_changes: Vec<String>,
    pub replacements: Vec<Replacement>,
    pub staged: Vec<StagedPart>,
    /// True only for a draft root (Checker, trial): the root may be `draft` status.
    pub allow_draft_root: bool,
    /// None = the source bound by `install_part_source`; Some = this source for this request only.
    pub source: Option<SourceOverride>,
}

/// `compile_instructions` output: the always-on instructions as one Markdown text, and the lock file of
/// the plan they came from. Nothing is written to disk.
#[derive(Debug, Clone)]
pub struct CompiledInstructions {
    pub agents_md: String,
    pub lock: LockFile,
}

/// Every pack and overlay the agent allows (fixed packs, packs a workspace match could add, every pack
/// and overlay **Change** offers): the space the Checker's conflict check covers.
#[derive(Debug, Clone, Default)]
pub struct PackSpace {
    pub packs: Vec<SourcedPart>,
    pub overlays: Vec<SourcedPart>,
}

#[derive(Debug, Clone, Default)]
pub struct PruneReport {
    pub removed: u32,
    pub kept: u32,
    pub bytes_freed: u64,
}
