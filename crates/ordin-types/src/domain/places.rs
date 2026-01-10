//! 2C part 1 § Workspaces, places and content.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- Workspace ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub own_folder_place_id: PlaceId,
    pub state: WorkspaceState,
    /// Not stored in `workspaces`: filled on read from Home and Search's own table
    /// `workspace_last_seen` (amended January 2026, decision 80: C-31).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_to_bin_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Active,
    Archived,
    InBin,
}

// ---- HomeMarks ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HomeMarks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_last_opened_at: Option<Timestamp>,
    pub changed_marks: Vec<ChangedMark>,
}

/// A document changed elsewhere that the person has not opened since (ADR-079).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ChangedMark {
    pub id: ChangedMarkId,
    pub workspace_id: WorkspaceId,
    pub place_id: PlaceId,
    pub path: String,
    pub changed_at: Timestamp,
}

// ---- Place ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Place {
    pub id: PlaceId,
    pub friendly_name: String,
    pub path: String,
    pub kind: PlaceKind,
    pub added_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// `WorkingCopy` (the local working copy of a SharePoint or Confluence link) and
/// `CodingTaskWorktree` (a coding task's worktree) amended January 2026, decision 80: C-9.
pub enum PlaceKind {
    PersonsFolder,
    WorkspaceFolder,
    AgentFolder,
    WorkingCopy,
    CodingTaskWorktree,
}

/// Membership of a place in a workspace (a place may belong to several, 04:65).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WorkspacePlace {
    pub workspace_id: WorkspaceId,
    pub place_id: PlaceId,
    pub added_at: Timestamp,
}

// ---- FolderGrant ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct FolderGrant {
    pub id: FolderGrantId,
    pub workspace_id: WorkspaceId,
    pub grantee: Grantee,
    pub place_id: PlaceId,
    pub access: FolderAccess,
    pub granted_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Grantee {
    AgentInstance {
        agent_instance_id: AgentInstanceId,
    },
    /// "crew" is not given an identifier in 04; the charter is assumed (LD-31).
    Crew {
        crew_charter_id: CrewCharterId,
    },
}

/// "can't use" is the absence of a grant (04:61).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FolderAccess {
    Read,
    ReadAndChange,
}

// ---- SourceLink ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SourceLink {
    pub id: SourceLinkId,
    pub workspace_id: WorkspaceId,
    pub kind: SourceLinkKind,
    pub origin_location: String,
    pub display_name: String,
    /// Set for local and repository folders, which are also places (04:75), and for SharePoint
    /// and Confluence links once their working copy exists (amended January 2026, decision 80: C-9).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<PlaceId>,
    pub state: SourceLinkState,
    pub added_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_read_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkKind {
    LocalFolder,
    GitRepositoryFolder,
    SharepointLocation,
    ConfluenceLocation,
    /// A repository folder linked without a local clone (`LinkLocation::GitRemote { url, branch, path }`,
    /// WorkspaceService.md SA-CLOSE-03; closing round, K21). `origin_location` = `git:<url without .git>#<branch>:<path>`.
    GitRemote,
    /// A Jira Cloud issue, read only (`LinkLocation::JiraIssue { address }`; amended January 2026, ADR-106).
    /// `origin_location` = the canonical `https://{site}.atlassian.net/browse/{KEY}-{N}`.
    JiraIssue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourceLinkState {
    Proposed,
    Active,
}

// ---- WorkingCopyItem ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct WorkingCopyItem {
    pub id: WorkingCopyItemId,
    pub source_link_id: SourceLinkId,
    pub origin_path: String,
    /// Version or change time at the origin when read (A-25).
    pub origin_version_read: String,
    pub local_version: u32,
    pub state: WorkingCopyState,
    pub read_at: Timestamp,
    /// Jira issue items only: when the origin check saw a newer version, which waits for
    /// **Bring in the new version** (amended January 2026, ADR-106; WorkspaceService.md §J).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_newer_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkingCopyState {
    Read,
    Changed,
    AwaitingPush,
    Pushed,
    OriginChanged,
}

