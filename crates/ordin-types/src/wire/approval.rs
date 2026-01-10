//! 2C part 2 §5 Approval card (and ApprovalPress).

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- ApprovalCard ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ApprovalCard {
    pub approval_id: ApprovalId,
    pub backend: BackendId,
    pub workspace_id: WorkspaceId,
    /// The run or crew round that asked (entity `Approval`, field *run or crew round*).
    pub asked_by: ApprovalAsker,
    /// "[Verb] [object]?": the question in plain words, e.g. "Publish the revised account brief?"
    pub question: String,
    /// The approve button's label; repeats the verb ("Publish", "Approve and post").
    pub verb: String,
    /// Where: the destination.
    pub where_line: String,
    /// Affects: records or files, by name and version.
    pub affects: Vec<AffectedItem>,
    /// Changes: what will be different afterwards.
    pub changes: String,
    /// What the first button shows; absent when nothing more can be shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ApprovalContent>,
    /// `reject` → "Reject"; `not_now` → "Not now" where the person may decide later.
    pub reject_style: RejectStyle,
    /// Single-use references the buttons carry (entity `CardReference`).
    pub buttons: ApprovalButtons,
    pub created_at: Timestamp,
}

// SPEC-GAP-3: 2C derives `PartialEq` on the domain entities that hold this type (`Approval`)
// but not on this wire type, so those derives cannot compile as written; `PartialEq` is added here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalAsker {
    Run {
        run_id: RunId,
    },
    CrewRound {
        crew_run_id: CrewRunId,
        round: u32,
    },
    /// The person's own request (Share…, Publish a copy…, Send…).
    Person,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AffectedItem {
    File {
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        place_id: Option<PlaceId>,
    },
    Record {
        label: String,
    },
    Recipient {
        name: String,
        address: String,
        outside_company: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RejectStyle {
    Reject,
    NotNow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ApprovalButtons {
    pub approve: CardRef,
    pub reject: CardRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_details: Option<CardRef>,
}

/// Opaque single-use reference; the value of entity `CardReference`'s *reference* field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct CardRef(String);

// ---- ApprovalContent ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalContent {
    /// A message shown open on the card; longer than 12 lines folds to its first 6.
    Message {
        label: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        destination_note: Option<String>,
    },
    /// A command shown in full in a code block, with where it runs.
    Command {
        label: String,
        command: String,
        runs_in: String,
    },
    Changes {
        label: String,
        change_set: ContentRef,
    },
    Page {
        label: String,
        page: ContentRef,
    },
    Files {
        label: String,
        files: Vec<ContentRef>,
    },
    /// ADR-070: built from the `EmailDraft`, never from an agent's words.
    Email {
        label: String,
        email: EmailApproval,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentRef {
    Artefact {
        artefact_id: ArtefactId,
    },
    PlaceFile {
        place_id: PlaceId,
        relative_path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EmailApproval {
    pub draft_id: EmailDraftId,
    pub from: String,
    pub to: Vec<EmailRecipient>,
    pub cc: Vec<EmailRecipient>,
    pub bcc: Vec<EmailRecipient>,
    pub subject: String,
    /// The whole message as it will be sent, with the person's signature.
    pub body: String,
    pub attachments: Vec<EmailAttachmentLine>,
    /// Agent instance display name, "Ordin" or absent when the person wrote it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drafted_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EmailRecipient {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub address: String,
    pub outside_company: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EmailAttachmentLine {
    pub file_name: String,
    pub comes_from: String,
    pub bytes: u64,
}

// ---- ApprovalPress (command input) ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApprovalPress {
    pub card_ref: CardRef,
    pub decision: PressDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PressDecision {
    Approve,
    Reject,
}
