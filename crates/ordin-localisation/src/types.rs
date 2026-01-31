//! The public data types of Localisation (Localisation.md *Public Interface*).

use ordin_types::domain::LocaleSource;
use ordin_types::num::DecimalString;
use ordin_types::time::Timestamp;
use serde::{Deserialize, Serialize};

/// A message identifier that exists in the `en-GB` catalogue. Constructed only by the generated
/// constants in `ordin_localisation::id` (build.rs): a typo does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(&'static str);

impl MessageId {
    pub const fn as_str(&self) -> &'static str {
        self.0
    }

    /// Used by the generated constants only.
    pub(crate) const fn new(id: &'static str) -> MessageId {
        MessageId(id)
    }
}

/// One named argument of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    Text(String),
    /// Formatted with CLDR number rules of the active locale.
    Number(DecimalString),
    /// Whole count; drives plural selection.
    Count(u64),
    /// Formatted with CLDR date-time rules in the computer's local offset.
    DateTime(Timestamp),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageArgs(pub Vec<(&'static str, ArgValue)>);

impl MessageArgs {
    pub fn new() -> Self {
        MessageArgs(Vec::new())
    }

    pub fn with(mut self, name: &'static str, value: ArgValue) -> Self {
        self.0.push((name, value));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    Ltr,
    Rtl,
}

/// The locale in force, published to every subscriber.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActiveLocale {
    /// The negotiated, bundled, offered tag, e.g. "en-GB".
    pub locale: String,
    pub source: LocaleSource,
    pub direction: TextDirection,
    /// Tags tried for a missing message, in order, ending in "en-GB"; no duplicates.
    pub fallback_chain: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateTimeStyle {
    Date,
    Time,
    DateTime,
    ShortDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageKind {
    Shipped,
    Made,
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageState {
    Drafting,
    NeedsReview,
    InUse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LanguageSummary {
    pub locale: String,
    /// The language's own served `locale-native-name`, else the name it was made with; `None` when neither exists.
    pub native_name: Option<String>,
    pub direction: TextDirection,
    pub kind: LanguageKind,
    pub state: LanguageState,
    /// In `offered()`, so `set_locale` accepts it.
    pub choosable: bool,
    /// A bundled locale in the profile's `locales.offered` (or every bundled locale when it is empty); `false` for person languages.
    pub offered_by_company: bool,
    /// Served entries with origin `edited`.
    pub edited: u32,
    /// en-GB messages this language shows in English: no served entry and, for a shipped language, no shipped text.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DraftProgress {
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSource {
    Shipped,
    Edited,
    Drafted,
    Imported,
    English,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MessageFilter {
    /// 1–200 characters; a case-folded substring of the identifier, the en-GB text or the language's text.
    #[serde(default)]
    pub text: Option<String>,
    /// One of `areas()`.
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub only: Option<MessageOnly>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOnly {
    NeedsReview,
    InEnglish,
    Edited,
    NeedsChanging,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MessageRow {
    pub message_id: String,
    pub area: String,
    /// The en-GB entry (Fluent source).
    pub english: String,
    /// The language's entry as stored (person layer) or shipped; `None` when it shows in English.
    pub text: Option<String>,
    /// The shipped entry of a shipped language; `None` otherwise.
    pub shipped: Option<String>,
    pub source: MessageSource,
    pub safety: bool,
    pub reviewed: bool,
    pub needs_changing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LanguageFile {
    pub file_name: String,
    pub ftl: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum LanguageMatch {
    Choosable { locale: String },
    NotConfirmed { locale: String },
    NotOffered { locale: String },
    Missing { locale: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftSource {
    pub message_id: String,
    pub english: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DraftBatchOutcome {
    pub stored: u32,
    pub left_out: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftEnd {
    Finished,
    Stopped,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LanguageChange {
    pub locale: String,
    pub change: LanguageChangeKind,
}

/// Why `check_entry` refused an entry. Each has a plain sentence, catalogue id
/// `language-problem-<kebab of the variant>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntryProblem {
    TooLong,
    ForbiddenCharacter,
    DoesNotParse,
    NotOneMessage,
    UnknownMessage,
    IdMismatch,
    VariableMissing {
        name: String,
    },
    VariableUnknown {
        name: String,
    },
    AttributeMismatch,
    FunctionNotAllowed {
        name: String,
    },
    ReferenceUnknown {
        id: String,
    },
    PluralCategory {
        key: String,
    },
    /// Not raised by `check_entry`: storing the entry would take the language's stored entries over
    /// 2,097,152 characters (LOC-LF-2a).
    LanguageFull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OfferedLocale {
    pub locale: String,
    pub native_name: String,
    pub direction: TextDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CatalogueText {
    pub locale: String,
    pub direction: TextDirection,
    /// One entry per tag of the chain, first = `locale`; `ftl` is the concatenated source.
    pub resources: Vec<CatalogueResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CatalogueResource {
    pub locale: String,
    pub ftl: String,
}

impl EntryProblem {
    /// The catalogue message id of the problem's sentence: `language-problem-<kebab of the variant>`.
    pub fn sentence_id(&self) -> &'static str {
        match self {
            EntryProblem::TooLong => "language-problem-too-long",
            EntryProblem::ForbiddenCharacter => "language-problem-forbidden-character",
            EntryProblem::DoesNotParse => "language-problem-does-not-parse",
            EntryProblem::NotOneMessage => "language-problem-not-one-message",
            EntryProblem::UnknownMessage => "language-problem-unknown-message",
            EntryProblem::IdMismatch => "language-problem-id-mismatch",
            EntryProblem::VariableMissing { .. } => "language-problem-variable-missing",
            EntryProblem::VariableUnknown { .. } => "language-problem-variable-unknown",
            EntryProblem::AttributeMismatch => "language-problem-attribute-mismatch",
            EntryProblem::FunctionNotAllowed { .. } => "language-problem-function-not-allowed",
            EntryProblem::ReferenceUnknown { .. } => "language-problem-reference-unknown",
            EntryProblem::PluralCategory { .. } => "language-problem-plural-category",
            EntryProblem::LanguageFull => "language-problem-language-full",
        }
    }

    /// The kind as a lowercase word, for log lines (never the entry's text).
    pub fn kind(&self) -> &'static str {
        match self {
            EntryProblem::TooLong => "too_long",
            EntryProblem::ForbiddenCharacter => "forbidden_character",
            EntryProblem::DoesNotParse => "does_not_parse",
            EntryProblem::NotOneMessage => "not_one_message",
            EntryProblem::UnknownMessage => "unknown_message",
            EntryProblem::IdMismatch => "id_mismatch",
            EntryProblem::VariableMissing { .. } => "variable_missing",
            EntryProblem::VariableUnknown { .. } => "variable_unknown",
            EntryProblem::AttributeMismatch => "attribute_mismatch",
            EntryProblem::FunctionNotAllowed { .. } => "function_not_allowed",
            EntryProblem::ReferenceUnknown { .. } => "reference_unknown",
            EntryProblem::PluralCategory { .. } => "plural_category",
            EntryProblem::LanguageFull => "language_full",
        }
    }
}
