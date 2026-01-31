//! The Push Service's own public types (PushService.md *Public Interface* and *Boundary
//! interfaces*). The 2C types it carries are used from `ordin-types`, never redefined.

use std::path::PathBuf;
use std::sync::Arc;

use ordin_types::domain::agents::PullRequestRef;
use ordin_types::domain::places::{
    CopyDestination, CopyFormat, CopyOriginal, SourceLinkKind, WorkingCopyItem, WorkingCopyState,
};
use ordin_types::wire::approval::{AffectedItem, ApprovalContent};
use ordin_types::{
    AgentDraftId, ArtefactId, HandoverId, PartDraftId, PendingPushId, PlaceId, PublishedCopyId,
    RunId, TeamShareId, WorkingCopyItemId, WorkspaceId,
};
use serde::{Deserialize, Serialize};

/// The services this one calls (Public Interface). Each `Arc` is the program's one instance.
pub struct PushDeps {
    /// This service's tables live in `journal.datastore()` (decision D4; no separate pool).
    pub journal: Arc<ordin_journal::Journal>,
    pub approvals: Arc<ordin_approval::ApprovalService>,
    /// Implemented by the program (D3).
    pub working_copy: Arc<dyn WorkingCopyStore>,
    pub files: Arc<ordin_files::FileService>,
    pub projects: Arc<ordin_projects::ProjectService>,
    /// The Project Service as `GithubAccess` (the same object as `projects`).
    pub github: Arc<dyn ordin_projects::GithubAccess>,
    pub sharepoint: Arc<ordin_sharepoint::SharePointAdapter>,
    /// `None` on the ordin server, which has no Confluence Adapter.
    pub confluence: Option<Arc<ordin_confluence::ConfluenceAdapter>>,
    pub craft: Arc<ordin_craft_toolkit::CraftToolkit>,
    /// Formats the card and copy-header text (Fluent messages `push-*`).
    pub localisation: Arc<ordin_localisation::Localisation>,
}

/// Mine (decision D3). The Workspace Service owns `working_copy_items` (ruling R12), and 2B-3 gives
/// this crate no edge to it, so the desktop and server programs implement this trait.
// spec: ordin-desktop/components/PushService#SA-RECON-01
#[async_trait::async_trait]
pub trait WorkingCopyStore: Send + Sync {
    async fn entry(&self, id: WorkingCopyItemId) -> Result<WorkingCopyEntry, StoreFailure>;
    /// `origin_version` is required for `pushed` (the new origin version) and ignored otherwise.
    async fn set_state(
        &self,
        id: WorkingCopyItemId,
        state: WorkingCopyState,
        origin_version: Option<String>,
    ) -> Result<(), StoreFailure>;
    /// The current bytes, or those of `local_version` (File Service version number) when given.
    async fn content(
        &self,
        id: WorkingCopyItemId,
        local_version: Option<u32>,
    ) -> Result<Vec<u8>, StoreFailure>;
    async fn restore_version(
        &self,
        id: WorkingCopyItemId,
        local_version: u32,
    ) -> Result<(), StoreFailure>;
    /// Ids of items changed on this computer (an item moving to `changed`, or a further change to it).
    fn local_changes(&self) -> tokio::sync::broadcast::Receiver<WorkingCopyItemId>;
}

#[derive(Debug, Clone)]
pub struct WorkingCopyEntry {
    pub item: WorkingCopyItem,
    pub workspace_id: WorkspaceId,
    pub link_kind: SourceLinkKind,
    /// SharePoint `graph:drives/<drive>/items/<item>`; Confluence `confluence:space/{key}` or
    /// `confluence:page/{id}`; the canonical folder path for local and git folder links.
    pub origin_location: String,
    pub display_name: String,
    pub place_id: PlaceId,
    pub relative_path: String,
    pub git_branch: Option<String>,
    /// Git folder links whose `origin` remote is on GitHub or GitLab: the host name and `owner/name`.
    pub git_host: Option<String>,
    pub git_repository: Option<String>,
}

/// The provider's error code (`WS_*` or `FILE_*`) as the program received it.
#[derive(Debug, Clone)]
pub struct StoreFailure {
    pub code: &'static str,
    pub retryable: bool,
}

