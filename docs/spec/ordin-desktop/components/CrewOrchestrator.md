# SPEC: Crew Orchestrator

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-crew-orchestrator` · **Build phase:** 4 · **Programs:** D P
> **Dependencies:** Session Supervisor, Runtime Adapter interface, Rules Engine, Approval Service, Usage Planner, Organisation Profile, Journal

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

## Purpose

The Crew Orchestrator runs a crew from its charter (02:663–671; ADR-006, ADR-027, ADR-064, ADR-065). It validates the charter against the guardrail defaults and maxima, starts each member's step as a run through the Session Supervisor, and shows the crew's start-up report. When Ordin coordinates as it goes, it gives the coordinator the purpose, the crew's state and the members' latest results each round, receives the coordinator's crew-tool calls (`delegate`, `finish`, `escalate`), checks each against the guardrails in fixed code — is the member allowed, is the hand-off allowed, is there round, time and request budget left, does the step need the person's approval — dispatches the allowed step, and records the round. When the charter carries a saved workflow's steps, it walks the outline itself with no coordinator turn per step: agent steps in order, steps marked to run at the same time together, one path of each If, each item of a For each within the limit of agents at once, a Repeat until up to its maximum, another workflow's steps in place, and each step's *if it fails*; it decides application conditions itself, and calls Ordin only at *Ordin checks the result* marks, for conditions written in words, for a member's question, at the end, and when the steps do not fit — from then on Ordin coordinates round by round. Members never address each other; every message passes through the orchestrator. The crew stops on `finish`, `escalate`, a guardrail limit, the same member failing twice in a row, or the kill switch, and never continues on its own after a restart.

It owns the `Coordinator` trait (ADR-100 item 1; 2B-2 D7), which the Ordin Host implements and passes in when it starts a crew run, so the orchestrator never depends on the Ordin Host.

## Approach

One supervisor task per crew run drives a small state machine (`starting → running ⇄ paused → finished | escalated | stopped_at_limit | killed | failed`), in one of two modes: *coordinated* (a loop of coordinator turns) or *saved steps* (a recursive walk of the workflow outline with a cursor persisted after every step, so a restart can rebuild exactly where the crew stood). Every guardrail decision is a pure function of the charter, the counters and the call; every step is a Session Supervisor run whose events the orchestrator follows. Facts and words the orchestrator cannot produce itself — whether an agent is present at the right version, another workflow's steps, an approval card's words in the person's locale, the start-up report's sentences — come through a second trait it owns, `CrewHost`, implemented by the program. Error prefix: `CREW_` (2C also gives `CHRT_` and `ROUND_` for charter and round rules). Rejected: a coordinator turn per saved step — ADR-064 removed that cost. Rejected: letting a member's run start another member's run — members never address each other (ADR-006).

## Shared Context

### The `Coordinator` trait — owned here (ADR-100), implemented by the Ordin Host

See *Public Interface*; it is defined in this crate and nowhere else.

### From `ordin-types` (2C domain, copied)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CrewCharter {
    pub id: CrewCharterId, pub workspace_id: WorkspaceId, pub name: Option<String>, pub purpose: String,
    pub charter_version: u32,
    /// 2C enum `Coordinator { Ordin }`; imported here as `ordin_types::Coordinator as CoordinatorKind`
    /// because this crate's trait is also named `Coordinator`.
    pub coordinator: CoordinatorKind,
    pub members: Vec<CrewMember>, pub allowed_handoffs: Vec<Handoff>, pub guardrails: Guardrails,
    pub input_fields: Vec<String>, pub workflow_id: Option<WorkflowId>, pub workflow_version: Option<u32>,
    pub steps: Vec<WorkflowStep>, pub state: CrewCharterState, pub written_at: Timestamp,
}
pub struct CrewMember { pub member: AgentRef, pub role: String }
pub struct Handoff { pub from: AgentRef, pub to: AgentRef }
pub struct Guardrails {
    pub max_rounds: u32, pub wall_clock_secs: u32, pub request_budget: u32, pub max_members_running: u32,
    pub members_allowed_external_actions: Vec<AgentRef>, pub approval_points: Vec<String>, pub failure_repeat_limit: u32,
}
pub enum CrewCharterState { Written, Valid, Started, Kept, Discarded, Archived }
pub enum AgentRef { AgentInstance { agent_instance_id: AgentInstanceId }, RemoteAgent { remote_agent_id: RemoteAgentId } }

pub struct CrewRun {
    pub id: CrewRunId, pub workspace_id: WorkspaceId, pub crew_charter_id: CrewCharterId, pub charter_version: u32,
    pub workflow_id: Option<WorkflowId>, pub workflow_version: Option<u32>, pub taken_over_from_step: Option<String>,
    pub inputs: Vec<InputUsed>, pub state: CrewRunState, pub pause_reason: Option<CrewPauseReason>,
    pub round_counter: u32, pub budget: CrewBudgetCounters, pub end_reason: Option<String>,
    pub started_at: Timestamp, pub ended_at: Option<Timestamp>,
}
pub enum CrewRunState { Starting, Running, Paused, Finished, Escalated, StoppedAtLimit, Killed, Failed }
pub enum CrewPauseReason { Approval, Restart, Escalation }
pub struct CrewBudgetCounters { pub requests_used: u32, pub wall_clock_used_secs: u32, pub repeated_failures: u32 }
pub struct Round {
    pub crew_run_id: CrewRunId, pub number: u32, pub coordinator_decision: String,
    pub guardrail_decisions: Vec<String>, pub member_runs: Vec<RunId>, pub opened_at: Timestamp, pub closed_at: Option<Timestamp>,
}

pub enum WorkflowStep { Agent(AgentStep), If(IfBlock), Each(EachBlock), Until(UntilBlock), Call(CallBlock) } // tag "kind"
pub struct AgentStep { pub position: u32, pub group: Option<u32>, pub agent: AgentRef, pub line: String,
    pub uses: Vec<StepUse>, pub asks_first: bool, pub ordin_checks_result: bool, pub if_it_fails: IfItFails }
pub enum StepUse { Input { label: String }, Result { position: u32 } }                 // tag "kind"
pub enum IfItFails { OrdinTakesOver, AskMe, SkipIt }
pub struct IfBlock { pub position: u32, pub condition: Condition, pub paths: Vec<NamedPath> }
pub struct NamedPath { pub name: String, pub steps: Vec<WorkflowStep> }
pub struct EachBlock { pub position: u32, pub list: StepUse, pub maximum: u32, pub steps: Vec<WorkflowStep> }
pub struct UntilBlock { pub position: u32, pub condition: Condition, pub maximum: u32, pub steps: Vec<WorkflowStep> }
pub struct CallBlock { pub position: u32, pub workflow_id: WorkflowId }
pub enum Condition {                                                                  // tag "decided_by"
    Application { condition_kind: String, looks_at_position: u32 },
    Ordin { words: String },
    Person { words: String },
}
pub struct InputUsed { pub name: String, pub location: String, pub version_read: String }
```

