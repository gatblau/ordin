//! 2C part 1 § Email.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- EmailLink ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EmailLink {
    pub id: EmailLinkId,
    pub workspace_id: WorkspaceId,
    pub kind: EmailLinkKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_or_label: Option<String>,
    pub addresses: Vec<String>,
    pub domains: Vec<String>,
    pub display_name: String,
    pub state: EmailLinkState,
    pub added_at: Timestamp,
    /// Check fields: owned by the Mail Service in its own table `mail_link_check_state`, not
    /// stored in `email_links`; filled on read from the Mail Service (amended January 2026, decision 80: C-26).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<Timestamp>,
    pub new_since_last_looked: u32,
    /// Company mail server only (ADR-071).
    pub imap_folders: Vec<ImapFolderState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EmailLinkKind {
    FolderOrLabel,
    PeopleAndDomains,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EmailLinkState {
    Proposed,
    Active,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ImapFolderState {
    pub folder: String,
    pub uid_validity: u32,
    pub highest_uid: u32,
}

// ---- EmailGrant ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EmailGrant {
    pub id: EmailGrantId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: EmailAccess,
    pub granted_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EmailAccess {
    ReadEmail,
    ReadAndDraftEmail,
}

// ---- TelegramGrant ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TelegramGrant {
    pub id: TelegramGrantId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: TelegramAccess,
    pub granted_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TelegramAccess {
    ReadTelegram,
    ReadAndDraftTelegram,
}

// ---- SlackGrant ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SlackGrant {
    pub id: SlackGrantId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: SlackAccess,
    pub granted_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlackAccess {
    ReadSlack,
    ReadAndDraftSlack,
}

// ---- TeamKnowledgeBinding ----
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TeamFolderRole {
    Own,
    Read,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TeamKnowledgeBinding {
    pub id: TeamFolderId,
    pub workspace_id: WorkspaceId,
    pub role: TeamFolderRole,
    /// Own: Some (an active GitRemote link). Read: Some only when chosen from a link; a read folder added by address adds no link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_link_id: Option<SourceLinkId>,
    pub host: String,       // "github.com", or the company GitLab host
    pub repository: String, // "owner/name"
    pub branch: String,     // the linked branch, 1–255 bytes
    pub folder: String, // relative, no "..", ≤ 1,024 bytes; "" = repository root; a read binding may name a sub-path
    pub access: TeamAccess,
    /// The person added it (person origin).
    pub added_by_person: bool,
    /// Company origin: the `team_knowledge.read_folders[].key` that names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_key: Option<String>,
    /// The company entry's display name; the folder's shown name when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The company entry's `review_required`; false otherwise.
    pub review_required: bool,
    /// False only when the binding has the company origin alone and the entry's `person_may_stop` is false.
    pub person_may_stop: bool,
    pub enabled_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TeamAccess {
    ReadWrite,
    ReadOnly,
}

// ---- TeamAdoption ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamAdoption {
    pub format: u8,          // 1
    pub kind: String,        // "fact" | "page"
    pub id: String,          // the adopted item's id; equals the file stem
    pub from_folder: String, // "owner/name › path"
    pub from_host: String,
    pub from_repository: String,
    pub from_branch: String,
    pub from_path: String,
    pub source_id: String,   // the source item's id in the read folder
    pub source_blob: String, // 40 lowercase hex: the source file's blob at adoption
    pub adopted_by: String,  // 1–200, PersonProfile.name
    pub adopted_at: Timestamp,
    pub edited: bool,
    pub version: u32, // ≥ 1; Adopt again adds 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AdoptionMark {
    ChangedAtSource,
    RemovedAtSource,
}

// ---- EmailDraft ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EmailDraft {
    pub id: EmailDraftId,
    pub workspace_id: WorkspaceId,
    pub kind: EmailDraftKind,
    /// Message-ID of the message it answers or forwards.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answers_message_id: Option<String>,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<EmailAttachment>,
    pub drafted_by: DraftedBy,
    pub edited_by_person: bool,
    pub state: EmailDraftState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<ApprovalId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_address: Option<String>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EmailDraftKind {
    New,
    Reply,
    Forward,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct EmailAttachment {
    pub place_id: PlaceId,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DraftedBy {
    AgentInstance {
        agent_instance_id: AgentInstanceId,
    },
    CrewMember {
        crew_run_id: CrewRunId,
        member: AgentRef,
    },
    Person,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EmailDraftState {
    Draft,
    WaitingForYou,
    Sending,
    Sent,
    NotSureItWasSent,
    NotSent,
    Discarded,
}

// ---- SentEmail ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SentEmail {
    pub id: SentEmailId,
    pub email_draft_id: EmailDraftId,
    pub approval_id: ApprovalId,
    pub action_key: ExternalActionKey,
    /// Mail system's message identifier; with the company mail server, ordin's Message-ID recorded before sending.
    pub message_id: String,
    pub sent_at: Timestamp,
    pub recipients: Vec<String>,
    pub attachment_names: Vec<String>,
    pub saved_in_sent: bool,
}
