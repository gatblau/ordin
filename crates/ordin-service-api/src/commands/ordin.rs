//! Conversation with Ordin (group `ordin`, X5) — restated from `OrdinHost.md` *Service API commands*
//! (SA-RECON-02), with continuing an earlier conversation (ADR-108) and Ordin's drafting of a language
//! (ADR-109).

use std::collections::BTreeMap;

use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde::{Deserialize, Deserializer, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};
use crate::blocks::{Block, UsageEstimate};

/// `ordin.send_message.text`: 1–8,000 characters.
const MESSAGE_MAX_CHARS: usize = 8_000;
/// `ordin.send_message.attachments`: 0–20.
const MAX_ATTACHMENTS: usize = 20;
/// `kept_points`: 0–10, each 1–500 characters.
const MAX_KEPT_POINTS: usize = 10;
const KEPT_POINT_MAX_CHARS: usize = 500;
/// `ordin.ask_about_conversation.question` and `BlockAnswer::Other.text`: 1–2,000 characters.
const QUESTION_MAX_CHARS: usize = 2_000;
/// `ordin.draft_language.name`: 1–60 characters.
const LANGUAGE_NAME_MAX_CHARS: usize = 60;

/// A conversation of Ordin's (UUIDv7, *chosen here* by the Ordin Host).
///
/// SPEC-GAP-7: the owner writes `pub struct OrdinConversationId(pub uuid::Uuid)`; this crate has no
/// `uuid` dependency, so the identifier keeps its canonical 36-character text, refused unless it is a
/// lowercase hyphenated version-7 UUID (the same wire form and check as every 2C identifier).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct OrdinConversationId(String);

impl OrdinConversationId {
    /// Parses the 36-character form; refuses anything but a lowercase hyphenated v7 UUID.
    pub fn parse(raw: &str) -> Result<Self, ordin_types::TypesError> {
        ordin_types::id::parse_v7(raw).map(|_| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for OrdinConversationId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// The person's answer to a question, form or offer block.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlockAnswer {
    Choice {
        choice: String,
    },
    /// 1–2,000 characters.
    Other {
        text: String,
    },
    Form {
        fields: BTreeMap<String, serde_json::Value>,
    },
    /// An offer block (`ResultBlock::Offer`): **Yes** or **No**.
    Offer {
        accept: bool,
    },
}

/// One shown block of the current or an earlier conversation (`ordin.block`, and `BlockAvailable`
/// follow-ups).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ConversationBlock {
    pub block_id: BlockId,
    pub conversation_id: OrdinConversationId,
    pub at: Timestamp,
    pub from: MessageAuthor,
    pub block: Block,
    /// Folded to one line by **Close** (UX-A12).
    pub closed: bool,
    /// A question, form or offer not yet answered; a waiting block cannot be closed.
    pub waiting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ConversationSummary {
    pub conversation_id: OrdinConversationId,
    pub state: ConversationState,
    pub started_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_by: Option<ConversationEnd>,
    /// Messages of this conversation and of every conversation it continues (its chain; ADR-108).
    pub message_count: u32,
    /// The conversation this one continues (ADR-108); `None` for one opened by a first message, a
    /// summary or Clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continues: Option<OrdinConversationId>,
    /// The first `person` message of the chain's first conversation, cut to 120 characters at a word
    /// end; `None` before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_request: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationState {
    Current,
    Earlier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConversationEnd {
    Summarised,
    StartedFresh,
    AfterBreak,
    ContextLimit,
    PutAway,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ConversationMessage {
    pub seq: u32,
    pub author: MessageAuthor,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_id: Option<BlockId>,
    pub at: Timestamp,
    /// A fixed turn the Ordin Host wrote (author `System`); `None` for every other message (F-1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<MessageKind>,
    /// A waiting item moved into this conversation from the one it replaced (step 49; ADR-108).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub carried: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageAuthor {
    Person,
    Ordin,
    System,
}

/// The kind of a `System` message (step 46.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    SlackTurn,
}

// ---- inputs and results, in catalogue order ----

/// `ordin.send_message`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinSendMessageInput {
    pub workspace_id: WorkspaceId,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<ContentRef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinSendMessageResult {
    pub accepted: bool,
    pub position_in_queue: u8,
}

/// `ordin.answer_block`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinAnswerBlockInput {
    pub workspace_id: WorkspaceId,
    pub block_id: BlockId,
    pub answer: BlockAnswer,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinAnswerBlockResult {
    pub accepted: bool,
}

/// `ordin.summarise`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinSummariseInput {
    pub workspace_id: WorkspaceId,
}

/// The new current conversation.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinSummariseResult {
    pub conversation_id: OrdinConversationId,
}

/// `ordin.prepare_start_fresh`: Ordin shows its key points as a question block.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinPrepareStartFreshInput {
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinPrepareStartFreshResult {
    pub accepted: bool,
}

/// `ordin.start_fresh`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinStartFreshInput {
    pub workspace_id: WorkspaceId,
    pub kept_points: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinStartFreshResult {
    pub conversation_id: OrdinConversationId,
}

/// `ordin.continue_conversation` (ADR-108).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinContinueConversationInput {
    pub workspace_id: WorkspaceId,
    pub conversation_id: OrdinConversationId,
    pub kept_points: Vec<String>,
}