2C validation copied: charter `purpose` 1–2,000; members 1–20 (LD-47; the application maximum of 8 members, 07 and 2D-4, is the bound applied here); member role 1–200; hand-offs between members (`CHRT_HANDOFF_INVALID`); each guardrail ≥ 1 and at or below the profile's maximum (`CHRT_GUARDRAIL_ABOVE_MAX`); members allowed external actions ⊆ members (`CHRT_EXTERNAL_NOT_MEMBER`); nothing starts until the person presses **Start** (`CHRT_NOT_STARTED_BY_PERSON`). Crew run: `workflow_id` and `workflow_version` both or neither (`CREW_WORKFLOW_MISMATCH`); `round_counter ≤ max_rounds` (`CREW_ROUNDS_EXCEEDED`); `requests_used ≤ request_budget` (`CREW_BUDGET_EXCEEDED`); `pause_reason` iff paused (`CREW_PAUSE_REASON_MISMATCH`); `end_reason ≤ 500`. Round: `(crew_run_id, number)` primary key from 1 with no gaps (`ROUND_NUMBER_GAP`); `coordinator_decision` 1–4,000 (`ROUND_DECISION_INVALID`); immutable once closed (`ROUND_IMMUTABLE`). Steps: each.maximum 1–50; until.maximum 1–10; nesting ≤ 4; ≤ 50 steps and blocks; calls ≤ 3 levels, no cycle; if 2–4 paths.

Crew run transitions (2C): starting → running; running → paused (approval, restart, escalation) → running; running → finished; running, paused → escalated; running → stopped_at_limit; starting, running, paused → killed; starting, running → failed.

`ServiceEvent` emitted (2C wire §1): `CrewRoundChanged { crew_run_id, round, state }`, `StartUpReportReady { subject: CrewRun { crew_run_id }, items }`, `NeedsYouChanged { workspace_id }` (F2).

### Other crates used

