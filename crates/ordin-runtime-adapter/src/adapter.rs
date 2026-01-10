//! The `RuntimeAdapter` trait and the run types it carries, copied from 2C wire §2
//! (RuntimeAdapterInterface.md, *Types defined in this crate*).

use futures::stream::BoxStream;
use ordin_types::time::Timestamp;
use ordin_types::{ApprovalId, ArtefactId, CodingTaskId, RunId, ScopeId, WorkspaceId};
use serde::{Deserialize, Serialize};

use crate::error::AdapterError;
use crate::{
    AdapterKind, AdapterVerb, ModelChoice, RunLimits, RunProgress, RunState, RunTarget,
    ThinkingLevel, ToolCaller,
};

/// One runtime route in ordin's caller vocabulary: start, progress, pause, resume, read, cancel
/// (ADR-003). Implemented by the Claude Local, Copilot Local, Hosted Agent and Slack Thread
/// adapters; called by the Session Supervisor through `AdapterRegistry::get`.
// spec: ordin-desktop/components/RuntimeAdapterInterface#SA-RECON-01
// spec: ordin-desktop/components/RuntimeAdapterInterface#SA-RECON-03
#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> AdapterKind;

    /// ADR-003: each adapter states the verbs it cannot offer; the start-up report shows them.
    fn unsupported_verbs(&self) -> &'static [AdapterVerb];

    /// start: a command with arguments, or free text.
    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError>;

    /// progress events and pauses for a decision arrive on one ordered stream per run.
    /// The stream ends after exactly one `RunEvent::Ended`.
    fn events(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError>;

    /// resume with approve or reject.
    async fn resume(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), AdapterError>;

    /// read state and result.
    async fn read(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<RunSnapshot, AdapterError>;

    /// cancel, where the route offers it; otherwise `AdapterError::VerbNotOffered`.
    async fn cancel(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError>;
}

/// Per-call context: correlation identifier and a deadline for this call (not for the run).
#[derive(Debug, Clone)]
pub struct AdapterContext {
    pub correlation_id: String,
    pub deadline: std::time::Instant,
    pub cancel: tokio_util::sync::CancellationToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunHandle {
    pub run_id: RunId,
    pub adapter: AdapterKind,
    /// Harness session identifier (local) or remote run identifier (hosted, Slack).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_thread_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct StartRun {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,
    pub target: RunTarget,
    pub input: StartInput,
    /// Artefacts attached to the task (04 — Task, *attached artefacts*).
    pub attachments: Vec<ArtefactId>,
    pub limits: RunLimits,
    /// An existing remote thread to continue (hosted agent); absent starts a new thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The model level and model the Usage Planner chose (ADR-062, C-37); absent for remote
    /// adapters, whose model is the remote side's. 2C wire §2, decision 80: C-11.
    // spec: ordin-desktop/components/RuntimeAdapterInterface#C-37
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelChoice>,
    /// The thinking setting the Usage Planner chose; absent uses the harness's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
    /// Who the session's ordin tools act for; the local adapters copy it, with `scope_id` and
    /// `coding_task_id`, into the MCP Front's `SessionGrant` (2C wire §2; SA-RECON-04).
    /// Remote adapters ignore these three fields.
    // spec: ordin-desktop/components/RuntimeAdapterInterface#SA-RECON-04
    pub caller: ToolCaller,
    /// The hosted agent's scope (agent profile); absent elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    /// The coding task whose worktree the session works in (Project Service); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartInput {
    Command { name: String, arguments: String },
    FreeText { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        #[serde(skip_serializing_if = "Option::is_none")]
        runtime_session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_thread_id: Option<String>,
    },
    Progress {
        progress: RunProgress,
    },
    /// The harness's own to-do list, where the adapter can read it (ADR-068 item 4, C-41).
    HarnessPlan {
        steps: Vec<String>,
    },
    Paused {
        reason: PauseReason,
    },
    Resumed,
    Usage {
        usage: UsageDelta,
    },
    Ended {
        end: RunEnd,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PauseReason {
    /// The Rules Engine decided *ask*; the Approval Service holds the card.
    Approval { approval_id: ApprovalId },
    /// The harness is signed out (ADR-044).
    SignIn,
    /// A question for the person (a member's question, a person-decided condition).
    Person { question: String },
    /// Slack route: the decision is made in Slack (ADR-009); the link opens the thread.
    DecisionElsewhere { link: url::Url },
    /// Hosted agent: waiting for the hosted agent's approvers, who decide on their own cards;
    /// no `Answer` is accepted. 2C wire §2, decision 80: C-14.
    RemoteApprovers,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsageDelta {
    pub model_turns: u32,
    pub tool_calls: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunEnd {
    pub reason: RunEndReason,
    /// The final message, plain text, at most 32,768 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,
    pub artefacts: Vec<ArtefactId>,
}

/// The ended states of `Run`; the subset of `RunState` an adapter may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEndReason {
    Completed,
    Failed,
    StoppedAtLimit,
    Cancelled,
    Interrupted,
    Abandoned,
    NoReply,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResumeDecision {
    Approve {
        approval_id: ApprovalId,
    },
    Reject {
        approval_id: ApprovalId,
        /// Told to the model so it can continue.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Answer to `PauseReason::Person`.
    Answer {
        text: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSnapshot {
    pub run_id: RunId,
    pub state: RunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<RunEnd>,
    /// When the route was last read; drives "last checked 20 seconds ago" (11 §5.4).
    pub checked_at: Timestamp,
}
