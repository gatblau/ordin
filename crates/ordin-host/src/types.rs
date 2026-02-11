//! The public types of the Ordin Host (OrdinHost.md *Public Interface*): construction, the
//! in-process interface other modules use, and the argument types of the tools of server `ordin`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ordin_types::domain::runs::WorkflowInput;
use ordin_types::domain::{HarnessKind, VoiceLength, VoiceRegister, VoiceTone, WorkflowState};
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::{WorkflowFileInput, WorkflowFileLimits, WorkflowFileStep};
use ordin_types::wire::ContentRef;
use ordin_types::{
    AgentDraftId, AgentInstanceId, AgentRepositoryId, ArtefactId, BlockId, CrewRunId, MemoryFactId,
    PartDraftId, RunId, TriggerId, WorkflowId, WorkingCopyItemId, WorkspaceId,
};
use serde::{Deserialize, Serialize};

pub use ordin_mcp_front::ProgramProfile;
pub use ordin_service_api::commands::workflows::RepositoryFolder;

use crate::error::OhError;

// ---------------------------------------------------------------- construction

/// The version of the fixed instructions (seed section 1), which this crate owns; it advances with
/// every amendment of OrdinHost.md that changes the fixed instructions.
// spec: ordin-desktop/components/OrdinHost#OH-REC-2
// spec: ordin-desktop/components/OrdinHost#OH-TR-1
// spec: ordin-desktop/components/OrdinHost#OH-TR-3
// spec: ordin-desktop/components/OrdinHost#OH-TF-6
// spec: ordin-desktop/components/OrdinHost#OH-TOP-2
pub const FIXED_INSTRUCTIONS_VERSION: &str = "1.4.0";

#[derive(Debug, Clone)]
pub struct OrdinHostConfig {
    /// `BundledAgent::Ordin.instance_id()`, a fixed id; the Package Catalogue adds the instance from
    /// the bundled catalogue on first use (PackageCatalogue.md PKG-REC-2).
    // spec: ordin-desktop/components/OrdinHost#OH-REC-3
    pub ordin_agent_instance_id: AgentInstanceId,
    /// `BundledAgent::AgentMaker.instance_id()`, a fixed id; the Package Catalogue adds the instance
    /// from the bundled catalogue on first use (PackageCatalogue.md PKG-REC-2). Started by
    /// `AuthoringHost::start_agent_maker`.
    pub agent_maker_instance_id: AgentInstanceId,
    /// The harness the profile names first (`harnesses[0]`), resolved by the program.
    pub harness: HarnessKind,
    /// Desktop or PersonalServer.
    pub profile: ProgramProfile,
    /// Fixed instructions version shipped with the application (seed section 1).
    pub fixed_instructions_version: String,
    /// Default 10 min: how long a Coordinator call waits for Ordin's tool call.
    pub coordinator_turn_timeout: Duration,
    /// Default 15 min: repository workflow re-check period.
    pub repository_check_period: Duration,
    /// Default 5: person's messages queued while Ordin's turn runs.
    pub max_waiting_messages: usize,
}