// ---- inputs ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PushProposal {
    pub workspace_id: WorkspaceId,
    /// Ordin's run when proposed through `propose_push`.
    pub run_id: Option<RunId>,
    pub request: String,
    pub items: Vec<ProposedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProposedItem {
    pub working_copy_item_id: WorkingCopyItemId,
    pub summary: String,
    pub change_view: Option<ArtefactId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChooseVersionInput {
    pub pending_push_id: PendingPushId,
    pub working_copy_item_id: WorkingCopyItemId,
    pub choice: HeldChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeldChoice {
    KeepMine,
    KeepOrdins,
    ApplyOrdinsToMine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TryAgainInput {
    pub pending_push_id: PendingPushId,
    pub working_copy_item_id: WorkingCopyItemId,
}

/// UUIDv7 (*chosen here*).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubmissionId(pub uuid::Uuid);

impl SubmissionId {
    pub fn new() -> SubmissionId {
        SubmissionId(uuid::Uuid::now_v7())
    }
}

impl Default for SubmissionId {
    fn default() -> Self {
        SubmissionId::new()
    }
}

impl std::fmt::Display for SubmissionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PullRequestProposal {
    pub workspace_id: WorkspaceId,
    pub origin: SubmissionOrigin,
    pub host: String,
    pub repository: String,
    pub content: PullRequestContent,
    pub branch: String,
    pub base: String,
    /// 1–256 chars.
    pub title: String,
    /// At most 65,536 chars (GitHub's body limit).
    pub body: String,
    pub labels: Vec<String>,
    pub reviewers: Vec<String>,
    /// *Send the change…*: push to the same pull request.
    pub existing: Option<PullRequestRef>,
    pub card: CardText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubmissionOrigin {
    AgentDraft {
        id: AgentDraftId,
    },
    PartDraft {
        id: PartDraftId,
    },
    Handover {
        id: HandoverId,
    },
    /// A saved workflow shared to a repository folder by the Ordin Host (ADR-066): `content` is
    /// `Files` with the one `.workflow.yaml` file.
    WorkflowShare {
        workspace_id: WorkspaceId,
        file_name: String,
    },
}

impl SubmissionOrigin {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            SubmissionOrigin::AgentDraft { .. } => "agent_draft",
            SubmissionOrigin::PartDraft { .. } => "part_draft",
            SubmissionOrigin::Handover { .. } => "handover",
            SubmissionOrigin::WorkflowShare { .. } => "workflow_share",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PullRequestContent {
    /// A worktree whose changed files are committed with git2 and pushed (agent and part drafts).
    Worktree { path: PathBuf, paths: Vec<String> },
    /// Files written through GitHub's API on a new branch, no clone (handovers).
    Files { files: Vec<(String, Vec<u8>)> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CardText {
    pub question: String,
    pub verb: String,
    pub where_line: String,
    pub affects: Vec<AffectedItem>,
    pub changes: String,
    pub content: Option<ApprovalContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovedPullRequest {
    pub approval_id: ordin_types::ApprovalId,
    pub proposal: PullRequestProposal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PublishCopyRequest {
    pub workspace_id: WorkspaceId,
    /// `original_version`: the commit for an original in a repository place; otherwise the File
    /// Service version number as text ("4").
    pub original: CopyOriginal,
    pub original_version: String,
    pub original_name: String,
    /// `CopyOriginal::KnowledgePage` only: the page's file (place and path), supplied by the
    /// Knowledge Service from its page row (SA-CLOSE-01); `None` for `PlaceFile`.
    pub page_file: Option<(PlaceId, String)>,
    pub destination: CopyDestination,
    pub format: CopyFormat,
    pub followed_branch: Option<String>,
    pub keep_up_to_date: bool,
    /// "Use another name" sets it.
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CopyActionInput {
    pub published_copy_id: PublishedCopyId,
    pub action: CopyAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyAction {
    TryAgain,
    BringItIn,
    LeaveIt,
    ReplaceAtNextUpdate,
    StopKeepingUpToDate,
    KeepUpToDate,
    Remove,
}

/// A commit the Team Knowledge Service built in its own bare clone (ADR-103; owned by the Team
/// Knowledge Service, copied here).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamShareProposal {
    pub share_id: TeamShareId,
    pub workspace_id: WorkspaceId,
    pub clone_path: PathBuf,
    /// 40 lowercase hex.
    pub commit: String,
    pub host: String,
    pub repository: String,
    pub branch: String,
    /// `Some` = the branch requires review: push here and open a pull or merge request into `branch`.
    pub review_branch: Option<String>,
    /// 1–256 chars.
    pub title: String,
    /// At most 65,536 chars.
    pub body: String,
    pub card: CardText,
}

/// What the Push Service tells its subscribers (Ordin Host, Authoring Service, Handover Service,
/// Team Knowledge Service).
#[derive(Debug, Clone)]
pub enum PushNotice {
    PushFinished {
        pending_push_id: PendingPushId,
        workspace_id: WorkspaceId,
        pushed: Vec<WorkingCopyItemId>,
        failed: Vec<(WorkingCopyItemId, &'static str)>,
        origin_changed: Vec<WorkingCopyItemId>,
    },
    PushHeld {
        pending_push_id: PendingPushId,
        item: WorkingCopyItemId,
    },
    PullRequestOpened {
        submission: SubmissionId,
        origin: SubmissionOrigin,
        pull_request: PullRequestRef,
    },
    PullRequestFailed {
        submission: SubmissionId,
        origin: SubmissionOrigin,
        code: &'static str,
    },
    /// Card rejected.
    PullRequestRejected {
        submission: SubmissionId,
        origin: SubmissionOrigin,
    },
    CopiesUpdated {
        original: CopyOriginal,
        updated: u32,
        held: u32,
        failed: u32,
        from_version: String,
    },
    TeamShareSent {
        share_id: TeamShareId,
        commit: String,
        pull_request: Option<PullRequestRef>,
    },
    TeamShareMoved {
        share_id: TeamShareId,
    },
    TeamShareFailed {
        share_id: TeamShareId,
        code: &'static str,
    },
    TeamShareRejected {
        share_id: TeamShareId,
    },
}
