# SPEC: Session Supervisor

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-session-supervisor` · **Build phase:** 3 · **Programs:** D P A
> **Dependencies:** Runtime Adapter interface, Journal, Usage Planner, Run Queue

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
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)

## Purpose

The Session Supervisor owns the lifecycle of every run, local or remote (02:486–495). It records each task, starts and stops runs through the Runtime Adapter interface, applies each run's limits (wall-clock time, request budget, idle timeout), limits how many local sessions run at once, propagates cancellation, records every state change in the journal, assembles the start-up report of each run, and on application start reconciles the runs the journal shows as in progress (03 *Recovery after restart*). It asks the Usage Planner for each run's model level and gives it each run's usage. In the agent profile it also takes runs from the Run Queue with a lease, expands a run's command into its message, writes one run summary per stretch of a run, keeps the harness's session record in the datastore at a pause so that any instance can resume it (C-80), and lets its runs finish within the stop period when told to stop (02:487, ADR-098 item 3).

## Approach

One supervisor task per run (a `tokio` task owning the run's adapter event stream, its limit timers and its cancellation token), coordinated by a small scheduler that holds the local-session limit and a FIFO of queued runs; state is written to the `runs` table before each event is emitted, so the journal and the store never disagree after a crash. Adapters are registered by the program as `Arc<dyn RuntimeAdapter>` per `AdapterKind` (2B-1 rule 3; the supervisor never names a concrete adapter). Facts it cannot see itself — the start-up report's rule and tool lines, the command expansion of the agent profile, and the control facts of a run summary — come through one trait it owns, `SupervisorHost`, implemented by the program. Error prefix: `SUP_` (*chosen here*), plus 2C's `TASK_` and `RUNST_` validation codes. Rejected: one supervisor task for all runs — one slow adapter call would delay every run's limits. Rejected: a database-polled queue on the desktop — the desktop has one process, so an in-memory FIFO with the store as the record of truth suffices; the Run Queue is used only where 2B-3 places it (agent profile).

## Shared Context

### From `ordin-runtime-adapter` (2C wire §2, copied)

```rust
#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> AdapterKind;
    fn unsupported_verbs(&self) -> &'static [AdapterVerb];
    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError>;
    fn events(&self, ctx: &AdapterContext, run: &RunHandle)
        -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError>;
    async fn resume(&self, ctx: &AdapterContext, run: &RunHandle, decision: ResumeDecision) -> Result<(), AdapterError>;
    async fn read(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<RunSnapshot, AdapterError>;
    async fn cancel(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError>;
}
pub struct AdapterContext { pub correlation_id: String, pub deadline: std::time::Instant, pub cancel: tokio_util::sync::CancellationToken }
pub struct RunHandle { pub run_id: RunId, pub adapter: AdapterKind, pub runtime_session_id: Option<String>, pub remote_thread_id: Option<String> }
pub enum AdapterKind { ClaudeLocal, CopilotLocal, HostedAgent, SlackThread }
pub enum AdapterVerb { Start, Progress, Pause, Resume, Read, Cancel }
pub struct StartRun {
    pub run_id: RunId, pub workspace_id: WorkspaceId, pub target: RunTarget, pub input: StartInput,
    pub attachments: Vec<ArtefactId>, pub limits: RunLimits /* wire */, pub thread: Option<String>,
    pub model: Option<ModelChoice>, pub thinking: Option<ThinkingLevel>,
    pub caller: ToolCaller, pub scope_id: Option<ScopeId>, pub coding_task_id: Option<CodingTaskId>,
}
pub enum ModelLevel { Automatic, Smaller, Standard, MostCapable }
pub enum ThinkingLevel { Low, Automatic, High }
pub struct ModelChoice { pub level: ModelLevel, pub model: Option<String> }
pub enum ToolCaller { Ordin, Agent, CrewMember { crew_run_id: CrewRunId }, HostedAgent }   // tag "kind"
// Checks the supervisor applies (Runtime Adapter interface crate): AdapterRegistry { register, get },
// check_start(kind, &StartRun), check_verb(&dyn RuntimeAdapter, verb), check_resume(&decision, &paused_on),
// check_snapshot(&snapshot), guard_events(kind, run_id, stream) -> BoxStream<Guarded>, end_state(reason) -> RunState.
pub enum RunTarget { AgentInstance { agent_instance_id: AgentInstanceId }, RemoteAgent { remote_agent_id: RemoteAgentId } }
pub enum StartInput { Command { name: String, arguments: String }, FreeText { text: String } }
/// Wire RunLimits (the request budget is optional here; the domain Run.limits holds u32).
pub struct RunLimits { pub wall_clock_secs: u32, pub request_budget: Option<u32>, pub idle_timeout_secs: u32 }
pub enum RunEvent {
    Started { runtime_session_id: Option<String>, remote_thread_id: Option<String> },
    Progress { progress: RunProgress },
    HarnessPlan { steps: Vec<String> },
    Paused { reason: PauseReason },
    Resumed,
    /// Copied as the provider defines it. When C-29 confirms the harness reports context-window
    /// use, 2C adds `context_used_percent: Option<u8>` to this variant (SA-CLOSE-01); C.3 states
    /// what this module then does with it.
    Usage { usage: UsageDelta },
    Ended { end: RunEnd },
}
pub enum PauseReason { Approval { approval_id: ApprovalId }, SignIn, Person { question: String }, DecisionElsewhere { link: url::Url },
    /// Hosted agent: waiting for the hosted agent's own approvers; no `Answer` is accepted (2C C-14).
    RemoteApprovers }
pub struct UsageDelta { pub model_turns: u32, pub tool_calls: u32, pub input_tokens: Option<u64>, pub output_tokens: Option<u64>, pub cached_input_tokens: Option<u64> }
pub struct RunEnd { pub reason: RunEndReason, pub final_message: Option<String>, pub artefacts: Vec<ArtefactId> }
pub enum RunEndReason { Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply }
pub enum ResumeDecision { Approve { approval_id: ApprovalId }, Reject { approval_id: ApprovalId, reason: Option<String> }, Answer { text: String } }
pub struct RunSnapshot { pub run_id: RunId, pub state: RunState, pub end: Option<RunEnd>, pub checked_at: Timestamp }
pub enum AdapterError { VerbNotOffered { verb: AdapterVerb }, SignedOut, HarnessUntested { found: String }, Unreachable,
    AllowanceExhausted { service_message: String }, ThreadBusy, UnknownRun, Deadline,
    /// The request failed the adapter's own validation (2C C-13); `code` is an `RA_*` code.
    InvalidRequest { code: String },
    /// The remote side refused the request (2C C-13); `code` is the adapter's journalled code.
    Refused { code: String } }
// codes RA_VERB_NOT_OFFERED, RA_SIGNED_OUT, RA_HARNESS_UNTESTED, RA_UNREACHABLE, RA_ALLOWANCE_EXHAUSTED,
//       RA_THREAD_BUSY, RA_UNKNOWN_RUN, RA_DEADLINE; InvalidRequest and Refused carry their own code
```

2C limits copied: `input.name` 1–64; arguments and free text ≤ 16,384 characters (`RA_INPUT_TOO_LARGE`); attachments ≤ 50; `wall_clock_secs` 1–28,800; `idle_timeout_secs` 30–3,600 (`RA_LIMIT_OUT_OF_RANGE`); `remote_agent` targets only for `hosted_agent` and `slack_thread`, `agent_instance` only for local adapters (`RA_TARGET_MISMATCH`). Event stream: exactly one `started` first and one `ended` last (`RA_EVENT_ORDER`).

### From `ordin-types` (2C domain, copied)

```rust
pub struct Task {
    pub id: TaskId, pub workspace_id: WorkspaceId, pub target: TaskTarget,
    pub command: Option<String>, pub free_text: Option<String>, pub arguments: Vec<TaskArgument>,
    pub attached_artefacts: Vec<ArtefactId>, pub run_again_of: Option<TaskId>, pub requested_at: Timestamp,
}
pub enum TaskTarget { AgentInstance { agent_instance_id }, RemoteAgent { remote_agent_id }, Crew { crew_charter_id } }
pub struct TaskArgument { pub name: String, pub value: String }
pub struct Run {
    pub id: RunId, pub origin: RunOrigin, pub agent: AgentRef, pub adapter: AdapterKind,
    pub runtime_session_id: Option<String>, pub remote_run_id: Option<String>, pub remote_thread_id: Option<String>,
    pub state: RunState, pub pause_reason: Option<RunPauseReason>, pub limits: RunLimits /* domain: u32 budget */,
    pub usage: UsageCounters, pub started_at: Option<Timestamp>, pub ended_at: Option<Timestamp>,
    pub end_reason: Option<String>, pub inputs_used: Vec<InputUsed>, pub queued_at: Timestamp,
}
pub enum RunOrigin { Task { task_id: TaskId }, CrewRound { crew_run_id: CrewRunId, round_number: u32 },
    Conversation { workspace_id: WorkspaceId } }
pub enum RunState { Queued, Running, Paused, Completed, Failed, StoppedAtLimit, Cancelled, Interrupted, Abandoned, NoReply }
pub enum RunPauseReason { Approval, SignIn, Person }
pub struct UsageCounters { pub model_turns: u32, pub tool_calls: u32, pub model_level: Option<String>,
    pub tokens: Option<u64>, pub tokens_from_cache: Option<u64> }
// 2C agent_profile (Model Policy types placed in ordin-types)
pub enum ModelFailureCause { ModelFailed, ContentFilter, SummaryFailed }
pub enum CallPlace { Agent, Subagent }
pub struct StartupReport { pub id: StartupReportId, pub subject: StartupSubject, pub enforced_rules: Vec<String>,
    pub rules_not_enforced: Vec<RuleNotEnforced>, pub unavailable_parts: Vec<String>, pub available_tools: Vec<String>,
    pub adapter_verbs_not_offered: Vec<String>, pub generated_at: Timestamp }
pub struct RunRecord { /* Run Queue's entity, agent profile */ pub id: RunRecordId, pub run_id: String, pub thread_id: String,
    pub started_by: RunStarter, pub scope_id: ScopeId, pub request: RunRequest, pub gathered_context: serde_json::Value,
    pub state: RunRecordState, pub end_kind: Option<RunEndKind>, pub lease_holder: Option<String>,
    pub lease_expires_at: Option<Timestamp>, pub result: Option<String>, pub stretch_summaries: Vec<String>, pub queued_at: Timestamp }
pub enum RunRequest { Command { command: String, arguments: Vec<TaskArgument> }, Message { text: String } }
/// `Control`: a control ended the run (wire state `ended-by-control`; 2C decision 80, C-28).
pub enum RunEndKind { Completed, Control, Failed, RunLimit }
```

2C validation copied: Task — exactly one of command and free text (`TASK_COMMAND_XOR_TEXT`); command 1–100 (`TASK_COMMAND_INVALID`); free text 1–8,000 (`TASK_TEXT_INVALID`); ≤ 50 arguments, name 1–64, value ≤ 4,000 (`TASK_ARGUMENTS_INVALID`); immutable once submitted (`TASK_IMMUTABLE`). Run — adapter matches the agent kind (`RUNST_ADAPTER_MISMATCH`); `pause_reason` present iff paused (`RUNST_PAUSE_REASON_MISMATCH`); `no_reply` only with `slack_thread` (`RUNST_NO_REPLY_NOT_SLACK`), `abandoned` only remote (`RUNST_ABANDONED_NOT_REMOTE`); `ended_at` iff ended (`RUNST_ENDED_AT_MISMATCH`); `end_reason` ≤ 500 (`RUNST_END_REASON_TOO_LONG`); each limit ≥ 1 and within the profile's maxima (`RUNST_LIMIT_OUT_OF_RANGE`); `tokens_from_cache ≤ tokens` (`RUNST_CACHE_EXCEEDS_TOKENS`); ≤ 100 inputs (`RUNST_INPUTS_TOO_MANY`) (ordin's own codes on the 2C `Run` entity, moved off wrightrun's carried `RUN_` prefix, EH-D8); in-progress runs never pruned. Run transitions: queued → running → paused ⇄ running; running → completed | failed | stopped_at_limit | abandoned | no_reply; queued, running, paused → cancelled; running, paused → interrupted.

`ServiceEvent` variants emitted through `Journal::append_event` (2C wire §1, `EventKind`): `RunStateChanged { run_id, state }`, `RunProgress { run_id, progress }`, `StartUpReportReady { subject: StartUpSubject::Run { run_id }, items }`.

### Other crates used

- `ordin-usage-planner` (Usage Planner spec): `choose(WorkKind, AdapterKind) -> LevelChoice`, `record_usage(RunUsage) -> Result<(), UsageError>`, `record_run_finished(work_key: &str, run_id, model_turns: u32) -> Result<(), UsageError>`; `WorkKind { Talking, Coordinating, Routine, Documents, Code }`; `LevelChoice { level, model, thinking, stepped_up }` with `model_choice() -> ModelChoice`; `RunUsage { run_id, crew_run_id, round, level, delta }`.
- `ordin-run-queue` (Run Queue spec; trait `RunQueue`, held as `Arc<dyn RunQueue>`): `take(&self, instance: &str) -> Result<Option<TakenRun>, RunQueueError>` with `TakenRun { view, decision: Option<serde_json::Value>, session_record: Option<Vec<u8>> }`; `renew(&self, run_id: &str, instance: &str) -> Result<LeaseRenewal, RunQueueError>` with `LeaseRenewal { Kept, Lost, CancelRequested }`; `pause(&self, run_id, instance, pause: RunPause { outcome: RunOutcome, session_record: Vec<u8>, stretch_summary: Option<String> })`; `resume(&self, run_id, decision: serde_json::Value) -> Result<RunRecord, _>`; `end(&self, run_id, instance, end: RunEnd { end_kind: RunEndKind, outcome: RunOutcome, stretch_summary: Option<String> })` (imported as `QueueRunEnd`); `record_cancelled(&self, run_id: &str, instance: &str, outcome: RunOutcome) -> Result<(), RunQueueError>`; `append_progress(&self, run_id: &str, instance: &str, line: &str) -> Result<bool, RunQueueError>`; `in_progress(&InProgressFilter { command, intake }) -> Result<u64, _>`; `paused_with_command(&[String], except_run_id: &str) -> Result<bool, _>`; `RunOutcome { reply, ending, pending, actions_asked, actions_attached, delivery, opened_items, error_code, safe_reply, ended_by_control }`.
- `ordin-journal` (Journal spec, B2 form, R3): `Journal::external_actions_for_run(&self, run_id: RunId) -> Result<Vec<ExternalAction>, JournalError>` (the call behind the Service API's `journal.run_external_actions`; 2C `ExternalAction { action_key, run_id, kind, destination, state, sent_at, confirmed_at }`), read at run end for the run summary (H.2); `Datastore` (the program passes `journal.datastore().clone()`), `ModuleMigrations`, `MigrationStep`, `EventSink`, `JournalEvent`, `PruneParticipant`, `PruneRequest`, `PruneFailure`; `Journal::append_event(&self, kind: &str, data: serde_json::Value) -> Result<JournalEvent, JournalError>` (kinds from 2C `EventKind::as_str()`); `Journal::append_entry(&self, NewJournalEntry { kind, component: "session_supervisor", run_id, crew_run_id, workspace_id, summary, data }) -> Result<JournalEntry, JournalError>`; `store_startup_report(StartupReport, replace: bool)`, `startup_report(StartupSubject) -> Result<Option<StartupReport>, _>`; `state_plan(run_id, Vec<PlannedStep>)`.

### Configuration and constants

| Name | Value | Source |
|---|---|---|
| Concurrent local sessions | 8 | 2D-4, A-04 |
| `limits.run.wall_clock_minutes` | profile default 60, 1–480 | 2D-2 |
| Run wall-clock, application maximum | 8 hours | 2D-4 |
| Run idle timeout | 10 minutes | 2D-4 |
| `ORDIN_RUN_CONCURRENCY` | 1–100; 10 in the runs role, 8 in the single shape | 2D-1a (agent profile) |
| Stop deadline on desktop quit or sleep | 10 s per harness, then killed and marked interrupted | SA-STOP-01 |
| Personal server stop on `SIGTERM` | running runs may finish until 30 s after the signal (the program waits, then calls `shutdown(Signal)`); then each run still running is cancelled and recorded `interrupted` | P4-2, OrdinServer.md step 11.1(d), SA-CLOSE-03 |
| Agent profile stop (single and scaled shapes) | never cut short by ordin: runs go on within the deployment's stop period (Helm `roles.runs.stopPeriodSeconds`, or the agent unit's `TimeoutStopSec`); a run still going when the process ends is left to lease expiry | SA-STOP-01, 2D-3, P4-2, SA-CLOSE-03 |
| Run lease | 30 s, renewed every 10 s | 2D-4 |
| Adapter call deadline (`AdapterContext.deadline`) | 30 s for `start`, `resume`, `read`, `cancel` | *chosen here* |

## Public Interface

Crate `ordin-session-supervisor`; names *chosen here* unless marked.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartRequest {
    pub origin: RunOrigin,
    pub workspace_id: WorkspaceId,
    pub target: RunTarget,
    pub input: StartInput,
    pub attachments: Vec<ArtefactId>,
    /// None takes the profile defaults (D.2).
    pub limits: Option<RunLimits>,
    /// What kind of work, for the model level (ADR-062).
    pub work_kind: WorkKind,
    /// "Same work" key for estimates (Usage Planner).
    pub work_key: String,
    pub inputs_used: Vec<InputUsed>,
    pub thread: Option<String>,
    /// For local adapters: true when the person, not Ordin, started it (daily pause does not apply).
    pub started_by_person: bool,
    /// A stepped-up level chosen by the caller (Crew Orchestrator retry, ADR-062); None asks the Usage Planner.
    pub model_override: Option<LevelChoice>,
    /// The coding task whose worktree the run works in (engineering mode; Project Service).
    pub coding_task_id: Option<CodingTaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunView { pub run: Run, pub startup: Option<Vec<StartUpReportItem>> }

/// Facts only the program can assemble.
#[async_trait::async_trait]
pub trait SupervisorHost: Send + Sync + 'static {
    /// Rule, tool, unavailable-part and own-screens lines for the start-up report (the Rules Engine's
    /// `startup_items`, the MCP Front's offered tools, the Installers' unavailable parts).
    /// The adapter's unsupported verbs are passed so the host words their lines too.
    async fn startup_items(&self, run: &Run, target: &RunTarget, unsupported: &[AdapterVerb]) -> Vec<StartUpReportItem>;
    /// The route of a remote agent: `HostedAgent` or `SlackThread`; None when the agent is unknown.
    fn remote_route(&self, remote_agent_id: RemoteAgentId) -> Option<AdapterKind>;
    /// Agent profile: wrightrun CommandExpansion over the obtained release's bundle.
    async fn expand_command(&self, target: &RunTarget, name: &str, arguments: &str) -> Result<String, HostError>;
    /// Agent profile: control facts for the run summary (Rules Engine run facts).
    async fn run_controls(&self, run_id: RunId) -> RunControls;
    /// Before `adapter.resume` of a paused run: the program calls the Rules Engine's `resume_run`.
    /// Some(control) = the run may not resume (a duty's lock is held: ending `overlap`); the run
    /// then ends `completed` with that control named in `end_reason`.
    async fn run_resuming(&self, run_id: RunId) -> Option<String>;
    /// After every ended state is recorded: the program calls the Rules Engine's `end_run` and, for a
    /// run with a `coding_task_id`, the Project Service's `end_coding_run`.
    async fn run_ended(&self, run_id: RunId);
    /// After the supervisor marks a remote run `abandoned` because its adapter offers no `cancel`
    /// (D.3, D.4): for a `SlackThread` run the program calls the Slack Thread Adapter's
    /// `abandon(run_id)`, since the supervisor holds only `dyn RuntimeAdapter`.
    fn run_abandoned(&self, run_id: RunId, adapter: AdapterKind);
    /// Agent profile: the text a run ends with after a model failure (Model Policy `reply_for`);
    /// None = no reply, the run ends `failed`. Types from `ordin-types` (`agent_profile`).
    fn reply_for(&self, cause: ModelFailureCause, place: CallPlace) -> Option<String>;
    /// Agent profile: after a content-filter block (Model Policy `after_block`). True = run once more
    /// without the gathered context (`BlockAction::RetryWithoutGatheredContext`); false = safe reply.
    fn retry_after_block(&self, carried_gathered_context: bool, already_retried: bool) -> bool;
    /// Agent profile (amended January 2026, ADR-107): the instance's content checks for one point of a run
    /// (the program calls the Rules Engine's `check_content` and maps its outcome). `Passed` everywhere
    /// else, and when the instance has no content check.
    async fn check_content(&self, run_id: RunId, point: ContentPoint, text: &str) -> ContentCheck;
}
/// ADR-107. Local to this crate; the program maps the Rules Engine's `CheckPoint` and `ContentCheckOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ContentPoint { Input, Output }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentCheck { Passed, Blocked { retry_without_context: bool } }
// Implemented by: the desktop and server programs, over the Rules Engine (`startup_items`,
// `resume_run`, `end_run`, run facts), the Slack Thread Adapter (`abandon`), the MCP Front (offered tools), the Installers (unavailable
// parts), the Agent Loader (command expansion, agent profile) and the Model Policy (`reply_for`,
// `after_block`, agent profile) and the Rules Engine (`check_content`, agent profile, ADR-107).
#[derive(Debug, Clone, Default, Serialize)]
pub struct RunControls {
    pub tools: Vec<(String, u32)>, pub tool_calls: Vec<String>, pub controls: Vec<(String, String)>,
    pub gates_set_aside: Vec<String>, pub argument_rules: Vec<(String, String, String)>, pub ending: Option<String>,
}
#[derive(Debug, thiserror::Error)] #[error("{code}")]
pub struct HostError { pub code: String }

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub program: ProgramKind,                       // Desktop | PersonalServer | AgentSingle | AgentScaled
    /// The adapter for local agent instances (the profile's first harness; ClaudeLocal by default).
    pub local_adapter: AdapterKind,
    pub local_session_limit: u32,                   // 8; agent profile: ORDIN_RUN_CONCURRENCY
    pub instance_name: String,                      // agent profile: holder name for leases
    pub default_wall_clock_secs: u32,               // from limits.run.wall_clock_minutes × 60
    pub idle_timeout_secs: u32,                     // 600
    pub stop_deadline: std::time::Duration,         // desktop 10 s; personal server: the wait for each cancelled harness after the 30 s window (I); agent profile: unused (H.5)
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("task invalid: {code}")] TaskInvalid { code: &'static str },
    #[error("start request invalid: {code}")] StartInvalid { code: &'static str },
    #[error("no adapter for {0:?}")] NoAdapter(AdapterKind),
    #[error("adapter refused: {0}")] Adapter(#[source] AdapterError),
    #[error("run not found")] RunNotFound,
    #[error("run already ended")] RunEnded,
    #[error("run not paused")] RunNotPaused,
    #[error("verb not offered")] VerbNotOffered(AdapterVerb),
    #[error("stopping")] Stopping,
    #[error("datastore unavailable")] Store(#[source] sqlx::Error),
    #[error("journal unavailable")] Journal(#[source] JournalError),
}
impl SupervisorError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }

