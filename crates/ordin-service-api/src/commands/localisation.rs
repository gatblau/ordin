//! Language — restated from `Localisation.md` (SA-RECON-02), with its amendment for Ordin's language and
//! language files (ADR-109: the ten commands from `localisation.list_languages` down).
//!
//! `localisation.import_language` does not refuse an `ftl` over 2 MiB here: Localisation refuses it with
//! `LOC_FILE_TOO_LARGE`, which ServiceApi.md expects the person to see; the body limit of the command
//! (9 MiB) is the envelope's.

use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

use super::rules;

pub use ordin_types::domain::identity::LocaleSource;
pub use ordin_types::{Cursor, Page};

/// `localisation.list_messages.text`: 1–200 characters.
const FILTER_TEXT_MAX_CHARS: usize = 200;
/// `localisation.edit_message.ftl`: 1–4,096 characters.
const FTL_ENTRY_MAX_CHARS: usize = 4096;
/// `localisation.mark_reviewed.message_ids`: 1–200 identifiers.
const MARK_REVIEWED_MAX_IDS: usize = 200;

// ---- results ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    Ltr,
    Rtl,
}

/// The locale in force; the payload of the `locale_changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ActiveLocale {
    /// The negotiated, bundled, offered tag, e.g. "en-GB".
    pub locale: String,
    pub source: LocaleSource,
    pub direction: TextDirection,
    /// Tags tried for a missing message, in order, ending in "en-GB"; no duplicates.
    pub fallback_chain: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OfferedLocale {
    pub locale: String,
    pub native_name: String,
    pub direction: TextDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CatalogueText {
    pub locale: String,
    pub direction: TextDirection,
    /// One entry per tag of the chain, first = `locale`; `ftl` is the concatenated source.
    pub resources: Vec<CatalogueResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CatalogueResource {
    pub locale: String,
    pub ftl: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LanguageKind {
    Shipped,
    Made,
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LanguageState {
    Drafting,
    NeedsReview,
    InUse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LanguageSummary {
    pub locale: String,
    /// The language's own served `locale-native-name`, else the name it was made with.
    pub native_name: Option<String>,
    pub direction: TextDirection,
    pub kind: LanguageKind,
    pub state: LanguageState,
    /// In `offered()`, so `localisation.set_locale` accepts it.
    pub choosable: bool,
    /// A bundled locale the profile offers; `false` for person languages.
    pub offered_by_company: bool,
    /// Served entries with origin `edited`.
    pub edited: u32,
    /// en-GB messages this language shows in English.
    pub in_english: u32,
    /// Safety entries stored with `reviewed = 0`.
    pub needs_review: u32,
    /// Stored entries with `usable = 0` after a re-check.
    pub needs_changing: u32,
    /// `None` for a shipped language with no person row.
    pub made_for_version: Option<String>,
    /// `Some` while state is `drafting`.
    pub draft: Option<DraftProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DraftProgress {
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageSource {
    Shipped,
    Edited,
    Drafted,
    Imported,
    English,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageOnly {
    NeedsReview,
    InEnglish,
    Edited,
    NeedsChanging,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MessageRow {
    pub message_id: String,
    pub area: String,
    /// The en-GB entry (Fluent source).
    pub english: String,
    /// The language's entry as stored or shipped; `None` when it shows in English.
    pub text: Option<String>,
    /// The shipped entry of a shipped language; `None` otherwise.
    pub shipped: Option<String>,
    pub source: MessageSource,
    pub safety: bool,
    pub reviewed: bool,
    pub needs_changing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LanguageFile {
    pub file_name: String,
    pub ftl: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ImportReport {
    pub locale: String,
    pub stored: u32,
    pub dropped_unknown: u32,
    pub in_english: u32,
    pub left_out: u32,
    /// At most 100, in message-id order.
    pub left_out_ids: Vec<String>,
    pub summary: LanguageSummary,
}

/// The `change` of the `language_changed` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LanguageChangeKind {
    Edited,
    Restored,
    Reviewed,
    Confirmed,
    Imported,
    Removed,
    DraftStarted,
    DraftProgress,
    DraftEnded,
}

/// `localisation.get_settings`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LocalisationGetSettingsResult {
    pub active: ActiveLocale,
    pub offered: Vec<OfferedLocale>,
    pub following_operating_system: bool,
}

/// `localisation.list_languages`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LocalisationListLanguagesResult {
    pub languages: Vec<LanguageSummary>,
    pub areas: Vec<String>,
}

// ---- inputs ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationGetSettingsInput {}

/// `Some(tag)` stores the person's choice; `None` returns to following the operating system.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationSetLocaleInput {
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationGetCatalogueInput {
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationListLanguagesInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationListMessagesInput {
    pub locale: String,
    /// 1–200 characters; a case-folded substring of the identifier, the en-GB text or the language's text.
    pub text: Option<String>,
    /// One of the areas `localisation.list_languages` returns.
    pub area: Option<String>,
    pub only: Option<MessageOnly>,
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationEditMessageInput {
    pub locale: String,
    pub message_id: String,
    /// 1–4,096 characters.
    pub ftl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationRestoreMessageInput {
    pub locale: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationRestoreLanguageInput {
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationMarkReviewedInput {
    pub locale: String,
    /// 1–200 identifiers.
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationConfirmLanguageInput {
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationRemoveLanguageInput {
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationExportLanguageInput {
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalisationImportLanguageInput {
    /// 1–255 characters.
    pub file_name: String,
    /// The file's text; Localisation refuses more than 2 MiB with `LOC_FILE_TOO_LARGE`.
    pub ftl: String,
    pub replace: bool,
}

// ---- checks ----
//
// Every `locale` is a BCP 47 tag of 2–35 characters (2C `LocalePreference.locale`, ≤ 35 characters).

impl CommandInput for LocalisationGetSettingsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for LocalisationSetLocaleInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        match &self.locale {
            Some(locale) => rules::locale(locale, "/locale"),
            None => Ok(()),
        }
    }
}

impl CommandInput for LocalisationGetCatalogueInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

impl CommandInput for LocalisationListLanguagesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for LocalisationListMessagesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")?;
        rules::opt_text(&self.text, 1, FILTER_TEXT_MAX_CHARS, "/text")?;
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for LocalisationEditMessageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")?;
        rules::text(&self.ftl, 1, FTL_ENTRY_MAX_CHARS, "/ftl")
    }
}

impl CommandInput for LocalisationRestoreMessageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

impl CommandInput for LocalisationRestoreLanguageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

impl CommandInput for LocalisationMarkReviewedInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")?;
        rules::count(&self.message_ids, 1, MARK_REVIEWED_MAX_IDS, "/message_ids")
    }
}

impl CommandInput for LocalisationConfirmLanguageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

impl CommandInput for LocalisationRemoveLanguageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

impl CommandInput for LocalisationExportLanguageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.locale, "/locale")
    }
}

impl CommandInput for LocalisationImportLanguageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.file_name, 1, 255, "/file_name")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(result: Result<(), InputRejection>) -> String {
        let rejection = result.unwrap_err();
        assert_eq!(rejection.code, "SVC_INPUT_INVALID");
        rejection.at
    }

    #[test]
    fn locale_inputs() {
        assert!(LocalisationSetLocaleInput { locale: None }.check().is_ok());
        assert!(LocalisationSetLocaleInput {
            locale: Some("fr-FR".into())
        }
        .check()
        .is_ok());
        assert_eq!(
            at(LocalisationSetLocaleInput {
                locale: Some("x".into())
            }
            .check()),
            "/input/locale"
        );
        assert_eq!(
            at(LocalisationGetCatalogueInput {
                locale: "fr_FR".into()
            }
            .check()),
            "/input/locale"
        );
        let tag = || "ja-JP".to_owned();
        assert!(LocalisationRestoreMessageInput {
            locale: tag(),
            message_id: "a".into()
        }
        .check()
        .is_ok());
        assert!(LocalisationRestoreLanguageInput { locale: tag() }
            .check()
            .is_ok());
        assert!(LocalisationConfirmLanguageInput { locale: tag() }
            .check()
            .is_ok());
        assert!(LocalisationRemoveLanguageInput { locale: tag() }
            .check()
            .is_ok());
        assert!(LocalisationExportLanguageInput { locale: tag() }
            .check()
            .is_ok());
        assert!(LocalisationGetSettingsInput {}.check().is_ok());
        assert!(LocalisationListLanguagesInput {}.check().is_ok());
    }

    #[test]
    fn list_messages() {
        let input = |text: Option<&str>, cursor: Option<Cursor>| LocalisationListMessagesInput {
            locale: "fr-FR".into(),
            text: text.map(str::to_owned),
            area: None,
            only: None,
            cursor,
        };
        assert!(input(Some("approval"), Some(Cursor::encode(&"k")))
            .check()
            .is_ok());
        assert_eq!(at(input(Some(""), None).check()), "/input/text");
        assert_eq!(
            at(input(Some(&"t".repeat(201)), None).check()),
            "/input/text"
        );
        let rejection = input(None, Some(Cursor::encode(&"x".repeat(900))))
            .check()
            .unwrap_err();
        assert_eq!(
            (rejection.code, rejection.at.as_str()),
            ("SRV_BAD_CURSOR", "/input/cursor")
        );
        // An unknown filter value is a shape error (Gherkin: only "everything").
        let err = serde_json::from_value::<LocalisationListMessagesInput>(
            serde_json::json!({"locale": "fr-FR", "only": "everything"}),
        );
        assert!(err.is_err());
    }

    #[test]
    fn edits_reviews_and_imports() {
        let edit = |ftl: &str| LocalisationEditMessageInput {
            locale: "fr-FR".into(),
            message_id: "run-progress".into(),
            ftl: ftl.into(),
        };
        assert!(edit("run-progress = Fait").check().is_ok());
        assert_eq!(at(edit("").check()), "/input/ftl");
        assert_eq!(at(edit(&"f".repeat(4097)).check()), "/input/ftl");
        let review = |n: usize| LocalisationMarkReviewedInput {
            locale: "fr-FR".into(),
            message_ids: (0..n).map(|i| format!("m-{i}")).collect(),
        };
        assert!(review(200).check().is_ok());
        assert_eq!(at(review(0).check()), "/input/message_ids");
        assert_eq!(at(review(201).check()), "/input/message_ids");
        let import = |name: &str, ftl: String| LocalisationImportLanguageInput {
            file_name: name.into(),
            ftl,
            replace: false,
        };
        // An ftl over 2 MiB passes here; Localisation refuses it with LOC_FILE_TOO_LARGE.
        assert!(import("fr.ftl", "x".repeat(2_097_153)).check().is_ok());
        assert_eq!(at(import("", String::new()).check()), "/input/file_name");
    }

    #[test]
    fn event_payload_shapes() {
        let active: ActiveLocale = serde_json::from_value(serde_json::json!(
            {"locale":"fr-FR","source":"person","direction":"ltr","fallback_chain":["fr-FR","en-GB"]}
        ))
        .unwrap();
        assert_eq!(active.locale, "fr-FR");
        assert_eq!(active.source, LocaleSource::Person);
        let change: LanguageChangeKind = serde_json::from_str("\"draft_progress\"").unwrap();
        assert_eq!(change, LanguageChangeKind::DraftProgress);
    }
}
