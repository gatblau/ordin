//! The errors of the Knowledge Service (*Public Interface* § Errors and its amendments).

use ordin_types::{CodedError, RepairAction};

/// Every `KNOW_` code this crate owns (X4; ErrorHandling.md *Internal Logic* 11), sorted. The
/// `okf` module's four internal codes and the two argument-rule codes (`KNOW_PAGE_ID_INVALID`,
/// `KNOW_QUERY_INVALID` as a rule) are among them; `SRV_BAD_CURSOR` is owned by `ordin-types`.
pub const ALL_CODES: &[&str] = &[
    "KNOW_ADOPT_PERSONAL_DATA",
    "KNOW_ADOPT_REFUSED",
    "KNOW_AGENT_PAGE_NOT_FOUND",
    "KNOW_ALIAS_INVALID",
    "KNOW_BAD_PATTERN",
    "KNOW_BODY_TOO_LARGE",
    "KNOW_CHECKPOINT_INVALID",
    "KNOW_CONCEPT_INVALID",
    "KNOW_DB_UNAVAILABLE",
    "KNOW_DESCRIPTION_INVALID",
    "KNOW_ENTRY_INVALID",
    "KNOW_ENTRY_NOT_FOUND",
    "KNOW_FILE_UNAVAILABLE",
    "KNOW_HEADER_INVALID",
    "KNOW_HISTORY_UNAVAILABLE",
    "KNOW_LANGUAGE_UNKNOWN",
    "KNOW_NOTE_EMPTY",
    "KNOW_NOTE_LIMIT",
    "KNOW_NOTE_NOT_FOUND",
    "KNOW_NOTE_TOO_LONG",
    "KNOW_NOT_OUT_OF_DATE",
    "KNOW_NOT_REMOVED",
    "KNOW_NO_NOTES_SOURCE",
    "KNOW_NO_PENDING_UPDATE",
    "KNOW_OKF_HEADER_INVALID",
    "KNOW_OKF_NOT_UTF8",
    "KNOW_OKF_RESERVED",
    "KNOW_OKF_TOO_LARGE",
    "KNOW_PAGE_ID_INVALID",
    "KNOW_PAGE_LIMIT",
    "KNOW_PAGE_NOT_FOUND",
    "KNOW_PAGE_REMOVED",
    "KNOW_PROFILE_MISMATCH",
    "KNOW_PUBLISH_FAILED",
    "KNOW_QUERY_INVALID",
    "KNOW_READ_ONLY",
    "KNOW_SCOPE_NOT_ENABLED",
    "KNOW_SOURCES_TOO_MANY",
    "KNOW_SOURCE_INVALID",
    "KNOW_SUGGESTION_NOT_FOUND",
    "KNOW_SUGGESTION_STALE",
    "KNOW_TAG_INVALID",
    "KNOW_TEAM_PAGE_ELSEWHERE",
    "KNOW_TEAM_PAGE_INVALID",
    "KNOW_TITLE_DUPLICATE",
    "KNOW_TITLE_INVALID",
    "KNOW_TYPE_INVALID",
    "KNOW_UNKNOWN_KIND",
    "KNOW_UNKNOWN_SOURCE",
    "KNOW_WORKSPACE_NOT_FOUND",
];