pub struct SessionSupervisor { /* private */ }
impl SessionSupervisor {
    pub const MODULE: &'static str = "session_supervisor";
    pub fn migrations() -> ModuleMigrations;   // module "session_supervisor" (ModuleMigrations.module)
    pub fn new(datastore: Datastore, journal: Arc<Journal>, usage: Arc<UsagePlanner>,
               run_queue: Option<Arc<dyn RunQueue>>, host: Arc<dyn SupervisorHost>, config: SupervisorConfig) -> Arc<Self>;
    /// Program wiring: one adapter per kind, before `recover`. Kept in an `AdapterRegistry`
    /// (Runtime Adapter interface); a second adapter of one kind → `RA_ADAPTER_DUPLICATE`.
    /// Called by: the desktop and server programs.
    pub fn register_adapter(&self, adapter: Arc<dyn RuntimeAdapter>) -> Result<(), SupervisorError>;
    /// The Journal subscription (approval decisions resume agent-profile runs).
    /// Called by: the desktop and server programs (`journal.subscribe(supervisor.event_sink())`).
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink>;
    /// The Journal's pruning hook for `runs` and `tasks`. Called by: the programs (`journal.register_prune_participant`).
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant>;

    /// Records a task (immutable) and starts its run. TaskTarget::Crew is refused (the Crew Orchestrator starts crews).
    /// `coding_task_id`: the coding task the Ordin Host started for it (ADR-053), copied into `StartRun`.
    /// Called by: Ordin Host (a delegated single-agent task).
    pub async fn submit_task(&self, task: Task, work_kind: WorkKind, started_by_person: bool,
                             coding_task_id: Option<CodingTaskId>) -> Result<RunId, SupervisorError>;
    /// Starts a run (tasks and crew steps). Returns once the run is recorded as queued.
    /// Called by: Crew Orchestrator (member runs).
    pub async fn start_run(&self, req: StartRequest) -> Result<RunId, SupervisorError>;
    /// Follow a run: every RunEvent the adapter sends, after the supervisor has recorded it.
    /// Called by: Ordin Host (Ordin's turns), Crew Orchestrator (member runs).
    pub fn subscribe(&self, run_id: RunId) -> Result<tokio::sync::broadcast::Receiver<RunEvent>, SupervisorError>;
    /// Person's Stop, crew kill switch, crew stop.
    /// Called by: Ordin Host, Crew Orchestrator, the programs' Service API (`runs.stop`).
    pub async fn cancel(&self, run_id: RunId) -> Result<(), SupervisorError>;
    /// A decision for a paused run, or a late hook decision (Hook Relay), or a member's answer.
    /// Called by: Ordin Host (the person's next message), Crew Orchestrator (a member's answer),
    /// Hook Relay (a late hook decision), the programs' Service API (`runs.answer`).
    pub async fn resume_with_decision(&self, run_id: RunId, decision: ResumeDecision) -> Result<(), SupervisorError>;
    /// Called by: Crew Orchestrator, the programs' Service API (`runs.get`).
    pub async fn get(&self, run_id: RunId) -> Result<RunView, SupervisorError>;
    /// Called by: Ordin Host, the programs' Service API (`runs.list`).
    pub async fn list(&self, workspace_id: Option<WorkspaceId>, cursor: Option<String>) -> Result<Page<Run>, SupervisorError>;
    /// Live-session check for the Hook Relay: the run exists and is queued, running or paused.
    /// Called by: Hook Relay.
    pub fn is_live(&self, run_id: RunId) -> bool;
    /// The latest context-window use (0–100) the harness reported for a live run (SA-CLOSE-01);
    /// None until C-29 confirms the harness reports it, for an unknown or ended run, or before the
    /// first report. Called by: Ordin Host (`should_summarise`).
    pub fn context_used_percent(&self, run_id: RunId) -> Option<u8>;
    /// On application start, before any new run: sequence *Recovery after restart*.
    /// Called by: the desktop and server programs.
    pub async fn recover(&self) -> Result<RecoveryReport, SupervisorError>;
    /// Agent profile: start N workers taking runs from the Run Queue. Called by: the server program.
    pub fn start_queue_workers(self: &Arc<Self>);
    /// Quit, SIGTERM, sleep (desktop): stop every run within the stop deadline
    /// (`SupervisorConfig.stop_deadline`). Called by: the desktop and server programs (there is no `stop_all`).
    pub async fn shutdown(&self, reason: ShutdownReason) -> ShutdownReport;