// ---- PendingPush ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct PendingPush {
    pub id: PendingPushId,
    pub workspace_id: WorkspaceId,
    /// The person's request the changes answer, as text (assumed, LD-30).
    pub request: String,
    pub items: Vec<PendingPushItem>,
    pub origins: Vec<SourceLinkId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    pub state: PendingPushState,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct PendingPushItem {
    pub working_copy_item_id: WorkingCopyItemId,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_action_key: Option<ExternalActionKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PendingPushState {
    Proposed,
    Held,
    Approved,
    Rejected,
    Closed,
}

// ---- FileVersion ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct FileVersion {
    pub id: FileVersionId,
    pub place_id: PlaceId,
    pub path: String,
    pub version: u32,
    pub hash: Sha256Hex,
    pub origin: FileVersionOrigin,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileVersionOrigin {
    Person,
    Run { run_id: RunId, tool: String },
    ArtCraftEdit { app: String },
}

// ---- Artefact ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Artefact {
    pub id: ArtefactId,
    pub location: ArtefactLocation,
    pub path: String,
    pub version: u32,
    pub hash: Sha256Hex,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producing_run_id: Option<RunId>,
    pub media_type: String,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtefactLocation {
    AgentFolder { agent_instance_id: AgentInstanceId },
    Place { place_id: PlaceId },
}

// ---- MemoryFact ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct MemoryFactRef {
    pub id: MemoryFactId,
    /// None = installation-wide scope; Some = the workspace's team scope (02:350).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<WorkspaceId>,
}

// ---- KnowledgePage ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KnowledgePage {
    pub id: KnowledgePageId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub body_markdown: String,
    pub written_by: KnowledgeAuthor,
    pub edited_by_person: bool,
    pub sources: Vec<KnowledgeSource>,
    pub state: KnowledgePageState,
    pub last_changed_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAuthor {
    Ordin,
    Person,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeSource {
    pub origin: String,
    pub location: String,
    pub version_read: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KnowledgePageState {
    Current,
    MayBeOutOfDate,
    Removed,
}

// ---- ConfluenceLink ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ConfluenceLink {
    pub id: ConfluenceLinkId,
    pub workspace_id: WorkspaceId,
    pub place_id: PlaceId,
    /// Folder relative to the place; empty = the place's root.
    pub folder: String,
    pub confluence_space: String,
    pub parent_page_id: String,
    pub audience: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<Timestamp>,
    pub state: ConfluenceLinkState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ConfluenceLinkState {
    Proposed,
    Active,
    Paused,
}

// ---- PublishedCopy ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct PublishedCopy {
    pub id: PublishedCopyId,
    pub original: CopyOriginal,
    pub destination: CopyDestination,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_item_id: Option<String>,
    pub format: CopyFormat,
    pub maker: CopyMaker,
    pub published_via: PublishedVia,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followed_branch: Option<String>,
    pub keep_up_to_date: bool,
    pub audience: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_version_published: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_version_pushed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<Timestamp>,
    pub state: PublishedCopyState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyOriginal {
    PlaceFile { place_id: PlaceId, path: String },
    KnowledgePage { knowledge_page_id: KnowledgePageId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CopyDestination {
    ConfluencePage {
        space: String,
        parent_page_id: String,
    },
    ConfluenceAttachment {
        space: String,
        parent_page_id: String,
    },
    SharepointFile {
        site: String,
        library: String,
        folder: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CopyFormat {
    Page,
    Word,
    Pdf,
    Slides,
    ExactFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CopyMaker {
    Ordin,
    WordCraft,
    PdfCraft,
    DeckCraft,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PublishedVia {
    ConfluenceLink {
        confluence_link_id: ConfluenceLinkId,
    },
    PublishACopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PublishedCopyState {
    ReadyToPublish,
    AwaitingApproval,
    Current,
    OutOfDate,
    Updating,
    ChangedAtDestination,
    Failed,
    NotKeptUpToDate,
    Removed,
}