#[derive(Debug, Clone)]
pub struct OrdinHostPolicy {
    /// The coordinator's display name (Brand `coordinator_name()`, default "Ordin").
    pub coordinator_name: String,
    pub voice_defaults: CompanyVoice,
    pub company_workflow_repository: Option<RepositoryFolder>,
    /// The profile's defaults.
    pub crew_defaults: CrewLimitPolicy,
    /// The profile's maxima, each ≤ the application maximum.
    pub crew_maxima: CrewLimitPolicy,
    pub repository_profile_keys: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CompanyVoice {
    pub tone: Option<VoiceTone>,
    pub register: Option<VoiceRegister>,
    pub terms_use: Vec<String>,
    pub terms_avoid: Vec<String>,
}

/// Built by the program from the Organisation Profile's `limits.crew`.
#[derive(Debug, Clone, Copy)]
pub struct CrewLimitPolicy {
    pub rounds: u16,
    pub wall_clock_minutes: u16,
    pub members_at_once: u8,
    pub request_budget: Option<u32>,
    pub failure_repeat_limit: u8,
}

/// Every dependency, constructed by the program and passed as Arcs of the dependency crates' types.
pub struct OrdinHostDeps {
    pub supervisor: Arc<ordin_session_supervisor::SessionSupervisor>,
    pub orchestrator: Arc<ordin_crew_orchestrator::CrewOrchestrator>,
    pub usage: Arc<ordin_usage_planner::UsagePlanner>,
    pub inspector: Arc<ordin_package_inspector::PackageInspector>,
    pub catalogue: Arc<ordin_package_catalogue::PackageCatalogue>,
    pub memory: Arc<ordin_memory_service::MemoryService>,
    pub knowledge: Arc<ordin_knowledge::KnowledgeService>,
    pub push: Arc<ordin_push::PushService>,
    pub projects: Arc<ordin_projects::ProjectService>,
    pub workspaces: Arc<ordin_workspaces::WorkspaceService>,
    pub journal: Arc<ordin_journal::Journal>,
    /// This crate's tables live in the Journal's datastore (D4, R20).
    pub datastore: ordin_journal::Datastore,
    pub blocks: Arc<dyn ordin_service_api::blocks::BlockCatalogueCheck>,
    /// Reads an immediate start's place-file inputs (SA-CLOSE-02).
    pub files: Arc<ordin_files::FileService>,
    /// The program's `RefResolver` for the block check (ServiceApi.md).
    pub refs: Arc<dyn ordin_service_api::blocks::RefResolver>,
    /// The active locale, the language match and the person's language drafts (X-30; ADR-109).
    pub localisation: Arc<ordin_localisation::Localisation>,
}

/// Owned here, implemented by the personal-server program over the Trigger Service (SA-RECON-03).
#[async_trait::async_trait]
pub trait TriggerDesk: Send + Sync {
    /// JSON Schema of the Trigger Service's `TriggerProposal` without `workspace_id`.
    fn proposal_schema(&self) -> serde_json::Value;
    /// Stores the proposal (state `proposed`) and sends the trigger card; returns the trigger id as text.
    async fn propose(
        &self,
        workspace_id: WorkspaceId,
        proposal: serde_json::Value,
    ) -> Result<String, TriggerDeskError>;
}

#[derive(Debug, Clone)]
pub struct TriggerDeskError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

// ---------------------------------------------------------------- in-process interface

/// A saved workflow version and its inputs (Trigger Service: proposal checks and **/run**).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowInputsView {
    pub workflow_id: WorkflowId,
    pub name: String,
    pub state: WorkflowState,
    /// The version these inputs belong to.
    pub version: u32,
    pub latest_version: u32,
    pub inputs: Vec<WorkflowInput>,
}

/// What **Try again** on a run did (OH-TA-1); restated as the Service API's `RunsTryAgainResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TryAgainOutcome {
    /// The task started again as the run `run_id`.
    Started { run_id: RunId },
    /// Nothing started yet: the conversation asks the person first (a folder, a pack or the
    /// estimate), and `text` says what is waiting.
    Waiting { text: String },
}

/// What `runs.start_task` did (OH-TF-1); restated as the Service API's `RunsStartTaskResult`.
// spec: ordin-desktop/components/OrdinHost#OH-TF-1
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartTaskOutcome {
    /// The task started as the run `run_id`.
    Started { run_id: RunId },
    /// Nothing started: the conversation asks the person first; `text` is written for Ordin.
    Waiting { text: String },
    /// Nothing started: the values break the task's inputs, one entry per field in input order.
    Refused {
        problems: Vec<ordin_service_api::commands::runs::TaskInputProblem>,
    },
}