/// `Display` is the code alone: it never carries a title, a body, a query or a path.
#[derive(Debug, thiserror::Error)]
pub enum KnowledgeError {
    #[error("KNOW_TITLE_INVALID")]
    TitleInvalid,
    #[error("KNOW_TITLE_DUPLICATE")]
    TitleDuplicate,
    #[error("KNOW_BODY_TOO_LARGE")]
    BodyTooLarge,
    #[error("KNOW_SOURCES_TOO_MANY")]
    SourcesTooMany,
    #[error("KNOW_SOURCE_INVALID")]
    SourceInvalid,
    #[error("KNOW_PAGE_LIMIT")]
    PageLimit,
    #[error("KNOW_PAGE_NOT_FOUND")]
    PageNotFound,
    #[error("KNOW_PAGE_REMOVED")]
    PageRemoved,
    #[error("KNOW_NOT_REMOVED")]
    NotRemoved,
    #[error("KNOW_WORKSPACE_NOT_FOUND")]
    WorkspaceNotFound,
    #[error("KNOW_NO_PENDING_UPDATE")]
    NoPendingUpdate,
    #[error("KNOW_NOT_OUT_OF_DATE")]
    NotOutOfDate,
    #[error("KNOW_FILE_UNAVAILABLE")]
    FileUnavailable,
    #[error("KNOW_HEADER_INVALID")]
    HeaderInvalid,
    #[error("KNOW_AGENT_PAGE_NOT_FOUND")]
    AgentPageNotFound,
    #[error("KNOW_PUBLISH_FAILED")]
    PublishFailed,
    #[error("KNOW_PROFILE_MISMATCH")]
    ProfileMismatch,
    // wrightrun codes, carried unchanged
    #[error("KNOW_UNKNOWN_SOURCE")]
    UnknownSource,
    #[error("KNOW_UNKNOWN_KIND")]
    UnknownKind,
    #[error("KNOW_BAD_PATTERN")]
    BadPattern { at: String },
    #[error("KNOW_LANGUAGE_UNKNOWN")]
    LanguageUnknown { language: String },
    #[error("KNOW_NOTE_LIMIT")]
    NoteLimit { max: u32 },
    #[error("KNOW_NOTE_TOO_LONG")]
    NoteTooLong { max: u32, len: u32 },
    #[error("KNOW_NOTE_EMPTY")]
    NoteEmpty,
    #[error("KNOW_NOTE_NOT_FOUND")]
    NoteNotFound,
    #[error("KNOW_NO_NOTES_SOURCE")]
    NoNotesSource,
    #[error("KNOW_ENTRY_INVALID")]
    EntryInvalid,
    #[error("KNOW_ENTRY_NOT_FOUND")]
    EntryNotFound,
    #[error("KNOW_DB_UNAVAILABLE")]
    DbUnavailable,
    #[error("SRV_BAD_CURSOR")]
    BadCursor,
    // ingest (SA-RECON-01; code named by the Job Runner)
    #[error("KNOW_SCOPE_NOT_ENABLED")]
    ScopeNotEnabled,
    #[error("KNOW_QUERY_INVALID")]
    QueryInvalid,
    #[error("KNOW_HISTORY_UNAVAILABLE")]
    HistoryUnavailable,
    #[error("KNOW_CHECKPOINT_INVALID")]
    CheckpointInvalid,
    // amended January 2026 (ADR-103)
    // Variant names follow their codes (KnowledgeService.md, SA-KBUILT-06).
    #[error("KNOW_TEAM_PAGE_INVALID")]
    TeamPageInvalid,
    // amended January 2026 (ADR-104)
    #[error("KNOW_TAG_INVALID")]
    TagInvalid,
    #[error("KNOW_ALIAS_INVALID")]
    AliasInvalid,
    #[error("KNOW_SUGGESTION_NOT_FOUND")]
    SuggestionNotFound,
    #[error("KNOW_SUGGESTION_STALE")]
    SuggestionStale,
    // amended January 2026 (ADR-110)
    #[error("KNOW_TEAM_PAGE_ELSEWHERE")]
    TeamPageElsewhere,
    #[error("KNOW_READ_ONLY")]
    ReadOnly,
    #[error("KNOW_ADOPT_PERSONAL_DATA")]
    AdoptPersonalData,
    #[error("KNOW_ADOPT_REFUSED")]
    AdoptRefused { retryable: bool },
    // amended January 2026 (ADR-112)
    #[error("KNOW_TYPE_INVALID")]
    TypeInvalid,
    #[error("KNOW_DESCRIPTION_INVALID")]
    DescriptionInvalid,
    #[error("KNOW_CONCEPT_INVALID")]
    ConceptInvalid,
}

