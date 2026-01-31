//! The Journal's own public types (JournalAndArtefactStore.md *Public Interface*, *The `EventSink`
//! hub trait and the prune participant trait* and *Supporting types*).

use ordin_types::common::InputUsed;
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::domain::runs::{DashboardSource, ExternalActionKind, RunState, StepSource};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::{CrewRunId, ExternalActionKey, ReportId, RunId, WorkspaceId};
use serde::{Deserialize, Serialize};

use crate::datastore::Datastore;

/// Implemented by the Command Bridge, the Remote API, the Channel Gateway (ADR-100 item 1) and
/// by every module that reacts to events; registered with `Journal::subscribe`. A sink decodes
/// `JournalEvent.kind` with `EventKind::parse` and `data` itself.
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    /// `[a-z_]{1,40}`, used in logs only.
    fn name(&self) -> &'static str;
    /// Called once per event, in `seq` order, never concurrently for one sink.
    async fn deliver(&self, event: &JournalEvent);
    /// The sink's queue overflowed; events from `next_seq` on were not delivered. The sink
    /// re-reads them with `Journal::events_since(next_seq - 1, …)`. Delivery resumes after this call.
    async fn lagged(&self, next_seq: u64);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JournalEvent {
    /// The installation's backend identifier (`Journal::backend_uuid`).
    pub backend: uuid::Uuid,
    /// Strictly increasing per backend, starting at 1, contiguous, never reused.
    pub seq: u64,
    pub at: Timestamp,
    /// `^[a-z][a-z0-9_]{0,63}$`.
    pub kind: String,
    /// The `ServiceEvent` variant's fields, without `kind`.
    pub data: serde_json::Value,
}

/// Implemented by modules that keep history in their own tables and depend on the Journal, and
/// registered by the program.
#[async_trait::async_trait]
pub trait PruneParticipant: Send + Sync {
    /// The participant's `ModuleMigrations::module` name.
    fn module(&self) -> &'static str;
    /// Remove this module's history recorded before `request.cutoff`, never touching anything
    /// linked to a run in `request.runs_in_progress`, and never a pending approval (A-08).
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneRequest {
    pub cutoff: Timestamp,
    pub runs_in_progress: Vec<RunId>,
}

/// The participant's own error code (its registry prefix), e.g. `APPR_STORE_UNAVAILABLE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneFailure {
    pub code: &'static str,
}

/// UUIDv7 (SA-ID-01 "journal record").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JournalEntryId(pub uuid::Uuid);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewJournalEntry {
    /// `^[a-z][a-z0-9_]{0,63}$`, for example `help_request_sent`, `setup_check_result`, `repair_done`.
    pub kind: String,
    /// The component that wrote it, `[a-z_]{1,40}`.
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crew_run_id: Option<CrewRunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    /// One plain sentence for Diagnostics, 1–300 characters; never a token or file content.
    pub summary: String,
    /// At most 65,536 bytes serialised.
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct JournalEntry {
    pub id: JournalEntryId,
    pub kind: String,
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crew_run_id: Option<CrewRunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
    pub summary: String,
    pub data: serde_json::Value,
    pub recorded_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EntryFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewArtefact {
    pub location: ArtefactLocation,
    pub path: String,
    pub hash: Sha256Hex,
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producing_run_id: Option<RunId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ArtefactFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<ArtefactLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producing_run_id: Option<RunId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewExternalAction {
    /// Minted by the caller (UUIDv7) before the action leaves ordin.
    pub action_key: ExternalActionKey,
    /// `None` for an action started outside any run (an automatic push, a handover).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    pub kind: ExternalActionKind,
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportSummary {
    pub id: ReportId,
    pub run_id: RunId,
    pub title: String,
    pub as_of: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewDashboard {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub made_by: DashboardSource,
    pub inputs: Vec<InputUsed>,
    /// The run whose report is kept; its report must exist.
    pub run_id: RunId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlannedStep {
    pub line: String,
    pub came_from: StepSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RetentionSetting {
    pub days: u32,
    /// The company maximum when the organisation profile sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_max_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pruned_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PruneSummary {
    pub cutoff: Timestamp,
    /// module → rows removed. `journal` holds this crate's own count.
    pub removed: std::collections::BTreeMap<String, u64>,
    /// modules whose participant failed, with its code; their rows stay until the next prune.
    pub failed: std::collections::BTreeMap<String, String>,
}

/// One row of the Journal's run index (`journal_runs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunIndexEntry {
    pub run_id: RunId,
    pub state: RunState,
    pub first_seen_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
}

/// A journal entry as one Diagnostics line. The program maps it field for field to the
/// Service API's `DiagnosticLine { reference, at, level, code, summary }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagnosticEntry {
    /// The first 8 characters of the entry's `JournalEntryId`, which the person reads out to IT.
    pub reference: String,
    pub at: Timestamp,
    /// `info`, `warning` or `error`.
    pub level: String,
    /// A registry code, or empty when the entry carries none.
    pub code: String,
    pub summary: String,
}

/// The bytes of a support export: JSON Lines, UTF-8, at most 10,000,000 bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SupportBundle {
    /// `ordin-support-<YYYYMMDDTHHMMSSZ>.jsonl`.
    pub suggested_file_name: String,
    pub bytes: Vec<u8>,
    /// `true` when the 10,000,000-byte limit cut the newest lines.
    pub truncated: bool,
}
