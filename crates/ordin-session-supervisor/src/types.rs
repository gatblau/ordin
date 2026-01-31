//! The public value types and the host trait of the Session Supervisor (SessionSupervisor.md
//! *Public Interface*, with the ADR-107 and ADR-111 amendments).

use ordin_runtime_adapter::{AdapterKind, AdapterVerb, RunLimits, RunTarget, StartInput, StartRun};
use ordin_types::common::InputUsed;
use ordin_types::domain::agent_profile::{CallPlace, ModelFailureCause};
use ordin_types::domain::runs::{Run, RunOrigin, Task};
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{CodingTaskId, RemoteAgentId, RunId, WorkspaceId};
use ordin_usage_planner::{LevelChoice, WorkKind};
use serde::{Deserialize, Serialize};

/// A run to start: a task's run, a crew member's run, or (internally) Ordin's conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartRequest {
    pub origin: RunOrigin,
    pub workspace_id: WorkspaceId,
    pub target: RunTarget,
    pub input: StartInput,
    pub attachments: Vec<ordin_types::ArtefactId>,
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
// spec: ordin-desktop/components/SessionSupervisor#SUP-REC-1
pub struct RunView {
    pub run: Run,
    pub startup: Option<Vec<StartUpReportItem>>,
    /// The `runs.workspace_id` column: the run's workspace, whatever its origin.
    pub workspace_id: WorkspaceId,
    /// The `tasks` row of a `RunOrigin::Task` run; `None` for any other origin.
    pub task: Option<Task>,
}

/// ADR-107. Local to this crate; the program maps the Rules Engine's `CheckPoint` and
/// `ContentCheckOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentPoint {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentCheck {
    Passed,
    Blocked { retry_without_context: bool },
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunControls {
    pub tools: Vec<(String, u32)>,
    pub tool_calls: Vec<String>,
    pub controls: Vec<(String, String)>,
    pub gates_set_aside: Vec<String>,
    pub argument_rules: Vec<(String, String, String)>,
    pub ending: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct HostError {
    pub code: String,
}

/// Facts only the program can assemble.
// Implemented by: the desktop and server programs, over the Rules Engine (`startup_items`,
// `resume_run`, `end_run`, run facts), the Slack Thread Adapter (`abandon`), the MCP Front
// (offered tools), the Installers (unavailable parts), the Agent Loader (command expansion, agent
// profile), the Model Policy (`reply_for`, `after_block`, agent profile), the Rules Engine
// (`check_content`, agent profile, ADR-107) and the Memory Service (`run_starting`, ADR-111).
#[async_trait::async_trait]
pub trait SupervisorHost: Send + Sync + 'static {
    /// Rule, tool, unavailable-part and own-screens lines for the start-up report. The adapter's
    /// unsupported verbs are passed so the host words their lines too.
    async fn startup_items(
        &self,
        run: &Run,
        target: &RunTarget,
        unsupported: &[AdapterVerb],
    ) -> Vec<StartUpReportItem>;
    /// The route of a remote agent: `HostedAgent` or `SlackThread`; None when the agent is unknown.
    fn remote_route(&self, remote_agent_id: RemoteAgentId) -> Option<AdapterKind>;
    /// Agent profile: wrightrun CommandExpansion over the obtained release's bundle.
    async fn expand_command(
        &self,
        target: &RunTarget,
        name: &str,
        arguments: &str,
    ) -> Result<String, HostError>;
    /// Agent profile: control facts for the run summary (Rules Engine run facts).
    async fn run_controls(&self, run_id: RunId) -> RunControls;
    /// Before `adapter.resume` of a paused run. Some(control) = the run may not resume; the run
    /// then ends `completed` with that control named in `end_reason`.
    async fn run_resuming(&self, run_id: RunId) -> Option<String>;
    /// After every ended state is recorded.
    async fn run_ended(&self, run_id: RunId);
    /// After the supervisor marks a remote run `abandoned` because its adapter offers no `cancel`.
    fn run_abandoned(&self, run_id: RunId, adapter: AdapterKind);
    /// Agent profile: the text a run ends with after a model failure; None = no reply.
    fn reply_for(&self, cause: ModelFailureCause, place: CallPlace) -> Option<String>;
    /// Agent profile: after a content-filter block. True = run once more without the gathered
    /// context; false = safe reply.
    fn retry_after_block(&self, carried_gathered_context: bool, already_retried: bool) -> bool;
    /// Agent profile (ADR-107): the instance's content checks for one point of a run. `Passed`
    /// everywhere else, and when the instance has no content check.
    async fn check_content(&self, run_id: RunId, point: ContentPoint, text: &str) -> ContentCheck;
    /// ADR-111: before `adapter.start` of a run whose caller is `ToolCaller::Agent` and whose
    /// target is an `AgentInstance`, the program sets or clears the agent's memory rules.
    async fn run_starting(&self, start: &StartRun);
}

/// The program this supervisor runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramKind {
    Desktop,
    PersonalServer,
    AgentSingle,
    AgentScaled,
}

impl ProgramKind {
    /// True in the agent profile (H).
    pub fn is_agent(self) -> bool {
        matches!(self, ProgramKind::AgentSingle | ProgramKind::AgentScaled)
    }
}

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// Desktop | PersonalServer | AgentSingle | AgentScaled.
    pub program: ProgramKind,
    /// The adapter for local agent instances (the profile's first harness; ClaudeLocal by default).
    pub local_adapter: AdapterKind,
    /// 8; agent profile: `ORDIN_RUN_CONCURRENCY`.
    pub local_session_limit: u32,
    /// Agent profile: holder name for leases.
    pub instance_name: String,
    /// From `limits.run.wall_clock_minutes` × 60.
    pub default_wall_clock_secs: u32,
    /// 600.
    pub idle_timeout_secs: u32,
    /// Desktop 10 s; personal server: the wait for each cancelled harness after the 30 s window;
    /// agent profile: unused (H.5).
    pub stop_deadline: std::time::Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    Quit,
    Signal,
    Sleep,
}

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

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct RecoveryReport {
    pub resumed: u32,
    pub interrupted: u32,
    pub ended_remotely: u32,
    pub awaiting_decision: u32,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ShutdownReport {
    pub stopped: u32,
    pub interrupted: u32,
}

/// Counter sink for the agent profile (Metrics.md: a component without a Telemetry edge declares
/// its own sink and the server program implements it with `event_from_named` and `record`).
pub trait MetricsSink: Send + Sync {
    fn add(&self, name: &str, labels: &[(&str, &str)], by: u64);
}