    /// Ordin's conversation run (J.1). Called by: Ordin Host. Ordin's turns are read from
    /// `subscribe`: each `Paused { reason: Person { question } }` ends one Ordin turn with `question`
    /// as its text; the next person's message is `resume_with_decision(Answer { text })`; stopping is
    /// `cancel`. A delegated single-agent task is `submit_task`.
    pub async fn start_conversation(&self, req: ConversationStart) -> Result<RunId, SupervisorError>;
    /// Local sessions running now (not queued). Called by: desktop program (quit question).
    pub fn running_local_sessions(&self) -> u32;
    /// Local plus remote runs queued, running or paused. Called by: server program (compaction).
    pub fn runs_going(&self) -> u32;
    /// The computer woke after `slept_for` (J.2). Called by: desktop program's wake watcher.
    pub async fn on_wake(&self, slept_for: std::time::Duration);
    /// Personal server: stops local runs paused for the person longer than `idle_for` (ADR-049);
    /// returns how many (J.3). Called by: server program, every 60 s with 30 minutes.
    pub async fn stop_idle_sessions(&self, idle_for: std::time::Duration) -> u32;
    /// Agent profile: workers take no new record (H.5). Called by: server program on SIGTERM.
    pub async fn stop_taking_runs(&self);
    /// Agent profile: returns when no run is executing or `stop` is cancelled (H.5).
    /// Called by: server program, after `stop_taking_runs`.
    pub async fn wait_for_runs(&self, stop: tokio_util::sync::CancellationToken);

    /// Facts for the Rules Engine's `RulesHost` (desktop, personal server; the Run Queue answers in
    /// the agent profile). Called by: the program's `RulesHost`.
    /// Runs queued, running or paused whose command is `command` (StartInput::Command name).
    pub async fn runs_with_command(&self, command: &str) -> Result<u32, SupervisorError>;
    /// Whether a paused run other than `except` has one of `commands`.
    pub async fn paused_with_command(&self, commands: &[String], except: RunId) -> Result<bool, SupervisorError>;
    /// Some(true) ended, Some(false) not ended, None unknown.
    pub async fn has_ended(&self, run_id: RunId) -> Result<Option<bool>, SupervisorError>;

