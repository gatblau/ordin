//! The Localisation error type and the codes it owns (Localisation.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

use crate::types::EntryProblem;

/// Every code the registry gives this crate.
///
/// Three of them are owned here but never returned to a caller: `LOC_STORE_CORRUPT` (recovered at
/// start, logged only), `LOC_FORMAT_ERROR` (recovered, logged only) and `LOC_BUILD_CATALOGUE_INVALID`
/// (raised by `build.rs`, so the build fails).
pub const ALL_CODES: &[&str] = &[
    "LOC_BUILD_CATALOGUE_INVALID",
    "LOC_CATALOGUE_MISSING",
    "LOC_FILE_TOO_LARGE",
    "LOC_FILE_UNREADABLE",
    "LOC_FORMAT_ERROR",
    "LOC_LANGUAGES_UNAVAILABLE",
    "LOC_LANGUAGE_ACTIVE",
    "LOC_LANGUAGE_DRAFTING",
    "LOC_LANGUAGE_EXISTS",
    "LOC_LANGUAGE_NOT_CONFIRMED",
    "LOC_LANGUAGE_NOT_EDITABLE",
    "LOC_LANGUAGE_UNKNOWN",
    "LOC_MESSAGE_REFUSED",
    "LOC_MESSAGE_UNKNOWN",
    "LOC_NOT_OFFERED",
    "LOC_NOT_SHIPPED",
    "LOC_REVIEW_INCOMPLETE",
    "LOC_STORE_CORRUPT",
    "LOC_STORE_UNAVAILABLE",
    "LOC_TAG_INVALID",
    "LOC_TOO_MANY_LANGUAGES",
];

/// Every failure a caller can see. `Display` is the short phrase of the spec and never carries a
/// value from the caller or the datastore.
#[derive(Debug, thiserror::Error)]
pub enum LocalisationError {
    #[error("locale tag is not valid BCP 47")]
    TagInvalid, // LOC_TAG_INVALID
    #[error("locale is not offered")]
    NotOffered, // LOC_NOT_OFFERED
    #[error("no bundled catalogue for the locale")]
    CatalogueMissing, // LOC_CATALOGUE_MISSING
    #[error("preference store unavailable")]
    StoreUnavailable(#[source] sqlx::Error), // LOC_STORE_UNAVAILABLE
    #[error("preference row unreadable")]
    StoreCorrupt, // LOC_STORE_CORRUPT
    // Amended January 2026 (ADR-109):
    #[error("languages cannot be changed here")]
    LanguagesUnavailable, // LOC_LANGUAGES_UNAVAILABLE
    #[error("language is not editable")]
    LanguageNotEditable, // LOC_LANGUAGE_NOT_EDITABLE
    #[error("language unknown")]
    LanguageUnknown, // LOC_LANGUAGE_UNKNOWN
    #[error("message unknown")]
    MessageUnknown, // LOC_MESSAGE_UNKNOWN
    #[error("cursor unreadable")]
    BadCursor, // SRV_BAD_CURSOR, owned by `ordin-types`
    #[error("message refused")]
    MessageRefused {
        message_id: String,
        problem: EntryProblem,
    }, // LOC_MESSAGE_REFUSED
    #[error("language is not shipped")]
    NotShipped, // LOC_NOT_SHIPPED
    #[error("file is not a language file")]
    FileUnreadable, // LOC_FILE_UNREADABLE
    #[error("file too large")]
    FileTooLarge, // LOC_FILE_TOO_LARGE
    #[error("language exists")]
    LanguageExists, // LOC_LANGUAGE_EXISTS
    #[error("language is being drafted")]
    LanguageDrafting, // LOC_LANGUAGE_DRAFTING
    #[error("review incomplete")]
    ReviewIncomplete { count: u32 }, // LOC_REVIEW_INCOMPLETE
    #[error("language not confirmed")]
    LanguageNotConfirmed, // LOC_LANGUAGE_NOT_CONFIRMED
    #[error("language is active")]
    LanguageActive, // LOC_LANGUAGE_ACTIVE
    #[error("too many languages")]
    TooManyLanguages, // LOC_TOO_MANY_LANGUAGES
}

/// The one error contract of module errors (2C `CodedError`).
impl CodedError for LocalisationError {
    fn code(&self) -> &'static str {
        match self {
            LocalisationError::TagInvalid => "LOC_TAG_INVALID",
            LocalisationError::NotOffered => "LOC_NOT_OFFERED",
            LocalisationError::CatalogueMissing => "LOC_CATALOGUE_MISSING",
            LocalisationError::StoreUnavailable(_) => "LOC_STORE_UNAVAILABLE",
            LocalisationError::StoreCorrupt => "LOC_STORE_CORRUPT",
            LocalisationError::LanguagesUnavailable => "LOC_LANGUAGES_UNAVAILABLE",
            LocalisationError::LanguageNotEditable => "LOC_LANGUAGE_NOT_EDITABLE",
            LocalisationError::LanguageUnknown => "LOC_LANGUAGE_UNKNOWN",
            LocalisationError::MessageUnknown => "LOC_MESSAGE_UNKNOWN",
            LocalisationError::BadCursor => "SRV_BAD_CURSOR",
            LocalisationError::MessageRefused { .. } => "LOC_MESSAGE_REFUSED",
            LocalisationError::NotShipped => "LOC_NOT_SHIPPED",
            LocalisationError::FileUnreadable => "LOC_FILE_UNREADABLE",
            LocalisationError::FileTooLarge => "LOC_FILE_TOO_LARGE",
            LocalisationError::LanguageExists => "LOC_LANGUAGE_EXISTS",
            LocalisationError::LanguageDrafting => "LOC_LANGUAGE_DRAFTING",
            LocalisationError::ReviewIncomplete { .. } => "LOC_REVIEW_INCOMPLETE",
            LocalisationError::LanguageNotConfirmed => "LOC_LANGUAGE_NOT_CONFIRMED",
            LocalisationError::LanguageActive => "LOC_LANGUAGE_ACTIVE",
            LocalisationError::TooManyLanguages => "LOC_TOO_MANY_LANGUAGES",
        }
    }

    /// `true` only for `StoreUnavailable`.
    fn retryable(&self) -> bool {
        matches!(self, LocalisationError::StoreUnavailable(_))
    }

    /// `Some(TryAgain)` for `StoreUnavailable`; every other variant is permanent with no repair.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            LocalisationError::StoreUnavailable(_) => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for LocalisationError {
    fn from(error: sqlx::Error) -> Self {
        LocalisationError::StoreUnavailable(error)
    }
}
