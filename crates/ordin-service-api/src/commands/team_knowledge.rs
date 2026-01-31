//! Team knowledge — restated from `TeamKnowledgeService.md` (SA-RECON-02).
//!
//! The seven commands of ADR-103 and the seven of ADR-110 (with `check_now` and `get` taking an
//! optional `folder_id`), and the status shape with every amendment (ADR-110 read folders, ADR-112
//! `bundle_kind`). The adopt shapes are 2C wire §10's. `adopt`, `adopt_again` and `keep_as_is` are
//! interface commands only (`PERSON_ONLY_COMMANDS`).
//!
//! The owner's rules on these inputs (a suitable link, a folder this workspace reads, an edit's
//! lengths, a description's bounds) need its state or carry its own codes (`TEAM_LINK_NOT_SUITABLE`,
//! `TEAM_ADOPT_INVALID`, `TEAM_DESCRIPTION_NOT_TAKEN`), so this crate refuses only by shape.
//!
//! Amended January 2026 (ADR-121, ServiceApi.md SVC-TW-1): the ten team workspace commands, their
//! inputs (with the length checks SVC-TW-1 names) and the owner's result types, restated from
//! `TeamKnowledgeService.md` (*Public Interface (additions)*) with 2C wire §18's shapes.
//! `open_workspace`, `allow_grant` and `decline_grant` are person-only (SVC-TW-2).

use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde::{Deserialize, Serialize};

pub use ordin_types::domain::{
    FolderAccess, PullRequestRef, SourceLink, SourceLinkKind, TeamAccess, TeamKnowledgeBinding,
};

use super::rules;
use crate::api::{CommandInput, InputRejection};

// ---- status ----

/// The long command now running for a workspace (§H).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamBusy {
    Checking,
    Getting,
    Sharing,
    /// Amended January 2026 (ADR-110).
    CheckingRecall,
    /// **Open a team workspace…** is running (amended January 2026, ADR-121).
    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-3
    Opening,
}

/// **Share knowledge with a team…** / **Read another team's knowledge…**: an existing link or a new
/// `GitRemote` link added in the same step (SA-TEAM-01).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TeamFolderChoice {
    Link {
        source_link_id: SourceLinkId,
    },
    NewFolder {
        url: String,
        branch: String,
        path: String,
    },
}