    /// Agent profile: the harness's session record kept between a pause and a resume (C-80).
    /// Called by: the program's `SessionRecordStore` (Claude Local Adapter), `save` and `load`.
    pub async fn save_session_record(&self, run_id: RunId, runtime_session_id: &str, record: Vec<u8>)
        -> Result<(), SupervisorError>;
    pub async fn load_session_record(&self, run_id: RunId, runtime_session_id: &str)
        -> Result<Option<Vec<u8>>, SupervisorError>;
}
#[derive(Debug, Clone, Copy)] pub enum ShutdownReason { Quit, Signal, Sleep }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConversationStart {
    pub workspace_id: WorkspaceId,
    /// Ordin's own agent instance.
    pub target: RunTarget,
    /// The seed (ADR-062 fixed order), ≤ 16,384 characters (2C `StartInput` limit).
    pub seed: String,
    pub limits: RunLimits,
    /// The level the Usage Planner chose for `talking`; None asks it here.
    pub model_override: Option<LevelChoice>,
}
#[derive(Debug, Clone, Serialize)] pub struct RecoveryReport { pub resumed: u32, pub interrupted: u32, pub ended_remotely: u32, pub awaiting_decision: u32 }
#[derive(Debug, Clone, Serialize)] pub struct ShutdownReport { pub stopped: u32, pub interrupted: u32 }
```

Service API commands (SA-API-01, *chosen here*): `runs.get` {run_id} → `RunView`; `runs.list` {workspace_id?, cursor?} → `Page<Run>`; `runs.stop` {run_id} → `{}` (calls `cancel`); `runs.answer` {run_id, text} → `{}` (calls `resume_with_decision(Answer)` for a run paused for the person).

### Example

```rust
let run_id = supervisor.submit_task(Task {
    id: task_id, workspace_id: sales, target: TaskTarget::AgentInstance { agent_instance_id: analyst },
    command: Some("compare-quotes".into()), free_text: None,
    arguments: vec![TaskArgument { name: "quotes".into(), value: "Kestrel, Acme and Northwind Q3 quotes".into() }],
    attached_artefacts: vec![q1, q2, q3], run_again_of: None, requested_at: now,
}, WorkKind::Documents, false, None).await?;
```

The supervisor writes the task and a run (`queued`), asks the Usage Planner (`standard`, `sonnet`), calls `ClaudeLocal.start` with `limits {wall_clock_secs: 3600, idle_timeout_secs: 600}`, and emits `run_state_changed` (`queued`), then `running` on `Started`, `start_up_report_ready` with the adapter's verbs and the host's items, `run_progress` lines, and finally `run_state_changed` `completed` on `Ended`.

## Internal Logic

**A. `submit_task(task, kind, by_person, coding_task_id)`.**
1. Validate the task (2C rules); failure → `SupervisorError::TaskInvalid` with the 2C code (permanent). `TaskTarget::Crew` → `SUP_TARGET_IS_CREW`.
2. Insert into `tasks` (never updated afterwards: `TASK_IMMUTABLE` is enforced by having no update statement).
3. Build `StartRequest { origin: Task { task_id }, input: Command { name, arguments } }` with `arguments` = the task arguments rendered `name: value` per line, or `FreeText { text }`; target mapped from `TaskTarget`; `work_key = "task:<agent instance or remote agent id>:<command or 'free_text'>"`; `coding_task_id` as given (this module never calls the Project Service). Call `start_run`.

**B. `start_run(req)`.**
1. Adapter kind from the target: `AgentInstance` → `config.local_adapter` (the harness the profile names first, ClaudeLocal by default); `RemoteAgent` → `host.remote_route(remote_agent_id)`, `None` → `RA_TARGET_MISMATCH`. No registered adapter of that kind → `SUP_NO_ADAPTER`.
2. Limits: `req.limits` or the defaults (`default_wall_clock_secs`, no request budget, `idle_timeout_secs`); validate 2C ranges and the profile ceiling (`limits.run.wall_clock_minutes` maximum 480) → `RA_LIMIT_OUT_OF_RANGE`.
3. Model level: `req.model_override`, else `usage.choose(req.work_kind, kind)`; stored in `run.usage.model_level`. Caller of the session's ordin tools: origin `Task` → `ToolCaller::Agent`; `CrewRound { crew_run_id }` → `CrewMember { crew_run_id }`; `Conversation` → `Ordin`; agent profile → `HostedAgent` with `scope_id` from the Run Queue record.
4. Insert the `runs` row (`queued`, `queued_at` now, `limits`, `inputs_used`); `journal.append_event(EventKind::RunStateChanged.as_str(), {run_id, state: "queued"})`. Every event below is emitted the same way ("emit" in this spec means `append_event` with the 2C kind).
5. Local adapters: when running local sessions < `local_session_limit`, launch now (C); otherwise append to the in-memory FIFO and log `sup_run_waiting` {run_id, position}. Remote adapters are not counted against the local limit.
6. Return the run id.

**C. Launch and supervise one run.** A dedicated task per run:
1. `ctx = AdapterContext { correlation_id: run_id, deadline: now + 30 s, cancel: run token child }`.
2. Build `StartRun { run_id, workspace_id, target, input, attachments, limits, thread, model: Some(choice.model_choice()) (None for remote adapters), thinking: Some(choice.thinking) (None for remote), caller, scope_id, coding_task_id: req.coding_task_id }`; `check_start(kind, &start)` (an error fails the run with its `RA_*` code); then `adapter.start(ctx, start)`. Errors: `SignedOut` → state `paused`, pause_reason `sign_in` (the screens show **Sign in again**, ADR-044); `AllowanceExhausted` → state `failed`, `end_reason` = the service's message (03 golden path); `HarnessUntested` → `failed`, end reason "harness version not tested"; `Unreachable`, `Deadline` → `failed`; `ThreadBusy` → stays `queued` and retried when the adapter's thread frees (the hosted adapter queues it, 2C); `InvalidRequest { code }` and `Refused { code }` → `failed`, `end_reason` "couldn't start the agent", journalled with `code`; other → `failed`. Each failure is journalled with its code.
3. `guard_events(kind, run_id, adapter.events(ctx, handle)?)`; consume in order (a `Guarded` violation is journalled as entry `run_event_violation` and treated as step 4 describes). Every event is first applied to the `runs` row, then emitted, then broadcast to subscribers:
   - `Started` → `running`, `started_at`, runtime ids; then build the start-up report (E) and emit `StartUpReportReady`.
   - `Progress` → emit `RunProgress`; reset the idle timer. Agent profile: also `run_queue.append_progress(run_id, instance_name, &progress.line)` so the Caller API's event stream carries the line; `Ok(false)` (full or empty line) is ignored, and any error is logged as `sup_progress_not_kept` {run_id, code} and never ends the run (a `LeaseLost` reaches the lease keeper through its next `renew`).
   - `HarnessPlan` → `journal.state_plan(run_id, steps as PlannedStep)` (the Journal emits `plan_changed`, ADR-068 item 4); reset idle.
   - `Paused { reason }` → `paused` with `RunPauseReason` (`approval` → Approval; `sign_in` → SignIn; `person` and `decision_elsewhere` → Person; `remote_approvers` → Approval with no `pause_approval_id`, since the hosted agent's approvers decide on their own cards and the hosted agent resumes the run itself with `Resumed`), store `pause_approval_id` for an `approval`; stop the idle and wall-clock timers (time waiting for a person does not count, *assumed here*, SA-SUP-01). Agent profile: H.3.
   - `Resumed` → `running`; restart the timers with the remaining wall clock.
   - `Usage { usage }` → add `usage` to `run.usage`. Conditional on C-29 (SA-CLOSE-01): once 2C adds `context_used_percent: Option<u8>` to the variant, a `Some(p)` above 100 is a `Guarded` violation (journalled `run_event_violation`, value dropped); otherwise `p` is kept as the run's latest value in the in-memory map read by `context_used_percent(run_id)` and removed when the run ends. Then `usage.record_usage(RunUsage { run_id, crew_run_id, round, level, delta })` (a planner error is logged, never fatal); when `request_budget` is set and `model_turns ≥ budget` → stop at limit (D.3).
   - `Ended { end }` → ended state from `end.reason`, `ended_at`, `end_reason` (final message cut to 500 characters for the column; the full final message and artefacts are journalled as entry `run_result`); state from `end_state(end.reason)`; `usage.record_run_finished(work_key, run_id, model_turns)`; emit `run_state_changed`; `host.run_ended(run_id)`; free the local slot and launch the FIFO's head.
4. A stream that ends without `Ended`, or yields `Started` twice (`RA_EVENT_ORDER`) → `adapter.read` once: an ended snapshot is applied as `Ended`; otherwise state `failed`, end reason "the session ended unexpectedly" (03: harness exits unexpectedly → failed). Log `sup_event_order` {run_id}.
5. A store error while applying an event → retry once after 100 ms; then cancel the run through the adapter and mark it `failed` in memory, logging `sup_store_unavailable`; the next `recover` reconciles the row.

**D. Limits and cancellation.**
1. Wall clock: a `tokio::time::sleep` for the remaining running time; on expiry → stop at limit.
2. Idle: a timer reset by every event; on expiry with no event for `idle_timeout_secs` while `running` → stop at limit.
3. Stop at limit: `adapter.cancel`; when the adapter does not offer `cancel` (`slack_thread`) the run is marked `abandoned` for remote kinds and `host.run_abandoned(run_id, kind)` is called; otherwise wait up to the stop deadline for `Ended`, then record `stopped_at_limit` with `end_reason` "wall-clock limit" / "request budget" / "idle timeout". Partial artefacts the adapter reported are kept.
4. `cancel(run_id)`: unknown → `SUP_RUN_NOT_FOUND`; ended → `SUP_RUN_ENDED`. Queued (FIFO) → remove, `cancelled`. Running or paused: cancel the run's token; `adapter.cancel`; `VerbNotOffered` on a remote run → `abandoned` and `host.run_abandoned(run_id, kind)` (03 kill switch: remote runs cannot be cancelled and are marked abandoned; their results are not used); then `cancelled` on the `Ended` or after the stop deadline. `check_verb(adapter, Cancel)` first. Agent profile: the Run Queue's cancel path is the Caller API's; the worker learns it from `renew` returning `LeaseRenewal::CancelRequested`, cancels the harness, records the run `cancelled` here and calls `run_queue.record_cancelled(run_id, instance, outcome)`.

**E. Start-up report.** On `Started`: `items = host.startup_items(run, target, adapter.unsupported_verbs())`; the host returns every line, the `VerbNotOffered` lines included, with `plain` worded from the message catalogue, so this crate holds no wording. At most 200 items (a longer list is cut and the cut logged as `sup_startup_cut`, 2C `START_TOO_MANY_ITEMS`). Store it in the Journal, which holds every start-up report (2C): `journal.store_startup_report(StartupReport { subject: Run { run_id }, enforced_rules ← `EnforcedRule.plain`, rules_not_enforced ← `RuleNotEnforced`, unavailable_parts ← `UnavailablePart.plain`, available_tools ← `AvailableTool.tool`, adapter_verbs_not_offered ← `VerbNotOffered.verb`, … }, false)`; `RunView.startup` reads it back with `journal.startup_report`. Emit `StartUpReportReady { subject: Run { run_id }, items }`.

**F. `resume_with_decision(run_id, decision)`.** Unknown → `SUP_RUN_NOT_FOUND`; ended → `SUP_RUN_ENDED`. `check_verb(adapter, Resume)` (else `SUP_VERB_NOT_OFFERED`); `check_resume(&decision, &paused_on)` (a mismatch → `SUP_RUN_NOT_PAUSED`); `host.run_resuming(run_id)` (J.6). Desktop and personal server: `adapter.resume(ctx, handle, decision)`; an adapter error is returned (`SUP_ADAPTER_REFUSED`). For an `Answer`, the run must be paused for the person (`SUP_RUN_NOT_PAUSED`). Agent profile: H.4.

**G. `recover()`** (03 *Recovery after restart*), before any new run:
1. Select runs in `queued`, `running`, `paused`.
2. Local runs (`claude_local`, `copilot_local`): `queued` → re-queued in the FIFO in `queued_at` order. `running` or `paused` with a `runtime_session_id` → `adapter.read` then `check_snapshot`: an ended snapshot is applied; a `running` snapshot (the adapter re-attached the persisted session, C-19 for Claude Code; C-05 for Copilot: where a Copilot session cannot resume after restart, the run is marked `interrupted`, the HLD's fallback) re-opens `events` and supervises it again; `UnknownRun` or any error → `interrupted` with its partial artefacts.
3. Remote runs: `adapter.read` (hosted: state over HTTP; Slack: re-read the thread); apply the snapshot; a still-running remote run is supervised again.
4. Paused runs waiting for an approval stay paused (the approval survives, Approval Service).
5. External actions with unknown outcome are not this module's (Journal and the owning services; 03: never repeated). Crews are rebuilt by the Crew Orchestrator.
6. Journal entry `sup_recovered` and return the counts. The program has already run `journal.recover_external_actions()` before this call (Journal step 9).

**H. Agent profile** (`program` is `AgentSingle` or `AgentScaled`; `run_queue` is `Some`).
1. `start_queue_workers`: `local_session_limit` workers. Each loops: `run_queue.take(instance_name)`; `None` → sleep 1 s (wrightrun SA-77: one take a second from a worker with a free place). A `TakenRun { view, decision, session_record }` → parse `run_id` as `RunId` (malformed → `run_queue.end(Failed)` and log `sup_record_invalid`); a resumed record (a `runs` row exists in `paused`) → H.4 path; otherwise expand: `RunRequest::Command` → `host.expand_command(target, command, arguments joined)` → `StartInput::FreeText { text: expanded }` (an expansion error ends the record `Failed` with the Agent Loader's code, passed through by the host); `Message` → `FreeText`. Then B.2–B.4 and C, with a lease keeper renewing every 10 s (`run_queue.renew`: `Kept` → nothing; `CancelRequested` → D.4; `Lost` → cancel the harness and record `interrupted`, never started again, ADR-098 item 3). The run ends through `run_queue.end(run_id, instance, QueueRunEnd { end_kind, outcome, stretch_summary })`, `outcome.reply` = the final message, or the safe reply of J.6 with `safe_reply = true`.
2. Run summary: when a stretch ends or pauses, write one `tracing` event `run_summary` with wrightrun's fields (`agent-wright-runtime/docs/spec/cross-cutting/Logging.md`): `run_id`, `thread_id`, `started_by`, `connection`, `scope`, `command`, `tools`, `tool_calls`, `tokens` (model → input, output where reported), `used_fallback` (false; the Model Policy reports fallback through the adapter's progress, not modelled here), `context`, `context_cut`, `context_left_out` (*amended January 2026, ADR-107*: from the record's `gathered_context`, the Channel Gateway's context items — `context` one entry per context step `{step, kind, outcome, items, bytes, observed_bytes}` in step order, a step that found nothing included with `outcome` `empty` and `items` 0; `context_cut` the labels of cut pieces; `context_left_out` the labels and reasons of left-out items; zeros and empty lists for a run with no gathered context, such as a caller's or a schedule's), `retried_without_context` (true when J.7 or J.6 ran the record once more without gathered context), `controls`, `gates_set_aside`, `actions_left_out` [], `argument_rules`, `events` [], `state` (`finished`, `paused`, `ended-by-control`, `ended-at-limit`, `failed`, `interrupted`), `ending`, `error_code` (`RUN_LIMIT` when ended at limit: wrightrun's carried code, owned by the Run Queue's carried `RUN_` prefix; the Session Supervisor emits it only as the `error_code` value of this `run_summary` log event and never returns it to a caller, ErrorHandling.md registry), `safe_reply`, `resumed`, `elapsed_seconds`, `limit_seconds`, `actions_attached`, `delivery`, `controls_decided`, and (ADR-107) `checks` (content checks asked for in the stretch) and `checks_blocked` (of them, those that blocked); control lists from `host.run_controls(run_id)`. At the end of the run (not at a pause) the supervisor fills the last three from `journal.external_actions_for_run(run_id)`: `actions_attached` = each action's `kind` (2C `ExternalActionKind` as snake_case), in `sent_at` order; `delivery` = the `destination` of the run's latest `slack_post` or `telegram_post` action, absent when there is none; `controls_decided` = one `(kind, state)` pair per action, in `sent_at` order. The same `actions_attached` and `delivery` (`thread` when a post went to the run's thread, else `asker`) are written to `RunOutcome` before `run_queue.end`. A Journal error leaves the three empty and is logged `sup_summary_actions_unread` {run_id, code}; it never fails the run. Lists cut to 200 entries. The same text is passed to the Run Queue as the stretch summary.
3. Pause (`Paused { Approval }`): the adapter has saved the harness's session record through `save_session_record` (J.5) and ended the harness process; `run_queue.pause(run_id, instance, RunPause { outcome (pending = the approval), session_record, stretch_summary })`, state `paused`. The approval has no deadline in this profile (ADR-097 item 4).
4. Resume: the Journal sink receives `approval_decided`; for a run in `paused` with that `pause_approval_id` → `run_queue.resume(run_id, {"approval_id", "state"})`. A worker that later takes the record (its `session_record` held for J.5's `load_session_record`) finds the `paused` row, calls `host.run_resuming` (J.6), and `adapter.resume` (the adapter loads the record through `load_session_record`), and calls `adapter.resume(ResumeDecision::Approve|Reject)`; the Rules Engine allows the approved call once (C-80). Conditional on C-80: where a session record cannot move between machines, the run resumes only on the instance that paused it — the worker that took it re-queues it after 5 s when it is not that instance (*fallback chosen here*, SA-SUP-02, since the HLD names none).
5. Stop (P4-2, SA-CLOSE-03), both shapes (`AgentSingle` and `AgentScaled`): on `SIGTERM` `stop_taking_runs` takes no new record and `wait_for_runs` returns when no run is executing or the program cancels `stop`. Ordin never cuts an agent-profile run short: a run still executing when the process ends keeps its Run Queue record `executing` until its lease lapses, and the server program's call of the Run Queue's `mark_interrupted` (every 10 s, on another instance) records it `interrupted` (ADR-098 item 3). This module never calls `mark_interrupted`.

**I. `shutdown(reason)`.** Desktop `Quit`: the App Shell has already asked the person (07). For every running local run: cancel token, `adapter.cancel`, wait up to `stop_deadline` for `Ended`; then mark `interrupted`. `Sleep`: as Quit (07: "otherwise they are marked interrupted on wake"). Personal server `Signal` (P4-2, SA-CLOSE-03): running runs may finish until 30 s after the signal — the program does not call `shutdown` before then, checking `runs_going()` once a second, and calls it earlier only when no run is going (OrdinServer.md step 11.1(d)); this module never cancels a run inside that window. On the call: take no new run (`SUP_STOPPING`); every run still running: cancel token, `adapter.cancel`, wait up to `stop_deadline` for `Ended`, then record `interrupted`. The agent profile does not call `shutdown` (H.5). Remote runs are left running remotely and reconciled by `recover`. Flush: every row is already written before events (C.3).

**J. Additions of the January 2026 reconciliation.**
1. `start_conversation(req)`: seed empty → `SUP_SEED_EMPTY`; over 16,384 characters → `RA_INPUT_TOO_LARGE`; target must be `AgentInstance` → else `RA_TARGET_MISMATCH`. Then B.1–B.6 with `input = FreeText { text: seed }`, `work_kind = Talking`, `work_key = "conversation:<workspace_id>"`, `started_by_person = true`, no task row; the run's `origin` is 2C `RunOrigin::Conversation { workspace_id }` and its caller `ToolCaller::Ordin`. Ordin's run counts against the local session limit.
2. `on_wake(slept_for)`: for each local run in `running`: `adapter.read`; an ended snapshot or `UnknownRun` → applied as C.4 (ended, or `interrupted`); still running → timers restarted with the remaining time measured before the sleep (time asleep does not count). Each remote run in progress: `adapter.read` and apply. Log `sup_woke` {slept_seconds, interrupted}.
3. `stop_idle_sessions(idle_for)`: local runs `paused` with `pause_reason = person` whose pause began more than `idle_for` ago (pause time kept in memory per run): `adapter.cancel`, state `interrupted`, end reason "stopped after 30 idle minutes"; a conversation run resumes from Ordin's summary later (ADR-049, Ordin Host). Returns the count. Personal server only; elsewhere returns 0.
4. Command facts (desktop and personal server; in the agent profile the program's `RulesHost` asks the Run Queue's `in_progress` and `paused_with_command`): `runs_with_command` counts `runs` rows with `command = ?` in `queued`, `running`, `paused` (new column `command`, set from `StartInput::Command.name` at B.4, NULL for free text); `paused_with_command` selects one `paused` row with `command IN (…)` and `id <> except`; `has_ended` reads the row's state. Store errors → `SUP_STORE_UNAVAILABLE`.
5. Session records (agent profile): the record travels through the Run Queue, which owns it (`RunPause.session_record`, `TakenRun.session_record`). `save_session_record` holds the bytes in memory for the run (the run must exist and be `paused` or `running`, else `SUP_RUN_NOT_FOUND`) until H.3 passes them to `run_queue.pause`; `load_session_record` returns the record of the `TakenRun` this instance took for the run when `runtime_session_id` matches the run's, else None. The adapter pushes the record before ending the harness and loads it when resumed; no table here keeps it.
6. Host calls: before every `adapter.resume` (F, H.4) `host.run_resuming(run_id)`; `Some(control)` → state `completed`, end reason "ended by the control <control>", no resume; agent profile: the record ends through `run_queue.end` with `end_kind: RunEndKind::Control` and `outcome.ended_by_control = true` (summary `state` `ended-by-control`). After every ended state is recorded (C.3, D, G, I) `host.run_ended(run_id)`. Agent profile: an `Ended { Failed }` whose final message reports a model failure or summary failure (cause read by the adapter, *assumed here*) → `host.reply_for(cause, Agent)`: Some(text) → passed to the Run Queue as the reply with `safe_reply = true`. *Amended January 2026 (ADR-107):* `ContentFilter` is no longer read from harness output; it is raised only by ordin's own content check (J.7), so this step needs no rule about what the harness prints for a block.
7. **Content checks (agent profile; amended January 2026, ADR-107).** For every run taken in H.1 whose instance has content checks (the host answers `Passed` when it has none):
   1. *Input.* After the request is expanded (H.1) and before `adapter.start`: `host.check_content(run_id, Input, <the expanded text>)`. `Blocked { .. }` → no harness is started and no model is called; the run ends through `run_queue.end` with `end_kind: Completed`, `outcome.reply = host.reply_for(ContentFilter, Agent)` and `safe_reply = true`, which the Channel Gateway delivers as a safe reply (ChannelGateway.md K.5); `SafeReply { reason: content-filter }` is counted. A retry without gathered context is not offered at this point: the question itself was blocked.
   2. *Output.* When the adapter emits `Ended { Completed, final_message: Some(text) }` and before the record ends: `host.check_content(run_id, Output, text)`. `Passed` → the run ends as before. `Blocked { retry_without_context: true }` and the record carried gathered context → `host.retry_after_block(true, already_retried)`: true → the record is run once more from H.1 without gathered context (the channel-document tool stays offered, MP-8), and the second run's output is checked again; false, or `retry_without_context: false`, or no gathered context → the record ends with `outcome.reply = host.reply_for(ContentFilter, Agent)`, `safe_reply = true`. The blocked text is never delivered, journalled with content or logged.
   3. The input check runs before the run starts, so before the run-start decision's message rules (*chosen here*, SA-HOST-06; confirmed by the user January 2026): `RunEnd` carries no cause, and placing the check here keeps both the adapter and the run-start decision unchanged. A message rule that would have replied without the model is therefore not reached for a blocked question.

**K. Reconciliation assumptions (January 2026).**
- **SA-RECON-01** — R6: the Session Supervisor adopts its providers' names: the Run Queue's `TakenRun`, `LeaseRenewal`, `RunPause` and `RunEnd`; the Journal's `append_event`, `append_entry`, `state_plan` and `store_startup_report`; the Usage Planner's `LevelChoice`.
- **SA-RECON-02** — The Journal holds every start-up report (2C); this module keeps no `run_startup_reports` table. The Run Queue holds the agent-profile session record; this module keeps no `session_records` table.
- **SA-RECON-03** — R20, D4: the module's tables live in the Journal's `Datastore`, handed over by the program from `journal.datastore()`.
- **SA-RECON-04** — Every `StartRun` carries the session's `ToolCaller` (B.3), which the local adapters copy into the MCP Front's `SessionGrant` (R2).
- **SA-CLOSE-01** (C-29, conditional; K8) — **When C-29 confirms** that the harness reports context-window use, 2C `RunEvent::Usage` carries `context_used_percent: Option<u8>` (0–100) and this supervisor keeps the latest value per live run and exposes it through `context_used_percent(run_id)`. **Until C-29 confirms it** (the HLD's fallback), no adapter reports the value, `context_used_percent` answers `None`, and the Ordin Host passes `None` to the Usage Planner's `should_summarise`, which then summarises after 40 turns *(assumed)*. Not a Gap: the bounded behaviour holds either way.
- **SA-CLOSE-02** (K20) — `StartRequest` carries no caller. The supervisor itself fills `StartRun.caller` from the run's origin (B.3): `Task` → `ToolCaller::Agent`, `CrewRound { crew_run_id }` → `CrewMember { crew_run_id }`, `Conversation` → `Ordin`, agent profile → `HostedAgent`. Callers (Ordin Host, Crew Orchestrator, Trigger Service) do not choose it.
- **SA-CLOSE-03** (P4-2, *chosen here*) — Runs at `SIGTERM`: personal server — running runs may finish until 30 s after the signal, then are cancelled and recorded `interrupted` (I); agent profile, single and scaled shapes alike — never cut short by ordin, a run still executing when the process ends is left to lease expiry and recorded `interrupted` by another instance's `mark_interrupted` (H.5). This matches OrdinServer.md *Runs on `SIGTERM`* and step 11.

## Data Model

Module `session_supervisor`. SQLite everywhere; PostgreSQL in the scaled shape for `runs` (`tasks` does not exist in the agent profile, whose runs come from the Run Queue). Start-up reports are the Journal's; session records travel in the Run Queue.

```sql
-- SQLite, migration 1
CREATE TABLE tasks (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,                                   -- JSON TaskTarget
    command TEXT CHECK (command IS NULL OR length(command) BETWEEN 1 AND 100),
    free_text TEXT CHECK (free_text IS NULL OR length(free_text) BETWEEN 1 AND 8000),
    arguments TEXT NOT NULL,                                -- JSON Vec<TaskArgument>
    attached_artefacts TEXT NOT NULL,                       -- JSON Vec<ArtefactId>
    run_again_of TEXT,
    requested_at TEXT NOT NULL,
    CHECK ((command IS NULL) <> (free_text IS NULL))
);
CREATE TABLE runs (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    origin TEXT NOT NULL,                                   -- JSON RunOrigin
    task_id TEXT, crew_run_id TEXT, round_number INTEGER,
    workspace_id TEXT NOT NULL,
    agent TEXT NOT NULL,                                    -- JSON AgentRef
    adapter TEXT NOT NULL CHECK (adapter IN ('claude_local', 'copilot_local', 'hosted_agent', 'slack_thread')),
    runtime_session_id TEXT, remote_run_id TEXT, remote_thread_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('queued','running','paused','completed','failed','stopped_at_limit',
                                         'cancelled','interrupted','abandoned','no_reply')),
    pause_reason TEXT CHECK (pause_reason IN ('approval', 'sign_in', 'person')),
    pause_approval_id TEXT,
    wall_clock_secs INTEGER NOT NULL, request_budget INTEGER NOT NULL DEFAULT 0, idle_timeout_secs INTEGER NOT NULL,
    model_turns INTEGER NOT NULL DEFAULT 0, tool_calls INTEGER NOT NULL DEFAULT 0, model_level TEXT,
    tokens INTEGER, tokens_from_cache INTEGER,
    work_key TEXT NOT NULL,
    command TEXT CHECK (command IS NULL OR length(command) BETWEEN 1 AND 64),
    started_at TEXT, ended_at TEXT,
    end_reason TEXT CHECK (end_reason IS NULL OR length(end_reason) <= 500),
    inputs_used TEXT NOT NULL,                              -- JSON Vec<InputUsed>
    queued_at TEXT NOT NULL,
    CHECK ((state = 'paused') = (pause_reason IS NOT NULL)),
    CHECK ((state IN ('completed','failed','stopped_at_limit','cancelled','interrupted','abandoned','no_reply')) = (ended_at IS NOT NULL)),
    CHECK (state <> 'no_reply' OR adapter = 'slack_thread'),
    CHECK (state <> 'abandoned' OR adapter IN ('hosted_agent', 'slack_thread')),
    CHECK (tokens_from_cache IS NULL OR tokens IS NULL OR tokens_from_cache <= tokens)
);
CREATE INDEX runs_in_progress ON runs (state) WHERE state IN ('queued', 'running', 'paused');
CREATE INDEX runs_workspace ON runs (workspace_id, queued_at);
CREATE INDEX runs_crew ON runs (crew_run_id) WHERE crew_run_id IS NOT NULL;
CREATE INDEX runs_command ON runs (command, state) WHERE command IS NOT NULL;
```

`request_budget = 0` stores "no budget" (2C `RunLimits` domain type requires ≥ 1; recorded as a disagreement in findings). Migrations are `MigrationStep` tuples (R3). PostgreSQL migration 1 creates `runs` with the same columns (`uuid`, `timestamptz`, `jsonb` types) and the same checks. Pruning: this module implements the Journal's `PruneParticipant` (`module()` = `session_supervisor`; the program registers it): `prune` deletes `runs` rows ended before `request.cutoff` and `tasks` rows whose runs are all deleted, never a run in `request.runs_in_progress` or in `queued`, `running` or `paused` (04:386, A-08); a store error → `PruneFailure { code: "SUP_STORE_UNAVAILABLE" }`.

## Error Table

`context_used_percent` never fails: an unknown run, an ended run or no report answers `None`.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Task breaks a 2C rule (A.1) | `Err(permanent)` | `TASK_COMMAND_XOR_TEXT`, `TASK_COMMAND_INVALID`, `TASK_TEXT_INVALID`, `TASK_ARGUMENTS_INVALID` | `SupervisorError::TaskInvalid { code }` |
| A crew task submitted here (A.1) | `Err(permanent)` | `SUP_TARGET_IS_CREW` | `SupervisorError::StartInvalid` |
| Limits out of range (B.2) | `Err(permanent)` | `RA_LIMIT_OUT_OF_RANGE` | `SupervisorError::StartInvalid` |
| Target does not match the adapter (B.1) | `Err(permanent)` | `RA_TARGET_MISMATCH` | `SupervisorError::StartInvalid` |
| No adapter registered for the kind (B.1) | `Err(permanent)` | `SUP_NO_ADAPTER` | `SupervisorError::NoAdapter` |
| A second adapter of one kind registered | `Err(permanent)` | `RA_ADAPTER_DUPLICATE` | `SupervisorError::StartInvalid`; the program does not start |
| Conversation seed empty (J.1) | `Err(permanent)` | `SUP_SEED_EMPTY` | `SupervisorError::StartInvalid` |
| Conversation seed over 16,384 characters (J.1) | `Err(permanent)` | `RA_INPUT_TOO_LARGE` | `SupervisorError::StartInvalid` |
| A duty lock refuses a resume (J.6) | run completed | `SUP_RESUME_REFUSED` | `end_reason` "ended by the control <control>"; agent profile: Run Queue record ended with `RunEndKind::Control` |
| Session record saved for an unknown or ended run (J.5) | `Err(permanent)` | `SUP_RUN_NOT_FOUND` | `SupervisorError::RunNotFound` |
| Harness signed out at start (C.2) | run paused, `Ok` | `RA_SIGNED_OUT` | state `paused`, pause_reason `sign_in`; screens show "Sign in again" |
| Plan allowance exhausted (C.2) | run failed | `RA_ALLOWANCE_EXHAUSTED` | `end_reason` is the service's message, shown in plain words |
| Adapter returns `InvalidRequest` or `Refused` at start (C.2) | run failed | the adapter's `code` | `end_reason` "couldn't start the agent" |
| Route unreachable or start deadline passed (C.2) | run failed | `RA_UNREACHABLE`, `RA_DEADLINE` | `end_reason` "couldn't reach the agent" |
| Events out of order or stream ends early (C.4) | run failed | `RA_EVENT_ORDER` | `end_reason` "the session ended unexpectedly" |
| Wall clock, budget or idle limit (D.3) | run stopped | `SUP_LIMIT_REACHED` | state `stopped_at_limit` with the limit named |
| Cancel or resume of an unknown run (D.4, F) | `Err(permanent)` | `SUP_RUN_NOT_FOUND` | Service API "This run no longer exists." |
| Cancel or resume of an ended run | `Err(permanent)` | `SUP_RUN_ENDED` | Service API "This run has already finished." |
| Answer to a run not paused for the person (F) | `Err(permanent)` | `SUP_RUN_NOT_PAUSED` | `SupervisorError::RunNotPaused` |
| Adapter lacks the verb (F) | `Err(permanent)` | `SUP_VERB_NOT_OFFERED` | `SupervisorError::VerbNotOffered` |
| Adapter refuses a resume (F) | `Err(retryable)` | `SUP_ADAPTER_REFUSED` | `SupervisorError::Adapter` |
| Lease lost (agent profile, H.1) | run interrupted | `SUP_LEASE_LOST` | state `interrupted`, never started again |
| Command expansion fails (agent profile, H.1) | run failed | the Agent Loader's code, passed through (`CMD_UNKNOWN_COMMAND`, `CMD_PROMPT_UNREADABLE`, `CMD_PLACEHOLDER_MISSING`; wrightrun *CommandExpansion*, owned and listed by AgentLoader.md, EH-D2) | Run Queue record ended `failed` |
| New run while shutting down | `Err(permanent)` | `SUP_STOPPING` | `SupervisorError::Stopping` |
| Store unavailable | `Err(retryable)` | `SUP_STORE_UNAVAILABLE` | `SupervisorError::Store`; "Something went wrong starting this. Try again." |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Session Supervisor

  Scenario: Context use is exposed when the harness reports it (C-29 confirmed)
    Given a running run whose adapter reports Usage with context_used_percent 62
    When the Ordin Host asks context_used_percent for the run
    Then the answer is 62
    And after the run ends the answer is none

  Scenario: Context use before C-29 is confirmed
    Given a running run whose adapter reports Usage with no context_used_percent
    When the Ordin Host asks context_used_percent for the run
    Then the answer is none

  Scenario: The caller follows the run's origin
    Given a task submitted by the Ordin Host for instance "analyst"
    When the run starts
    Then StartRun.caller is Agent
    And a crew round's run starts with caller CrewMember for its crew run

  Scenario: Happy path — a task runs to completion
    Given the Claude Local Adapter is registered
    When the Ordin Host submits a task "compare-quotes" for instance "analyst"
    Then a task row and a run row in state queued exist
    And the run becomes running on Started and a start-up report is emitted
    And on Ended with reason completed the run is completed with ended_at set
    And the Usage Planner received the run's usage and its finish

  Scenario: The ninth local session waits
    Given 8 local runs are running
    When a ninth local run is started
    Then it stays queued
    When one running run ends
    Then the ninth run starts

  Scenario: Wall-clock limit
    Given a run with wall_clock_secs 60 that is still running after 60 seconds of running time
    Then the supervisor cancels it through the adapter
    And the run is stopped_at_limit with end_reason "wall-clock limit"

  Scenario: Time paused for an approval does not count
    Given a run with wall_clock_secs 600 paused for an approval for 20 minutes after 5 minutes of running
    When it resumes
    Then it may run 5 more minutes before the limit

  Scenario: Request budget
    Given a run with request_budget 10
    When Usage events bring model_turns to 10
    Then the run is stopped_at_limit with end_reason "request budget"

  Scenario: Idle timeout
    Given a running run with idle_timeout_secs 600
    When no event arrives for 600 seconds
    Then the run is stopped_at_limit with end_reason "idle timeout"

  Scenario: Signed out at start
    Given the adapter's start returns SignedOut
    Then the run is paused with pause_reason sign_in

  Scenario: The harness exits unexpectedly
    Given a running run whose event stream ends without Ended
    And the adapter's read reports the run as unknown
    Then the run is failed with end_reason "the session ended unexpectedly"

  Scenario: Stop a remote run that cannot be cancelled
    Given a running slack_thread run
    When the person presses Stop
    Then the run is abandoned and its result is not used
    And the host's run_abandoned is called for the run with adapter slack_thread

  Scenario: A hosted agent waits for its own approvers
    Given a running hosted_agent run
    When the adapter reports Paused with reason remote_approvers
    Then the run is paused with pause_reason approval and no pause_approval_id
    And its wall-clock and idle timers are stopped

  Scenario: An adapter refuses the start
    Given the adapter's start returns Refused with code "HAA_NO_ACCESS"
    Then the run is failed with end_reason "couldn't start the agent" and the code is journalled

  Scenario: Agent profile — progress reaches the Run Queue
    Given a run executing on "runs-1"
    When the adapter reports Progress "Reading 3 tickets"
    Then run_queue.append_progress is called with that line for "runs-1"

  Scenario: Recovery after restart
    Given the journal shows a local run running and a hosted run running at start
    And the local session cannot be resumed
    When recover runs
    Then the local run is interrupted with its partial artefacts
    And the hosted run's state is read over HTTP and applied

  Scenario: A crew task is refused here
    When submit_task is called with target crew
    Then it returns SUP_TARGET_IS_CREW

  Scenario: Invalid task
    When submit_task is called with both a command and free text
    Then it returns TASK_COMMAND_XOR_TEXT and nothing is stored

  Scenario: Agent profile — pause and resume on another instance
    Given instance "runs-1" took run "R1" and the harness paused for approval "A1"
    Then the session record is stored, the harness ended, and the Run Queue record paused
    When approval_decided for "A1" with state approved is journalled
    Then the Run Queue record is queued again
    When instance "runs-2" takes it
    Then it restores the session record and resumes the harness with Approve "A1"

  Scenario: Ordin's conversation run
    When the Ordin Host calls start_conversation with a seed for workspace "Sales"
    Then a run with no task row is queued with work_kind talking
    When the adapter reports Paused with reason person and question "Here is the comparison."
    Then a subscriber sees that turn's text
    When resume_with_decision is called with Answer "Thanks, now draft the email"
    Then the adapter is resumed with that answer

  Scenario: Wake after sleep
    Given a local run running when the computer slept for 40 minutes
    When on_wake is called and the adapter reports the run unknown
    Then the run is interrupted

  Scenario: Idle conversation stopped on the personal server
    Given a local run paused for the person 31 minutes ago
    When stop_idle_sessions is called with 30 minutes
    Then it returns 1 and the run is interrupted

  Scenario: A duty lock refuses the resume
    Given a paused run whose host run_resuming returns "duty:triage-inbox:lock"
    When an approval decision resumes it
    Then the adapter is not resumed and the run ends completed naming that control

  Scenario: Session record saved and loaded
    When save_session_record is called for paused run "R1" and session "s-1"
    Then load_session_record for "R1" and "s-1" returns the same bytes
    And load_session_record for "R1" and "s-2" returns none

  Scenario: Agent profile — lease lost
    Given a run executing on "runs-1"
    When renew returns false and the lease cannot be taken again
    Then the harness is cancelled and the run is interrupted and never started again

  Scenario: An empty conversation seed is refused
    When the Ordin Host calls start_conversation with an empty seed
    Then it returns SUP_SEED_EMPTY and no run is queued

  Scenario: Personal server — runs may finish until 30 s after SIGTERM (SA-CLOSE-03)
    Given the personal server with local runs "A" and "B" running
    And SIGTERM arrives at time T
    When run "A" ends completed at T + 20 s
    And the program calls shutdown with reason signal at T + 30 s
    Then run "A" is completed and was never cancelled
    And run "B" is cancelled and recorded interrupted

  Scenario Outline: Agent profile — runs are never cut short on SIGTERM (SA-CLOSE-03)
    Given the program kind is <shape> and instance "runs-1" is executing run "R1"
    When the program calls stop_taking_runs and wait_for_runs
    Then no new record is taken and "R1" is not cancelled by ordin
    And when the process ends with "R1" still executing, its record stays executing until its lease lapses

    Examples:
      | shape       |
      | AgentSingle |
      | AgentScaled |

  Scenario: Agent profile — the run summary names the run's external actions
    Given run "R1" sent one slack_post to its thread and then completed
    Then the last run_summary has actions_attached ["slack_post"] and controls_decided [("slack_post", "confirmed")]

  Scenario: Agent profile — one run summary per stretch
    Given a run that paused once and then completed
    Then two run_summary lines were written, the second with resumed true
    And neither holds a message, an argument or an asker's identifier

  Scenario: Agent profile — the input check blocks before the model (ADR-107)
    Given the host's check_content answers Blocked for point input
    When the worker takes a chat run
    Then adapter.start is never called and no model call is made
    And the record ends completed with the safe reply and safe_reply true

  Scenario: Agent profile — a blocked answer after gathered context is tried once more without it (ADR-107)
    Given the record carried gathered context and check_content answers Blocked { retry_without_context: true } for the first output
    When the run's final text arrives
    Then the run is started once more without the gathered context
    And when the second output is blocked too, the record ends with the safe reply and retried_without_context true

  Scenario: Agent profile — a blocked answer with on-block end is never delivered (ADR-107)
    Given check_content answers Blocked { retry_without_context: false } for point output
    When the run's final text arrives
    Then the record ends with the safe reply and the blocked text appears in no log line and no journal entry

  Scenario: Agent profile — the run summary lists each context step (ADR-107)
    Given a record whose gathered context came from a transcript step with 12 messages and a knowledge-lookup step that found nothing
    Then the run_summary's context holds two entries in that order, the second with outcome "empty" and items 0
```