/// The new current conversation, which continues the chosen one.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinContinueConversationResult {
    pub conversation_id: OrdinConversationId,
}

/// `ordin.list_conversations`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinListConversationsInput {
    pub workspace_id: WorkspaceId,
    pub cursor: Option<Cursor>,
}

/// `ordin.read_conversation`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinReadConversationInput {
    pub conversation_id: OrdinConversationId,
    pub cursor: Option<Cursor>,
}

/// `ordin.ask_about_conversation`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinAskAboutConversationInput {
    pub workspace_id: WorkspaceId,
    pub conversation_id: OrdinConversationId,
    pub question: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinAskAboutConversationResult {
    pub accepted: bool,
}

/// `ordin.block`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinBlockInput {
    pub workspace_id: WorkspaceId,
    pub block_id: BlockId,
}

/// `ordin.close_item`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinCloseItemInput {
    pub workspace_id: WorkspaceId,
    pub block_id: BlockId,
}

/// `ordin.show_item`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinShowItemInput {
    pub workspace_id: WorkspaceId,
    pub block_id: BlockId,
}

/// `ordin.close_finished`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinCloseFinishedInput {
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinCloseFinishedResult {
    pub closed: u32,
}

/// `ordin.bring_page_up_to_date`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinBringPageUpToDateInput {
    pub workspace_id: WorkspaceId,
    pub page_id: KnowledgePageId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinBringPageUpToDateResult {
    pub accepted: bool,
}

/// `ordin.estimate_language_draft` (ADR-109; step 54.1).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinEstimateLanguageDraftInput {
    /// BCP 47, 2–35 characters.
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinEstimateLanguageDraftResult {
    pub messages: u32,
    pub estimate: Option<UsageEstimate>,
}

/// `ordin.draft_language` (ADR-109; step 54.2); one draft at a time.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinDraftLanguageInput {
    pub locale: String,
    /// 1–60 characters.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OrdinDraftLanguageResult {
    pub accepted: bool,
    pub messages: u32,
}

/// `ordin.stop_language_draft` (ADR-109; step 54.4); nothing running → `{}`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrdinStopLanguageDraftInput {
    pub locale: String,
}

// ---- checks ----

fn kept_points(points: &[String]) -> Result<(), InputRejection> {
    rules::count(points, 0, MAX_KEPT_POINTS, "/kept_points")?;
    for (i, point) in points.iter().enumerate() {
        rules::text(
            point.trim(),
            1,
            KEPT_POINT_MAX_CHARS,
            &format!("/kept_points/{i}"),
        )?;
    }
    Ok(())
}

impl CommandInput for OrdinSendMessageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.text, 1, MESSAGE_MAX_CHARS, "/text")?;
        if let Some(items) = &self.attachments {
            rules::count(items, 0, MAX_ATTACHMENTS, "/attachments")?;
            for (i, item) in items.iter().enumerate() {
                super::check_content_ref(item, &format!("/attachments/{i}"))?;
            }
        }
        Ok(())
    }
}

