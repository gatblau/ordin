//! Inputs and results of the Catalogue Service (CatalogueService.md *Public Interface*, *Inputs
//! and results*). Inputs that arrive from the Service API carry `deny_unknown_fields`.

use ordin_agent_model::VocabularyAxis;
use ordin_projects::{ReviewComment, ReviewDecision};
use ordin_types::domain::agents::{AgentFormat, AgentRepository, Part, PartKind, PullRequestState};
use ordin_types::paging::Cursor;
use ordin_types::time::Timestamp;
use ordin_types::{AgentRepositoryId, HandoverPlaceId, PartId, VocabularyTermId};
use serde::{Deserialize, Serialize};

pub use ordin_types::domain::handover::HandoverPlaceType;
pub use ordin_types::wire::workflow_file::AgentRepositoryRef;

/// `<app data root>/catalogue` and the program's read-only bundled catalogue.
#[derive(Debug, Clone)]
pub struct CatalogueConfig {
    /// `<app data root>/catalogue`; clones under `clones/`, draft worktrees under `worktrees/`,
    /// probes under `probes/`.
    pub root: std::path::PathBuf,
    /// Read-only folder of the bundled catalogue shipped with the program (the bundled agents'
    /// `models/` at their pinned commit, ADR-082).
    pub bundled_catalogue: std::path::PathBuf,
    /// The ordin version, recorded as the bundled catalogue's commit value.
    pub ordin_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProbeRepositoryInput {
    /// At most 2,048 bytes.
    pub web_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryProbe {
    pub probe_id: uuid::Uuid,
    pub host: String,
    pub repository: String,
    pub suggested_name: String,
    pub format: AgentFormat,
    pub agent_count: u32,
    pub person_can_publish: bool,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AddRepositoryInput {
    pub probe_id: uuid::Uuid,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RenameRepositoryInput {
    pub id: AgentRepositoryId,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepositorySummary {
    pub repository: AgentRepository,
    pub agent_count: u32,
    /// `source = profile`.
    pub set_by_company: bool,
    pub last_fetch: Option<FetchOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FetchOutcome {
    pub agent_repository_id: AgentRepositoryId,
    pub result: FetchResult,
    pub commit: Option<String>,
    pub at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FetchResult {
    Ingested {
        offered: u32,
        not_offered: u32,
    },
    Unchanged,
    /// ADR-084: not published until signed.
    MergeUnsigned,
    /// Fail closed, R-78.
    WithdrawnListUnreadable,
    CommitRefused {
        reason_code: String,
    },
    Unreachable,
    SignInNeeded,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LibraryQuery {
    /// At most 500 characters.
    #[serde(default)]
    pub text: Option<String>,
    /// Empty = WorkspaceProfile only (the agent library).
    #[serde(default)]
    pub kinds: Vec<PartKind>,
    /// Filter "From:".
    #[serde(default)]
    pub repositories: Vec<AgentRepositoryId>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    /// Deprecated parts (curators).
    #[serde(default)]
    pub include_not_for_new: bool,
    #[serde(default)]
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LibraryEntry {
    pub part: PartView,
    /// Repository display name or "ordin".
    pub from: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LibraryFacets {
    pub area: Vec<(String, u32)>,
    pub audience: Vec<(String, u32)>,
    pub publisher: Vec<(String, u32)>,
    pub repository: Vec<(AgentRepositoryId, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MatchRequirementsInput {
    /// 1–50 items, each 1–2000 characters.
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Covered,
    PartlyCovered,
    NotCovered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RequirementMatch {
    pub requirement: String,
    pub coverage: Coverage,
    /// At most 3, best first.
    pub matched: Vec<PartView>,
    /// Present for `partly_covered`.
    pub shortfall: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PartView {
    pub part: Part,
    /// One sentence "what it does".
    pub summary: String,
    pub offered: bool,
    pub state_label: PartStateLabel,
    pub from: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartStateLabel {
    Current,
    NotForNewAgents,
    Withdrawn,
    NotOffered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PartDocument {
    pub view: PartView,
    /// Agent Model fields by kind.
    pub body: serde_json::Value,
    /// Shown only in code workspaces.
    pub file_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsedBy {
    pub part_id: PartId,
    pub agents: Vec<PartView>,
    pub parts: Vec<PartView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MadeOf {
    pub agent: PartView,
    pub groups: Vec<(PartKind, Vec<PartView>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PartHistory {
    pub versions: Vec<PartVersionLine>,
    pub replaces: Vec<PartView>,
    pub replaced_by: Vec<PartView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PartVersionLine {
    pub version: String,
    pub commit: String,
    pub merged_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HealthReport {
    pub agent_repository_id: AgentRepositoryId,
    pub computed_at: Timestamp,
    pub commit: String,
    pub lines: Vec<HealthLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthLine {
    UnusedPart {
        part_id: PartId,
    },
    LookAlike {
        part_id: PartId,
        similar_to: PartId,
    },
    SharedIdentifier {
        identifier: String,
        other_repository_id: AgentRepositoryId,
    },
    BrokenBorrowedReference {
        part_id: PartId,
        reference: String,
    },
    UsedWhileDeprecated {
        part_id: PartId,
        used_by: Vec<PartId>,
    },
    UsedWhileWithdrawn {
        part_id: PartId,
        used_by: Vec<PartId>,
    },
    UncitedTerm {
        vocabulary_term_id: VocabularyTermId,
    },
    /// A journey whose seams loop (ADR-099 item 7).
    JourneyNotOffered {
        part_id: PartId,
    },
    /// ADR-102: a stage's Slack channel the catalogue could not read.
    LifecycleSlackChannelIgnored {
        lifecycle: String,
        stage: String,
    },
    /// ADR-106: a stage whose handover place is not valid; the lifecycle is not offered.
    LifecyclePlaceInvalid {
        lifecycle: String,
        stage: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryLayout {
    pub kinds: Vec<KindLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct KindLine {
    pub kind: PartKind,
    pub count: u32,
    pub directory: String,
    pub readme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReviewerInfo {
    pub readable: bool,
    pub review_required: Option<bool>,
    pub reviewers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PullRequestStatus {
    pub state: PullRequestState,
    pub review: ReviewDecision,
    pub change_requests: Vec<ReviewComment>,
    pub merged_signed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LifecyclePlace {
    pub id: HandoverPlaceId,
    pub place_type: HandoverPlaceType,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DraftWorktreeInput {
    pub agent_repository_id: AgentRepositoryId,
    /// A valid git ref of at most 255 bytes.
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DraftWorktree {
    pub path: std::path::PathBuf,
    pub base_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepositorySyncReport {
    pub results: Vec<RepositorySyncResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepositorySyncResult {
    pub display_name: String,
    pub outcome: RepositorySyncOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositorySyncOutcome {
    Cloned,
    Fetched,
    Repaired,
    /// `code` is a code from the Error Table.
    Failed {
        code: String,
    },
}

#[derive(Debug, Clone)]
pub enum CatalogueNotice {
    RepositoryUpdated {
        agent_repository_id: AgentRepositoryId,
        commit: String,
        newly_withdrawn: Vec<ordin_types::WithdrawnListEntryId>,
    },
    RepositoryDropped {
        agent_repository_id: AgentRepositoryId,
    },
}

/// Used by the vocabulary axis codes of a stored term row.
pub(crate) fn axis_text(axis: VocabularyAxis) -> &'static str {
    match axis {
        VocabularyAxis::PackClass => "pack-class",
        VocabularyAxis::Category => "category",
        VocabularyAxis::AudienceRole => "audience-role",
        VocabularyAxis::RuleSubvariant => "rule-subvariant",
        VocabularyAxis::ArtefactStatus => "artefact-status",
        VocabularyAxis::MemoryContentClass => "memory-content-class",
        VocabularyAxis::LoopStrategy => "loop-strategy",
        VocabularyAxis::RuntimeCapability => "runtime-capability",
    }
}

pub(crate) fn axis_from_text(text: &str) -> Option<VocabularyAxis> {
    Some(match text {
        "pack-class" => VocabularyAxis::PackClass,
        "category" => VocabularyAxis::Category,
        "audience-role" => VocabularyAxis::AudienceRole,
        "rule-subvariant" => VocabularyAxis::RuleSubvariant,
        "artefact-status" => VocabularyAxis::ArtefactStatus,
        "memory-content-class" => VocabularyAxis::MemoryContentClass,
        "loop-strategy" => VocabularyAxis::LoopStrategy,
        "runtime-capability" => VocabularyAxis::RuntimeCapability,
        _ => return None,
    })
}
