//! The public types of Checker.md *Public Interface*, as the spec writes them.

use std::collections::BTreeMap;

use ordin_agent_model::{RepositoryRef, WithdrawnListEntry};
use ordin_agent_model::{ResolutionPlan, SourceOverride, SourceScope, StagedPart};
use ordin_types::domain::agents::{
    AgentDraft, ExecutablePartKind, PartKind, VocabularyAxis, WordingChange,
};
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentDraftId, AgentInstanceId, AgentRepositoryId, PartDraftId, RehearsalId, RunId,
};
use serde::{Deserialize, Serialize};

/// The most findings one run keeps; one more, `findings_truncated`, says the rest were dropped.
pub const MAX_FINDINGS_PER_RUN: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckRunId(pub uuid::Uuid); // UUIDv7; 2C `check_results_ref` = its string

impl std::fmt::Display for CheckRunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Failure,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fix", rename_all = "snake_case")]
pub enum Fix {
    /// "Fix it" — fixed code knows the one right answer (ADR-094 item 4).
    FixIt { action: FixAction },
    /// "Ask Ordin to fix it" — judgement needed; the agent maker suggests text.
    AskOrdin,
    /// "Change it yourself" — opens the field.
    ChangeItYourself { field: String },
    /// "Leave it out" / "Leave [tool] out".
    LeaveItOut { what: String },
    /// "Publish to an ordin-format repository" (ADR-087).
    PublishToOrdinFormat,
    /// "Ask IT" / "Ask IT about [tool]" (ADR-012).
    AskIt,
    /// "Ask the agent maker to fix this" (ADR-088 *Conflicts*).
    AskAgentMaker,
    /// "Choose which [class] this agent uses when the workspace doesn't say" (ADR-089).
    ChoosePacks {
        rule_index: u32,
        choices: Vec<String>,
    },
    /// "Let the agent ask" (decision 64).
    LetAgentAsk { rule_index: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FixAction {
    MintIdentifier {
        field: String,
        value: String,
    },
    PointToReplacement {
        field: String,
        from: String,
        to: String,
    },
    AskMissingField {
        field: String,
    },
    DropUnknownField {
        field: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Finding {
    /// Stable machine code: kept as the agent format's checks name it (`sync_pack_not_applicable`, `kind_unknown`) or
    /// ordin's (`trust_needs_it_approval`). Lower snake case, ≤ 64 characters.
    pub code: String,
    pub severity: Severity,
    /// `metadata.id` of the part the finding concerns, or None for the agent as a whole.
    pub part_identifier: Option<String>,
    /// Fluent message id of the one plain sentence (`check-…`); the screens render it.
    pub message_id: String,
    /// Plain values the sentence needs (part names, display names, counts); never file content.
    pub args: BTreeMap<String, String>,
    pub fixes: Vec<Fix>,
    /// Technical detail for Diagnostics only.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckSubject {
    RepositoryRead {
        repository: String,
        commit: String,
    },
    AgentDraft {
        agent_draft_id: AgentDraftId,
    },
    PartDraft {
        part_draft_id: PartDraftId,
    },
    Withdrawal {
        part_draft_id: PartDraftId,
    },
    VocabularyChange {
        part_draft_id: PartDraftId,
    },
    InstalledAgent {
        agent_instance_id: AgentInstanceId,
    },
    LibraryItem {
        part_identifier: String,
        version: String,
    },
    Rehearsal {
        rehearsal_id: RehearsalId,
    },
    /// `check_for_install`: an agent about to be added, changed, updated or tried.
    InstallCandidate {
        root_id: String,
        plan_hash: String,
    },
}

impl CheckSubject {
    /// The `subject_kind` column value: the serde tag.
    pub(crate) fn kind_text(&self) -> &'static str {
        match self {
            CheckSubject::RepositoryRead { .. } => "repository_read",
            CheckSubject::AgentDraft { .. } => "agent_draft",
            CheckSubject::PartDraft { .. } => "part_draft",
            CheckSubject::Withdrawal { .. } => "withdrawal",
            CheckSubject::VocabularyChange { .. } => "vocabulary_change",
            CheckSubject::InstalledAgent { .. } => "installed_agent",
            CheckSubject::LibraryItem { .. } => "library_item",
            CheckSubject::Rehearsal { .. } => "rehearsal",
            CheckSubject::InstallCandidate { .. } => "install_candidate",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CheckReport {
    pub check_run_id: CheckRunId,
    pub subject: CheckSubject,
    /// True when no finding has severity Failure.
    pub passed: bool,
    pub findings: Vec<Finding>,
    /// Applied wording changes ("ordin tidied N things"); empty for non-draft subjects.
    pub tidied: Vec<WordingChange>,
    /// "Checked [time], against the version in use" (11 §5.30).
    pub checked_at: Timestamp,
    pub version_checked: String,
    /// Agents that stop when a withdrawal is published (UI-D99), by identifier.
    pub agents_that_stop: Vec<String>,
    /// False only when step 25 reported `publish_gate_not_met`; true for every subject with no
    /// publish gate. Derived from the findings, so `report(id)` returns the same value.
    pub publish_gate_met: bool,
}

/// Mapping or loss report items, as plain data from the Format Plug-ins (via the caller).
// spec: ordin-desktop/components/Checker#SA-RECON-04
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportItem {
    pub what: String,
    pub part_identifier: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RepositoryCheck {
    pub repository: RepositoryRef,
    pub repository_display_name: String,
    /// The repositories this one borrows from (ADR-090 §2), for reference resolution.
    pub scope: SourceScope,
    /// New and changed parts; an empty list checks the repository as it is (a read).
    pub changes: Vec<StagedPart>,
    /// Identifiers of parts the change deletes from the tree.
    pub deletions: Vec<String>,
    /// None = the bound source; Some = this source for this check only — the Catalogue
    /// Service's `draft_source(repository, worktree)` for a draft check (R18).
    pub source: Option<SourceOverride>,
}

#[derive(Debug, Clone)]
pub struct AgentDraftCheck {
    pub draft: AgentDraft,
    /// The draft's WorkspaceProfile and every new or changed part, generated by the Agent Model.
    pub parts: Vec<StagedPart>,
    pub target: RepositoryCheck,
    /// Target repository format, for the loss report and SA-APM-01.
    pub target_format: TargetFormat,
    /// Which tasks serve each job (index into `draft.jobs`), from the Authoring Service.
    pub job_tasks: BTreeMap<u32, Vec<String>>,
    /// Format Plug-ins outputs passed through by the caller.
    pub mapping_report: Vec<ReportItem>,
    pub loss_report: Vec<ReportItem>,
    /// Identifiers the draft references from other repositories.
    pub borrowed: Vec<String>,
    /// Other agents' names in the target repository, for the collision check.
    pub repository_agent_names: Vec<String>,
    /// The latest rehearsal's expectation outcomes, None before the first rehearsal.
    pub rehearsal: Option<Vec<ExpectationOutcome>>,
    /// The agent's language for the wording rules (BCP 47); "en-GB" when the agent has none.
    pub agent_language: String,
    /// For a lifecycle part: which handover places the person can reach (ADR-095 item 2).
    pub reachable_places: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFormat {
    Ordin,
    Apm,
}

#[derive(Debug, Clone)]
pub struct WithdrawalCheck {
    pub repository: RepositoryCheck,
    pub entry: WithdrawnListEntry,
    /// The repository's withdrawn list as it is now (entries already merged).
    pub existing_entries: Vec<WithdrawnListEntry>,
    /// The file's proposed full text, to verify the change only appends.
    pub proposed_list: Vec<WithdrawnListEntry>,
}

#[derive(Debug, Clone)]
pub struct VocabularyCheck {
    pub repository: RepositoryCheck,
    pub axis: VocabularyAxis,
    pub added: Vec<(String, String, String)>, // (code, label, description)
    pub retired: Vec<String>,                 // codes
}

/// A deterministic rehearsal expectation (ADR-085, ADR-090 §4). Closed set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// What the run left. Built by the Ordin Host when a rehearsal run ends (`AuthoringHost::rehearse`,
/// OrdinHost.md SA-CLOSE-06, X-28) and passed by the Authoring Service to `check_rehearsal`.
#[derive(Debug, Clone, Default)]
pub struct RunEvidence {
    pub run_id: RunId,
    pub final_message: String,
    pub tools_used: Vec<String>,
    pub approvals_asked: u32,
    pub external_actions: u32,
    pub artefact_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExpectationOutcome {
    pub rule: ExpectationRule,
    pub met: bool,
    pub evidence: String,
}

/// One executable part of an installed agent, as the trust decision sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecutablePart {
    pub part_name: String, // MCP server name, program name, or hook part identifier
    pub kind: ExecutablePartKind,
    pub declared_by: String, // ToolBinding or hook part identifier
    /// True when the declaring part is read from the bundled catalogue: the company's trust policy
    /// does not apply to it (Checker.md CHK-TR-1). Absent in a row stored before that amendment.
    // spec: ordin-desktop/components/Checker#CHK-TR-1
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bundled: bool,
}

/// The organisation's trust policy (2D-2 `agent_catalogue.trust_policy`); owned here (row 624).
// The profile's default value is the empty object `{}` (2D-2). Missing keys read as empty lists, so
// `{}` is the empty policy; unknown keys are still refused.
// spec: ordin-desktop/components/Checker#SA-RECON-06
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields, default)]
pub struct TrustPolicy {
    /// ToolBinding parts the company trusts, by `metadata.id`, with the programs or MCP servers
    /// each may start (ADR-082: "by identifier and the program each starts").
    pub tool_bindings: Vec<TrustedToolBinding>,
    /// Hook parts (event-loaded parts) the company trusts, by `metadata.id`.
    pub hooks: Vec<String>,
    /// Built-in harness tools a tool part may name (ADR-094 item 5: "a tool part must name a
    /// tool the organisation profile allows"); empty = every built-in tool allowed.
    pub allowed_builtin_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TrustedToolBinding {
    pub id: String,
    pub programs: Vec<String>,
}

/// `check_for_install` input: a plan from `ResolverAndCompiler::resolve`.
#[derive(Debug, Clone)]
pub struct InstallCheck {
    pub plan: ResolutionPlan,
    pub scope: SourceScope,
    /// None for a new agent; Some for a change, update or trial of an existing instance.
    pub agent_instance_id: Option<AgentInstanceId>,
    /// Some(draft source) for a trial of the person's own draft; None otherwise.
    pub source: Option<SourceOverride>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum InstallVerdict {
    Clean,
    /// At least one Failure finding; the findings are in the report.
    Failed,
    /// No Failure, but these executable parts are not trusted by the profile.
    NeedsItApproval {
        parts: Vec<ExecutablePart>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallCheckReport {
    pub verdict: InstallVerdict,
    pub report: CheckReport,
}

/// `check_handover` input, built by the Handover Service from the Workspace Service's facts.
#[derive(Debug, Clone)]
pub struct HandoverCheckInput {
    pub results: Vec<HandoverResult>,
    /// The stage's OutputContract; None for a handover outside any lifecycle.
    pub contract: Option<HandoverContract>,
    pub receiver: ReceiverFacts,
}

#[derive(Debug, Clone)]
pub struct HandoverResult {
    pub link: String,
    pub label: String,
    /// True when the result exists only on this computer (Workspace Service `ThisComputerOnly`).
    pub local_only: bool,
    /// None = unknown (no check possible), Some(false) = the receiver cannot open it.
    pub readable_by_receiver: Option<bool>,
    /// The result's Markdown headings, None when the result is not a readable document.
    pub headings: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct HandoverContract {
    pub scope: SourceScope,
    pub identifier: String,
}

#[derive(Debug, Clone)]
pub struct ReceiverFacts {
    pub is_person: bool,
    pub reachable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoverCheckReport {
    pub passed: bool,
    pub findings: Vec<HandoverFinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoverFinding {
    /// `results_readable`, `contract_sections` or `receiver_reachable`.
    pub code: String,
    pub passed: bool,
    /// The result link or contract identifier the finding concerns.
    pub item: Option<String>,
    pub missing_sections: Vec<String>,
}

/// `read_dossier` output.
#[derive(Debug, Clone, Serialize)]
pub struct DossierReading {
    /// Contract sections found, in the contract's order.
    pub sections: Vec<DossierSection>,
    /// Contract section names no heading matched.
    pub missing_sections: Vec<String>,
    /// Normalised headings matching no contract section, first-seen order.
    pub unmatched_headings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DossierSection {
    pub name: String,
    pub text: String,
    pub evidence_count: u32,
    pub required: bool,
    pub low_confidence: bool,
    pub derived: bool,
}

/// `check_parts` output for the Release Builder: one entry per finding, located by file.
#[derive(Debug, Clone, Serialize)]
pub struct PackageCheckReport {
    pub findings: Vec<PackageFinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageFinding {
    /// True for a Failure finding.
    pub stop: bool,
    /// Path of the part's file within the package, or "" for a finding about the agent as a whole.
    pub file: String,
    /// The part's `metadata.id`, or the finding code when there is no part.
    pub entry: String,
    /// The finding's `detail`.
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TrustChanges {
    pub lost: Vec<AgentInstanceId>,
    pub gained: Vec<AgentInstanceId>,
}

/// `checker.check_installed` (ruling R1).
pub struct CheckInstalledInput {
    pub agent_instance_id: AgentInstanceId,
}

/// `checker.check_library_item` (ruling R1).
pub struct CheckLibraryItemInput {
    pub agent_repository_id: Option<AgentRepositoryId>, // None = bundled catalogue
    pub kind: PartKind,
    pub identifier: String,
}

/// `checker.report` (ruling R1).
pub struct CheckRunRef {
    pub check_run_id: CheckRunId,
}
