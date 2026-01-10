//! Email — restated from `MailService.md` (SA-RECON-02).
//!
//! The owner's named types (`ListMessages`, `MessagePage`, `EditDraft`, `MailSettings`, …) keep their
//! names; the inline inputs take the mechanical `Mail<Verb>Input` names. The 2C email entities
//! (`EmailDraft`, `SentEmail`, `EmailGrant`) are imported from `ordin-types`.

use ordin_types::domain::{EmailAccess, EmailAttachment};
use ordin_types::prelude::*;
use ordin_types::wire::EmailRecipient;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{EmailDraft, EmailGrant, SentEmail};
pub use ordin_types::ApprovalId;

/// A `message_ref` is 1–998 bytes (MailService.md `MessageSummary`).
const MESSAGE_REF_MAX_BYTES: usize = 998;
/// Recipients of one draft, `to`, `cc` and `bcc` together: 1–100 (`MAIL_RECIPIENTS_INVALID`).
const RECIPIENTS_MAX: usize = 100;
/// Subject at most 998 characters (`MAIL_SUBJECT_TOO_LONG`).
const SUBJECT_MAX_CHARS: usize = 998;
/// Body at most 1 MiB (`MAIL_BODY_TOO_LARGE`).
const BODY_MAX_BYTES: usize = 1024 * 1024;
/// At most 20 attachments (`MAIL_ATTACHMENTS_TOO_LARGE`).
const ATTACHMENTS_MAX: usize = 20;
/// Signature at most 2,000 characters (`MailSettings.signature`).
const SIGNATURE_MAX_CHARS: usize = 2000;

// ---- named owner types ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ListMessages {
    pub workspace_id: WorkspaceId,
    pub email_link_id: EmailLinkId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// SA-PAGE-01, page size 100.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MessagePage {
    pub items: Vec<MessageSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MessageSummary {
    /// Opaque message reference: "imap:<folder-b64url>:<uidvalidity>:<uid>", "graph:<id>" or
    /// "gmail:<id>"; 1–998 bytes. Used as `message_id` in `EmailBlock` / `EmailListBlock`.
    pub message_ref: String,
    pub email_link_id: EmailLinkId,
    /// `outside_company` set from `email.company_domains`.
    pub from: EmailRecipient,
    pub to: Vec<EmailRecipient>,
    pub cc: Vec<EmailRecipient>,
    pub date: Timestamp,
    pub subject: String,
    pub attachment_names: Vec<String>,
    /// RFC 5322 Message-ID header, without angle brackets; None when the message has none.
    pub internet_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MessageView {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub text: String,
    pub truncated: bool,
    /// True when the HTML part held text a reader would not see.
    pub hidden_text: bool,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AttachmentInfo {
    pub index: u16,
    pub file_name: String,
    pub media_type: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OpenMessage {
    pub workspace_id: WorkspaceId,
    pub message_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SaveAttachment {
    pub workspace_id: WorkspaceId,
    pub message_ref: String,
    pub index: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SavedAttachment {
    pub place_id: PlaceId,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EditDraft {
    pub draft_id: EmailDraftId,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<EmailAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GrantEmail {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: EmailAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LinkCheckState {
    pub email_link_id: EmailLinkId,
    pub last_checked_at: Option<Timestamp>,
    pub new_since_last_looked: u32,
    /// Error code, e.g. "MAIL_SERVER_UNREACHABLE".
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MailSettings {
    /// Settings › Email "Add my signature" (on).
    pub add_signature: bool,
    /// At most 2,000 characters; empty = nothing appended.
    pub signature: String,
    /// "Say at the bottom that an agent drafted it" (off).
    pub agent_foot_line: bool,
    /// True when `email.foot_line_required`.
    pub agent_foot_line_locked: bool,
    pub check: CheckSchedule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckSchedule {
    Every15Minutes,
    OnlyOnWorkspaceOpen,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailSettingsUpdate {
    pub add_signature: bool,
    pub signature: String,
    pub agent_foot_line: bool,
    pub check: CheckSchedule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignInCheck {
    SignedIn,
    NotSignedIn,
    Refused,
    NotReachableSecurely,
    Unreachable,
}

// ---- inline inputs (mechanical names, SA-RECON-02) ----

/// `mail.get_draft`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailGetDraftInput {
    pub id: EmailDraftId,
}

/// `mail.list_drafts`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailListDraftsInput {
    pub workspace: WorkspaceId,
}

/// `mail.send`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailSendInput {
    pub id: EmailDraftId,
}

/// `mail.mark_as_sent`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailMarkAsSentInput {
    pub id: EmailDraftId,
}

/// `mail.save_to_mail_drafts`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailSaveToMailDraftsInput {
    pub id: EmailDraftId,
}

/// `mail.save_in_sent`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailSaveInSentInput {
    pub id: SentEmailId,
}

/// `mail.discard_draft`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailDiscardDraftInput {
    pub id: EmailDraftId,
}

/// `mail.check`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailCheckInput {
    pub workspace: WorkspaceId,
}

/// `mail.link_states`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailLinkStatesInput {
    pub workspace: WorkspaceId,
}

/// `mail.mark_looked`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailMarkLookedInput {
    pub link: EmailLinkId,
}

/// `mail.settings`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailSettingsInput {}

/// `mail.verify_sign_in`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailVerifySignInInput {}

// ---- checks ----

fn message_ref(value: &str, at: &str) -> Result<(), InputRejection> {
    rules::bytes(value, 1, MESSAGE_REF_MAX_BYTES, at)
}

impl CommandInput for ListMessages {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor_text(&self.cursor, "/cursor")
    }
}

impl CommandInput for OpenMessage {
    fn check(&mut self) -> Result<(), InputRejection> {
        message_ref(&self.message_ref, "/message_ref")
    }
}

impl CommandInput for SaveAttachment {
    fn check(&mut self) -> Result<(), InputRejection> {
        message_ref(&self.message_ref, "/message_ref")
    }
}

impl CommandInput for EditDraft {
    fn check(&mut self) -> Result<(), InputRejection> {
        // `MAIL_RECIPIENTS_INVALID`: 1 to 100 valid addresses in all.
        let total = self.to.len() + self.cc.len() + self.bcc.len();
        if total == 0 || total > RECIPIENTS_MAX {
            return Err(rules::reject("/to"));
        }
        for (field, list) in [("to", &self.to), ("cc", &self.cc), ("bcc", &self.bcc)] {
            for (i, address) in list.iter().enumerate() {
                rules::email(address, 3, usize::MAX, &format!("/{field}/{i}"))?;
            }
        }
        rules::text(&self.subject, 0, SUBJECT_MAX_CHARS, "/subject")?;
        rules::bytes(&self.body, 0, BODY_MAX_BYTES, "/body")?;
        rules::count(&self.attachments, 0, ATTACHMENTS_MAX, "/attachments")
    }
}

impl CommandInput for GrantEmail {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for MailSettingsUpdate {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.signature, 0, SIGNATURE_MAX_CHARS, "/signature")
    }
}

