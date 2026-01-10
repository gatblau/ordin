//! 2C part 1 § Tasks, runs and crews.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- Task ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Task {
    pub id: TaskId,
    pub workspace_id: WorkspaceId,
    pub target: TaskTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_text: Option<String>,
    pub arguments: Vec<TaskArgument>,
    pub attached_artefacts: Vec<ArtefactId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_again_of: Option<TaskId>,
    pub requested_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskTarget {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
    Crew { crew_charter_id: CrewCharterId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TaskArgument {
    pub name: String,
    pub value: String,
}

// ---- Run ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Run {
    pub id: RunId,
    pub origin: RunOrigin,
    pub agent: AgentRef,
    pub adapter: AdapterKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_thread_id: Option<String>,
    pub state: RunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<RunPauseReason>,
    pub limits: RunLimits,
    pub usage: UsageCounters,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    pub inputs_used: Vec<InputUsed>,
    pub queued_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunOrigin {
    Task {
        task_id: TaskId,
    },
    CrewRound {
        crew_run_id: CrewRunId,
        round_number: u32,
    },
    /// Ordin's conversation run for one workspace (amended January 2026, decision 80).
    Conversation {
        workspace_id: WorkspaceId,
    },
}

// `AdapterKind` is defined once, in part 2 (wire §2, `ordin_types::wire::run`), with the
// variant `HostedAgent` (ADR-098), stored `hosted_agent`; this module re-exports it
// (amended January 2026, decision 80: C-1, C-3).
pub use crate::wire::run::AdapterKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    StoppedAtLimit,
    Cancelled,
    Interrupted,
    Abandoned,
    NoReply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RunPauseReason {
    Approval,
    SignIn,
    Person,
}

// `RunLimits` is defined once, in part 2 (wire §2, `ordin_types::wire::run`), with
// `request_budget: Option<u32>` (no default ceiling until OQ-3); this module re-exports it
// (amended January 2026, decision 80: C-1, C-4).
pub use crate::wire::run::RunLimits;

/// Turns, tool calls, level, tokens and cache share where the harness reports them (ADR-062).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct UsageCounters {
    pub model_turns: u32,
    pub tool_calls: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_from_cache: Option<u64>,
}

// ---- StartupReport ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct StartupReport {
    pub id: StartupReportId,
    pub subject: StartupSubject,
    pub enforced_rules: Vec<String>,
    pub rules_not_enforced: Vec<RuleNotEnforced>,
    pub unavailable_parts: Vec<String>,
    pub available_tools: Vec<String>,
    pub adapter_verbs_not_offered: Vec<String>,
    pub generated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartupSubject {
    Run { run_id: RunId },
    CrewRun { crew_run_id: CrewRunId },
    AgentRelease { agent_release_id: AgentReleaseId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RuleNotEnforced {
    pub rule: String,
    pub reason: String,
}

// ---- ToolRequest ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ToolRequest {
    pub id: ToolRequestId,
    pub run_id: RunId,
    pub tool: String,
    pub arguments_digest: Sha256Hex,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<ToolRequestDecision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deciding_rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    pub state: ToolRequestState,
    pub requested_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// The stored verdict of a Tool request. Renamed from `ToolDecision` (amended January 2026,
/// decision 80: C-2); the wire `ToolDecision` (part 2 §3, the Rules Engine's full decision
/// with its `Verdict`) keeps its name.
pub enum ToolRequestDecision {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ToolRequestState {
    Requested,
    Decided,
    Executed,
    Refused,
}

// ---- Approval ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Approval {
    pub id: ApprovalId,
    /// Who asked: a run, a crew round, or the person (person-initiated cards such as a push,
    /// a handover, an email or a Telegram post started from a screen); amended January 2026, decision 80: C-19.
    pub asked_by: ApprovalAsker,
    /// Absent for an `ApprovalAsker::Person` card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<ApprovalSubject>,
    pub action: String,
    pub destination: String,
    pub affected_records: Vec<String>,
    pub consequence: String,
    pub state: ApprovalState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

/// "run or crew round" (04:195).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalSubject {
    Run {
        run_id: RunId,
    },
    CrewRound {
        crew_run_id: CrewRunId,
        round_number: u32,
    },
}

// `ApprovalAsker { Run, CrewRound, Person }` is defined once, in part 2 (§5, `ordin_types::wire::approval`);
// this module re-exports it (amended January 2026, decision 80: C-19).
pub use crate::wire::approval::ApprovalAsker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Approved,
    Rejected,
    Expired,
}

// ---- ExternalAction ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ExternalAction {
    pub action_key: ExternalActionKey,
    /// Absent for an action the person started outside any run (a push, a handover, a post
    /// from a screen); amended January 2026, decision 80: C-7.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,
    pub destination: String,
    pub state: ExternalActionState,
    pub sent_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<Timestamp>,
}

/// Kinds named across 04: Slack post (04:198), declared-connection write (04:198), wrightrun run start (04:198),
/// email send (04:98), origin push (04:103), copy publish (04:125), Telegram post (04:295);
/// pull request opened and handover written (amended January 2026, decision 80: C-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionKind {
    SlackPost,
    ConnectionWrite,
    WrightrunRunStart,
    EmailSend,
    OriginPush,
    CopyPublish,
    TelegramPost,
    PullRequestOpened,
    HandoverWritten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ExternalActionState {
    Sent,
    Confirmed,
    UnknownOutcome,
}

// ---- ReusedAnswer ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ReusedAnswer {
    pub id: ReusedAnswerId,
    pub request: String,
    pub agent: AgentRef,
    pub workspace_id: WorkspaceId,
    pub input_versions: Vec<InputUsed>,
    pub source_run_id: RunId,
    /// The earlier answer, shown again when nothing has changed (ADR-062; amended January 2026, orchestrator decision F2).
    pub text: String,
    pub answered_at: Timestamp,
    pub shown_again_at: Timestamp,
    pub expires_at: Timestamp,
}

