//! 2C part 1 § Agents and the catalogue.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- AgentInstance ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentInstance {
    pub id: AgentInstanceId,
    pub display_name: String,
    pub package_name: String,
    pub package_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_repository_id: Option<AgentRepositoryId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub agent_folder_path: String,
    /// Lock file inside the agent folder: repository, commit and digest per part (02:451).
    pub lock_file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_result: Option<AuditResult>,
    pub authority: Authority,
    /// Key of the Package Inspector's parsed package model for this instance (02:476).
    pub parsed_declaration_ref: String,
    pub is_trial_of_own_draft: bool,
    pub status: AgentInstanceStatus,
    /// Why the instance was stopped, in plain words (a withdrawn part); with `status = refused`
    /// this is how a stopped instance reads (amended January 2026, decision 80: C-22).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_reason: Option<String>,
    /// What stopped a `refused` instance that was once `ready`, and so which event may make it
    /// `ready` again (Phase 3 closing round, K13): `Withdrawal` → a fixed version the person
    /// accepts; `Trust` → the company's trust policy trusting its executable parts again.
    /// Present if and only if `stopped_reason` is present. Column `agent_instances.stopped_by`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<StoppedBy>,
    /// Set when an update that changes nothing the agent may do or read was applied between runs (A-16).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_between_runs_at: Option<Timestamp>,
    pub installed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StoppedBy {
    Withdrawal,
    Trust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    DocumentOnly,
    SystemChanging,
    RoutingOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentInstanceStatus {
    Installing,
    Ready,
    Refused,
    Updating,
    /// An update changes what it may do or read and waits for the person's choice (A-16).
    UpdateAwaitingChoice,
    Removed,
}

// ---- ExecutablePartDecision ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ExecutablePartDecision {
    pub id: ExecutablePartDecisionId,
    pub agent_instance_id: AgentInstanceId,
    pub part_name: String,
    pub kind: ExecutablePartKind,
    pub decision: ExecutablePartTrust,
    pub source: DecisionSource,
    pub decided_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExecutablePartKind {
    Hook,
    McpServer,
    BundledProgram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExecutablePartTrust {
    Trusted,
    NeedsItApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    OrganisationPolicy,
}

// ---- RemoteAgent ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RemoteAgent {
    pub id: RemoteAgentId,
    pub display_name: String,
    pub route: RemoteRoute,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_channel: Option<String>,
    /// Name of the identity-provider settings entry in the active organisation profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_provider_ref: Option<String>,
    pub status: RemoteAgentStatus,
    pub listed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RemoteRoute {
    Http,
    Slack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentStatus {
    Listed,
    Enabled,
    Verified,
    Withdrawn,
}

// ---- AgentRepository ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRepository {
    pub id: AgentRepositoryId,
    pub display_name: String,
    pub host: String,
    pub repository: String,
    pub protected_branch: String,
    pub format: AgentFormat,
    pub format_version: String,
    pub source: RepositorySource,
    /// Profile repositories only (04:298).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_merges_required: Option<bool>,
    pub person_can_publish: bool,
    pub person_can_curate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fetched_commit: Option<String>,
    pub clone_path: String,
    pub state: AgentRepositoryState,
    pub added_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentFormat {
    Ordin,
    Apm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RepositorySource {
    Profile,
    Person,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentRepositoryState {
    Named,
    Cloned,
    Dropped,
    Removed,
}

// ---- Part ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Part {
    pub id: PartId,
    pub agent_repository_id: AgentRepositoryId,
    pub kind: PartKind,
    /// `metadata.id`.
    pub identifier: String,
    pub name: String,
    pub version: String,
    pub status: PartStatus,
    /// `metadata.supersedes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    pub owner: String,
    pub file_path: String,
    pub commit: String,
    pub content_digest: Sha256Hex,
    pub vocabulary_terms_cited: Vec<VocabularyTermId>,
    /// Dependency edges (stored in child table `part_dependencies`).
    pub uses: Vec<PartId>,
    pub withdrawn: bool,
}

/// The 25 kinds, spelt as crucible declares them. Ordered by declaration order (amended January 2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum PartKind {
    SystemRole,
    Rule,
    OperatingMode,
    Skill,
    Command,
    ContextResource,
    ToolBinding,
    AgentIdentity,
    Capability,
    WorkspaceProfile,
    CrewProfile,
    Journey,
    PracticePack,
    BrandOverlay,
    JurisdictionOverlay,
    LanguageOverlay,
    SourceWorkflowOverlay,
    StyleOverlay,
    HandoffOverlay,
    Evaluation,
    OutputContract,
    Vocabulary,
    Guideline,
    Memory,
    ControlLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PartStatus {
    Draft,
    Approved,
    Deprecated,
}

// ---- WithdrawnListEntry ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WithdrawnListEntry {
    pub id: WithdrawnListEntryId,
    pub agent_repository_id: AgentRepositoryId,
    pub part_kind: PartKind,
    pub part_identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_content_digest: Option<Sha256Hex>,
    pub reason: String,
    pub withdrawn_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_version: Option<String>,
    /// Set when a later read no longer finds the entry; it stays in force (04:325).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_by_hand_seen_at: Option<Timestamp>,
    pub read_at: Timestamp,
}

// ---- VocabularyTerm ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct VocabularyTerm {
    pub id: VocabularyTermId,
    pub agent_repository_id: AgentRepositoryId,
    pub vocabulary_part_id: PartId,
    pub axis: VocabularyAxis,
    pub code: String,
    pub label: String,
    pub description: String,
    pub state: VocabularyTermState,
    /// The code of the term that replaces a retired one, when the vocabulary names it; enables
    /// ADR-094 item 4 *Fix it* (amended January 2026, decision 80: C-32; name *chosen here*).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_code: Option<String>,
}

/// Fixed in ordin's code (04:345); spelt as crucible's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum VocabularyAxis {
    PackClass,
    Category,
    AudienceRole,
    RuleSubvariant,
    ArtefactStatus,
    MemoryContentClass,
    LoopStrategy,
    RuntimeCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VocabularyTermState {
    Active,
    Retired,
}

// ---- Lifecycle ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Lifecycle {
    pub id: LifecycleId,
    pub part_id: PartId,
    pub agent_repository_id: AgentRepositoryId,
    pub identifier: String,
    pub name: String,
    pub stages: Vec<LifecycleStage>,
    pub status: PartStatus,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct LifecycleStage {
    pub name: String,
    pub role: String,
    /// WorkspaceProfile identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// OutputContract identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hands_over: Option<String>,
    pub handover_place_id: HandoverPlaceId,
    /// 0 = not set (no reminder); otherwise 1–365 (amended January 2026, decision 80: C-21).
    pub days_to_answer: u32,
    pub next: StageNext,
    pub send_backs: Vec<SendBack>,
    /// The Slack channel where this stage's handover notices are posted, as the person who acted
    /// (amended January 2026, ADR-102). Absent = no stage channel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_channel: Option<StageSlackChannel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StageNext {
    One { stage: String },
    OneOf { choices: Vec<StageChoice> },
    SideBySide { stages: Vec<String> },
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct StageChoice {
    pub stage: String,
    pub when: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SendBack {
    pub to_stage: String,
    pub when: String,
    pub max_rounds: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct StageSlackChannel {
    pub channel_id: String,
    pub name: String,
}

/// Which stage a workspace follows (at most one per workspace, 04:330).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceStage {
    pub id: WorkspaceStageId,
    pub workspace_id: WorkspaceId,
    pub lifecycle_id: LifecycleId,
    pub stage_name: String,
    pub set_at: Timestamp,
}

// ---- AgentDraft ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentDraft {
    pub id: AgentDraftId,
    pub agent_repository_id: AgentRepositoryId,
    pub name: String,
    pub purpose: String,
    pub jobs: Vec<DraftJob>,
    pub tasks: Vec<String>,
    pub will: Vec<String>,
    pub wont: Vec<String>,
    pub needs: Vec<String>,
    /// Identifiers of reused parts.
    pub reused_parts: Vec<PartId>,
    pub origin: DraftOrigin,
    pub branch: String,
    /// Key of the Checker's latest result set for this draft (02:446).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_results_ref: Option<String>,
    pub wording_changes: Vec<WordingChange>,
    /// Who wrote each field (ADR-090 §7), as on `PartDraft`; stored in the side table
    /// `draft_field_authors` (amended January 2026, decision 80: C-21).
    pub field_authors: Vec<FieldAuthor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rehearsal_id: Option<RehearsalId>,
    pub trial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequestRef>,
    pub state: ReviewDraftState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct DraftJob {
    pub text: String,
    pub came_from: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WordingChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DraftOrigin {
    Made,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct PullRequestRef {
    pub number: u64,
    pub url: String,
    pub state: PullRequestState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// Shared by AgentDraft and PartDraft (04:305, 04:320).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReviewDraftState {
    Draft,
    WaitingForReview,
    ChangesAskedFor,
    ApprovedNotSignedYet,
    Published,
    Discarded,
}

// ---- Rehearsal ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Rehearsal {
    pub id: RehearsalId,
    pub agent_draft_id: AgentDraftId,
    /// Commit of the draft's branch rehearsed.
    pub draft_version: String,
    pub situations: Vec<RehearsalSituation>,
    pub run_ids: Vec<RunId>,
    pub rehearsed_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RehearsalSituation {
    pub description: String,
    pub documents_used: Vec<String>,
    pub expectations: Vec<RehearsalExpectation>,
    pub reviewer_opinion: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RehearsalExpectation {
    pub text: String,
    /// The machine rule the Checker checks (ADR-085, ADR-090 §4); absent for a model-judged
    /// expectation, which is an opinion and never checked by the Checker (amended January 2026, decision 80: C-32).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<ExpectationRule>,
    pub outcome: ExpectationOutcome,
}

/// A deterministic rehearsal expectation; closed set, as the Checker spec defines it
/// (amended January 2026, decision 80: C-32).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectationRule {
    ResultContains { text: String },
    ResultHasSection { heading: String },
    ToolUsed { tool: String },
    ToolNotUsed { tool: String },
    ApprovalAsked,
    NoExternalAction,
    ArtefactProduced { name_suffix: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExpectationOutcome {
    Met,
    NotMet,
}

// ---- PartDraft ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct PartDraft {
    pub id: PartDraftId,
    pub agent_repository_id: AgentRepositoryId,
    pub change_kind: PartChangeKind,
    /// None for a new part until it is merged (04:319).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<PartId>,
    /// The part's fields as the Agent Model holds them (02:433).
    pub fields: serde_json::Value,
    pub computed_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes_what_part_does: Option<bool>,
    pub used_by_at_check: Vec<PartId>,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_results_ref: Option<String>,
    pub wording_changes: Vec<WordingChange>,
    pub field_authors: Vec<FieldAuthor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequestRef>,
    pub state: ReviewDraftState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PartChangeKind {
    NewPart,
    ChangedPart,
    Deprecation,
    Withdrawal,
    VocabularyTerm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct FieldAuthor {
    pub field: String,
    pub written_by: FieldWriter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FieldWriter {
    Person,
    ModelThenPerson,
    Model,
}

// ---- AgentRelease ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentRelease {
    pub id: AgentReleaseId,
    /// Registry reference, `registry/repository:tag`.
    pub reference: String,
    pub digest: String,
    pub bundle_path: String,
    pub agent_configuration_path: String,
    pub configuration_files: Vec<String>,
    pub package_name: String,
    pub package_version: String,
    pub ordin_version: String,
    pub contract_version: String,
    pub startup_report_id: StartupReportId,
    pub state: AgentReleaseState,
    pub published_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentReleaseState {
    Published,
    Obtained,
    Running,
}
