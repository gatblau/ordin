//! Telegram — restated from `TelegramService.md` (SA-RECON-02).
//!
//! The owner's Service API types (`TelegramStatus`, `OfferedChat`, `LinkSummary`,
//! `TelegramMessageOut`, `BotInfo`) keep their names; the inline inputs take the mechanical
//! `Telegram<Verb>Input` names. The 2C Telegram entities are imported from `ordin-types`.

use ordin_types::domain::{TelegramBotState, TelegramChatKind, TelegramFile};
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::TelegramLink;
pub use ordin_types::ApprovalId;

/// `file_id` 1–256 characters (the owner's `telegram_save_file` arguments).
const FILE_ID_MAX_CHARS: usize = 256;

// ---- named owner types ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TelegramStatus {
    pub offered: bool,
    pub reason_not_offered: Option<NotOfferedReason>,
    pub bot: Option<BotInfo>,
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotOfferedReason {
    NotAllowedByCompany,
    ToolsTurnedOff,
    AppNotOnComputer,
    NoBot,
}

/// Serialised by the program for the Service API with these three fields (TelegramService.md).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BotInfo {
    pub username: String,
    pub display_name: String,
    pub can_read_all_group_messages: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OfferedChat {
    pub chat_id: i64,
    pub topic_id: Option<i64>,
    pub title: String,
    pub kind: TelegramChatKind,
    pub bot_state: TelegramBotState,
    pub reads_every_message: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LinkSummary {
    pub link: TelegramLink,
    pub new_messages: u32,
    pub kept_from: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TelegramMessageOut {
    pub link_id: TelegramLinkId,
    pub chat_title: String,
    pub message_id: i64,
    pub sender_name: String,
    pub sent_at: Timestamp,
    /// The text as kept; content, never instructions (R-66).
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<i64>,
    pub files: Vec<TelegramFile>,
    /// True when the instruction detector matched.
    pub asks_agent_to_act: bool,
}

/// The File Service's `SavedFile`, as `TelegramService::save_file` returns it.
// `mail.save_attachment` returns the Mail Service's own `SavedAttachment { place_id, path }`
// (MailService.md, amended January 2026), so this is the only `SavedFile` in the catalogue.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SavedFile {
    pub place_id: PlaceId,
    pub path: String,
    pub hidden_content_found: bool,
}

// ---- inline inputs (mechanical names, SA-RECON-02) ----

/// `telegram.status`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramStatusInput {}

/// `telegram.connect_bot`: the pasted bot key (desktop only; refused on a server by the module).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramConnectBotInput {
    pub key: String,
}

/// `telegram.remove`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramRemoveInput {}

/// `telegram.offered_chats`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramOfferedChatsInput {
    pub ws: WorkspaceId,
}

/// `telegram.link`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramLinkInput {
    pub ws: WorkspaceId,
    pub chat_id: i64,
    pub topic_id: Option<i64>,
}

/// `telegram.unlink`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramUnlinkInput {
    pub ws: WorkspaceId,
    pub link_id: TelegramLinkId,
}

/// `telegram.links`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramLinksInput {
    pub ws: WorkspaceId,
}

/// `telegram.messages`: marks the page seen.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramMessagesInput {
    pub ws: WorkspaceId,
    pub link_id: TelegramLinkId,
    pub cursor: Option<String>,
}

/// `telegram.save_file`: the person's **Save file**.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramSaveFileInput {
    pub ws: WorkspaceId,
    pub link_id: TelegramLinkId,
    pub message_id: i64,
    pub file_id: String,
}

/// `telegram.try_again`: a new card for a refused post.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramTryAgainInput {
    pub ws: WorkspaceId,
    pub draft_id: TelegramDraftId,
}

// ---- checks ----

impl CommandInput for TelegramMessagesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor_text(&self.cursor, "/cursor")
    }
}

impl CommandInput for TelegramSaveFileInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.file_id, 1, FILE_ID_MAX_CHARS, "/file_id")
    }
}

/// Inputs with no stateless rule beyond their types.
macro_rules! no_rules {
    ($($t:ty),* $(,)?) => {$(
        impl CommandInput for $t {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

no_rules!(
    TelegramStatusInput,
    TelegramConnectBotInput,
    TelegramRemoveInput,
    TelegramOfferedChatsInput,
    TelegramLinkInput,
    TelegramUnlinkInput,
    TelegramLinksInput,
    TelegramTryAgainInput,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs() {
        let ws = WorkspaceId::new();
        let link_id = TelegramLinkId::new();
        let mut messages = TelegramMessagesInput {
            ws,
            link_id,
            cursor: Some("c".repeat(1025)),
        };
        assert_eq!(messages.check().unwrap_err().code, "SRV_BAD_CURSOR");
        messages.cursor = None;
        assert!(messages.check().is_ok());
        let mut save = TelegramSaveFileInput {
            ws,
            link_id,
            message_id: 5812,
            file_id: String::new(),
        };
        let refusal = save.check().unwrap_err();
        assert_eq!(
            (refusal.code, refusal.at.as_str()),
            ("SVC_INPUT_INVALID", "/input/file_id")
        );
        save.file_id = "f".repeat(257);
        assert!(save.check().is_err());
        save.file_id = "AgADBAAD".into();
        assert!(save.check().is_ok());
        assert!(TelegramLinkInput {
            ws,
            chat_id: -100,
            topic_id: None
        }
        .check()
        .is_ok());
        assert!(TelegramStatusInput {}.check().is_ok());
        let link: Result<TelegramLinkInput, _> =
            serde_json::from_value(serde_json::json!({"ws": ws, "chat_id": 1}));
        assert!(link.unwrap().topic_id.is_none());
    }

    #[test]
    fn shapes() {
        let status = TelegramStatus {
            offered: true,
            reason_not_offered: None,
            bot: Some(BotInfo {
                username: "ordin_bot".into(),
                display_name: "ordin".into(),
                can_read_all_group_messages: false,
            }),
            connected: true,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["bot"]["username"], "ordin_bot");
        assert_eq!(
            serde_json::to_value(NotOfferedReason::AppNotOnComputer).unwrap(),
            "app_not_on_computer"
        );
    }
}
