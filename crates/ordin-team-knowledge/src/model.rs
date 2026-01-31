//! The public shapes of the service (TeamKnowledgeService.md *Public Interface* and the team workspace
//! amendment's *Public Interface (additions)*).

use ordin_types::domain::{FolderAccess, PullRequestRef, SourceLinkKind, TeamKnowledgeBinding};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::ContentRef;
use ordin_types::{
    ApprovalId, SourceLinkId, TeamActivityId, TeamGrantId, TeamShareId, WorkspaceId,
};
use serde::{Deserialize, Serialize};

/// The long command now running for a workspace (§H; `Opening` added by TW-I step 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamBusy {
    Checking,
    Getting,
    Sharing,
    Opening,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TeamFolderChoice {
    Link {
        source_link_id: SourceLinkId,
    },
    /// A new GitRemote link, added in the same step (SA-TEAM-01).
    NewFolder {
        url: String,
        branch: String,
        path: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamKnowledgeStatus {
    pub workspace_id: WorkspaceId,
    pub binding: Option<TeamKnowledgeBinding>,
    /// "acme/supplier-team › knowledge"; None when off.
    pub header_line: Option<String>,
    pub available: Option<TeamAvailable>,
    pub to_share: Option<TeamCounts>,
    /// Facts waiting for the person's check (D10); 0 when none.
    pub held_for_check: u32,
    pub waiting_for_review: Option<PullRequestRef>,
    pub problems: Vec<TeamProblem>,
    /// None = Share allowed; Some(sentence code) otherwise, e.g. TEAM_READ_ONLY.
    pub share_unavailable: Option<&'static str>,
    pub last_checked_at: Option<Timestamp>,
    pub folder_large: bool,
    /// The long command now running for this workspace (§H); None when idle.
    pub busy: Option<TeamBusy>,
    /// Outcome of the latest get and share started by the person.
    pub last_get: Option<GetReport>,
    pub last_share: Option<ShareProposed>,
    /// Code of the latest long command that failed; cleared when one succeeds.
    pub last_error: Option<&'static str>,
    /// None for a workspace that is not a team workspace (team workspace amendment).
    pub team_workspace: Option<TeamWorkspaceStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamAvailable {
    pub counts: TeamCounts,
    /// Sharer names, ≤ 5, then "and N others".
    pub from: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamCounts {
    pub new_facts: u32,
    pub changed_facts: u32,
    pub removed_facts: u32,
    pub new_pages: u32,
    pub changed_pages: u32,
    pub removed_pages: u32,
}

impl TeamCounts {
    pub(crate) fn is_empty(&self) -> bool {
        *self == TeamCounts::default()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamProblem {
    pub path: String,
    pub code: &'static str,
    pub first_seen_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetReport {
    pub counts: TeamCounts,
    pub not_taken: u32,
    pub skipped_files: u32,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ShareProposed {
    pub share_id: TeamShareId,
    pub approval_id: ApprovalId,
    pub counts: TeamCounts,
    pub held_for_check: u32,
}

// ---- the team workspace (amendment January 2026, ADR-121) ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TeamResultsChoice {
    /// `results/` beneath the own team folder (SA-TEAM-TW-05). An empty struct variant, so an extra
    /// field is refused (serde ignores `deny_unknown_fields` on a unit variant of a tagged enum).
    TeamFolder {},
    /// A SharePoint link naming a folder, or a Confluence link naming a page, of the workspace.
    Link { source_link_id: SourceLinkId },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamWorkspaceSharePreview {
    /// "acme/supplier-team › knowledge".
    pub folder_name: String,
    /// Shared, in the tree's order.
    pub places: Vec<TeamPlaceLine>,
    /// Display names of places and agents that stay on this computer.
    pub stays_here: Vec<String>,
    pub agents: Vec<TeamAgentLine>,
    /// What the first share would add.
    pub knowledge: TeamCounts,
    /// TeamFolder first, then each suitable link.
    pub results_choices: Vec<TeamResultsOption>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamWorkspaceOpenPreview {
    pub name: String,
    pub name_in_use: bool,
    pub folder_name: String,
    /// With reach.
    pub places: Vec<TeamPlaceLine>,
    /// With found.
    pub agents: Vec<TeamAgentLine>,
    pub knowledge: TeamCounts,
    /// "acme/supplier-team › knowledge › results" or the link's display name.
    pub results: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamPlaceLine {
    pub key: String,
    pub display_name: String,
    pub kind: SourceLinkKind,
    /// Open preview only; None in a share preview.
    pub reach: Option<TeamReach>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamReach {
    WillLink,
    NoAccess,
    SignInNeeded,
    NotSetUp,
    CannotTell,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamAgentLine {
    pub key: String,
    pub name: String,
    pub access: Vec<TeamAccessLine>,
    /// Open preview only.
    pub found: Option<TeamAgentFound>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamAccessLine {
    pub place_name: String,
    pub access: FolderAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamAgentFound {
    InYourLibrary,
    AlreadyHere,
    NotInYourRepositories,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamResultsOption {
    pub choice: TeamResultsChoice,
    pub name: String,
}

/// Where results may be saved in a team workspace, and where they are saved now (TW-K step 1;
/// amended January 2026 by the user's decision: **Change…** after sharing reads its own choices).
// spec: ordin-desktop/components/TeamKnowledgeService#TW-K
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamResultsChoices {
    /// `TeamFolder` first, then each suitable link (TW-A step 4).
    pub choices: Vec<TeamResultsOption>,
    /// The results place now; None when it is a link that is not linked on this computer.
    pub current: Option<TeamResultsChoice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamWorkspaceOpened {
    pub workspace_id: WorkspaceId,
    pub status: TeamKnowledgeStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamWorkspaceStatus {
    /// proposed (the first share's card waits) | team.
    pub state: TeamWorkspaceState,
    pub places: Vec<TeamPlaceStatus>,
    /// Agents of the description not added here (declined or not found).
    pub agents_not_here: Vec<TeamAgentLine>,
    /// The results place's name.
    pub results: String,
    /// Results stay on this computer (read-only access, or no access to the place).
    pub results_here_only: bool,
    pub pending_grants: u32,
    pub unsent_results: u32,
    /// Entries of this computer not yet shared.
    pub activity_waiting: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamWorkspaceState {
    Proposed,
    Team,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamPlaceStatus {
    pub key: String,
    pub display_name: String,
    pub state: TeamPlaceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamPlaceState {
    Linked,
    NoAccess,
    Waiting,
    RemovalWaits,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamPendingGrantView {
    pub grant_id: TeamGrantId,
    /// The login.
    pub added_by: String,
    pub agent_name: String,
    /// True: the agent is not in this workspace yet.
    pub new_agent: bool,
    /// Only the access asked for; places this person cannot open are left out.
    pub access: Vec<TeamAccessLine>,
    /// Place names asked for that this person cannot open.
    pub left_out: Vec<String>,
    pub added_at: Timestamp,
    /// False: the repository's history does not record `added_by` as the author (TW-J).
    pub added_by_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamActivityKind {
    Run,
    Replaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunOutcome {
    Completed,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamActivityView {
    pub id: TeamActivityId,
    pub kind: TeamActivityKind,
    /// None: this computer's entry, not yet shared.
    pub by: Option<String>,
    /// TW-J; true for this computer's own entries.
    pub by_confirmed: bool,
    pub mine: bool,
    pub at: Timestamp,
    pub task: Option<String>,
    pub agent: Option<String>,
    pub outcome: Option<TeamRunOutcome>,
    pub results: Vec<TeamResultRef>,
    pub change: Option<String>,
    pub subject: Option<String>,
    pub replaced_by: Option<String>,
    /// TW-J; false when `replaced_by` is None.
    pub replaced_by_confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamResultRef {
    /// The file name.
    pub name: String,
    /// Where it opens; None when the file is not on this computer yet. `ContentRef::PlaceFile` only.
    pub file: Option<ContentRef>,
}