// SPEC-GAP-14: `share_unavailable`, `last_error`, `TeamProblem.code` and `TeamFolderStatus.blocked` are
// `&'static str` in the owner (a registry code); restated as `String`, the same JSON, so the type can be
// read back.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamKnowledgeStatus {
    pub workspace_id: WorkspaceId,
    /// The own binding; None when the workspace has no own folder.
    pub binding: Option<TeamKnowledgeBinding>,
    /// "acme/supplier-team › knowledge"; None when off.
    pub header_line: Option<String>,
    pub available: Option<TeamAvailable>,
    pub to_share: Option<TeamCounts>,
    /// Facts waiting for the person's check (D10); 0 when none.
    pub held_for_check: u32,
    pub waiting_for_review: Option<PullRequestRef>,
    pub problems: Vec<TeamProblem>,
    /// None = Share allowed; Some(sentence code) otherwise, e.g. `TEAM_READ_ONLY`, `TEAM_NO_OWN_FOLDER`.
    pub share_unavailable: Option<String>,
    pub last_checked_at: Option<Timestamp>,
    pub folder_large: bool,
    /// The long command now running for this workspace (§H); None when idle.
    pub busy: Option<TeamBusy>,
    /// Outcome of the latest get and share started by the person.
    pub last_get: Option<GetReport>,
    pub last_share: Option<ShareProposed>,
    /// Code of the latest long command that failed; cleared when one succeeds.
    pub last_error: Option<String>,
    /// Amended January 2026 (ADR-110) from here on.
    pub own_folder_id: Option<TeamFolderId>,
    /// Read folders, company ones first, then by name.
    pub folders: Vec<TeamFolderStatus>,
    pub workspace_facts: u32,
    /// `workspace_facts` > 100,000 (A-58).
    pub workspace_large: bool,
    pub last_recall_check: Option<RecallCheckReport>,
    /// The own folder's local descriptions.
    pub descriptions: Vec<FolderDescription>,
    /// None for a workspace that is not a team workspace (amended January 2026, ADR-121). An answer
    /// written before ADR-121 has no such field and reads as None.
    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-3
    pub team_workspace: Option<TeamWorkspaceStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamAvailable {
    pub counts: TeamCounts,
    /// Sharer names, at most 5, then "and N others".
    pub from: Vec<String>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub struct TeamCounts {
    pub new_facts: u32,
    pub changed_facts: u32,
    pub removed_facts: u32,
    pub new_pages: u32,
    pub changed_pages: u32,
    pub removed_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamProblem {
    pub path: String,
    pub code: String,
    pub first_seen_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct GetReport {
    pub counts: TeamCounts,
    pub not_taken: u32,
    pub skipped_files: u32,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ShareProposed {
    pub share_id: TeamShareId,
    pub approval_id: ApprovalId,
    pub counts: TeamCounts,
    pub held_for_check: u32,
}

// SPEC-GAP-15: the owner's `TeamFolderStatus` (and its example) serialises `description`, `review` and
// `blocked` always, `null` when absent; 2C wire §10 marks the three `skip_serializing_if`. The owner's
// form is restated.
/// One read folder (ADR-110).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamFolderStatus {
    pub folder_id: TeamFolderId,
    pub name: String,
    pub description: Option<String>,
    pub set_by_company: bool,
    pub may_stop: bool,
    pub access: TeamAccess,
    pub available: Option<TeamAvailable>,
    pub held: TeamHeld,
    pub problems: Vec<TeamProblem>,
    pub last_checked_at: Option<Timestamp>,
    pub folder_large: bool,
    pub review: Option<ReviewState>,
    /// `TEAM_COMPANY_FOLDER_NO_ACCESS` | `TEAM_REVIEW_NOT_REQUIRED` | `TEAM_REVIEW_CANNOT_TELL`; None when
    /// the folder is read.
    pub blocked: Option<String>,
    pub last_get: Option<GetReport>,
    /// A team folder or an Open Knowledge Format bundle (amended January 2026, ADR-112).
    pub bundle_kind: BundleKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BundleKind {
    Team,
    Okf,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub struct TeamHeld {
    pub facts: u32,
    pub pages: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Required,
    NotRequired,
    CannotTell,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FolderDescription {
    /// Relative to the own folder; "" = the folder.
    pub path: String,
    /// 1–500 characters.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RecallCheckReport {
    pub ran_at: Timestamp,
    pub folders: Vec<RecallCheckFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RecallCheckFolder {
    /// None = the own folder.
    pub folder_id: Option<TeamFolderId>,
    pub name: String,
    pub checks: u32,
    pub precision_at_5_permille: u16,
    pub recall_at_5_permille: u16,
    pub mrr_permille: u16,
}

// ---- adoption (2C wire §10) ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdoptItem {
    Fact { id: MemoryFactId },
    Page { page_id: KnowledgePageId },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AdoptEdit {
    pub statement: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AdoptResult {
    /// "fact" | "page".
    pub kind: String,
    pub id: String,
    pub edited: bool,
    pub version: u32,
}

// ---- a team workspace (ADR-121; 2C wire §18) ----

// SPEC-GAP-29: 2C wire §18 writes the optional fields of these results as absent when None (`reach?`,
// `found?`, `team_workspace?`, `by?` and the others), while the owner's types (TeamKnowledgeService.md
// *Public Interface (additions)*) carry no `skip_serializing_if` and so write `null`. The owner's form
// is restated, as SPEC-GAP-15 does for `TeamFolderStatus`.

/// Where a team workspace's results are kept (TeamKnowledgeService.md *Shared Context* 1).
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-1
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TeamResultsChoice {
    /// `results/` beneath the own team folder.
    // SPEC-GAP-30: the owner writes `TeamFolder` as a unit variant; serde does not apply
    // `deny_unknown_fields` to a unit variant of an internally tagged enum, so
    // `{"kind":"team_folder","place":…}` would be taken. Restated as an empty struct variant: the
    // same JSON (`{"kind":"team_folder"}`), and an unknown field is refused as WIRE-TW-1 states.
    TeamFolder {},
    /// A SharePoint link naming a folder, or a Confluence link naming a page, of the workspace.
    Link { source_link_id: SourceLinkId },
}

/// What **Share as a team workspace…** will share and what stays here.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-2
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// `TeamFolder` first, then each suitable link.
    pub results_choices: Vec<TeamResultsOption>,
}

/// What **Open a team workspace…** will add; the preview stores nothing.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamWorkspaceOpenPreview {
    pub name: String,
    pub name_in_use: bool,
    pub folder_name: String,
    /// With `reach`.
    pub places: Vec<TeamPlaceLine>,
    /// With `found`.
    pub agents: Vec<TeamAgentLine>,
    pub knowledge: TeamCounts,
    /// "acme/supplier-team › knowledge › results" or the link's display name.
    pub results: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamPlaceLine {
    pub key: String,
    pub display_name: String,
    pub kind: SourceLinkKind,
    /// Open preview only; None in a share preview.
    pub reach: Option<TeamReach>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamReach {
    WillLink,
    NoAccess,
    SignInNeeded,
    NotSetUp,
    CannotTell,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamAgentLine {
    pub key: String,
    pub name: String,
    pub access: Vec<TeamAccessLine>,
    /// Open preview only: `in_your_library`, `already_here` or `not_in_your_repositories`.
    pub found: Option<TeamAgentFound>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamAccessLine {
    pub place_name: String,
    pub access: FolderAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamAgentFound {
    InYourLibrary,
    AlreadyHere,
    NotInYourRepositories,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamResultsOption {
    pub choice: TeamResultsChoice,
    pub name: String,
}

/// Where results may be saved in a team workspace, and where they are saved now: the answer of
/// `team_knowledge.results_choices`, which **Change…** reads after sharing (amended January 2026 by
/// the user's decision).
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-2
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamResultsChoices {
    /// `TeamFolder` first, then each suitable link.
    pub choices: Vec<TeamResultsOption>,
    /// The results place now; absent when it is a link not linked on this computer.
    pub current: Option<TeamResultsChoice>,
}

/// The answer of `team_knowledge.open_workspace`: the new workspace, with `busy: opening`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamWorkspaceOpened {
    pub workspace_id: WorkspaceId,
    pub status: TeamKnowledgeStatus,
}

/// `TeamKnowledgeStatus.team_workspace`.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-3
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamWorkspaceStatus {
    /// `proposed` (the first share's card waits) or `team`.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamWorkspaceState {
    Proposed,
    Team,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamPlaceStatus {
    pub key: String,
    pub display_name: String,
    pub state: TeamPlaceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamPlaceState {
    Linked,
    NoAccess,
    Waiting,
    RemovalWaits,
}

/// A colleague's added agent or access that waits for **Allow**.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// False: the repository's history does not record `added_by` as the author; the screens say
    /// "says it was".
    pub added_by_confirmed: bool,
}

/// One entry of **Team activity**.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamActivityView {
    pub id: TeamActivityId,
    pub kind: TeamActivityKind,
    /// None: this computer's entry, not yet shared.
    pub by: Option<String>,
    /// True for this computer's own entries.
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
    /// False when `replaced_by` is None.
    pub replaced_by_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamActivityKind {
    Run,
    Replaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TeamRunOutcome {
    Completed,
    Failed,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TeamResultRef {
    /// The file name.
    pub name: String,
    /// Where it opens (`ContentRef::PlaceFile` only); None when the file is not on this computer yet.
    pub file: Option<ContentRef>,
}

// ---- inputs ----

macro_rules! ws_only {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        pub struct $name {
            pub ws: WorkspaceId,
        }
    )*};
}

ws_only!(
    TeamKnowledgeStatusInput,
    TeamKnowledgeCandidateLinksInput,
    TeamKnowledgeTurnOffInput,
    TeamKnowledgeShareInput,
    TeamKnowledgeCheckRecallInput,
);

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeTurnOnInput {
    pub ws: WorkspaceId,
    pub choice: TeamFolderChoice,
}

/// An absent `folder_id` means the own folder, so an input written before ADR-110 is still valid.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeCheckNowInput {
    pub ws: WorkspaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<TeamFolderId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeGetInput {
    pub ws: WorkspaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<TeamFolderId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeAddReadFolderInput {
    pub ws: WorkspaceId,
    pub choice: TeamFolderChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeStopReadingInput {
    pub ws: WorkspaceId,
    pub folder_id: TeamFolderId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeAdoptInput {
    pub ws: WorkspaceId,
    pub item: AdoptItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<AdoptEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeAdoptAgainInput {
    pub ws: WorkspaceId,
    pub item: AdoptItem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<AdoptEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeKeepAsIsInput {
    pub ws: WorkspaceId,
    pub item: AdoptItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeSetDescriptionInput {
    pub ws: WorkspaceId,
    pub path: String,
    /// None removes the description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Every input of the group: the typed read is this crate's whole check (module comment).
macro_rules! shape_only {
    ($($ty:ty),* $(,)?) => {$(
        impl CommandInput for $ty {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

shape_only!(
    TeamKnowledgeStatusInput,
    TeamKnowledgeCandidateLinksInput,
    TeamKnowledgeTurnOnInput,
    TeamKnowledgeTurnOffInput,
    TeamKnowledgeCheckNowInput,
    TeamKnowledgeGetInput,
    TeamKnowledgeShareInput,
    TeamKnowledgeAddReadFolderInput,
    TeamKnowledgeStopReadingInput,
    TeamKnowledgeAdoptInput,
    TeamKnowledgeAdoptAgainInput,
    TeamKnowledgeKeepAsIsInput,
    TeamKnowledgeSetDescriptionInput,
    TeamKnowledgeCheckRecallInput,
);

// ---- inputs of a team workspace (ADR-121, SVC-TW-1) ----

/// `url`: 1–2,048 bytes.
const URL_MAX_BYTES: usize = 2048;
/// `branch`: 1–255 bytes.
const BRANCH_MAX_BYTES: usize = 255;
/// `path`: at most 1,024 bytes, no `..`.
const PATH_MAX_BYTES: usize = 1024;
/// `name`: 1–80 characters after trimming.
const NAME_MAX_CHARS: usize = 80;

/// The repository folder of a team workspace, checked as SVC-TW-1 states; `at` is the pointer of the
/// object that holds the three fields. Everything that depends on the folder is the owner's.
// spec: ordin-desktop/components/ServiceApi#SVC-TW-1
fn check_folder(url: &str, branch: &str, path: &str, at: &str) -> Result<(), InputRejection> {
    rules::bytes(url, 1, URL_MAX_BYTES, &format!("{at}/url"))?;
    rules::bytes(branch, 1, BRANCH_MAX_BYTES, &format!("{at}/branch"))?;
    rules::bytes(path, 0, PATH_MAX_BYTES, &format!("{at}/path"))?;
    if path.split(['/', '\\']).any(|segment| segment == "..") {
        return Err(rules::reject(&format!("{at}/path")));
    }
    Ok(())
}

/// A new folder named by a share's `TeamFolderChoice` is checked as the open inputs are.
fn check_choice(choice: &TeamFolderChoice) -> Result<(), InputRejection> {
    match choice {
        TeamFolderChoice::Link { .. } => Ok(()),
        TeamFolderChoice::NewFolder { url, branch, path } => {
            check_folder(url, branch, path, "/choice")
        }
    }
}

ws_only!(
    TeamKnowledgePendingGrantsInput,
    TeamKnowledgeLeaveWorkspaceInput,
    TeamKnowledgeResultsChoicesInput,
);

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeWorkspaceSharePreviewInput {
    pub ws: WorkspaceId,
    pub choice: TeamFolderChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeShareWorkspaceInput {
    pub ws: WorkspaceId,
    pub choice: TeamFolderChoice,
    pub results: TeamResultsChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeWorkspaceOpenPreviewInput {
    pub url: String,
    pub branch: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeOpenWorkspaceInput {
    pub url: String,
    pub branch: String,
    pub path: String,
    /// The new workspace's name.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeAllowGrantInput {
    pub ws: WorkspaceId,
    pub grant_id: TeamGrantId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeDeclineGrantInput {
    pub ws: WorkspaceId,
    pub grant_id: TeamGrantId,
}

/// 50 entries a page, newest first.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeActivityInput {
    pub ws: WorkspaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeSetResultsPlaceInput {
    pub ws: WorkspaceId,
    pub results: TeamResultsChoice,
}

shape_only!(
    TeamKnowledgePendingGrantsInput,
    TeamKnowledgeLeaveWorkspaceInput,
    TeamKnowledgeResultsChoicesInput,
    TeamKnowledgeAllowGrantInput,
    TeamKnowledgeDeclineGrantInput,
    TeamKnowledgeSetResultsPlaceInput,
);

impl CommandInput for TeamKnowledgeWorkspaceSharePreviewInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_choice(&self.choice)
    }
}

impl CommandInput for TeamKnowledgeShareWorkspaceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_choice(&self.choice)
    }
}

impl CommandInput for TeamKnowledgeWorkspaceOpenPreviewInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_folder(&self.url, &self.branch, &self.path, "")
    }
}

impl CommandInput for TeamKnowledgeOpenWorkspaceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_folder(&self.url, &self.branch, &self.path, "")?;
        rules::text(self.name.trim(), 1, NAME_MAX_CHARS, "/name")
    }
}

impl CommandInput for TeamKnowledgeActivityInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WS: &str = "0192a0f1-1111-7222-8333-444455556666";
    const ID: &str = "0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1b";

    fn passes<T: CommandInput>(value: serde_json::Value) -> bool {
        let mut input: T = serde_json::from_value(value).expect("shape");
        input.check().is_ok()
    }

    #[test]
    fn inputs_read_and_pass() {
        assert!(passes::<TeamKnowledgeStatusInput>(json!({ "ws": WS })));
        assert!(passes::<TeamKnowledgeCandidateLinksInput>(
            json!({ "ws": WS })
        ));
        assert!(passes::<TeamKnowledgeTurnOffInput>(json!({ "ws": WS })));
        assert!(passes::<TeamKnowledgeShareInput>(json!({ "ws": WS })));
        assert!(passes::<TeamKnowledgeCheckRecallInput>(json!({ "ws": WS })));
        // An input written before ADR-110 is still valid.
        assert!(passes::<TeamKnowledgeGetInput>(json!({ "ws": WS })));
        assert!(passes::<TeamKnowledgeCheckNowInput>(
            json!({ "ws": WS, "folder_id": ID })
        ));
        assert!(passes::<TeamKnowledgeTurnOnInput>(
            json!({ "ws": WS, "choice": { "kind": "link", "source_link_id": ID } })
        ));
        assert!(passes::<TeamKnowledgeAddReadFolderInput>(json!({ "ws": WS,
            "choice": { "kind": "new_folder", "url": "https://github.com/acme/t", "branch": "main", "path": "knowledge" } })));
        assert!(passes::<TeamKnowledgeStopReadingInput>(
            json!({ "ws": WS, "folder_id": ID })
        ));
        assert!(passes::<TeamKnowledgeAdoptInput>(
            json!({ "ws": WS, "item": { "kind": "fact", "id": ID }, "edit": { "statement": "Edited." } })
        ));
        assert!(passes::<TeamKnowledgeAdoptAgainInput>(
            json!({ "ws": WS, "item": { "kind": "page", "page_id": ID } })
        ));
        assert!(passes::<TeamKnowledgeKeepAsIsInput>(
            json!({ "ws": WS, "item": { "kind": "fact", "id": ID } })
        ));
        assert!(passes::<TeamKnowledgeSetDescriptionInput>(
            json!({ "ws": WS, "path": "" })
        ));
    }

    #[test]
    fn unknown_kinds_and_fields_are_refused_by_shape() {
        assert!(serde_json::from_value::<TeamKnowledgeTurnOnInput>(
            json!({ "ws": WS, "choice": { "kind": "local_folder", "path": "/tmp/x" } })
        )
        .is_err());
        assert!(serde_json::from_value::<TeamKnowledgeAdoptInput>(
            json!({ "ws": WS, "item": { "kind": "relationship", "id": ID } })
        )
        .is_err());
        assert!(serde_json::from_value::<TeamKnowledgeAdoptInput>(
            json!({ "ws": WS, "item": { "kind": "fact", "id": ID }, "edit": { "colour": "red" } })
        )
        .is_err());
        assert!(
            serde_json::from_value::<TeamKnowledgeStatusInput>(json!({ "ws": WS, "x": 1 }))
                .is_err()
        );
    }

    fn refusal<T: CommandInput>(value: serde_json::Value) -> Option<String> {
        let mut input: T = serde_json::from_value(value).expect("shape");
        input.check().err().map(|r| r.at)
    }

    fn open(url: &str, branch: &str, path: &str, name: &str) -> serde_json::Value {
        json!({ "url": url, "branch": branch, "path": path, "name": name })
    }

    const URL: &str = "https://github.com/acme/supplier-team";

    #[test]
    fn team_workspace_inputs_read_and_pass() {
        let new_folder =
            json!({ "kind": "new_folder", "url": URL, "branch": "main", "path": "knowledge" });
        assert!(passes::<TeamKnowledgeWorkspaceSharePreviewInput>(
            json!({ "ws": WS, "choice": new_folder })
        ));
        assert!(passes::<TeamKnowledgeShareWorkspaceInput>(
            json!({ "ws": WS,
            "choice": { "kind": "link", "source_link_id": ID }, "results": { "kind": "team_folder" } })
        ));
        assert!(passes::<TeamKnowledgeWorkspaceOpenPreviewInput>(
            json!({ "url": URL, "branch": "main", "path": "" })
        ));
        assert!(passes::<TeamKnowledgeOpenWorkspaceInput>(open(
            URL,
            "main",
            "knowledge",
            "Q3"
        )));
        assert!(passes::<TeamKnowledgePendingGrantsInput>(
            json!({ "ws": WS })
        ));
        assert!(passes::<TeamKnowledgeAllowGrantInput>(
            json!({ "ws": WS, "grant_id": ID })
        ));
        assert!(passes::<TeamKnowledgeDeclineGrantInput>(
            json!({ "ws": WS, "grant_id": ID })
        ));
        assert!(passes::<TeamKnowledgeActivityInput>(json!({ "ws": WS })));
        assert!(passes::<TeamKnowledgeSetResultsPlaceInput>(
            json!({ "ws": WS, "results": { "kind": "link", "source_link_id": ID } })
        ));
        assert!(passes::<TeamKnowledgeLeaveWorkspaceInput>(
            json!({ "ws": WS })
        ));
        assert!(passes::<TeamKnowledgeResultsChoicesInput>(
            json!({ "ws": WS })
        ));
        assert!(serde_json::from_value::<TeamKnowledgeResultsChoicesInput>(
            json!({ "ws": WS, "x": 1 })
        )
        .is_err());
    }

    // spec: ordin-desktop/components/ServiceApi#SVC-TW-1
    #[test]
    fn team_workspace_folder_bounds() {
        let long_url = format!("{URL}/{}", "a".repeat(2048 - URL.len() - 1));
        assert_eq!(long_url.len(), 2048);
        assert!(passes::<TeamKnowledgeOpenWorkspaceInput>(open(
            &long_url, "main", "", "Q3"
        )));
        let too_long_url = format!("{long_url}a");
        let cases = [
            (open("", "main", "k", "Q3"), "/input/url"),
            (open(&too_long_url, "main", "k", "Q3"), "/input/url"),
            (open(URL, "", "k", "Q3"), "/input/branch"),
            (open(URL, &"b".repeat(256), "k", "Q3"), "/input/branch"),
            (open(URL, "main", &"p".repeat(1025), "Q3"), "/input/path"),
            (open(URL, "main", "knowledge/../x", "Q3"), "/input/path"),
            (open(URL, "main", "..", "Q3"), "/input/path"),
            (open(URL, "main", "k", "   "), "/input/name"),
            (open(URL, "main", "k", &"n".repeat(81)), "/input/name"),
        ];
        for (input, at) in cases {
            assert_eq!(
                refusal::<TeamKnowledgeOpenWorkspaceInput>(input.clone()).as_deref(),
                Some(at),
                "{input}"
            );
        }
        // At the bounds, and a name padded with spaces counts its trimmed characters.
        assert!(passes::<TeamKnowledgeOpenWorkspaceInput>(open(
            URL,
            &"b".repeat(255),
            &"p".repeat(1024),
            &format!("  {}  ", "n".repeat(80))
        )));
        // A segment that merely contains two dots is a name, not `..`.
        assert!(passes::<TeamKnowledgeOpenWorkspaceInput>(open(
            URL,
            "main",
            "notes..old",
            "Q3"
        )));
        assert_eq!(
            refusal::<TeamKnowledgeWorkspaceOpenPreviewInput>(
                json!({ "url": URL, "branch": "", "path": "" })
            )
            .as_deref(),
            Some("/input/branch")
        );
        let bad_choice = json!({ "ws": WS,
            "choice": { "kind": "new_folder", "url": URL, "branch": "main", "path": "../x" },
            "results": { "kind": "team_folder" } });
        assert_eq!(
            refusal::<TeamKnowledgeShareWorkspaceInput>(bad_choice).as_deref(),
            Some("/input/choice/path")
        );
        assert_eq!(
            refusal::<TeamKnowledgeWorkspaceSharePreviewInput>(json!({ "ws": WS,
                "choice": { "kind": "new_folder", "url": "", "branch": "main", "path": "" } }))
            .as_deref(),
            Some("/input/choice/url")
        );
    }

    #[test]
    fn team_workspace_inputs_refuse_unknown_fields_and_kinds() {
        assert!(
            serde_json::from_value::<TeamKnowledgeOpenWorkspaceInput>(json!({
                "url": URL, "branch": "main", "path": "knowledge", "name": "Q3", "token": "x"
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<TeamKnowledgeSetResultsPlaceInput>(
            json!({ "ws": WS, "results": { "kind": "team_folder", "place": ID } })
        )
        .is_err());
        assert!(serde_json::from_value::<TeamKnowledgeSetResultsPlaceInput>(
            json!({ "ws": WS, "results": { "kind": "local_folder" } })
        )
        .is_err());
        assert!(serde_json::from_value::<TeamKnowledgeAllowGrantInput>(
            json!({ "ws": WS, "grant_id": ID, "always": true })
        )
        .is_err());
    }

    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-2
    #[test]
    fn pending_grant_reads_the_wire_example() {
        let grant: TeamPendingGrantView = serde_json::from_value(json!({
            "grant_id": "0192c3b0-0000-7000-8000-0000000000c1", "added_by": "priya-shah",
            "agent_name": "Contract checker", "new_agent": true,
            "access": [{ "place_name": "Contracts", "access": "read_and_change" }], "left_out": [],
            "added_at": "2026-01-20T11:04:10.250Z", "added_by_confirmed": true
        }))
        .expect("pending grant");
        assert_eq!(grant.access[0].access, FolderAccess::ReadAndChange);
        let out = serde_json::to_value(&grant).expect("serialise");
        assert_eq!(out["grant_id"], "0192c3b0-0000-7000-8000-0000000000c1");
    }

    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-TW-3
    #[test]
    fn status_carries_the_team_workspace_and_reads_an_older_answer() {
        assert_eq!(
            serde_json::to_value(TeamBusy::Opening).unwrap(),
            json!("opening")
        );
        let team: TeamWorkspaceStatus = serde_json::from_value(json!({
            "state": "team",
            "places": [{ "key": "k1", "display_name": "Contracts", "state": "removal_waits" }],
            "agents_not_here": [{ "key": "a1", "name": "Supplier analyst",
                "access": [{ "place_name": "Contracts", "access": "read" }], "found": "not_in_your_repositories" }],
            "results": "acme/supplier-team › knowledge › results", "results_here_only": false,
            "pending_grants": 1, "unsent_results": 0, "activity_waiting": 2
        }))
        .expect("team workspace status");
        assert_eq!(team.state, TeamWorkspaceState::Team);
        assert_eq!(team.places[0].state, TeamPlaceState::RemovalWaits);
        assert_eq!(
            team.agents_not_here[0].found,
            Some(TeamAgentFound::NotInYourRepositories)
        );
        let activity: TeamActivityView = serde_json::from_value(json!({
            "id": ID, "kind": "run", "by": "asha-patel", "by_confirmed": true, "mine": false,
            "at": "2026-01-20T10:12:40.001Z", "task": "Compare documents", "agent": "Supplier analyst",
            "outcome": "completed",
            "results": [{ "name": "Kestrel quote — comparison.md",
                "file": { "kind": "place_file", "place_id": ID, "relative_path": "Kestrel quote — comparison.md" } }],
            "change": null, "subject": null, "replaced_by": null, "replaced_by_confirmed": false
        }))
        .expect("activity");
        assert_eq!(activity.kind, TeamActivityKind::Run);
        assert_eq!(activity.outcome, Some(TeamRunOutcome::Completed));
        let choice = TeamResultsOption {
            choice: TeamResultsChoice::TeamFolder {},
            name: "Team folder".into(),
        };
        assert_eq!(
            serde_json::to_value(&choice).unwrap()["choice"],
            json!({ "kind": "team_folder" })
        );
        let reach: TeamPlaceLine = serde_json::from_value(json!({
            "key": "k1", "display_name": "Contracts", "kind": "sharepoint_location", "reach": "sign_in_needed"
        }))
        .expect("place line");
        assert_eq!(reach.reach, Some(TeamReach::SignInNeeded));
        // An answer written before ADR-121 has no `team_workspace` and reads as not a team workspace.
        let older: TeamKnowledgeStatus = serde_json::from_value(json!({
            "workspace_id": WS, "held_for_check": 0, "problems": [], "folder_large": false,
            "folders": [], "workspace_facts": 0, "workspace_large": false, "descriptions": [],
            "busy": "opening"
        }))
        .expect("older status");
        assert!(older.team_workspace.is_none());
        assert_eq!(older.busy, Some(TeamBusy::Opening));
    }

    #[test]
    fn folder_status_reads_the_owners_example() {
        let folder: TeamFolderStatus = serde_json::from_value(json!({
            "folder_id": "0192b100-0000-7000-8000-0000000000c1", "name": "Security standards",
            "description": "Secure patterns every team follows", "set_by_company": true, "may_stop": false,
            "access": "read_only", "available": null, "held": { "facts": 412, "pages": 18 }, "problems": [],
            "last_checked_at": "2026-01-10T10:15:00.000Z", "folder_large": false, "review": "required",
            "blocked": null, "last_get": null, "bundle_kind": "team"
        }))
        .expect("folder status");
        assert_eq!(folder.review, Some(ReviewState::Required));
        let out = serde_json::to_value(&folder).expect("serialise");
        assert_eq!(out["blocked"], serde_json::Value::Null);
        assert_eq!(out["held"]["facts"], 412);
    }
}
