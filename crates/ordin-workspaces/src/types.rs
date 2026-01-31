//! The public types of the Workspace Service (WorkspaceService.md *Public Interface*).

use std::path::PathBuf;
use std::sync::Arc;

use ordin_confluence::{ConfluenceAdapter, ConfluenceLocation};
use ordin_files::FileService;
use ordin_jira::JiraAdapter;
use ordin_sharepoint::{SharePointAdapter, SharePointLocation};
use ordin_types::domain::email::EmailLinkKind;
use ordin_types::domain::places::{SourceLinkKind, WorkingCopyItem, Workspace, WorkspaceState};
use ordin_types::domain::runs::ShareHost;
use ordin_types::domain::server::ChannelKind;
use ordin_types::time::Timestamp;
use ordin_types::{PlaceId, SourceLinkId, WorkspaceId};
use serde::{Deserialize, Serialize};

/// What the program hands the service (Public Interface *WorkspaceDeps*).
pub struct WorkspaceDeps {
    /// Also the source of this crate's `Datastore` (`journal.datastore()`, D4).
    pub journal: ordin_journal::Journal,
    pub rules: Arc<ordin_rules::RulesEngine>,
    pub files: Arc<FileService>,
    pub sharepoint: Arc<SharePointAdapter>,
    /// `None` on the personal server (2B-3: Confluence is D only).
    pub confluence: Option<Arc<ConfluenceAdapter>>,
    /// `None` on the personal server: Jira is D only (amended January 2026, ADR-106).
    pub jira: Option<Arc<JiraAdapter>>,
    pub data_root: PathBuf,
    pub documents_dir: PathBuf,
    /// True in `ordin-server` (personal profile): folders on the person's computer cannot be linked there.
    pub on_server: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceListFilter {
    Active,
    Archived,
    InBin,
    NotInBin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkLocation {
    LocalFolder {
        path: String,
    },
    GitRepositoryFolder {
        path: String,
    },
    Sharepoint {
        location: SharePointLocation,
    },
    Confluence {
        location: ConfluenceLocation,
    },
    /// A folder of a GitHub or GitLab repository, linked without a local clone (2C addition, K21;
    /// SA-CLOSE-03). `url` `https://<host>/<owner>/<name>(.git)?` or `git@<host>:<owner>/<name>(.git)?`;
    /// `branch` 1–255 characters; `path` relative to the repository root, "" for the root, ≤ 1,024
    /// bytes, no `..`.
    GitRemote {
        url: String,
        branch: String,
        path: String,
    },
    /// A Jira Cloud issue address as the person pasted or picked it (amended January 2026, ADR-106;
    /// amendment §J.1).
    JiraIssue {
        address: String,
    },
}

/// A file this service saved in a workspace's own folder (SA-CLOSE-02).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlaceFileRef {
    pub place_id: PlaceId,
    pub relative_path: String,
    /// "<workspace name> › Inbox › <file name>", for the reply that names where the file went.
    pub display_path: String,
}

/// Where a file saved with `save_to_inbox` came from (SA-CLOSE-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InboxOrigin {
    Chat { channel: ChannelKind },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkProgress {
    pub source_link_id: SourceLinkId,
    pub listed: u64,
    pub read: u64,
    /// Beyond `LINK_READ_BUDGET` or failed.
    pub not_read: u64,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OpenReport {
    pub workspace: Workspace,
    /// Changes found by the adapters at this open.
    pub origin_changes: u32,
    pub checks_failed: Vec<SourceLinkId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BinImpact {
    /// Documents changed here and not yet sent back (working-copy items not `read`/`pushed`, and
    /// held edits).
    pub unsent_changes: u32,
    /// Display names, at most 10.
    pub documents: Vec<String>,
}

/// What the Push Service needs about one working-copy item (through the program's
/// `WorkingCopyStore`, R12).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkingCopyEntry {
    pub item: WorkingCopyItem,
    pub workspace_id: WorkspaceId,
    pub link_kind: SourceLinkKind,
    /// The link's `origin_location` (SharePoint `graph:drives/<drive>/items/<item>` form, Confluence
    /// origin form).
    pub origin_location: String,
    pub display_name: String,
    /// The working-copy place and the item's path in it; the bytes and versions are the File
    /// Service's. For a local or git folder link this is the folder's own place: the folder is its
    /// own working copy.
    pub place_id: PlaceId,
    pub relative_path: String,
    /// Git folder links only: the branch checked out when the item was recorded, and the repository
    /// (`None` when its `origin` remote is not on GitHub or GitLab, Internal Logic 10.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<LinkedRepository>,
}

/// The origin version this service last recorded for an item (Internal Logic 6.4, 7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginVersion {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_at: Option<Timestamp>,
}

/// Broadcast when an origin check records a new origin version for an item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginChanged {
    pub workspace_id: WorkspaceId,
    pub source_link_id: SourceLinkId,
    pub link_kind: SourceLinkKind,
    pub origin_path: String,
    pub version: OriginVersion,
    /// True when the item was removed at its origin.
    pub removed: bool,
}

/// A linked git repository folder whose `origin` remote is on GitHub or GitLab (Internal Logic 10.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LinkedRepository {
    pub source_link_id: SourceLinkId,
    pub host: ShareHost,
    /// `owner/name` (2C repository rule).
    pub repository: String,
    /// The remote's host name, e.g. "github.com" or "gitlab.example.com".
    pub host_name: String,
    pub place_id: PlaceId,
}

/// The facts of one workspace that Ordin's seed needs (OrdinHost X-7).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceSummary {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub state: WorkspaceState,
    /// Display names of the workspace's places and active links, in the tree's order.
    pub places: Vec<String>,
    pub repositories: Vec<LinkedRepository>,
}

pub struct NewEmailLink {
    pub kind: EmailLinkKind,
    pub folder_or_label: Option<String>,
    pub addresses: Vec<String>,
    pub domains: Vec<String>,
    pub display_name: String,
}

/// The value a linked folder reports: whether the person's own changes there are pushed to
/// Confluence without a card, and which grants turn that off (WorkspaceService.md amendment
/// 2026-01-24).
// spec: ordin-desktop/components/WorkspaceService#SA-WS-AUTO-03
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfluencePublishing {
    pub confluence_link_id: ordin_types::ConfluenceLinkId,
    pub automatic: ordin_confluence::AutomaticPublishing,
    /// The grantees whose grants turn automatic publishing off; empty when `automatic` is `On`.
    pub turned_off_by: Vec<ordin_types::domain::places::Grantee>,
}