impl KnowledgeError {
    /// The code above, as a static string.
    pub fn code(&self) -> &'static str {
        use KnowledgeError as E;
        match self {
            E::TitleInvalid => "KNOW_TITLE_INVALID",
            E::TitleDuplicate => "KNOW_TITLE_DUPLICATE",
            E::BodyTooLarge => "KNOW_BODY_TOO_LARGE",
            E::SourcesTooMany => "KNOW_SOURCES_TOO_MANY",
            E::SourceInvalid => "KNOW_SOURCE_INVALID",
            E::PageLimit => "KNOW_PAGE_LIMIT",
            E::PageNotFound => "KNOW_PAGE_NOT_FOUND",
            E::PageRemoved => "KNOW_PAGE_REMOVED",
            E::NotRemoved => "KNOW_NOT_REMOVED",
            E::WorkspaceNotFound => "KNOW_WORKSPACE_NOT_FOUND",
            E::NoPendingUpdate => "KNOW_NO_PENDING_UPDATE",
            E::NotOutOfDate => "KNOW_NOT_OUT_OF_DATE",
            E::FileUnavailable => "KNOW_FILE_UNAVAILABLE",
            E::HeaderInvalid => "KNOW_HEADER_INVALID",
            E::AgentPageNotFound => "KNOW_AGENT_PAGE_NOT_FOUND",
            E::PublishFailed => "KNOW_PUBLISH_FAILED",
            E::ProfileMismatch => "KNOW_PROFILE_MISMATCH",
            E::UnknownSource => "KNOW_UNKNOWN_SOURCE",
            E::UnknownKind => "KNOW_UNKNOWN_KIND",
            E::BadPattern { .. } => "KNOW_BAD_PATTERN",
            E::LanguageUnknown { .. } => "KNOW_LANGUAGE_UNKNOWN",
            E::NoteLimit { .. } => "KNOW_NOTE_LIMIT",
            E::NoteTooLong { .. } => "KNOW_NOTE_TOO_LONG",
            E::NoteEmpty => "KNOW_NOTE_EMPTY",
            E::NoteNotFound => "KNOW_NOTE_NOT_FOUND",
            E::NoNotesSource => "KNOW_NO_NOTES_SOURCE",
            E::EntryInvalid => "KNOW_ENTRY_INVALID",
            E::EntryNotFound => "KNOW_ENTRY_NOT_FOUND",
            E::DbUnavailable => "KNOW_DB_UNAVAILABLE",
            E::BadCursor => "SRV_BAD_CURSOR",
            E::ScopeNotEnabled => "KNOW_SCOPE_NOT_ENABLED",
            E::QueryInvalid => "KNOW_QUERY_INVALID",
            E::HistoryUnavailable => "KNOW_HISTORY_UNAVAILABLE",
            E::CheckpointInvalid => "KNOW_CHECKPOINT_INVALID",
            E::TeamPageInvalid => "KNOW_TEAM_PAGE_INVALID",
            E::TagInvalid => "KNOW_TAG_INVALID",
            E::AliasInvalid => "KNOW_ALIAS_INVALID",
            E::SuggestionNotFound => "KNOW_SUGGESTION_NOT_FOUND",
            E::SuggestionStale => "KNOW_SUGGESTION_STALE",
            E::TeamPageElsewhere => "KNOW_TEAM_PAGE_ELSEWHERE",
            E::ReadOnly => "KNOW_READ_ONLY",
            E::AdoptPersonalData => "KNOW_ADOPT_PERSONAL_DATA",
            E::AdoptRefused { .. } => "KNOW_ADOPT_REFUSED",
            E::TypeInvalid => "KNOW_TYPE_INVALID",
            E::DescriptionInvalid => "KNOW_DESCRIPTION_INVALID",
            E::ConceptInvalid => "KNOW_CONCEPT_INVALID",
        }
    }

    /// true for KNOW_DB_UNAVAILABLE, KNOW_FILE_UNAVAILABLE, KNOW_PUBLISH_FAILED and
    /// KNOW_HISTORY_UNAVAILABLE; KNOW_ADOPT_REFUSED carries its cause's retryability (ADR-110).
    pub fn retryable(&self) -> bool {
        match self {
            KnowledgeError::DbUnavailable
            | KnowledgeError::FileUnavailable
            | KnowledgeError::PublishFailed
            | KnowledgeError::HistoryUnavailable => true,
            KnowledgeError::AdoptRefused { retryable } => *retryable,
            _ => false,
        }
    }
}

impl CodedError for KnowledgeError {
    fn code(&self) -> &'static str {
        KnowledgeError::code(self)
    }

    fn retryable(&self) -> bool {
        KnowledgeError::retryable(self)
    }

    /// The `action` column of the Error Table: `try_again` for the two retryable Service API rows.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            KnowledgeError::FileUnavailable | KnowledgeError::PublishFailed => {
                Some(RepairAction::TryAgain)
            }
            _ => None,
        }
    }
}

/// Any statement failure on this module's tables is `KNOW_DB_UNAVAILABLE`; the error's class is
/// logged, never its text (which can quote a value).
impl From<sqlx::Error> for KnowledgeError {
    fn from(error: sqlx::Error) -> Self {
        tracing::warn!(
            component = "knowledge",
            error_class = db_class(&error),
            "knowledge_datastore_failed"
        );
        KnowledgeError::DbUnavailable
    }
}

pub(crate) fn db_class(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "pool",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::RowNotFound => "row_not_found",
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => "decode",
        _ => "other",
    }
}