- `ordin-session-supervisor`: `start_run(StartRequest) -> Result<RunId, SupervisorError>` with `StartRequest { origin: RunOrigin::CrewRound { crew_run_id, round_number }, workspace_id, target, input, attachments, limits, work_kind, work_key, inputs_used, thread, started_by_person, model_override: Option<LevelChoice>, coding_task_id: Option<CodingTaskId> }` (the supervisor sets `StartRun.caller = CrewMember { crew_run_id }` from the `CrewRound` origin; this crate passes `coding_task_id: None`); `subscribe(run_id) -> broadcast::Receiver<RunEvent>`; `cancel(run_id)`; `resume_with_decision(run_id, ResumeDecision::Answer { text })`; `get(run_id) -> RunView`.
- `ordin-runtime-adapter`: `RunTarget`, `StartInput`, `RunLimits`, `RunEvent`, `RunEnd`, `RunEndReason`, `PauseReason`, `ResumeDecision`, `UsageDelta`.
- `ordin-rules`: `restrict_run(run_id, RunRestriction { external_actions_allowed })`; `CardWording`.
- `ordin-approval`: `request(ApprovalRequest) -> ApprovalCard`; `wait(approval_id, None) -> WaitOutcome`; `expire_for_crew_run(crew_run_id)`.
- `ordin-usage-planner`: `step_up(&LevelChoice, WorkKind, AdapterKind) -> Option<LevelChoice>`; `record_run_finished(work_key: &str, run_id, model_turns: u32) -> Result<(), UsageError>`; `agents_at_once() -> u32`; `WorkKind`; `LevelChoice`.
- `ordin-organisation-profile`: `current()`; keys `limits.crew.rounds` (default 10, 1–30), `limits.crew.wall_clock_minutes` (60, 1–480), `limits.crew.members_at_once` (4, 1–8), `limits.crew.request_budget` (none until OQ-3).
- `ordin-journal` (Journal spec, B2 form, R3): `Datastore` (the program passes `journal.datastore().clone()`; D4), `ModuleMigrations` of `MigrationStep` tuples; `Journal::append_event(&self, kind: &str, data) -> Result<JournalEvent, JournalError>` with 2C `EventKind` names ("emit" in this spec); `Journal::append_entry(NewJournalEntry { kind, component: "crew_orchestrator", run_id, crew_run_id, workspace_id, summary, data })` ("journal X" in this spec); `PruneParticipant`, `PruneRequest`, `PruneFailure`, `register_prune_participant`.
- `ordin-types` (2C domain): `TriggerId` (Trigger Service's entity).

### Fixed constants (2D-4)

Crew members: 8 plus the coordinator. Rounds: application maximum 30. Wall clock: application maximum 8 hours. Members at once: application maximum 8. Same member failing in a row before escalation: 2 (and a saved step is tried twice before its *if it fails* applies). Workflow call depth 3; nesting 4; steps and blocks 50.

## Public Interface

Crate `ordin-crew-orchestrator`; names *chosen here* unless marked 2C.

### The `Coordinator` trait (owned here; the Ordin Host implements it)

```rust
/// One crew run's coordinator: Ordin, in its own harness session, which the Ordin Host owns.
/// Every method is one call the orchestrator makes to Ordin; each that needs a model costs one
/// Ordin turn, reported back in `turns` so the crew's request budget counts it.
#[async_trait::async_trait]
pub trait Coordinator: Send + Sync + 'static {
    /// Coordinated mode: one coordinator turn. The orchestrator gives the purpose, the crew's state and
    /// the members' latest results; Ordin answers with the crew-tool calls it made in that turn
    /// (`delegate`, `finish`, `escalate`; SA-NAME-01). An empty list is treated as `escalate`.
    async fn round(&self, view: &CrewView) -> Result<CoordinatorTurn, CoordinatorError>;

    /// "Ordin checks the result" mark (ADR-064): compare one step's result with the purpose.
    async fn check_result(&self, view: &CrewView, step: &StepOutcome) -> Result<CheckVerdict, CoordinatorError>;

    /// A condition written in words (ADR-065): choose one path and say why.
    async fn decide_condition(&self, view: &CrewView, words: &str, paths: &[String])
        -> Result<PathDecision, CoordinatorError>;

    /// A member asked a question (§5.16): Ordin answers from the request, the workspace and memory,
    /// or puts it to the person and returns the person's answer. May take as long as the person takes.
    async fn answer_member(&self, view: &CrewView, member: &AgentRef, question: &str)
        -> Result<MemberAnswer, CoordinatorError>;

    /// An "Ask me" block, a person-decided Repeat until, an `ask_me` *if it fails*, or a Repeat until at
    /// its maximum: Ordin shows a question with these choices in the conversation (a `question` block,
    /// 2C §7) and returns the choice the person pressed, read by the application, never by a model.
    async fn ask_person(&self, view: &CrewView, question: PersonQuestion) -> Result<String, CoordinatorError>;

    /// The saved steps do not fit (ADR-064): Ordin takes over from `from_position` and coordinates
    /// round by round from now on. Ordin says so in one line ("Step 3 failed twice, so I've taken over from there").
    /// Positions are text ("2.1", "3a"). Only the orchestrator decides a take-over (R10); when Ordin
    /// wants one, it asks through `CrewOrchestrator::request_take_over`.
    async fn take_over(&self, view: &CrewView, from_position: String, why: TakeOverReason) -> Result<(), CoordinatorError>;

    /// The crew ended: Ordin compares the result with the request and reports (one Ordin turn, ADR-036),
    /// and, after a take-over, offers to update the workflow.
    async fn crew_ended(&self, view: &CrewView, outcome: &CrewOutcome) -> Result<(), CoordinatorError>;
}

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
pub struct CoordinatorTurn { pub calls: Vec<CrewToolCall>, pub turns: u32 }

/// The crew tools of SA-NAME-01 that reach the orchestrator; their MCP arguments are checked by the
/// Ordin Host's provider before they arrive here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum CrewToolCall {
    Delegate {
        member: AgentRef,
        /// The brief in one line to a few sentences (≤ 2,000 characters).
        brief: String,
        /// Members whose latest results this step uses: each is a hand-off from that member.
        uses_results_of: Vec<AgentRef>,
        /// Inputs by label.
        uses_inputs: Vec<String>,
    },
    Finish { summary: String },
    /// `recommendation`: Ordin's suggested answer, shown on the person's question card (ADR-027).
    Escalate { question: String, recommendation: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum CheckVerdict { CarryOn { turns: u32 }, Shortfall { reason: String, turns: u32 } }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PathDecision { pub path: String, pub reason: String, pub turns: u32 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemberAnswer { pub text: String, pub answered_by: AnsweredBy, pub turns: u32 }
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnsweredBy { Ordin, Person }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PersonQuestion { pub kind: PersonQuestionKind, pub words: String, pub choices: Vec<String> }
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonQuestionKind { AskMe, RepeatUntil, RepeatAtMaximum, StepFailedTwice }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TakeOverReason {
    StepFailedTwice { position: String },
    CheckFoundShortfall { position: String, reason: String },
    AgentMissing { member: AgentRef },
    AgentChanged { member: AgentRef },
    InputMissing { label: String },
    /// Ordin asked for the take-over and the orchestrator accepted it (`request_take_over`).
    OrdinAsked { position: String, reason: String },
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
    #[error("Ordin's session is not available")] Unavailable,
    #[error("Ordin's answer could not be used: {0}")] Unusable(String),
    #[error("the person stopped the crew")] StoppedByPerson,
}
```

### What the program supplies

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentAvailability { Present { version: String }, Missing }

#[async_trait::async_trait]
pub trait CrewHost: Send + Sync + 'static {
    /// Package Catalogue / Catalogue Service: is the agent in this workspace, and at which version.
    async fn agent_status(&self, workspace_id: WorkspaceId, member: &AgentRef) -> AgentAvailability;
    /// The version each agent of the workflow was saved with (Ordin Host's workflow version).
    async fn saved_agent_versions(&self, workflow_id: WorkflowId, version: u32) -> Vec<(AgentRef, String)>;
    /// Ordin Host: the steps of another workflow of the workspace, for a "Run another workflow" block.
    async fn workflow_steps(&self, workspace_id: WorkspaceId, workflow_id: WorkflowId) -> Option<Vec<WorkflowStep>>;
    /// File Service: whether an input document changed since the workflow's last run (application condition).
    async fn document_changed(&self, workspace_id: WorkspaceId, input: &CrewInput) -> bool;
    /// Localisation: the words of an "Ask me first" card and of an approval point.
    fn word_step_approval(&self, view: &CrewView, step_label: &str, member: &AgentRef, line: &str) -> CardWording;
    /// Rules Engine lines, adapter verbs and tools for every member, worded (the crew's start-up report).
    async fn startup_items(&self, charter: &CrewCharter) -> Vec<StartUpReportItem>;
    /// Remote agents' route, for the supervisor's start request.
    fn remote_route(&self, remote_agent_id: RemoteAgentId) -> Option<AdapterKind>;
}
```

### The orchestrator

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CrewInput { pub label: String, pub value: CrewInputValue, pub used: Vec<InputUsed> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CrewInputValue { Text { text: String }, Documents { artefacts: Vec<ArtefactId> }, List { items: Vec<String> } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrewMode { Coordinated, SavedSteps }

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
pub struct PlanCheck { pub fits: bool, pub problems: Vec<PlanProblem>, pub lowered: Vec<LimitLowered> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanProblem { AgentMissing { member: AgentRef }, AgentChanged { member: AgentRef, saved: String, present: String },
    InputMissing { label: String }, TooManyMembers { count: u32 }, StepsInvalid { code: String, position: String } }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LimitLowered { pub limit: String, pub asked: u32, pub maximum: u32 }

#[derive(Debug, thiserror::Error)]
pub enum CrewError {
    #[error("charter invalid: {code}")] CharterInvalid { code: &'static str },
    #[error("not started by the person")] NotStartedByPerson,
    #[error("crew run not found")] NotFound,
    #[error("crew run already ended")] Ended,
    #[error("crew run not paused")] NotPaused,
    #[error("too many crew runs")] Busy,
    #[error("take-over refused: {reason}")] TakeOverRefused { reason: &'static str },
    #[error("datastore unavailable")] Store(#[source] sqlx::Error),
    #[error("journal unavailable")] Journal(#[source] JournalError),
}
impl CrewError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }

pub struct CrewOrchestrator { /* private */ }
impl CrewOrchestrator {
    pub const MODULE: &'static str = "crew_orchestrator";
    pub fn migrations() -> ModuleMigrations;   // module "crew_orchestrator" (ModuleMigrations.module)
    /// Called by: the desktop and personal-server programs at start.
    pub fn new(datastore: Datastore, journal: Arc<Journal>, supervisor: Arc<SessionSupervisor>,
               rules: Arc<RulesEngine>, approvals: Arc<ApprovalService>, usage: Arc<UsagePlanner>,
               profile: Arc<OrganisationProfileService>, host: Arc<dyn CrewHost>) -> Arc<Self>;
    /// Does the workflow or crew fit (03 Run a saved workflow). Called by: Ordin Host, after it writes
    /// the charter (a workflow file is converted to 2C `WorkflowStep`s in `charter.steps` first).
    pub async fn check_plan(&self, charter: &CrewCharter, inputs: &[CrewInput]) -> PlanCheck;
    /// The person pressed Start: validates, records the crew run, and starts driving it.
    /// Called by: Ordin Host (there is no `start_crew_run`).
    pub async fn start(&self, start: CrewStart) -> Result<CrewRunId, CrewError>;
    /// Kill switch / **Stop crew** (after the person's confirmation, 11 §5.5): every member run is cancelled.
    /// Called by: the program for Service API `crews.stop`; Ordin Host (`stop_workflow_run`, for the
    /// Trigger Service).
    pub async fn kill(&self, crew_run_id: CrewRunId) -> Result<(), CrewError>;
    /// After a restart the person presses **Continue**. Called by: the program for Service API
    /// `crews.continue`, with the coordinator it obtains from the Ordin Host's `coordinator()`.
    pub async fn continue_crew(&self, crew_run_id: CrewRunId, coordinator: Arc<dyn Coordinator>) -> Result<(), CrewError>;
    /// Called by: the program for Service API `crews.get` (`{crew_run, rounds}`); Ordin Host
    /// (`crew_run_state`, for the Trigger Service); the desktop and server programs' `SessionSetupSource`
    /// for the Claude and Copilot Local Adapters (a crew member run's `crew_charter_id`).
    pub async fn get(&self, crew_run_id: CrewRunId) -> Result<(CrewRun, Vec<Round>), CrewError>;
    /// The crew run of a workspace that is starting, running or paused, if any (one indexed read).
    /// Called by: Ordin Host (one crew at a time per workspace, `OH_CREW_RUNNING`).
    pub async fn running_crew(&self, workspace_id: WorkspaceId) -> Result<Option<CrewRunId>, CrewError>;
    /// Ordin asks to take over a saved-steps crew from `from_position` (R10): the orchestrator
    /// decides (D.8) and, when it accepts, carries the take-over out exactly as D.6.
    /// Called by: Ordin Host (Ordin's `take_over` tool).
    pub async fn request_take_over(&self, crew_run_id: CrewRunId, from_position: String, reason: String)
        -> Result<(), CrewError>;
    /// At start, before any crew is started: every crew in progress becomes paused (restart).
    /// Called by: the desktop and personal-server programs, after the Session Supervisor's `recover`.
    pub async fn recover(&self) -> Result<u32, CrewError>;
    /// The Journal's pruning hook (Data Model). Called by: the programs.
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant>;
}
// `CrewHost` is implemented by: the desktop and personal-server programs, over the Package
// Catalogue (`agent_status`), the Ordin Host (`saved_agent_versions`, `workflow_steps`), the File
// Service (`document_changed`), Localisation (`word_step_approval`), the Rules Engine and the MCP
// Front (`startup_items`), and the remote-agent list (`remote_route`).
// `Coordinator` is implemented by: the Ordin Host (`OrdinHost::coordinator()`).
```

