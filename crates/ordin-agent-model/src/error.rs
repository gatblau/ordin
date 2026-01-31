//! The Agent Model's errors (AgentModel.md *Errors* and *Error Table*).
//!
//! Every error is a returned value of an in-process API; none reaches the person directly, and the
//! caller maps it to its own surface. `Display` is the bare code, so a caller logging the error never
//! writes file content.

use ordin_types::domain::agents::{PartKind, VocabularyAxis};
use ordin_types::CodedError;

/// Every code this crate owns (ErrorHandling.md *Internal Logic* 11): the `AmError` codes and the
/// three `PartSourceError` codes.
pub const ALL_CODES: &[&str] = &[
    "AM_API_VERSION_UNKNOWN",
    "AM_DIGEST_INVALID",
    "AM_DRAFT_FIELDS_INVALID",
    "AM_DRAFT_KIND_NOT_COMPOSABLE",
    "AM_FIELD_SHAPE_INVALID",
    "AM_FILE_TOO_LARGE",
    "AM_ID_INVALID",
    "AM_KIND_UNKNOWN",
    "AM_LOCK_FILE_INVALID",
    "AM_MANIFEST_FIELD_MISSING",
    "AM_MANIFEST_MALFORMED",
    "AM_NO_FILE_CONVENTION",
    "AM_PATH_INVALID",
    "AM_SOURCE_CORRUPT",
    "AM_SOURCE_UNAVAILABLE",
    "AM_SOURCE_WITHDRAWN_LIST_UNREADABLE",
    "AM_STATUS_UNKNOWN",
    "AM_VERSION_INVALID",
    "AM_VOCABULARY_AXIS_DUPLICATE",
    "AM_WITHDRAWN_LIST_INVALID",
    "AM_YAML_FEATURE_REFUSED",
];

/// Every failure of the pure functions of this crate. None is retryable: each is a property of the
/// input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AmError {
    #[error("AM_MANIFEST_MALFORMED")]
    ManifestMalformed { path: String, line: Option<u32> },
    #[error("AM_MANIFEST_FIELD_MISSING")]
    ManifestFieldMissing {
        path: String,
        fields: Vec<&'static str>,
    },
    #[error("AM_API_VERSION_UNKNOWN")]
    ApiVersionUnknown { path: String, found: String },
    #[error("AM_KIND_UNKNOWN")]
    KindUnknown { path: String, authored: String },
    #[error("AM_YAML_FEATURE_REFUSED")]
    YamlFeatureRefused { path: String, feature: &'static str },
    #[error("AM_FILE_TOO_LARGE")]
    FileTooLarge { path: String, bytes: u64 },
    #[error("AM_PATH_INVALID")]
    PathInvalid { path: String },
    #[error("AM_ID_INVALID")]
    IdInvalid { id: String },
    #[error("AM_VERSION_INVALID")]
    VersionInvalid { version: String },
    #[error("AM_STATUS_UNKNOWN")]
    StatusUnknown { status: String },
    #[error("AM_FIELD_SHAPE_INVALID")]
    FieldShapeInvalid { key: String, expected: &'static str },
    #[error("AM_NO_FILE_CONVENTION")]
    NoFileConvention { kind: PartKind },
    #[error("AM_VOCABULARY_AXIS_DUPLICATE")]
    VocabularyAxisDuplicate { axis: VocabularyAxis },
    #[error("AM_DIGEST_INVALID")]
    DigestInvalid { value: String },
    #[error("AM_LOCK_FILE_INVALID")]
    LockFileInvalid { reason: &'static str },
    #[error("AM_WITHDRAWN_LIST_INVALID")]
    WithdrawnListInvalid {
        reason: &'static str,
        entry: Option<u32>,
    },
    #[error("AM_DRAFT_FIELDS_INVALID")]
    DraftFieldsInvalid {
        field: String,
        expected: &'static str,
    },
    #[error("AM_DRAFT_KIND_NOT_COMPOSABLE")]
    DraftKindNotComposable { kind: PartKind },
}

impl AmError {
    /// The registry code; equal to the `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            AmError::ManifestMalformed { .. } => "AM_MANIFEST_MALFORMED",
            AmError::ManifestFieldMissing { .. } => "AM_MANIFEST_FIELD_MISSING",
            AmError::ApiVersionUnknown { .. } => "AM_API_VERSION_UNKNOWN",
            AmError::KindUnknown { .. } => "AM_KIND_UNKNOWN",
            AmError::YamlFeatureRefused { .. } => "AM_YAML_FEATURE_REFUSED",
            AmError::FileTooLarge { .. } => "AM_FILE_TOO_LARGE",
            AmError::PathInvalid { .. } => "AM_PATH_INVALID",
            AmError::IdInvalid { .. } => "AM_ID_INVALID",
            AmError::VersionInvalid { .. } => "AM_VERSION_INVALID",
            AmError::StatusUnknown { .. } => "AM_STATUS_UNKNOWN",
            AmError::FieldShapeInvalid { .. } => "AM_FIELD_SHAPE_INVALID",
            AmError::NoFileConvention { .. } => "AM_NO_FILE_CONVENTION",
            AmError::VocabularyAxisDuplicate { .. } => "AM_VOCABULARY_AXIS_DUPLICATE",
            AmError::DigestInvalid { .. } => "AM_DIGEST_INVALID",
            AmError::LockFileInvalid { .. } => "AM_LOCK_FILE_INVALID",
            AmError::WithdrawnListInvalid { .. } => "AM_WITHDRAWN_LIST_INVALID",
            AmError::DraftFieldsInvalid { .. } => "AM_DRAFT_FIELDS_INVALID",
            AmError::DraftKindNotComposable { .. } => "AM_DRAFT_KIND_NOT_COMPOSABLE",
        }
    }
}

/// Every row of the Error Table for `AmError` is `Err(permanent)`.
impl CodedError for AmError {
    fn code(&self) -> &'static str {
        AmError::code(self)
    }
    fn retryable(&self) -> bool {
        false
    }
}

/// The failures a `PartSource` implementation reports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PartSourceError {
    /// The clone or index cannot be read now (disk, lock). Retryable.
    #[error("AM_SOURCE_UNAVAILABLE")]
    Unavailable { retryable: bool },
    /// The repository's withdrawn list could not be read and no earlier copy exists (R-78).
    #[error("AM_SOURCE_WITHDRAWN_LIST_UNREADABLE")]
    WithdrawnListUnreadable { repository: String },
    /// The source holds a row whose files no longer exist at the recorded commit.
    #[error("AM_SOURCE_CORRUPT")]
    Corrupt { part_identifier: String },
}

impl PartSourceError {
    /// The registry code; equal to the `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            PartSourceError::Unavailable { .. } => "AM_SOURCE_UNAVAILABLE",
            PartSourceError::WithdrawnListUnreadable { .. } => {
                "AM_SOURCE_WITHDRAWN_LIST_UNREADABLE"
            }
            PartSourceError::Corrupt { .. } => "AM_SOURCE_CORRUPT",
        }
    }
}

/// `Unavailable` carries its own retry flag (the Error Table row is `Err(retryable)`); the other two
/// are permanent.
impl CodedError for PartSourceError {
    fn code(&self) -> &'static str {
        PartSourceError::code(self)
    }
    fn retryable(&self) -> bool {
        match self {
            PartSourceError::Unavailable { retryable } => *retryable,
            PartSourceError::WithdrawnListUnreadable { .. } | PartSourceError::Corrupt { .. } => {
                false
            }
        }
    }
}
