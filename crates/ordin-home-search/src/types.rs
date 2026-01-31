//! The module's public types and the two traits it owns (HomeAndSearch.md *Shared Context*
//! *Boundary interfaces* and *Public Interface*).

use ordin_types::common::InputUsed;
use ordin_types::time::Timestamp;
use ordin_types::{
    ApprovalId, BlockId, CrewRunId, HandoverId, KnowledgePageId, PlaceId, RemoteAgentId, RunId,
    WorkspaceId,
};
use serde::{Deserialize, Serialize};

/// Where a Home row, a search result or a notification opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenTarget {
    Workspace {
        workspace_id: WorkspaceId,
    },
    Document {
        workspace_id: WorkspaceId,
        place_id: PlaceId,
        path: String,
    },
    Run {
        workspace_id: WorkspaceId,
        run_id: RunId,
    },
    CrewRun {
        workspace_id: WorkspaceId,
        crew_run_id: CrewRunId,
    },
    Approval {
        workspace_id: WorkspaceId,
        approval_id: ApprovalId,
    },
    /// A question, form, crew plan or offer block waiting for the person (reconciliation row 182).
    // spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-01
    Block {
        workspace_id: WorkspaceId,
        block_id: BlockId,
    },
    /// A hosted agent's approver card; the screen offers approve and reject through
    /// `hosted.decide_card`.
    // spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-05
    HostedApproval {
        remote_agent_id: RemoteAgentId,
        approval_id: ApprovalId,
    },
    /// A handover handed or returned to the person; the desktop's notifier reads this variant.
    Handover {
        workspace_id: Option<WorkspaceId>,
        handover_id: HandoverId,
    },
    KnowledgePage {
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    },
    /// "home", "settings", "add_documents" …
    Screen {
        name: String,
    },
    /// An item from a `SearchSource`.
    // The field is written `item_kind` on the wire because `kind` is the enum's serde tag
    // (HomeAndSearch.md, `OpenTarget`).
    Item {
        #[serde(rename = "item_kind")]
        kind: String,
        id: String,
        workspace_id: Option<WorkspaceId>,
    },
}

/// One line of Home or *Needs you*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HomeRow {
    pub workspace_id: Option<WorkspaceId>,
    pub workspace_name: Option<String>,
    pub line: String,
    pub at: Timestamp,
    pub is_new: bool,
    pub opens: OpenTarget,
    pub run_again: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HomeView {
    pub waiting: Vec<HomeRow>,
    pub running: Vec<HomeRow>,
    pub since_last_looked: Vec<HomeRow>,
    pub new_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoToKind {
    Workspace,
    Agent,
    Workflow,
    Dashboard,
    KnowledgePage,
    Conversation,
    Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GoToResult {
    pub kind: GoToKind,
    pub label: String,
    pub detail: Option<String>,
    pub workspace_id: Option<WorkspaceId>,
    pub opens: OpenTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FoundResult {
    pub label: String,
    pub snippet: Option<String>,
    pub workspace_id: WorkspaceId,
    pub opens: OpenTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchAnswer {
    pub query: String,
    pub go_to: Vec<GoToResult>,
    pub found: Vec<FoundResult>,
    pub actions: Vec<GoToResult>,
    /// "Searched 41,200 of 58,000 documents" while indexes are still building (11 §5.6).
    pub coverage: Option<(u64, u64)>,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunAgainSubject {
    Run { run_id: RunId },
    CrewRun { crew_run_id: CrewRunId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunAgainInput {
    pub input: InputUsed,
    pub newer_version: Option<String>,
    pub check: VersionCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionCheck {
    Same,
    Newer,
    Gone,
    CannotCheck,
}

/// One item waiting for the person, as the program's `JournalReader` composite builds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WaitingItem {
    /// `None`: a hosted agent's approver card.
    // spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-05
    pub workspace_id: Option<WorkspaceId>,
    pub kind: String,
    pub title: String,
    pub since: Timestamp,
    pub opens: OpenTarget,
}

/// A running or finished run or crew run, titled by the program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSummary {
    pub workspace_id: WorkspaceId,
    pub run_id: Option<RunId>,
    pub crew_run_id: Option<CrewRunId>,
    pub title: String,
    /// The run's state; `replied_in_slack` for a Slack remote agent's completed run.
    // spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-02
    pub state: String,
    pub at: Timestamp,
    pub can_run_again: bool,
}

/// Built here from `WorkspaceService::origin_changes_since`: one per (workspace, link).
// spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-03
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyUpdateSummary {
    pub workspace_id: WorkspaceId,
    pub original_name: String,
    pub count: u32,
    pub source: String,
    pub at: Timestamp,
}

/// Mine. `NotFound` → `HOME_RUN_NOT_FOUND` (`run_inputs` only); `Unavailable` →
/// `HOME_SOURCE_UNAVAILABLE`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HomeSourceError {
    #[error("not found")]
    NotFound,
    #[error("unavailable")]
    Unavailable,
}

/// Mine, implemented by the program over modules this crate has no edge to. A source the program
/// cannot reach leaves its rows out.
// spec: ordin-desktop/components/HomeAndSearch#SA-RECON-02
#[async_trait::async_trait]
pub trait JournalReader: Send + Sync {
    /// Approvals, unknown-outcome external actions, held pushes, waiting handovers, Ordin's waiting
    /// blocks and hosted agents' approver cards.
    async fn waiting_items(&self) -> Result<Vec<WaitingItem>, HomeSourceError>;
    /// Running runs and crew runs, titled.
    async fn running(&self) -> Result<Vec<RunSummary>, HomeSourceError>;
    /// Runs ended since `since`, at most `limit`, titled as `running`.
    async fn finished_since(
        &self,
        since: Timestamp,
        limit: u32,
    ) -> Result<Vec<RunSummary>, HomeSourceError>;
    /// The inputs a finished run or crew run read.
    async fn run_inputs(&self, run: RunAgainSubject) -> Result<Vec<InputUsed>, HomeSourceError>;
}

/// Mine: other listers the program registers (agents, workflows, dashboards, conversations).
// spec: ordin-desktop/components/HomeAndSearch#SA-RECON-02
#[async_trait::async_trait]
pub trait SearchSource: Send + Sync {
    fn group(&self) -> GoToKind;
    async fn find(&self, query: &str, limit: u32) -> Vec<GoToResult>;
}
