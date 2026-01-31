//! The public value types of the Run Queue (RunQueue.md *Public Interface*).

use ordin_types::domain::agent_profile::{
    RunEndKind, RunRecord, RunRecordState, RunRequest, RunStarter,
};
use ordin_types::time::Timestamp;
use ordin_types::{ApprovalId, ScopeId};
use serde::{Deserialize, Serialize};

/// `LEASE_SECONDS`: a run's lease (2D-4 *Run lease*).
pub const LEASE_SECONDS: i64 = 30;
/// `MAX_PROGRESS_LINE_CHARS`: a progress line is cut to this many characters (LW-7).
pub const MAX_PROGRESS_LINE_CHARS: usize = 160;
/// `MAX_EVENTS_PER_RUN`: bounds one run's stream rows.
pub const MAX_EVENTS_PER_RUN: i64 = 10_000;
/// `MAX_MESSAGE_CHARS`: the Caller API's text limit.
pub const MAX_MESSAGE_CHARS: usize = 262_144;
/// `MAX_ARGUMENTS_BYTES`: the largest intake body, which becomes a command's arguments.
pub const MAX_ARGUMENTS_BYTES: usize = 1_048_576;
/// `MAX_STARTER_ID_CHARS`.
pub const MAX_STARTER_ID_CHARS: usize = 512;
/// `MAX_SESSION_RECORD_BYTES`: 64 MiB.
pub const MAX_SESSION_RECORD_BYTES: usize = 67_108_864;
/// `MAX_DELETE_LIMIT`.
pub const MAX_DELETE_LIMIT: u32 = 500;

/// One item awaiting a decision while the run is paused (wrightrun RunResult.pending).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingItem {
    pub kind: PendingKind,
    pub tool: Option<String>,
    pub gate: Option<String>,
    /// "approval-list" | "argument-rule:<name>" | "refused-ending"
    pub reason: String,
    /// The Approval Service card raised for this item (SA-CLOSE-01, K14): set by the Session
    /// Supervisor when it pauses a run on a tool call that has an approval card; `None` for a
    /// refused ending and for an item with no card. Absent from older rows (serde default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingKind {
    #[serde(rename = "tool-call")]
    ToolCall,
    #[serde(rename = "refused-ending")]
    RefusedEnding,
}

/// What a run produced, kept beside the 2C record. `RunRecord.result` holds `reply`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunOutcome {
    pub reply: Option<String>,
    pub ending: Option<String>,
    #[serde(default)]
    pub pending: Vec<PendingItem>,
    #[serde(default)]
    pub actions_asked: Vec<String>,
    #[serde(default)]
    pub actions_attached: Vec<String>,
    pub delivery: Option<Delivery>,
    #[serde(default)]
    pub opened_items: Vec<String>,
    pub error_code: Option<String>,
    #[serde(default)]
    pub safe_reply: bool,
    /// True when a control ended the run with no model call (wrightrun "ended-by-control").
    #[serde(default)]
    pub ended_by_control: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Asker,
    Thread,
}

/// A record with the times and outcome the Caller API and operator routes return.
#[derive(Debug, Clone, PartialEq)]
pub struct RunView {
    pub record: RunRecord,
    pub outcome: Option<RunOutcome>,
    /// Equal to `record.created_at` (2C, decision 80: C-28).
    pub created_at: Timestamp,
    /// Equal to `record.started_at`.
    pub started_at: Option<Timestamp>,
    /// Equal to `record.ended_at`.
    pub ended_at: Option<Timestamp>,
    pub cancel_requested: bool,
}

/// What `take` hands the run instance.
#[derive(Debug, Clone)]
pub struct TakenRun {
    pub view: RunView,
    /// The decision given to the last `resume`; None on a first execution.
    pub decision: Option<serde_json::Value>,
    /// The harness session record stored at the last pause (C-80).
    pub session_record: Option<Vec<u8>>,
}

/// Input of `enqueue`.
#[derive(Debug, Clone)]
pub struct NewRun {
    pub started_by: RunStarter,
    pub scope_id: ScopeId,
    pub request: RunRequest,
    /// `{}` when none.
    pub gathered_context: serde_json::Value,
    /// None: a new thread.
    pub thread_id: Option<String>,
    /// None: minted here (UUIDv7).
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseRenewal {
    Kept,
    Lost,
    CancelRequested,
}

#[derive(Debug, Clone)]
pub struct RunEnd {
    pub end_kind: RunEndKind,
    pub outcome: RunOutcome,
    pub stretch_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunPause {
    /// `pending` non-empty.
    pub outcome: RunOutcome,
    pub session_record: Vec<u8>,
    pub stretch_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CancelOutcome {
    /// The run was queued or paused and is now cancelled.
    Cancelled(RunView),
    /// The run is executing; its instance stops it at its next renewal.
    Requested(RunView),
}

#[derive(Debug, Clone, Default)]
pub struct InProgressFilter {
    pub command: Option<String>,
    pub intake: Option<String>,
}

/// What a delete (or its count) of runs returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletedRuns {
    pub runs: u64,
    pub thread_ids: Vec<String>,
    pub run_ids: Vec<String>,
}

/// One stored event of a run's stream.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StoredRunEvent {
    pub run_id: String,
    /// 1, 2, 3 … per run.
    pub seq: u64,
    pub kind: RunEventKind,
    pub data: serde_json::Value,
    pub at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    RunStateChanged,
    RunProgress,
}

/// Counter sink; the program forwards to the Telemetry crate in the agent profile.
pub trait MetricsSink: Send + Sync {
    fn add(&self, name: &'static str, labels: &[(&'static str, &str)], by: u64);
}

/// The state's column text.
pub(crate) fn state_text(state: RunRecordState) -> &'static str {
    match state {
        RunRecordState::Queued => "queued",
        RunRecordState::Executing => "executing",
        RunRecordState::Paused => "paused",
        RunRecordState::Ended => "ended",
        RunRecordState::Cancelled => "cancelled",
        RunRecordState::Interrupted => "interrupted",
    }
}