## Performance / Security / Observability

- **Performance targets:** `start_run` returns p95 ≤ 20 ms (store write and emit), excluding the adapter's start; an adapter event reaches `emit` p95 ≤ 5 ms after the stream yields it, so a first progress event reaches the screens within 07's 1 s. Memory: ≤ 64 KiB per supervised run (timers, broadcast buffer of 256 events); 8 local runs on the desktop; 100 per instance in the agent profile (A-48) → ≤ 7 MiB. Recovery of 100 in-progress runs ≤ 5 s, excluding adapter reads.
- **Security:** In-process API; Service API commands arrive schema-checked. The supervisor never passes a credential: the adapters own sign-in and keys (ADR-044, C-74). Task arguments and free text are data passed to the harness, never interpreted. Never logged: task text, arguments, final messages, artefact contents; `end_reason` from a service is stored but logged only as its code. Session records (agent profile) hold conversation content: kept in the datastore only while the run is paused and deleted when it ends.
- **Observability:** log events (target `ordin_session_supervisor`): `sup_run_queued` {run_id, adapter}; `sup_run_waiting` {run_id, position}; `sup_run_started` {run_id, adapter, level}; `sup_run_paused` {run_id, reason}; `sup_run_ended` {run_id, state, elapsed_seconds}; `sup_limit_reached` {run_id, limit}; `sup_event_order` {run_id}; `sup_recovered` {resumed, interrupted, ended_remotely, awaiting_decision}; `sup_lease_lost` {run_id}; `sup_store_unavailable` {run_id}; `sup_progress_not_kept` {run_id, code}; `sup_summary_actions_unread` {run_id, code}; and, in the agent profile, `run_summary` (wrightrun's fields). Agent profile only (SA-XC-04/05): through its `MetricsSink` this crate feeds the families Metrics.md's closed list names it for — `wrightrun_runs_ended_at_limit_total`, `wrightrun_safe_replies_total`, `wrightrun_fallback_runs_total`, `wrightrun_run_leases_lost_total` (names from `agent-wright-runtime/docs/spec/cross-cutting/MetricsEmitter.md`, SA-OBS-02; the run start and end counters are the Run Queue's); and it opens the exported spans of the Tracing.md allow list (T-D1): `ordin.run_stretch` per stretch of a run (fields `run_id`, `started_by`, `command`, `outcome`), `ordin.model_turn` per assistant turn of the harness stream inside it (`model`, `input_tokens`, `output_tokens`, `outcome`) and `ordin.subagent_call` per sub-agent call (`subagent`, `outcome`), no other field. *Amended January 2026 (ADR-107):* a content-check block counts in `wrightrun_safe_replies_total{reason="content-filter"}` when it ends with the safe reply; the checks themselves are counted by the Rules Engine (family 50).

## Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (amends ADR-030: every agent session, agent task sessions included, remembers through the one memory under its own memory rules; open question 1). Counterparts: `components/MemoryService.md` (amendment January 2026: `offer` gives `ToolCaller::Agent` the `memory` tools only when rules are registered), `components/OrdinDesktop.md` and `components/OrdinServer.md` (amendments January 2026: the `SupervisorHost` implementation that registers and clears the rules).
> Every rule below adds to the sections above. The supervisor gains no dependency: it calls one more method of the trait it already owns.

### Purpose (addition)

Before an agent task session starts, the supervisor lets the program bring that agent's memory rules in the Memory Service up to date with its package, so the session is offered memory exactly when the agent's package declares a Memory part.

### Approach (addition)

The supervisor cannot depend on the Memory Service (build phase 3 against 5, 2B DAG) and does not read packages. It already has the seam for facts it cannot see: `SupervisorHost`. One method is added, called before `adapter.start`, at the point where the MCP Front's `open_session` has not yet run. The program sets the rules from the package's current Memory part, or clears them when the package has none; it does not clear them when the run ends, because the same agent instance may be a member of a running crew whose rules the Ordin Host registered from the same part (clearing would drop that member's class checks mid-crew). Rejected: *clearing at run end* — for the reason just given;  *registering rules in the Ordin Host only, at delegation (its step 8.3)* — task runs also start from the person's commands, triggers and workflow steps, which never pass through the Ordin Host; *registering every agent's rules once at start* — rules change when a package is updated, and an installed agent with no session would hold rules for nothing.

Assumption (*chosen here*):

- **SA-SUP-AMEM-01** `run_starting` is awaited for at most 2 s (the MCP Front's `offer` timeout); a slower or failing host call is logged and the run starts without memory, because memory is not a precondition of a task.

### Shared Context (additions)

None; `StartRun`, `ToolCaller` and `RunTarget` are unchanged.

### Public Interface (additions)

```rust
#[async_trait::async_trait]
pub trait SupervisorHost: Send + Sync + 'static {
    // … existing methods …
    /// Before `adapter.start` of a run whose caller is `ToolCaller::Agent` and whose target is an
    /// `AgentInstance` (amended January 2026, ADR-111): the program sets the agent's memory rules in the
    /// Memory Service from its package's Memory part, or clears them when it has none. Errors are the host's to log.
    async fn run_starting(&self, start: &StartRun);
}
```

`run_ended(run_id)` is unchanged and does not touch memory rules.

### Internal Logic (additions)

**C.2 (addition).** After `check_start(kind, &start)` succeeds and before `adapter.start(ctx, start)`: when `start.caller` is `ToolCaller::Agent` and `start.target` is `RunTarget::AgentInstance { .. }`, `tokio::time::timeout(2 s, host.run_starting(&start))`. A timeout logs `sup_run_starting_slow` {run_id} and the launch continues (SA-SUP-AMEM-01). Crew rounds (`CrewMember`), Ordin's conversation and the agent profile are not affected: the Ordin Host registers crew members' rules at `crews.start`, and the hosted agent's rules are set after the Agent Loader loads.


### Data Model

N/A — no table or column changes.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `host.run_starting` does not return within 2 s | logged; run starts | — | — (`sup_run_starting_slow` {run_id}) |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Session Supervisor — memory rules for agent task sessions

  Scenario: Happy path — the host is told before an agent task starts
    Given a task for agent instance "A" submitted with origin Task
    When the run is launched
    Then host.run_starting is called with caller Agent and target AgentInstance "A" before adapter.start

  Scenario: Edge — crew rounds and conversations do not call run_starting
    When a crew round run and Ordin's conversation run are launched
    Then host.run_starting is not called for either

  Scenario: Edge — a slow host does not hold the run
    Given host.run_starting takes 5 seconds
    When an agent task run is launched
    Then adapter.start is called after 2 seconds and "sup_run_starting_slow" is logged

  Scenario: Edge — a check failure still calls nothing
    Given a task whose StartRun fails check_start
    When the run is launched
    Then host.run_starting is not called and the run fails with its RA_* code
```

### Performance / Security / Observability (additions)

- **Performance:** at most 2 s added to a task's launch in the worst case; the program's implementation reads one package model held in memory and is expected well under 10 ms.
- **Security:** the supervisor passes only the `StartRun` it already holds; it reads no package and grants nothing.
- **Observability:** log `sup_run_starting_slow` {run_id}.

### Gaps

None.