Service API commands (SA-API-01, *chosen here*): `crews.get` {crew_run_id} → `{crew_run, rounds}`; `crews.stop` {crew_run_id} → `{}` (`kill`); `crews.continue` {crew_run_id} → `{}` (the program obtains the coordinator from the Ordin Host and calls `continue_crew`). Starting goes through the Ordin Host (`start` is called in-process when the person presses **Start** on the crew plan).

### Example

Workflow "Supplier rates refresh" version 3, saved-steps mode, two members (`analyst`, `writer`), steps: `1` For each over input "Quotes" (max 20) with step `1.1` (analyst); `2` If decided by Ordin "Any rate rose by more than 5 per cent" with paths "Yes" → `2.1` (writer, asks first, Ordin checks) and "No" → none.

1. `start` validates; crew run `0192a1d0-…` `starting` → `running`; start-up report emitted.
2. Round 1–3: step `1.1` for each of the 3 quotes, up to `max_members_running` 4 at once → three member runs.
3. Round 4: `decide_condition("Any rate rose by more than 5 per cent", ["Yes","No"])` → `{path:"Yes", reason:"Kestrel's rate rose 7 per cent", turns:1}`.
4. Step `2.1` asks first: an approval card "Start step 2.1: Update the summary and flag the rises?" (words from `CrewHost`); approved; round 5 runs the writer; `check_result` → `CarryOn`.
5. Outline done → `finished`; `crew_ended` called; journal holds 5 rounds with their guardrail decisions.

## Internal Logic

**A. `check_plan(charter, inputs)`.**
1. Members: count > 8 → `TooManyMembers`. Each member's `host.agent_status`: `Missing` → `AgentMissing`; with a workflow, the version differs from `host.saved_agent_versions` → `AgentChanged` (*fits* stays true for a changed version: ADR-064 lets the run start and Ordin takes over at that step; the plan shows it).
2. Each required input field of the charter (`input_fields`) absent from `inputs` → `InputMissing`.
3. Steps (saved mode): re-run the 2C step checks (nesting, counts, positions, uses on path, each/until maxima, call depth and cycles through `host.workflow_steps`); the first failure → `StepsInvalid { code: <2C STEP_* code>, position }`.
4. Limits: each guardrail above the company maximum (`limits.crew.*`) or the application maximum → lowered and listed in `lowered` (07: lowered and said so).
5. `fits` = no `AgentMissing`, `InputMissing`, `TooManyMembers` or `StepsInvalid`.

