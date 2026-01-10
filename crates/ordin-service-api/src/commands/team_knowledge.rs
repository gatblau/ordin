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

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

pub use ordin_types::domain::{PullRequestRef, SourceLink, TeamAccess, TeamKnowledgeBinding};

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
