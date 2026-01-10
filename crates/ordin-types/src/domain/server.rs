//! 2C part 1 § Server, channels and triggers.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- ChannelPairing ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ChannelPairing {
    pub id: ChannelPairingId,
    pub channel_kind: ChannelKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_chat_id: Option<String>,
    /// The setup-command invocation that made it.
    pub paired_by: String,
    pub code_expires_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
    pub state: PairingState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Telegram,
    Slack,
}

/// Shared by ChannelPairing and DesktopPairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PairingState {
    Pending,
    Paired,
    Ended,
}

// ---- CardReference ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CardReference {
    pub id: CardReferenceId,
    pub channel_pairing_id: ChannelPairingId,
    /// Random, single use; fits the channel's button data.
    pub reference: String,
    pub card: CardSubject,
    pub button: CardButton,
    pub state: CardReferenceState,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CardSubject {
    Approval {
        approval_id: ApprovalId,
    },
    /// Question cards have no domain entity in 04; identifier form fixed in /spec (LD-36).
    Question {
        question_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CardButton {
    Approve,
    NotNow,
    ShowDetails,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CardReferenceState {
    Open,
    Used,
    Closed,
}

/// One delivery of an approval card to one paired channel (amended January 2026, decision 80: C-27).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct CardDelivery {
    pub approval_id: ApprovalId,
    pub channel_pairing_id: ChannelPairingId,
    /// The channel's message identifier; absent until sent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// The text sent, kept to edit the message once the card is decided.
    pub card_text: String,
    pub pressed: bool,
}

// ---- DesktopPairing ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct DesktopPairing {
    pub id: DesktopPairingId,
    pub computer_name: String,
    pub desktop_key_hash: Sha256Hex,
    pub code_expires_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_connected_at: Option<Timestamp>,
    pub last_sequence_sent: u64,
    pub state: PairingState,
}

// ---- ServerConnection ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ServerConnection {
    pub id: ServerConnectionId,
    pub display_name: String,
    pub address: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_api_version: Option<String>,
    pub last_sequence_received: u64,
    /// Keychain item holding the desktop key; the key never enters the database.
    pub keychain_ref: String,
    pub state: ServerConnectionState,
    pub paired_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServerConnectionState {
    Connected,
    NotConnected,
    Disconnected,
    RefusedForVersion,
}

// ---- Trigger ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Trigger {
    pub id: TriggerId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub listens_for: TriggerSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<NatsDelivery>,
    pub filter: Vec<FilterCondition>,
    pub input_mapping: Vec<InputMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier_source: Option<IdentifierSource>,
    pub workflow_id: WorkflowId,
    pub workflow_version: u32,
    pub limits: TriggerLimits,
    pub state: TriggerState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turned_on_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turned_on_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSource {
    ChatFile {
        file_kind: String,
        name_pattern: String,
    },
    NatsEvent {
        subject: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NatsDelivery {
    JetStream { consumer_name: String },
    CoreNats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct FilterCondition {
    pub field: String,
    pub operator: FilterOperator,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

/// Fixed-code operators of 02:632.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FilterOperator {
    Equals,
    IsOneOf,
    Exists,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct InputMapping {
    pub input: String,
    pub from: MappingSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MappingSource {
    PayloadField { path: String },
    Fixed { value: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdentifierSource {
    NatsMsgId,
    CloudeventsId,
    Field { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TriggerLimits {
    pub max_waiting: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TriggerState {
    Proposed,
    On,
    Off,
    PausedByDailyLimit,
}

// ---- TriggerEvent ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TriggerEvent {
    pub id: TriggerEventId,
    pub trigger_id: TriggerId,
    pub identifier: String,
    pub received_at: Timestamp,
    pub outcome: TriggerOutcome,
    /// The crew run of the workflow the trigger started (through the Ordin Host), set once
    /// that run has started; amended January 2026, decision 80: C-8.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crew_run_id: Option<CrewRunId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TriggerOutcome {
    Started,
    SkippedByFilter,
    Duplicate,
    RefusedByLimit,
    Unreadable,
}

// ---- TelegramLink ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TelegramLink {
    pub id: TelegramLinkId,
    pub workspace_id: WorkspaceId,
    pub chat_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<i64>,
    pub title: String,
    pub kind: TelegramChatKind,
    pub bot_state: TelegramBotState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_count: Option<u32>,
    pub state: TelegramLinkState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TelegramChatKind {
    Group,
    ForumTopic,
    Channel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TelegramBotState {
    Member,
    Administrator,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// `Offered` is never written to `telegram_links`: the chats the bot is in but no workspace has
/// linked are the Telegram Service's offer list, kept apart; a row is written `linked`
/// (amended January 2026, decision 80: C-25). The variant stays for reading older rows.
pub enum TelegramLinkState {
    Offered,
    Linked,
}

// ---- KeptTelegramMessage ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KeptTelegramMessage {
    pub id: KeptTelegramMessageId,
    pub telegram_link_id: TelegramLinkId,
    pub message_id: i64,
    pub sender_name: String,
    pub sent_at: Timestamp,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    pub files: Vec<TelegramFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TelegramFile {
    pub file_id: String,
    pub file_name: String,
    pub size_bytes: u64,
}

// ---- TelegramDraft ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TelegramDraft {
    pub id: TelegramDraftId,
    pub telegram_link_id: TelegramLinkId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    pub text: String,
    pub files: Vec<EmailAttachment>,
    pub drafted_by: DraftedBy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_key: Option<ExternalActionKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram_message_id: Option<i64>,
    pub state: TelegramDraftState,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TelegramDraftState {
    Draft,
    Posted,
}

// ---- TelegramPostRequest ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct TelegramPostRequest {
    pub id: TelegramPostRequestId,
    pub draft_id: TelegramDraftId,
    pub workspace_id: WorkspaceId,
    pub run_id: RunId,
    pub approval_id: ApprovalId,
    pub state: TelegramPostRequestState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_key: Option<ExternalActionKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal_reason: Option<String>,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TelegramPostRequestState {
    WaitingForYou,
    Posting,
    Posted,
    Refused,
    Rejected,
    Expired,
    UnknownOutcome,
}

// ---- SlackLink ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SlackLink {
    pub id: SlackLinkId,
    pub workspace_id: WorkspaceId,
    pub team_id: String,
    pub channel_id: String,
    pub name: String,
    pub private: bool,
    /// "Let Ordin look at new messages" (ADR-102); default false.
    pub ordin_reads: bool,
    pub state: SlackLinkState,
    pub linked_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_up_to_ts: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlackLinkState {
    Linked,
    NoAccess,
}

// ---- KeptSlackMessage ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KeptSlackMessage {
    pub id: KeptSlackMessageId,
    pub slack_link_id: SlackLinkId,
    pub ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    pub sender_member_id: String,
    pub sender_name: String,
    pub sent_at: Timestamp,
    pub text: String,
    pub from_person: bool,
}

// ---- SlackDraft ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SlackDraft {
    pub id: SlackDraftId,
    pub slack_link_id: SlackLinkId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    pub text: String,
    pub drafted_by: DraftedBy,
    pub state: SlackDraftState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_post_id: Option<SlackPostId>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlackDraftState {
    Draft,
    Posted,
}

// ---- SlackRule ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SlackRule {
    pub id: SlackRuleId,
    pub kind: SlackRuleKind,
    pub scope: SlackRuleScope,
    pub channel_id: String,
    pub channel_name: String,
    pub daily_cap: u32,
    pub state: SlackRuleState,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turned_off_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlackRuleKind {
    HandoverNotices,
    OrdinAnswers,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlackRuleScope {
    Stage {
        lifecycle_id: LifecycleId,
        stage_name: String,
    },
    Link {
        slack_link_id: SlackLinkId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlackRuleState {
    On,
    Off,
}

// ---- SlackPost ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SlackPost {
    pub id: SlackPostId,
    pub destination: SlackDestination,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    pub text: String,
    pub origin: SlackPostOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<SlackPostAuthority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_key: Option<ExternalActionKey>,
    pub state: SlackPostState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permalink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal_code: Option<String>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlackDestination {
    Channel {
        channel_id: String,
        channel_name: String,
    },
    DirectMessage {
        member_id: String,
        person_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlackPostOrigin {
    Handover {
        handover_id: HandoverId,
        notice: HandoverNoticeKind,
    },
    Draft {
        slack_draft_id: SlackDraftId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlackPostAuthority {
    Approval { approval_id: ApprovalId },
    Rule { slack_rule_id: SlackRuleId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SlackPostState {
    Prepared,
    Waiting,
    Posting,
    Posted,
    Refused,
    Rejected,
    Expired,
    UnknownOutcome,
}