impl CommandInput for OrdinAnswerBlockInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        match &self.answer {
            BlockAnswer::Other { text } => rules::text(text, 1, QUESTION_MAX_CHARS, "/answer/text"),
            BlockAnswer::Choice { .. } | BlockAnswer::Form { .. } | BlockAnswer::Offer { .. } => {
                Ok(())
            }
        }
    }
}

impl CommandInput for OrdinStartFreshInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        kept_points(&self.kept_points)
    }
}

impl CommandInput for OrdinContinueConversationInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        kept_points(&self.kept_points)
    }
}

impl CommandInput for OrdinListConversationsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for OrdinReadConversationInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for OrdinAskAboutConversationInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.question, 1, QUESTION_MAX_CHARS, "/question")
    }
}

impl CommandInput for OrdinEstimateLanguageDraftInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

impl CommandInput for OrdinDraftLanguageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")?;
        rules::text(self.name.trim(), 1, LANGUAGE_NAME_MAX_CHARS, "/name")
    }
}

impl CommandInput for OrdinStopLanguageDraftInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

macro_rules! no_rules {
    ($($ty:ty),*) => {$(
        impl CommandInput for $ty {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

no_rules!(
    OrdinSummariseInput,
    OrdinPrepareStartFreshInput,
    OrdinBlockInput,
    OrdinCloseItemInput,
    OrdinShowItemInput,
    OrdinCloseFinishedInput,
    OrdinBringPageUpToDateInput
);

#[cfg(test)]
mod tests {
    use super::*;

    const CONVERSATION: &str = "0192a1c0-0d1e-7f20-8a31-425364758697";

    fn conversation() -> OrdinConversationId {
        OrdinConversationId::parse(CONVERSATION).unwrap()
    }

    fn at(result: Result<(), InputRejection>) -> Option<String> {
        result.err().map(|r| r.at)
    }

    #[test]
    fn conversation_id_form() {
        assert_eq!(conversation().as_str(), CONVERSATION);
        assert!(serde_json::from_value::<OrdinConversationId>(serde_json::json!("abc")).is_err());
        let v4 = "0192a1c0-0d1e-4f20-8a31-425364758697";
        assert!(OrdinConversationId::parse(v4).is_err());
        let back: OrdinConversationId =
            serde_json::from_value(serde_json::json!(CONVERSATION)).unwrap();
        assert_eq!(back, conversation());
    }

    #[test]
    fn send_message_rules() {
        let ws = WorkspaceId::new();
        let send = |text: &str, attachments: Option<Vec<ContentRef>>| {
            OrdinSendMessageInput {
                workspace_id: ws,
                text: text.into(),
                attachments,
            }
            .check()
        };
        assert_eq!(at(send("Hello", None)), None);
        assert_eq!(at(send("", None)).as_deref(), Some("/input/text"));
        assert_eq!(
            at(send(&"x".repeat(8_001), None)).as_deref(),
            Some("/input/text")
        );
        let item = |path: &str| ContentRef::PlaceFile {
            place_id: PlaceId::new(),
            relative_path: path.into(),
        };
        assert_eq!(
            at(send("a", Some(vec![item("a.pdf"); 21]))).as_deref(),
            Some("/input/attachments")
        );
        assert_eq!(
            at(send("a", Some(vec![item("a.pdf"), item("../b")]))).as_deref(),
            Some("/input/attachments/1/relative_path")
        );
    }

    #[test]
    fn answers_points_and_questions() {
        let answer = |answer| {
            OrdinAnswerBlockInput {
                workspace_id: WorkspaceId::new(),
                block_id: BlockId::new(),
                answer,
            }
            .check()
        };
        assert_eq!(
            at(answer(BlockAnswer::Other {
                text: String::new()
            }))
            .as_deref(),
            Some("/input/answer/text")
        );
        assert_eq!(at(answer(BlockAnswer::Offer { accept: true })), None);
        let fresh = |points: Vec<String>| {
            OrdinStartFreshInput {
                workspace_id: WorkspaceId::new(),
                kept_points: points,
            }
            .check()
        };
        assert_eq!(at(fresh(vec!["Keep the Q3 rates".into()])), None);
        assert_eq!(
            at(fresh(vec!["p".into(); 11])).as_deref(),
            Some("/input/kept_points")
        );
        assert_eq!(
            at(fresh(vec!["ok".into(), "  ".into()])).as_deref(),
            Some("/input/kept_points/1")
        );
        let mut cont = OrdinContinueConversationInput {
            workspace_id: WorkspaceId::new(),
            conversation_id: conversation(),
            kept_points: vec!["x".repeat(501)],
        };
        assert_eq!(at(cont.check()).as_deref(), Some("/input/kept_points/0"));
        let mut ask = OrdinAskAboutConversationInput {
            workspace_id: WorkspaceId::new(),
            conversation_id: conversation(),
            question: String::new(),
        };
        assert_eq!(at(ask.check()).as_deref(), Some("/input/question"));
    }

    #[test]
    fn cursors_and_languages() {
        let long = Some(Cursor::encode(&"x".repeat(900)));
        let mut list = OrdinListConversationsInput {
            workspace_id: WorkspaceId::new(),
            cursor: long.clone(),
        };
        assert_eq!(list.check().unwrap_err().code, "SRV_BAD_CURSOR");
        let mut read = OrdinReadConversationInput {
            conversation_id: conversation(),
            cursor: long,
        };
        assert_eq!(read.check().unwrap_err().code, "SRV_BAD_CURSOR");
        assert_eq!(
            at(OrdinEstimateLanguageDraftInput {
                locale: "ja-JP".into()
            }
            .check()),
            None
        );
        assert_eq!(
            at(OrdinStopLanguageDraftInput { locale: "j".into() }.check()).as_deref(),
            Some("/input/locale")
        );
        assert_eq!(
            at(OrdinDraftLanguageInput {
                locale: "ja-JP".into(),
                name: "日本語".into()
            }
            .check()),
            None
        );
        assert_eq!(
            at(OrdinDraftLanguageInput {
                locale: "ja-JP".into(),
                name: "x".repeat(61)
            }
            .check())
            .as_deref(),
            Some("/input/name")
        );
        assert_eq!(
            at(OrdinDraftLanguageInput {
                locale: "x".into(),
                name: "n".into()
            }
            .check())
            .as_deref(),
            Some("/input/locale")
        );
        let ws = WorkspaceId::new();
        let block = BlockId::new();
        assert!(OrdinSummariseInput { workspace_id: ws }.check().is_ok());
        assert!(OrdinPrepareStartFreshInput { workspace_id: ws }
            .check()
            .is_ok());
        assert!(OrdinBlockInput {
            workspace_id: ws,
            block_id: block
        }
        .check()
        .is_ok());
        assert!(OrdinCloseItemInput {
            workspace_id: ws,
            block_id: block
        }
        .check()
        .is_ok());
        assert!(OrdinShowItemInput {
            workspace_id: ws,
            block_id: block
        }
        .check()
        .is_ok());
        assert!(OrdinCloseFinishedInput { workspace_id: ws }.check().is_ok());
        assert!(OrdinBringPageUpToDateInput {
            workspace_id: ws,
            page_id: KnowledgePageId::new()
        }
        .check()
        .is_ok());
    }

    #[test]
    fn message_shape() {
        let message = ConversationMessage {
            seq: 1,
            author: MessageAuthor::Ordin,
            text: "Done.".into(),
            block_id: None,
            at: Timestamp::now(),
            kind: None,
            carried: false,
        };
        let json = serde_json::to_value(&message).unwrap();
        assert!(json.get("carried").is_none() && json.get("kind").is_none());
        let answer: BlockAnswer = serde_json::from_value(
            serde_json::json!({"kind": "form", "fields": {"quarter": "Q3"}}),
        )
        .unwrap();
        assert!(matches!(answer, BlockAnswer::Form { .. }));
    }
}