// ---- CrewCharter ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CrewCharter {
    pub id: CrewCharterId,
    pub workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub purpose: String,
    pub charter_version: u32,
    /// Ordin from revision 0.8.0; no other value exists.
    pub coordinator: CoordinatorKind,
    pub members: Vec<CrewMember>,
    pub allowed_handoffs: Vec<Handoff>,
    pub guardrails: Guardrails,
    pub input_fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<WorkflowId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_version: Option<u32>,
    pub steps: Vec<WorkflowStep>,
    pub state: CrewCharterState,
    pub written_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Renamed from `Coordinator` (amended January 2026, decision 80: C-6), because `Coordinator` is the
/// Crew Orchestrator's hub trait (2B-1 rule 4, ADR-100). Stored value unchanged: `ordin`.
pub enum CoordinatorKind {
    Ordin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CrewMember {
    pub member: AgentRef,
    pub role: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Handoff {
    pub from: AgentRef,
    pub to: AgentRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Guardrails {
    pub max_rounds: u32,
    pub wall_clock_secs: u32,
    pub request_budget: u32,
    pub max_members_running: u32,
    pub members_allowed_external_actions: Vec<AgentRef>,
    pub approval_points: Vec<String>,
    pub failure_repeat_limit: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CrewCharterState {
    Written,
    Valid,
    Started,
    Kept,
    Discarded,
    Archived,
}

// ---- CrewRun ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CrewRun {
    pub id: CrewRunId,
    pub workspace_id: WorkspaceId,
    pub crew_charter_id: CrewCharterId,
    pub charter_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<WorkflowId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_version: Option<u32>,
    /// Step position from which Ordin took over, in the workflow's display numbering (for
    /// example `"2.3"`); text because positions inside branches and loops are dotted
    /// (amended January 2026, decision 80).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taken_over_from_step: Option<String>,
    pub inputs: Vec<InputUsed>,
    pub state: CrewRunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<CrewPauseReason>,
    pub round_counter: u32,
    pub budget: CrewBudgetCounters,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,
    pub started_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CrewRunState {
    Starting,
    Running,
    Paused,
    Finished,
    Escalated,
    StoppedAtLimit,
    Killed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CrewPauseReason {
    Approval,
    Restart,
    Escalation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CrewBudgetCounters {
    pub requests_used: u32,
    pub wall_clock_used_secs: u32,
    pub repeated_failures: u32,
}

// ---- Round ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Round {
    pub crew_run_id: CrewRunId,
    pub number: u32,
    pub coordinator_decision: String,
    pub guardrail_decisions: Vec<String>,
    pub member_runs: Vec<RunId>,
    pub opened_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Timestamp>,
}

// ---- Workflow ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Workflow {
    pub id: WorkflowId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub state: WorkflowState,
    pub origin: WorkflowOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared: Option<WorkflowShare>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WorkflowVersion {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub purpose: String,
    pub change_note: String,
    pub inputs: Vec<WorkflowInput>,
    pub agents: Vec<WorkflowAgent>,
    pub steps: Vec<WorkflowStep>,
    /// The format 3 file's limits (part 2 §8), so a saved version and its file carry the same
    /// values (amended January 2026, decision 80: C-24).
    pub limits: crate::wire::workflow_file::WorkflowFileLimits,
    /// Set when made by **Restore** (equal to an earlier version).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from_version: Option<u32>,
    pub saved_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WorkflowInput {
    pub label: String,
    pub kind: WorkflowInputKind,
    pub required: bool,
}

// `WorkflowInputKind` is defined once, in part 2 (§8, `ordin_types::wire::workflow_file`),
// with the variants `Document`, `Text`, `List` (2C I-1); this module re-exports it
// (amended January 2026, decision 80: C-1).
pub use crate::wire::workflow_file::WorkflowInputKind;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WorkflowAgent {
    /// Short local name the steps use, unique in the version (format 3 `key`; amended January 2026, decision 80: C-24).
    pub key: String,
    /// Format 3 agent identity: the repository (profile key or address) and the `urn:ordin:`
    /// WorkspaceProfile identifier (amended January 2026, decision 80: C-24).
    pub repository: crate::wire::workflow_file::AgentRepositoryRef,
    pub id: String,
    pub agent: AgentRef,
    pub package: String,
    /// Range of versions it was saved with, e.g. `>=1.2.0, <2.0.0`.
    pub version_range: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowOrigin {
    SavedFromCrewRun {
        crew_run_id: CrewRunId,
    },
    DraftedByOrdin,
    Duplicated {
        from_workflow_id: WorkflowId,
        from_version: u32,
    },
    Imported {
        file_name: String,
        imported_on: Timestamp,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkflowState {
    NotSavedYet,
    Saved,
    NeedsAgents,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WorkflowShare {
    pub host: ShareHost,
    pub repository: String,
    pub folder: String,
    pub file_name: String,
    pub followed_date: Timestamp,
    pub followed_by: String,
    pub followed_commit: String,
    pub changed_here_not_sent: bool,
    pub newer_in_repository: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ShareHost {
    Github,
    Gitlab,
}

// ---- WorkflowStep ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowStep {
    Agent(AgentStep),
    If(IfBlock),
    Each(EachBlock),
    Until(UntilBlock),
    Call(CallBlock),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct AgentStep {
    /// Format 3 display number ("3", "2.1", "3a"); steps sharing a number with letters run at
    /// the same time (amended January 2026, decision 80: C-24).
    pub position: String,
    /// Steps sharing a group start at the same time; derived from the position's letters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<u32>,
    pub agent: AgentRef,
    pub line: String,
    pub uses: Vec<StepUse>,
    pub asks_first: bool,
    pub ordin_checks_result: bool,
    pub if_it_fails: IfItFails,
}

/// `domain::runs::StepUse` — the saved workflow's step input. A distinct type from
/// `wire::workflow_file::StepUse` (part 2 §8, the file format's); the Ordin Host converts
/// between them on import and export (amended January 2026, decision 80: C-1, namespaced).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepUse {
    Input { label: String },
    Result { position: String },
}

// `IfItFails` is defined once, in part 2 (§8, `ordin_types::wire::workflow_file`), with
// `#[default] OrdinTakesOver`; this module re-exports it (amended January 2026, decision 80: C-1).
pub use crate::wire::workflow_file::IfItFails;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct IfBlock {
    pub position: String,
    pub condition: Condition,
    pub paths: Vec<NamedPath>,
}

/// `domain::runs::NamedPath` — distinct from `wire::workflow_file::NamedPath` (part 2 §8);
/// converted by the Ordin Host on import and export (amended January 2026, decision 80: C-1, namespaced).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct NamedPath {
    pub name: String,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EachBlock {
    pub position: String,
    pub list: StepUse,
    pub maximum: u32,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct UntilBlock {
    pub position: String,
    pub condition: Condition,
    pub maximum: u32,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CallBlock {
    pub position: String,
    pub workflow_id: WorkflowId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "decided_by", rename_all = "snake_case")]
pub enum Condition {
    /// Kind catalogue fixed in /spec (LD-34).
    Application {
        condition_kind: String,
        looks_at_position: String,
    },
    Ordin {
        words: String,
    },
    Person {
        words: String,
    },
}

// ---- Report ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Report {
    pub id: ReportId,
    pub workspace_id: WorkspaceId,
    pub run_id: RunId,
    pub title: String,
    pub as_of: Timestamp,
    pub parts: Vec<ReportSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ReportSection {
    pub heading: String,
    pub data: ReportData,
    pub sources: Vec<ReportSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportData {
    Table {
        columns: Vec<ReportTableColumn>,
        rows: Vec<Vec<CellValue>>,
    },
    Chart {
        chart_kind: ChartKind,
        columns: Vec<ReportTableColumn>,
        rows: Vec<Vec<CellValue>>,
        x_column: String,
        series_columns: Vec<String>,
        x_title: String,
        y_title: String,
        /// For bar charts only (amended January 2026, decision 80: C-23).
        #[serde(skip_serializing_if = "Option::is_none")]
        bar_arrangement: Option<BarArrangement>,
    },
    KeyFigures {
        figures: Vec<ReportKeyFigure>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ReportTableColumn {
    pub name: String,
    pub column_type: ColumnType,
    /// ISO 4217, for `money` only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Decimal places shown, for `number` only (amended January 2026, decision 80: C-23).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ColumnType {
    Text,
    Number,
    Money,
    Percentage,
    Date,
    DocumentLink,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum CellValue {
    Text(String),
    Number(DecimalString),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    BarVertical,
    BarHorizontal,
    Line,
    Area,
    Scatter,
    Pie,
}

/// Defined once, here; the block catalogue (part 2 §7) re-exports it (amended January 2026, decision 80: C-23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BarArrangement {
    Grouped,
    Stacked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ReportKeyFigure {
    pub label: String,
    pub value: DecimalString,
    pub unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_against: Option<ReportFigureChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ReportFigureChange {
    pub earlier_label: String,
    pub earlier_value: DecimalString,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportSource {
    Document {
        location: String,
        version_read: String,
    },
    CompanySystem {
        system: String,
        read_at: Timestamp,
    },
    EarlierResult {
        run_id: RunId,
    },
    WorkedOutBy {
        agent: AgentRef,
        from: String,
    },
}

/// A report kept under a name (04:240, 04:242).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Dashboard {
    pub id: DashboardId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub made_by: DashboardSource,
    pub inputs: Vec<InputUsed>,
    /// Each refresh's run, oldest first; the last one's report is shown.
    pub refresh_runs: Vec<RunId>,
    pub kept_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DashboardSource {
    Task {
        task_id: TaskId,
    },
    Workflow {
        workflow_id: WorkflowId,
        version: u32,
    },
}

// ---- RunStep ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RunStep {
    pub id: RunStepId,
    pub run_id: RunId,
    pub plan_version: u32,
    pub position: u32,
    pub line: String,
    pub came_from: StepSource,
    pub mark: StepMark,
    pub marks_set: Vec<MarkSet>,
    /// Journal sequence numbers of the events seen while `now` (assumed representation, LD-35).
    pub events_seen: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StepSource {
    Workflow,
    CrewPlan,
    AgentsPlan,
    HarnessTodoList,
    AddedByOrdin,
}

// `StepMark` is defined once, in part 2 (§1, `ordin_types::wire::run`); this module
// re-exports it (amended January 2026, decision 80: C-1, C-17).
pub use crate::wire::run::StepMark;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct MarkSet {
    pub mark: StepMark,
    pub at: Timestamp,
}
