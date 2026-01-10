//! 2C part 1 § Handover.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- Handover ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Handover {
    pub id: HandoverId,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_id: Option<LifecycleId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_name: Option<String>,
    pub sender: String,
    pub receiver: HandoverReceiver,
    pub results: Vec<String>,
    pub note: HandoverNote,
    pub checks_passed: Vec<String>,
    pub handover_place_id: HandoverPlaceId,
    pub round: u32,
    /// True once a declared send-back takes `round` past the stage's agreed `max_rounds`; the
    /// round is marked, never refused (ADR-099 item 5; amended January 2026, decision 80: C-21).
    pub beyond_agreed_rounds: bool,
    pub events: Vec<HandoverEvent>,
    pub state: HandoverState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_workspace_id: Option<WorkspaceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_into_workspace_id: Option<WorkspaceId>,
    /// Slack messages posted about this handover, appended by the installation that posted and
    /// copied from the record (amended January 2026, ADR-102). Empty when none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slack_messages: Vec<HandoverSlackMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HandoverReceiver {
    pub name: String,
    pub address: String,
    pub uses_ordin: bool,
    /// For a Git handover place, the receiver's GitHub login (ADR-092 item 3; amended January 2026, decision 80: C-21).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
    /// For a Jira handover place, the receiver's Atlassian account id, 1–128 of `[A-Za-z0-9:_-]`;
    /// `address` may then be "" when Jira hides it (amended January 2026, ADR-106; C-88).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jira_account_id: Option<String>,
}

/// Four headings; their names are fixed in /spec from ADR-092 (LD-37).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HandoverNote {
    pub sections: [NoteSection; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct NoteSection {
    pub heading: String,
    pub body: String,
    pub drafted_by_ordin: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HandoverEvent {
    pub kind: HandoverEventKind,
    /// An email address; on a Jira place `jira:<accountId>` (amended January 2026, ADR-106).
    pub by: String,
    pub at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// UUIDv7 that de-duplicates a re-read after an unknown write outcome; always set on a Jira place
    /// (amended January 2026, ADR-106).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HandoverEventKind {
    Sent,
    Seen,
    Accepted,
    SentBack,
    TakenBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HandoverState {
    Draft,
    Sent,
    Seen,
    Accepted,
    SentBack,
    TakenBack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HandoverSlackMessage {
    pub notice: HandoverNoticeKind,
    pub permalink: String,
    pub by: String,
    pub at: Timestamp,
}

/// The handover moments a Slack notice announces (amended January 2026, ADR-102).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HandoverNoticeKind {
    HandedOver,
    Accepted,
    Returned,
    TakenBack,
    Reminder,
}

// ---- HandoverPlace ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HandoverPlace {
    pub id: HandoverPlaceId,
    pub place_type: HandoverPlaceType,
    pub address: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// `Jira` (amended January 2026, ADR-106): a Jira Cloud project; desktop only.
pub enum HandoverPlaceType {
    Git,
    Sharepoint,
    Jira,
}