/// Who started a workflow run that skips the crew plan (R11). Stored on the charter row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunStartedBy {
    /// The person, from a chat command (**/run**, **Run again**).
    Person { channel: String },
    /// A turned-on chat-file trigger.
    ChatFileTrigger {
        trigger_id: TriggerId,
        file_name: String,
    },
    /// A turned-on NATS event trigger.
    EventTrigger {
        trigger_id: TriggerId,
        event_label: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunInputValue {
    /// 1–8,000 characters.
    Text {
        text: String,
    },
    Document {
        content: ContentRef,
    },
    /// 1–100 items, each 1–500 characters.
    List {
        items: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunInput {
    pub label: String,
    pub value: RunInputValue,
}

/// The immediate-start option of `workflows.run` (R11). In-process only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartWorkflowRun {
    pub workspace_id: WorkspaceId,
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub inputs: Vec<RunInput>,
    pub started_by: RunStartedBy,
}

/// The inputs of an earlier crew run, for **Run again**.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunAgainInputs {
    pub workspace_id: WorkspaceId,
    pub workflow_id: WorkflowId,
    pub version: u32,
    /// Set when a version newer than `version` is saved.
    pub newer_version: Option<u32>,
    pub inputs: Vec<WorkflowRunAgainInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowRunAgainInput {
    pub input: WorkflowInput,
    /// The value used last time; `None` when the version's inputs changed since.
    pub value: Option<RunInputValue>,
    /// "A newer version of <name> exists" when the document's location reports a newer version.
    pub newer_input_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentMakerStart {
    pub workspace_id: WorkspaceId,
    pub request: String,
    pub requirements: Vec<String>,
    pub mode: AgentMakerMode,
}

#[derive(Debug, Clone)]
pub enum AgentMakerMode {
    WholeAgent { agent_draft_id: AgentDraftId },
    OnePart { part_draft_id: PartDraftId },
    Lifecycle { part_draft_id: PartDraftId },
    Fix { finding: String },
}

#[derive(Debug, Clone)]
pub struct GenerationRequest {
    pub template_id: &'static str,
    pub data: serde_json::Value,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone)]
pub struct RehearsalRunRequest {
    pub agent_draft_id: AgentDraftId,
    pub agent_repository_id: AgentRepositoryId,
    pub worktree: PathBuf,
    pub situation: String,
    pub documents: Vec<String>,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone)]
pub struct TrialRequest {
    pub agent_draft_id: AgentDraftId,
    pub agent_repository_id: AgentRepositoryId,
    pub worktree: PathBuf,
    pub agent: String,
    pub workspace_id: WorkspaceId,
}

#[async_trait::async_trait]
pub trait AuthoringHost: Send + Sync {
    async fn start_agent_maker(&self, req: AgentMakerStart) -> Result<RunId, OhError>;
    async fn generate(&self, req: GenerationRequest) -> Result<String, OhError>;
    /// Returns when the rehearsal run ends, with what it left (SA-CLOSE-06).
    async fn rehearse(
        &self,
        req: RehearsalRunRequest,
    ) -> Result<ordin_checker::RunEvidence, OhError>;
    async fn add_trial(&self, req: TrialRequest) -> Result<AgentInstanceId, OhError>;
}

/// Handover Service (types `NoteSection`, `HandoverNote`, `StageChoice` from 2C domain).
#[async_trait::async_trait]
pub trait HandoverHost: Send + Sync {
    async fn draft_note(
        &self,
        workspace: WorkspaceId,
        results: &[String],
        stage: Option<&str>,
    ) -> Result<[ordin_types::domain::NoteSection; 4], OhError>;
    async fn recommend_next_stage(
        &self,
        workspace: WorkspaceId,
        choices: &[ordin_types::domain::StageChoice],
    ) -> Result<(String, String), OhError>;
    async fn start_from_handover(
        &self,
        workspace: WorkspaceId,
        note: &ordin_types::domain::HandoverNote,
        stage_agent: Option<String>,
    ) -> Result<(), OhError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WaitingBlock {
    pub workspace_id: WorkspaceId,
    pub block_id: BlockId,
    /// `question`, `form`, `crew_plan` or `agent_proposal` (the block kind identifier, 08).
    pub kind: String,
    /// ≤ 200 characters.
    pub title: String,
    pub since: Timestamp,
}

// ---------------------------------------------------------------- tool arguments (server `ordin`)

/// Tool `change_setting` (step 52; ADR-063 *Words*, ADR-109). Internally tagged by `setting`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "setting", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeSettingArgs {
    Tone {
        value: VoiceTone,
    },
    Register {
        value: VoiceRegister,
    },
    Length {
        value: VoiceLength,
    },
    /// `None` or an empty string clears the name; ≤ 60 characters.
    NameToUse {
        value: Option<String>,
    },
    /// `locale`: 1–35 characters; `name`: 1–60 characters.
    InterfaceLanguage {
        locale: String,
        name: String,
    },
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegateArgs {
    /// A key from the agent directory (the agent instance or remote agent id as text).
    pub agent: String,
    /// 1–8,000 characters.
    pub brief: String,
    /// 1–100.
    #[serde(default)]
    pub command: Option<String>,
    /// 0–16,384.
    #[serde(default)]
    pub arguments: Option<String>,
    /// ≤ 50.
    #[serde(default)]
    pub artefacts: Vec<ArtefactId>,
    /// `owner/name` of one of the workspace's linked repositories, 1–200 characters.
    #[serde(default)]
    pub code_in: Option<String>,
    /// OH-TF-3: a task's named inputs, at most 8 keys; each text 1–4,000 characters, a list at most
    /// 20 items. A document is "<link name>: <relative path>". Only when the person's latest message
    /// names the task (OH-TF-5); for a request in their own words, give a brief and no inputs.
    // spec: ordin-desktop/components/OrdinHost#OH-TF-3
    // spec: ordin-desktop/components/OrdinHost#OH-TF-5
    #[serde(default)]
    pub inputs: Option<std::collections::BTreeMap<String, TaskInputText>>,
    /// OH-ACC-3: the work changes a linked document (SharePoint or Confluence); the person is asked for
    /// read and change on its working copy, and every change goes back only after their approval.
    // spec: ordin-desktop/components/OrdinHost#OH-ACC-3
    #[serde(default)]
    pub changes_documents: bool,
}

/// One value of `delegate`'s `inputs`: a text, or a list of texts for a documents input (OH-TF-3).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum TaskInputText {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinishArgs {
    /// 1–4,000.
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EscalateArgs {
    /// 1–500.
    pub question: String,
    /// 0–500.
    #[serde(default)]
    pub recommendation: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecideArgs {
    /// reason 1–500.
    Check {
        verdict: CheckVerdictArg,
        reason: String,
    },
    /// path 1–40.
    Condition { path: String, reason: String },
    /// 1–4,000.
    Question { answer: String },
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckVerdictArg {
    CarryOn,
    Shortfall,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCrewArgs {
    /// 1–2,000.
    pub purpose: String,
    /// 1–8.
    pub members: Vec<CrewMemberArg>,
    #[serde(default)]
    pub limits: Option<CrewLimitsArg>,
    /// ≤ 20 labels, 1–60.
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrewMemberArg {
    pub agent: String,
    /// 1–200.
    pub role: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrewLimitsArg {
    pub rounds: Option<u16>,
    pub wall_clock_minutes: Option<u16>,
    pub members_at_once: Option<u8>,
    pub request_budget: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeepCrewArgs {
    pub crew_run_id: CrewRunId,
    pub name: String,
}

/// Tool `keep_dashboard` (OH-KD-1): keeps a report of the workspace as a dashboard. `report` names
/// the report by its title; absent, the workspace's most recent report is kept.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeepDashboardArgs {
    pub name: String,
    #[serde(default)]
    pub report: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveWorkflowArgs {
    pub name: String,
    pub purpose: String,
    pub inputs: Vec<WorkflowFileInput>,
    pub agents: Vec<DraftAgentArg>,
    pub steps: Vec<WorkflowFileStep>,
    #[serde(default)]
    pub limits: Option<WorkflowFileLimits>,
    #[serde(default)]
    pub crew_run_id: Option<CrewRunId>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DraftAgentArg {
    pub key: String,
    pub agent: String,
    pub role: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeWorkflowChangeArgs {
    pub workflow_id: WorkflowId,
    pub base_version: u32,
    pub steps: Vec<WorkflowFileStep>,
    pub change_note: String,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub inputs: Option<Vec<WorkflowFileInput>>,
    #[serde(default)]
    pub limits: Option<WorkflowFileLimits>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TakeOverArgs {
    pub crew_run_id: CrewRunId,
    pub from_position: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShowArgs {
    pub block: ordin_service_api::blocks::Block,
}

/// Tool `remember` (amended January 2026, ADR-118; replaces the ADR-111 shape): exactly one of
/// `facts` and `summary` with `made_from`.
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RememberArgs {
    /// "conversation", or where the facts were read (1–2,048).
    pub source: String,
    /// 1–16; absent only with `summary`.
    #[serde(default)]
    #[schemars(schema_with = "crate::tools::triple_inputs_schema")]
    pub facts: Vec<ordin_memory_service::TripleInput>,
    /// A summary of 1–500 characters; only with `made_from`.
    #[serde(default)]
    pub summary: Option<String>,
    /// 2–64 fact ids; only with `summary`.
    #[serde(default)]
    pub made_from: Vec<MemoryFactId>,
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchKindArg {
    Lex,
    Vec,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArg {
    #[serde(rename = "type")]
    pub kind: SearchKindArg,
    /// 1–500.
    pub query: String,
}

/// Tool `recall` (amended January 2026, ADR-110, ADR-113).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecallArgs {
    /// 1–4,096; exactly one of `query` and `searches`.
    #[serde(default)]
    pub query: Option<String>,
    /// 1–4.
    #[serde(default)]
    pub searches: Option<Vec<SearchArg>>,
    /// 1–21 folder names, or "own".
    #[serde(default)]
    pub folders: Option<Vec<String>>,
    /// 1–20, default 10.
    #[serde(default)]
    pub result_cap: Option<u8>,
    #[serde(default)]
    pub wider: Option<bool>,
}

/// One source of a knowledge page.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceArg {
    /// Where the source comes from: `place` (a file in a linked folder), `sharepoint` or `confluence`.
    pub name: String,
    /// For `place`: `place:<place id>:<path>`, exactly as an "Attached:" line gives it after "as a page's source:".
    pub location: String,
    /// 1–256 characters: what was read, for example "as attached".
    pub version_read: String,
}

/// Tool `write_page` (amended January 2026, ADR-112).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WritePageArgs {
    /// 1–200.
    pub title: String,
    /// ≤ 1 MiB.
    pub body: String,
    /// ≤ 100.
    pub sources: Vec<SourceArg>,
    /// 1–200 characters, one sentence.
    pub description: String,
    /// One of Explanation, How-to, Decision, Reference, Glossary (default Explanation).
    #[serde(default, rename = "type")]
    pub page_type: Option<String>,
    /// The page to change, as an earlier `write_page` answered it; absent writes a new page, and a title that
    /// is already a page's is refused.
    #[serde(default)]
    pub page_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindAgentArgs {
    /// 1–500.
    pub need: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeAgentArgs {
    pub agent_repository_id: AgentRepositoryId,
    /// `urn:ordin:…`, 1–256.
    pub agent: String,
    /// 1–300.
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PushItemArg {
    pub item: WorkingCopyItemId,
    /// 1–120, one line.
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposePushArgs {
    /// 1–4,000.
    pub request: String,
    /// 1–200.
    pub items: Vec<PushItemArg>,
}

/// Amended January 2026 (ADR-102).
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposeWorkflowRunArgs {
    pub workflow_id: WorkflowId,
    /// 1–4,000 characters: what the run is for.
    pub request: String,
}