**B. `start(start)`.**
1. `started_by_person` false and `trigger_id` None → `CREW_NOT_STARTED_BY_PERSON` (2C `CHRT_NOT_STARTED_BY_PERSON`). With a `trigger_id` the charter must carry a saved workflow (`workflow_id` present), else `CREW_NOT_STARTED_BY_PERSON` too (a trigger starts saved workflows only); the crew run's journal entry `crew_started` records the trigger.
2. Validate the charter (2C rules); then `check_plan`; not `fits` → `CREW_CHARTER_INVALID` with the first problem's code. Guardrails are lowered to the maxima (A.4) before use.
3. Mode: `SavedSteps` when `charter.steps` is not empty, else `Coordinated`.
4. Insert the `crew_runs` row (`starting`, `round_counter` 0, budget zeros, `inputs` from each input's `used`); journal `crew_started`.
5. Start-up report: `host.startup_items(charter)` → `journal.store_startup_report(StartupReport { subject: CrewRun { crew_run_id }, … }, false)`; emit `StartUpReportReady`.
6. State `running`; emit `CrewRoundChanged { round: 0, state: running }`; spawn the driver task (C or D). Return the id.

**C. Coordinated mode — each round.**
1. Before the turn: check limits (F.1); a limit reached → stop (G).
2. `coordinator.round(view)`; `turns` added to `requests_used`. `CoordinatorError::Unavailable` twice in a row → `failed`, end reason "Ordin could not continue the crew"; `StoppedByPerson` → `killed`.
3. Open round `n = round_counter + 1` (`rounds` row, `opened_at`); `coordinator_decision` = a one-line rendering of the calls (`delegate <member>: <brief cut to 200>`, `finish`, `escalate`).
4. For each call in order:
   - `Finish { summary }` → close the round; state `finished`, end reason `summary` cut to 500; G.
   - `Escalate { question, recommendation }` → close the round; state `escalated`, end reason the question, `CrewOutcome.recommendation` the recommendation; G.
   - `Delegate` → F.2 guardrail checks; a refusal is recorded in `guardrail_decisions` (`refused: <reason>`) and reported to the coordinator in the next view as a result with state `failed` and summary "Refused: <reason>" (no model call in the decision, 03 crew sequence); allowed → E (dispatch) — several delegates in one turn run at the same time within `max_members_running`.
5. Wait for every dispatched step to end; close the round (`closed_at`, `member_runs`); emit `CrewRoundChanged`; loop.

**D. Saved-steps mode — walking the outline** (ADR-064, ADR-065). A cursor (the position path being executed and per-block counters) is persisted in `crew_step_states` after every step, so G.4 can rebuild it.
1. **Agent steps.** Consecutive steps sharing a `group` start together, at most min(`max_members_running`, `usage.agents_at_once()`) at once (the rest wait; ADR-062). Each step: inputs resolved from `uses` (`Input` → the crew input; `Result` → the outcome of that earlier position on this path; an absent input or result → take over with `InputMissing`); agent present at the saved version (host) — missing → take over `AgentMissing`; another version → take over `AgentChanged`.
   - `asks_first`, or an approval point naming this step (`step:<position>`) or its member (`member:<agent key>`, *assumed here*, SA-CREW-01: the form of `Guardrails.approval_points` entries) → H (approval) before dispatch; rejected → the crew is `escalated` with end reason "The person chose not to run step <position>." (*assumed here*, SA-CREW-02).
   - Dispatch (E). A step that ends `failed` or `stopped_at_limit` is tried again once; the retry asks `usage.step_up` and passes the stepped-up choice as `model_override` when the first run was on a lower level (ADR-062: a step that falls short is tried once on the next level). A second failure → `IfItFails`: `OrdinTakesOver` → take over `StepFailedTwice`; `AskMe` → `coordinator.ask_person(StepFailedTwice, choices ["Try again", "Skip it", "Stop"])` (*chosen here*): "Try again" → one more attempt, then take over if it fails; "Skip it" → mark skipped; "Stop" → `escalated`; `SkipIt` → mark skipped and carry on.
   - `ordin_checks_result` → `coordinator.check_result`; `Shortfall` → take over `CheckFoundShortfall`.
   - Each step (or group) is one round (F.1 limits are checked before it; every step a block repeats counts, ADR-065).
2. **If.** Decide: `Application` — `list_has_items` (the result at `looks_at_position` has at least one artefact, *assumed here*, SA-CREW-03: a step's list result is its artefacts), `step_failed` (that step ended failed or was skipped), `check_found_shortfall` (its check returned a shortfall), `document_changed` (`host.document_changed` for the input the step used); the first path is taken when the condition holds, the second otherwise (*assumed here*, SA-CREW-04). `Ordin { words }` → `coordinator.decide_condition(words, path names)`; an unknown path name → `CoordinatorError::Unusable` → take over. `Person { words }` → `coordinator.ask_person(AskMe, words, path names)`. Journal `crew_path_taken` {position, path, reason, decided_by} (11: "Took Yes: … — one Ordin turn"). Walk the chosen path's steps; empty path carries on.
3. **For each.** Items: a `List` input's items, a `Documents` input's artefacts, or an earlier step's artefacts; more than `maximum` → the first `maximum` (journal `crew_items_cut`). Run the block's steps once per item, items in parallel up to `max_members_running`; each item's step receives the item as an extra input named "item".
4. **Repeat until.** Run the steps; then decide the condition as in D.2 (holds → leave). After `maximum` passes without holding → `coordinator.ask_person(RepeatAtMaximum, choices ["Carry on with the last result", "Stop"])` (ADR-065); "Stop" → `escalated`.
5. **Run another workflow.** `host.workflow_steps(workspace, workflow_id)`; `None` → take over (`AgentMissing` is not the reason; *assumed* `InputMissing { label: "workflow" }`); otherwise walk them in place, numbering under the block (`3.1`, `3.1.1`), with the same counters; depth beyond 3 → `failed` (`STEP_CALL_INVALID`, a check A.3 should have caught).
6. **Take over.** `coordinator.take_over(view, position, reason)` (`position` as text); set `taken_over_from_step`; switch to C for the rest of the run; each step the coordinator then dispatches is journalled `crew_step_changed_by_ordin` {position}.
7. Outline finished → `finished`, end reason "all steps done"; G.
8. **`request_take_over(id, from_position, reason)`** (R10: the orchestrator alone decides). Unknown → `CREW_NOT_FOUND`; ended → `CREW_ENDED`; mode already coordinated → `CREW_TAKE_OVER_REFUSED` ("Ordin already coordinates this crew"); `reason` empty or over 500 characters → `CREW_TAKE_OVER_REFUSED` ("a reason is needed"); `from_position` not a position of the outline, or a position already `done` or `skipped` in `crew_step_states` → `CREW_TAKE_OVER_REFUSED` ("that step is not ahead of the crew"). Otherwise: wait for the steps running now to end (they are not cancelled), then D.6 with `TakeOverReason::OrdinAsked { position, reason }`. Journal `crew_take_over_requested` {crew_run_id, position, accepted}.

**E. Dispatch one step to a member.**
1. Target: `AgentInstance` → `RunTarget::AgentInstance`; `RemoteAgent` → `RunTarget::RemoteAgent` (route from `host.remote_route`).
2. Brief (ADR-062 *Focused briefs*), built in fixed code as `StartInput::FreeText { text }`: "Purpose: <purpose>\nYour part: <line or brief>\nInputs:\n- <label>: <location> (version <v>)…\nUse these earlier results:\n- <label>: <summary> (artefacts: <ids>)…\nReturn a short summary and references to what you made.\nLimits: <minutes> minutes." The conversation is never included.
3. Limits: `wall_clock_secs` = min(remaining crew time, 8 h); `request_budget` = remaining crew budget (None when the charter has none); idle default.
4. `supervisor.start_run(StartRequest { origin: CrewRound { crew_run_id, round_number }, work_kind: Documents (*assumed here*, SA-CREW-05), work_key: "crew:<charter id>:<member key>", attachments: input artefacts, started_by_person: false, model_override, … })`; then `rules.restrict_run(run_id, RunRestriction { external_actions_allowed: member ∈ members_allowed_external_actions })` before the run's first tool call (the restriction is held by the Rules Engine for the run's start, Rules Engine spec).
5. Follow `supervisor.subscribe(run_id)`: `Usage` → add `model_turns` to `requests_used`; `Paused { Person { question } }` → `coordinator.answer_member(view, member, question)` → `supervisor.resume_with_decision(run_id, Answer { text })`, turns counted; `Paused { Approval }` → the crew run is `paused` with reason `approval` until `Resumed` (the Approval Service and the Hook Relay settle it); `Ended { end }` → the step's `StepOutcome` (summary = `final_message` cut to 2,000 characters; artefacts).
6. A remote member that is unreachable (`failed` with `RA_UNREACHABLE`) counts as a failure (03: two consecutive failures escalate).

**F. Guardrails** (fixed code; every decision appended to the round's `guardrail_decisions` and journalled).
1. Limits before each round or step: `round_counter ≥ max_rounds` → stop (`rounds`); `wall_clock_used_secs ≥ wall_clock_secs` (running time only; time paused for an approval, a person's answer or a restart does not count, *assumed here*, SA-CREW-06) → stop (`time`); `request_budget > 0` and `requests_used ≥ request_budget` → stop (`requests`).
2. A `delegate` call: member not in `members` → refused "This agent is not in the crew."; each `uses_results_of` member X with `(X → member)` not in `allowed_handoffs` → refused "This hand-off is not allowed." (an empty `allowed_handoffs` allows no hand-off between members, *assumed here*, SA-CREW-07); an input label not in the crew's inputs → refused "This input is not part of the crew."; an approval point for the member → H first; members running = `max_members_running` → the step waits for a free place (not a refusal).
3. Same member failing: `repeated_failures` counts consecutive failed steps of the same member; reaching `failure_repeat_limit` (default 2) → `escalated` with end reason "<member> failed twice in a row." (coordinated mode; in saved-steps mode D.1 applies the step's *if it fails* instead, ADR-065).
4. A limit reached → state `stopped_at_limit`, the state kept; the coordinator is told through `crew_ended`, which puts the question to the person (03: "the crew stops and escalates to the person, with the state kept").

**F2. Needs you.** Emit `needs_you_changed { workspace_id }` (`journal.append_event(EventKind::NeedsYouChanged.as_str(), …)`) when the crew enters `escalated` or `stopped_at_limit`, when it becomes `paused` with reason `restart` or `escalation`, and when it leaves `paused` for `running`; a failure is logged `crew_emit_failed` and never stops the crew.

**G. Ending.** Cancel any member run still running (`supervisor.cancel`); `approvals.expire_for_crew_run`; update `crew_runs` (`ended_at`, `end_reason`); emit `CrewRoundChanged` with the final state; `usage.record_run_finished("crew:<charter id>" or "workflow:<id>:<version>", …)` with the crew's total turns; call `coordinator.crew_ended(view, outcome)` (an error is logged; the crew has ended either way); journal `crew_ended`.

**H. Approvals for steps.** `wording = host.word_step_approval(view, label, member, line)`; `approvals.request(ApprovalRequest { asked_by: CrewRound { crew_run_id, round }, question, verb, where_line, affects, changes, content: None, reject_style: NotNow, show_details: false, proposal: wording.proposal, asker: None, leaves_company: false, workspace_id })`; crew state `paused` (reason `approval`); `approvals.wait(id, None)` (no deadline: a card is never decided by timeout); approved → `running` and dispatch; rejected → D.1's rule; expired → the crew is ending.

**I. `kill(crew_run_id)`.** Unknown → `CREW_NOT_FOUND`; ended → `CREW_ENDED`. Signal the driver; cancel every member run (`supervisor.cancel`; remote runs that cannot be cancelled become `abandoned` in the supervisor and their results are not used); state `killed`, end reason "Stopped by you."; G.

**I2. `running_crew(workspace_id)`.** `SELECT id FROM crew_runs WHERE workspace_id = ? AND state IN ('starting','running','paused') ORDER BY started_at DESC LIMIT 1` (index `crew_runs_workspace`). Store error → `CREW_STORE_UNAVAILABLE`.

**J. Recovery and continue.**
1. `recover()`: every crew run in `starting`, `running` or `paused` → `paused` with reason `restart`; rebuild nothing yet; emit `CrewRoundChanged`. Crews never continue on their own (03, P8).
2. `continue_crew(id, coordinator)`: not paused for restart → `CREW_NOT_PAUSED`. Rebuild from the store: counters, the open round (closed if its member runs ended), the saved-steps cursor; member runs still in progress are followed again through the supervisor; a member run the supervisor marked `interrupted` counts as a failed attempt of its step. State `running`; resume C or D.

## Data Model

Module `crew_orchestrator`. SQLite on the desktop and the personal server (2B-3: programs D P; no PostgreSQL variant — a crew run as one hosted agent waits, OQ-64).

```sql
-- SQLite, migration 1
CREATE TABLE crew_runs (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL,
    crew_charter_id TEXT NOT NULL,
    charter_version INTEGER NOT NULL,
    charter TEXT NOT NULL,                                 -- JSON CrewCharter as started (guardrails lowered)
    mode TEXT NOT NULL CHECK (mode IN ('coordinated', 'saved_steps')),
    workflow_id TEXT, workflow_version INTEGER,
    taken_over_from_step TEXT,                             -- position text, "3" or "2.1" (2C)
    inputs TEXT NOT NULL,                                  -- JSON Vec<CrewInput>
    state TEXT NOT NULL CHECK (state IN ('starting','running','paused','finished','escalated','stopped_at_limit','killed','failed')),
    pause_reason TEXT CHECK (pause_reason IN ('approval', 'restart', 'escalation')),
    round_counter INTEGER NOT NULL DEFAULT 0,
    requests_used INTEGER NOT NULL DEFAULT 0,
    wall_clock_used_secs INTEGER NOT NULL DEFAULT 0,
    repeated_failures INTEGER NOT NULL DEFAULT 0,
    end_reason TEXT CHECK (end_reason IS NULL OR length(end_reason) <= 500),
    started_at TEXT NOT NULL, ended_at TEXT,
    CHECK ((workflow_id IS NULL) = (workflow_version IS NULL)),
    CHECK ((state = 'paused') = (pause_reason IS NOT NULL))
);
CREATE INDEX crew_runs_in_progress ON crew_runs (state) WHERE state IN ('starting', 'running', 'paused');
CREATE INDEX crew_runs_workspace ON crew_runs (workspace_id, state);
CREATE TABLE rounds (
    crew_run_id TEXT NOT NULL REFERENCES crew_runs (id),
    number INTEGER NOT NULL CHECK (number >= 1),
    coordinator_decision TEXT NOT NULL CHECK (length(coordinator_decision) BETWEEN 1 AND 4000),
    guardrail_decisions TEXT NOT NULL,                     -- JSON Vec<String>
    member_runs TEXT NOT NULL,                             -- JSON Vec<RunId>
    opened_at TEXT NOT NULL, closed_at TEXT,
    PRIMARY KEY (crew_run_id, number)
);
CREATE TABLE crew_step_states (
    crew_run_id TEXT NOT NULL REFERENCES crew_runs (id),
    position TEXT NOT NULL,                                -- "1", "1.1#2" (item 2 of a For each), "2.1"
    state TEXT NOT NULL CHECK (state IN ('waiting','running','done','failed','skipped','changed_by_ordin')),
    attempts INTEGER NOT NULL DEFAULT 0,
    run_id TEXT,
    outcome TEXT,                                          -- JSON StepOutcome
    decided_path TEXT,                                     -- If blocks: the path taken
    passes INTEGER,                                        -- Repeat until: passes made
    PRIMARY KEY (crew_run_id, position)
);
```

Rows are never pruned while the crew run is in progress; afterwards they follow the Journal's history retention (04:386) through this module's `PruneParticipant` (`module()` = `crew_orchestrator`): `prune` deletes ended crew runs with `ended_at < request.cutoff` and their rounds and step states; a store error → `PruneFailure { code: "CREW_STORE_UNAVAILABLE" }`. Migrations are `MigrationStep` tuples (R3). Start-up reports: the crew's report is also handed to the Journal (`store_startup_report`, subject `CrewRun`), which holds every start-up report (2C); the `crew_startup_reports` table is dropped from migration 1. Round rows are not updated after `closed_at` is set (no statement does so).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Start without the person's press (B.1) | `Err(permanent)` | `CREW_NOT_STARTED_BY_PERSON` | `CrewError::NotStartedByPerson` |
| Charter breaks a 2C rule or the plan does not fit (B.2) | `Err(permanent)` | `CREW_CHARTER_INVALID` (with the 2C `CHRT_*` or `STEP_*` code) | `CrewError::CharterInvalid { code }` |
| More than 8 members (A.1) | `Err(permanent)` | `CREW_TOO_MANY_MEMBERS` | `PlanProblem::TooManyMembers` |
| Kill, continue or get of an unknown crew run (I, J) | `Err(permanent)` | `CREW_NOT_FOUND` | Service API "This crew run no longer exists." |
| Kill of an ended crew run (I) | `Err(permanent)` | `CREW_ENDED` | Service API "This crew has already stopped." |
| Continue of a crew not paused by a restart (J.2) | `Err(permanent)` | `CREW_NOT_PAUSED` | `CrewError::NotPaused` |
| Delegate refused by a guardrail (F.2) | refusal to the coordinator, crew continues | `CREW_GUARDRAIL_REFUSED` | step result "Refused: This hand-off is not allowed." |
| Round, time or request limit (F.1) | crew stopped | `CREW_LIMIT_REACHED` | state `stopped_at_limit`, end reason "Stopped after 10 rounds." |
| Same member failed twice (F.3) | crew escalated | `CREW_MEMBER_FAILED_TWICE` | end reason "<member> failed twice in a row." |
| Ordin unavailable twice (C.2) | crew failed | `CREW_COORDINATOR_UNAVAILABLE` | end reason "Ordin could not continue the crew." |
| Ordin's answer unusable (D.2) | take over, then as C | `CREW_COORDINATOR_UNUSABLE` | journalled with the position |
| A member run cannot be started (E.4) | step failed | the supervisor's code (`SUP_NO_ADAPTER`, `RA_*`) | counted as a failed attempt |
| Store unavailable | `Err(retryable)`; in a driver, crew paused with reason `escalation` | `CREW_STORE_UNAVAILABLE` | `CrewError::Store` |
| Ordin asks for a take-over the orchestrator does not accept (D.8) | `Err(permanent)` | `CREW_TAKE_OVER_REFUSED` | `CrewError::TakeOverRefused { reason }`; Ordin's tool answers with the reason |

Person-facing words above come from the message catalogue keys `crew-end-rounds`, `crew-end-time`, `crew-end-requests`, `crew-end-member-failed`, `crew-end-stopped`, `crew-refused-member`, `crew-refused-handoff`, `crew-refused-input` (*chosen here*), resolved by the screens from the code and its fields; the end reasons stored in English are for Diagnostics.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Crew Orchestrator

  Scenario: Happy path — coordinated crew finishes
    Given a charter with members analyst and writer and max_rounds 10
    And the person pressed Start
    When the coordinator delegates to analyst, then to writer using analyst's result, then calls finish
    Then three rounds are recorded with their guardrail decisions
    And the crew run is finished and crew_ended was called once

  Scenario: Refused hand-off
    Given allowed_handoffs holds only analyst → writer
    When the coordinator delegates to analyst using writer's result
    Then the step is refused with "This hand-off is not allowed." and no member run starts
    And the next round's view shows the refusal

  Scenario: Agent not in the crew
    When the coordinator delegates to an agent that is not a member
    Then the step is refused with "This agent is not in the crew."

  Scenario: Round limit
    Given max_rounds 3
    When the coordinator delegates in each of 3 rounds
    Then before a fourth round the crew is stopped_at_limit and its state is kept

  Scenario: Same member fails twice
    Given coordinated mode and failure_repeat_limit 2
    When analyst's step fails in two consecutive rounds
    Then the crew is escalated with "analyst failed twice in a row."

  Scenario: Saved steps run with no coordinator turn per step
    Given the example workflow with three quotes
    When the crew runs
    Then coordinator.round is never called
    And decide_condition is called once and check_result once

  Scenario: Steps at the same time respect members at once
    Given a group of 5 steps and max_members_running 4
    Then 4 member runs start together and the fifth starts when one ends

  Scenario: Step fails once, tried again one level up, then succeeds
    Given step 2 ran on the smaller level and failed
    Then it is tried again with a stepped-up model override
    And the crew continues when the retry succeeds

  Scenario: Step fails twice with if it fails = ordin takes over
    When step 3 fails twice
    Then take_over is called with StepFailedTwice at position 3
    And the rest of the run is coordinated round by round

  Scenario: Agent missing at a step
    Given the agent of step 2 is no longer in the workspace
    When the walk reaches step 2
    Then take_over is called with AgentMissing

  Scenario: Ask me first, then rejected
    Given step 2.1 asks first
    When the person rejects the card
    Then the crew is escalated with "The person chose not to run step 2.1."

  Scenario: Repeat until reaches its maximum
    Given a Repeat until with maximum 3 whose condition never holds
    When the third pass ends
    Then ask_person is called with choices "Carry on with the last result" and "Stop"

  Scenario: For each beyond its maximum
    Given a For each with maximum 20 over an input of 25 items
    Then only the first 20 items run and crew_items_cut is journalled

  Scenario: Member's question answered by Ordin
    Given a member run pauses with a question
    When answer_member returns a text
    Then the member run is resumed with that answer and the coordinator's turn is counted

  Scenario: External actions only for allowed members
    Given members_allowed_external_actions holds only writer
    When analyst's step starts
    Then the Rules Engine holds a restriction for analyst's run with external actions not allowed

  Scenario: Kill switch
    Given a running crew with two member runs, one remote that cannot be cancelled
    When the person confirms Stop crew
    Then the local run is cancelled, the remote run is abandoned, and the crew is killed

  Scenario: Restart never continues on its own
    Given a crew running when the application restarts
    When recover runs
    Then the crew is paused with reason restart
    And no member run starts until continue_crew is called

  Scenario: Ordin asks to take over and the orchestrator accepts
    Given a saved-steps crew at step 2 of an outline with steps 1, 2 and 3
    When the Ordin Host calls request_take_over from position "3" with reason "the summary needs a new section"
    Then step 2 finishes, take_over is called with OrdinAsked at position "3"
    And the rest of the run is coordinated round by round

  Scenario: Ordin asks to take over from a step already done
    Given a saved-steps crew whose step 1 is done
    When request_take_over is called from position "1"
    Then it returns CREW_TAKE_OVER_REFUSED and the walk goes on

  Scenario: One crew per workspace
    Given a crew paused for restart in workspace "Sales"
    When running_crew is called for "Sales"
    Then it returns that crew run

  Scenario: Escalation carries Ordin's recommendation
    When the coordinator calls escalate with question "Which supplier first?" and recommendation "Kestrel"
    Then the crew is escalated and crew_ended receives recommendation "Kestrel"
    And needs_you_changed is journalled for the workspace

  Scenario: Not started by the person
    When start is called with started_by_person false and no trigger
    Then it returns CREW_NOT_STARTED_BY_PERSON

  Scenario: A trigger starts a saved workflow
    Given a charter carrying workflow "Supplier rates refresh" version 3
    When start is called with started_by_person false and trigger "T1"
    Then the crew run starts and crew_started records trigger "T1"
```

## Performance / Security / Observability

- **Performance targets:** a guardrail decision p95 ≤ 1 ms (pure function); from a step's end to the next step's `start_run` p95 ≤ 50 ms excluding Ordin turns; persisting the cursor p95 ≤ 5 ms. One crew of 8 members with up to 8 runs at once and a 50-step outline uses under 2 MiB of driver memory. The crew progress screen's events reach the screens within A-03's 150 ms of the change.
- **Security:** Guardrails are fixed code and cannot be raised by the coordinator, a member or a workflow file (A-05, 07). Members never address each other; briefs are built from fixed templates holding labels, locations, versions and short summaries, never the conversation. Every member tool call still passes the Rules Engine; members outside `members_allowed_external_actions` are refused external actions by the restriction it holds. Saved steps never remove an approval the application requires. Never logged: briefs, member summaries, questions and answers; logs hold identifiers, positions and states.
- **Observability:** log events (target `ordin_crew_orchestrator`): `crew_started` {crew_run_id, mode, members}; `crew_round_opened` {crew_run_id, round}; `crew_guardrail` {crew_run_id, round, decision}; `crew_step_dispatched` {crew_run_id, position, run_id, member}; `crew_path_taken` {crew_run_id, position, path, decided_by}; `crew_taken_over` {crew_run_id, position, reason}; `crew_ended` {crew_run_id, state, rounds, requests_used}; `crew_recovered` {count}. Journal records: `crew_started`, `crew_round_closed`, `crew_path_taken`, `crew_step_changed_by_ordin`, `crew_items_cut`, `crew_ended`. No metrics or spans (programs D and P, A-10).

Reconciliation assumptions (January 2026):
- **SA-RECON-01** — R10: only the orchestrator decides a take-over; Ordin asks through `request_take_over`.
- **SA-RECON-02** — R11: a trigger-started saved workflow starts with `started_by_person = false` and its `trigger_id`; nothing else may start a crew without the person.
- **SA-CLOSE-01** (K20) — The pair the Trigger Service sends (through the Ordin Host's immediate start) is exactly `started_by_person: false` with `trigger_id: Some(<the trigger>)`, and this crate accepts it (§B step 1); the Session Supervisor, not this crate or the Trigger Service, fills each member run's `StartRun.caller` (`CrewMember { crew_run_id }`) from the `CrewRound` origin.
- **SA-RECON-03** — R3, R20: events and records go through `append_event` and `append_entry`; the tables live in the Journal's `Datastore`; start-up reports are the Journal's; history is pruned through `PruneParticipant`.
- **SA-RECON-04** — R6: the Ordin Host implements `Coordinator` exactly as defined here, and the Service API catalogue adopts `crews.get`, `crews.stop` and `crews.continue` (R1).

## Gaps

None.