/// Inputs whose fields are identifiers only: the typed read is the whole check.
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
    MailGetDraftInput,
    MailListDraftsInput,
    MailSendInput,
    MailMarkAsSentInput,
    MailSaveToMailDraftsInput,
    MailSaveInSentInput,
    MailDiscardDraftInput,
    MailCheckInput,
    MailLinkStatesInput,
    MailMarkLookedInput,
    MailSettingsInput,
    MailVerifySignInInput,
);

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> EditDraft {
        EditDraft {
            draft_id: EmailDraftId::new(),
            to: vec!["dan.moss@kestrel.co.uk".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Re: Q3 quote".into(),
            body: "Hi Dan".into(),
            attachments: vec![],
        }
    }

    #[test]
    fn edit_draft_rules() {
        assert!(draft().check().is_ok());
        type Change = Box<dyn Fn(&mut EditDraft)>;
        let cases: Vec<(Change, &str)> = vec![
            (Box::new(|d| d.to.clear()), "/input/to"),
            (Box::new(|d| d.to = vec!["a@b.c".into(); 101]), "/input/to"),
            (Box::new(|d| d.cc = vec!["nope".into()]), "/input/cc/0"),
            (Box::new(|d| d.bcc = vec!["a@b@c".into()]), "/input/bcc/0"),
            (Box::new(|d| d.subject = "s".repeat(999)), "/input/subject"),
            (
                Box::new(|d| d.body = "b".repeat(BODY_MAX_BYTES + 1)),
                "/input/body",
            ),
            (
                Box::new(|d| {
                    d.attachments = (0..21)
                        .map(|_| EmailAttachment {
                            place_id: PlaceId::new(),
                            path: "a.pdf".into(),
                        })
                        .collect()
                }),
                "/input/attachments",
            ),
        ];
        for (change, at) in cases {
            let mut d = draft();
            change(&mut d);
            let refusal = d.check().unwrap_err();
            assert_eq!(
                (refusal.code, refusal.at.as_str()),
                ("SVC_INPUT_INVALID", at)
            );
        }
    }

    #[test]
    fn message_refs_cursors_and_settings() {
        let ws = WorkspaceId::new();
        let mut open = OpenMessage {
            workspace_id: ws,
            message_ref: String::new(),
        };
        assert_eq!(open.check().unwrap_err().at, "/input/message_ref");
        let mut save = SaveAttachment {
            workspace_id: ws,
            message_ref: "x".repeat(999),
            index: 0,
        };
        assert_eq!(save.check().unwrap_err().at, "/input/message_ref");
        save.message_ref = "imap:SU5CT1g:1712000000:4821".into();
        assert!(save.check().is_ok());
        let mut list = ListMessages {
            workspace_id: ws,
            email_link_id: EmailLinkId::new(),
            cursor: Some("c".repeat(1025)),
        };
        assert_eq!(list.check().unwrap_err().code, "SRV_BAD_CURSOR");
        list.cursor = None;
        assert!(list.check().is_ok());
        let mut settings = MailSettingsUpdate {
            add_signature: true,
            signature: "s".repeat(2001),
            agent_foot_line: false,
            check: CheckSchedule::Every15Minutes,
        };
        assert_eq!(settings.check().unwrap_err().at, "/input/signature");
        settings.signature = "Asha".into();
        assert!(settings.check().is_ok());
        let mut grant = GrantEmail {
            workspace_id: ws,
            agent_instance_id: AgentInstanceId::new(),
            access: EmailAccess::ReadEmail,
        };
        assert!(grant.check().is_ok());
        assert!(MailSettingsInput {}.check().is_ok());
        assert!(MailGetDraftInput {
            id: EmailDraftId::new()
        }
        .check()
        .is_ok());
    }

    #[test]
    fn shapes() {
        let page: MessagePage = serde_json::from_value(serde_json::json!({
            "items": [], "next_cursor": null
        }))
        .unwrap();
        assert!(page.items.is_empty());
        assert_eq!(
            serde_json::to_value(CheckSchedule::Every15Minutes).unwrap(),
            "every15_minutes"
        );
        assert!(serde_json::from_value::<MailSettingsInput>(serde_json::json!({"x": 1})).is_err());
    }
}
