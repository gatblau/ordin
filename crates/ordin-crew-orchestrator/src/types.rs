//! The public value types and the two traits of the Crew Orchestrator (CrewOrchestrator.md
//! *Public Interface*): [`Coordinator`], which the Ordin Host implements, and [`CrewHost`], which
//! the programs implement.

use std::sync::Arc;

use ordin_rules::CardWording;
use ordin_runtime_adapter::AdapterKind;
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::domain::runs::WorkflowStep;
use ordin_types::domain::runs::{CrewCharter, CrewMember, CrewRunState, RunState};
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{
    ArtefactId, CrewRunId, RemoteAgentId, RunId, TriggerId, WorkflowId, WorkspaceId,
};
use serde::{Deserialize, Serialize};

/// One coordinator turn's worth of what Ordin sees (ADR-062: summaries and references, never the
/// conversation).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CrewView {
    pub crew_run_id: CrewRunId,
    pub workspace_id: WorkspaceId,
    pub purpose: String,
    pub members: Vec<CrewMember>,
    pub round: u32,
    pub rounds_left: u32,
    pub seconds_left: u32,
    /// None when the charter sets no request budget.
    pub requests_left: Option<u32>,
    pub inputs: Vec<CrewInput>,
    /// The latest result of each member, newest first; at most one per member.
    pub latest_results: Vec<StepOutcome>,
    /// Saved-steps mode: the outline position reached; None in coordinated mode.
    pub position: Option<String>,
    /// Cancelled when the crew is killed or ends; a coordinator method waiting on the person or on a
    /// turn returns `CoordinatorError::StoppedByPerson` once it is cancelled.
    #[serde(skip)]
    pub cancel: tokio_util::sync::CancellationToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StepOutcome {
    pub member: AgentRef,
    /// "3", "2.1", "3a" in saved-steps mode; "round 4" in coordinated mode.
    pub label: String,
    pub run_id: RunId,
    pub state: RunState,
    /// The member's short summary (ADR-062: a short summary and references, not its transcript).
    pub summary: Option<String>,
    pub artefacts: Vec<ArtefactId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CoordinatorTurn {
    pub calls: Vec<CrewToolCall>,
    pub turns: u32,
}

/// The crew tools of SA-NAME-01 that reach the orchestrator; their MCP arguments are checked by the
/// Ordin Host's provider before they arrive here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum CrewToolCall {
    Delegate {
        member: AgentRef,
        /// The brief in one line to a few sentences (at most 2,000 characters).
        brief: String,
        /// Members whose latest results this step uses: each is a hand-off from that member.
        uses_results_of: Vec<AgentRef>,
        /// Inputs by label.
        uses_inputs: Vec<String>,
    },
    Finish {
        summary: String,
    },
    /// `recommendation`: Ordin's suggested answer, shown on the person's question card (ADR-027).
    Escalate {
        question: String,
        recommendation: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum CheckVerdict {
    CarryOn { turns: u32 },
    Shortfall { reason: String, turns: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PathDecision {
    pub path: String,
    pub reason: String,
    pub turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemberAnswer {
    pub text: String,
    pub answered_by: AnsweredBy,
    pub turns: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnsweredBy {
    Ordin,
    Person,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PersonQuestion {
    pub kind: PersonQuestionKind,
    pub words: String,
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonQuestionKind {
    AskMe,
    RepeatUntil,
    RepeatAtMaximum,
    StepFailedTwice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TakeOverReason {
    StepFailedTwice {
        position: String,
    },
    CheckFoundShortfall {
        position: String,
        reason: String,
    },
    AgentMissing {
        member: AgentRef,
    },
    AgentChanged {
        member: AgentRef,
    },
    InputMissing {
        label: String,
    },
    /// Ordin asked for the take-over and the orchestrator accepted it (`request_take_over`).
    OrdinAsked {
        position: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CrewOutcome {
    pub state: CrewRunState,
    pub end_reason: Option<String>,
    pub results: Vec<StepOutcome>,
    /// Positions Ordin changed after taking over (journalled as changed by Ordin).
    pub changed_by_ordin: Vec<String>,
    /// Set when the crew ended `escalated` by Ordin's `escalate` call.
    pub recommendation: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoordinatorError {
    #[error("Ordin's session is not available")]
    Unavailable,
    #[error("Ordin's answer could not be used: {0}")]
    Unusable(String),
    #[error("the person stopped the crew")]
    StoppedByPerson,
}

/// One crew run's coordinator: Ordin, in its own harness session, which the Ordin Host owns.
/// Every method is one call the orchestrator makes to Ordin; each that needs a model costs one
/// Ordin turn, reported back in `turns` so the crew's request budget counts it.
// spec: ordin-desktop/components/CrewOrchestrator#SA-RECON-04
#[async_trait::async_trait]
pub trait Coordinator: Send + Sync + 'static {
    /// Coordinated mode: one coordinator turn. The orchestrator gives the purpose, the crew's state and
    /// the members' latest results; Ordin answers with the crew-tool calls it made in that turn
    /// (`delegate`, `finish`, `escalate`; SA-NAME-01). An empty list is treated as `escalate`.
    async fn round(&self, view: &CrewView) -> Result<CoordinatorTurn, CoordinatorError>;

    /// "Ordin checks the result" mark (ADR-064): compare one step's result with the purpose.
    async fn check_result(
        &self,
        view: &CrewView,
        step: &StepOutcome,
    ) -> Result<CheckVerdict, CoordinatorError>;

    /// A condition written in words (ADR-065): choose one path and say why.
    async fn decide_condition(
        &self,
        view: &CrewView,
        words: &str,
        paths: &[String],
    ) -> Result<PathDecision, CoordinatorError>;

    /// A member asked a question (section 5.16): Ordin answers from the request, the workspace and
    /// memory, or puts it to the person and returns the person's answer. May take as long as the
    /// person takes.
    async fn answer_member(
        &self,
        view: &CrewView,
        member: &AgentRef,
        question: &str,
    ) -> Result<MemberAnswer, CoordinatorError>;

    /// An "Ask me" block, a person-decided Repeat until, an `ask_me` *if it fails*, or a Repeat until at
    /// its maximum: Ordin shows a question with these choices in the conversation (a `question` block,
    /// 2C section 7) and returns the choice the person pressed, read by the application, never by a model.
    async fn ask_person(
        &self,
        view: &CrewView,
        question: PersonQuestion,
    ) -> Result<String, CoordinatorError>;

    /// The saved steps do not fit (ADR-064): Ordin takes over from `from_position` and coordinates
    /// round by round from now on. Positions are text ("2.1", "3a"). Only the orchestrator decides
    /// a take-over (R10); when Ordin wants one, it asks through `CrewOrchestrator::request_take_over`.
    async fn take_over(
        &self,
        view: &CrewView,
        from_position: String,
        why: TakeOverReason,
    ) -> Result<(), CoordinatorError>;

    /// The crew ended: Ordin compares the result with the request and reports (one Ordin turn,
    /// ADR-036), and, after a take-over, offers to update the workflow.
    async fn crew_ended(
        &self,
        view: &CrewView,
        outcome: &CrewOutcome,
    ) -> Result<(), CoordinatorError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentAvailability {
    Present { version: String },
    Missing,
}

/// What the program supplies: facts and words the orchestrator cannot produce itself.
// Implemented by: the desktop and personal-server programs, over the Package Catalogue
// (`agent_status`), the Ordin Host (`saved_agent_versions`, `workflow_steps`), the File Service
// (`document_changed`), Localisation (`word_step_approval`), the Rules Engine and the MCP Front
// (`startup_items`), and the remote-agent list (`remote_route`).
#[async_trait::async_trait]
pub trait CrewHost: Send + Sync + 'static {
    /// Package Catalogue / Catalogue Service: is the agent in this workspace, and at which version.
    async fn agent_status(&self, workspace_id: WorkspaceId, member: &AgentRef)
        -> AgentAvailability;
    /// The version each agent of the workflow was saved with (Ordin Host's workflow version).
    async fn saved_agent_versions(
        &self,
        workflow_id: WorkflowId,
        version: u32,
    ) -> Vec<(AgentRef, String)>;
    /// Ordin Host: the steps of another workflow of the workspace, for a "Run another workflow" block.
    async fn workflow_steps(
        &self,
        workspace_id: WorkspaceId,
        workflow_id: WorkflowId,
    ) -> Option<Vec<WorkflowStep>>;
    /// File Service: whether an input document changed since the workflow's last run (application condition).
    async fn document_changed(&self, workspace_id: WorkspaceId, input: &CrewInput) -> bool;
    /// Localisation: the words of an "Ask me first" card and of an approval point.
    fn word_step_approval(
        &self,
        view: &CrewView,
        step_label: &str,
        member: &AgentRef,
        line: &str,
    ) -> CardWording;
    /// Rules Engine lines, adapter verbs and tools for every member, worded (the crew's start-up report).
    async fn startup_items(&self, charter: &CrewCharter) -> Vec<StartUpReportItem>;
    /// Remote agents' route, for the supervisor's start request.
    fn remote_route(&self, remote_agent_id: RemoteAgentId) -> Option<AdapterKind>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CrewInput {
    pub label: String,
    pub value: CrewInputValue,
    pub used: Vec<InputUsed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CrewInputValue {
    Text { text: String },
    Documents { artefacts: Vec<ArtefactId> },
    List { items: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrewMode {
    Coordinated,
    SavedSteps,
}

pub struct CrewStart {
    pub charter: CrewCharter,
    pub inputs: Vec<CrewInput>,
    pub coordinator: Arc<dyn Coordinator>,
    /// The person pressed Start (UX-A35). False is accepted only with `trigger_id` (R11, ADR-074:
    /// the trigger was approved when it was set up).
    pub started_by_person: bool,
    /// The trigger whose event started this workflow run (Trigger Service through the Ordin Host's
    /// `workflows.run` immediate start); None for every other start.
    pub trigger_id: Option<TriggerId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlanCheck {
    pub fits: bool,
    pub problems: Vec<PlanProblem>,
    pub lowered: Vec<LimitLowered>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanProblem {
    AgentMissing {
        member: AgentRef,
    },
    AgentChanged {
        member: AgentRef,
        saved: String,
        present: String,
    },
    InputMissing {
        label: String,
    },
    TooManyMembers {
        count: u32,
    },
    StepsInvalid {
        code: String,
        position: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LimitLowered {
    pub limit: String,
    pub asked: u32,
    pub maximum: u32,
}
