//! The Journal's one error type and the codes it owns (JournalAndArtefactStore.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction, TypesError};

/// Every code the registry gives this crate: its own `JRNL_` codes and the 2C entity codes of the
/// records it keeps (`ART_`, `EXT_`, `REPORT_`, `RSTEP_`, `START_`, `TREQ_`).
///
/// Four of them are owned here but never raised by this crate: `ART_DELETE_IN_PLACE` (the owner of
/// a place moves a file to the bin), `EXT_AUTO_RESEND` (nothing is resent, Internal Logic 9.3), and
/// `REPORT_ACTIVE_CONTENT` and `REPORT_LINK_OUTSIDE_WORKSPACE` (the Block Renderer's check, which runs
/// before a report reaches the Journal). `SRV_BAD_CURSOR` is raised but owned by `ordin-types`.
pub const ALL_CODES: &[&str] = &[
    "ART_DELETE_IN_PLACE",
    "ART_MEDIA_TYPE_INVALID",
    "ART_PATH_INVALID",
    "EXT_AUTO_RESEND",
    "EXT_DESTINATION_INVALID",
    "EXT_KEY_DUPLICATE",
    "JRNL_BACKUP_FAILED",
    "JRNL_DATASTORE_OPEN_FAILED",
    "JRNL_DATASTORE_UNAVAILABLE",
    "JRNL_ENTRY_INVALID",
    "JRNL_EVENT_KIND_INVALID",
    "JRNL_EVENT_TOO_LARGE",
    "JRNL_EXPORT_INVALID",
    "JRNL_EXT_STATE_INVALID",
    "JRNL_MIGRATION_CHANGED",
    "JRNL_MIGRATION_DUPLICATE",
    "JRNL_MIGRATION_FAILED",
    "JRNL_NOT_FOUND",
    "JRNL_PLAN_NOT_FOUND",
    "JRNL_RESUME_TOO_OLD",
    "JRNL_RETENTION_ABOVE_COMPANY_MAX",
    "JRNL_RETENTION_INVALID",
    "JRNL_SCHEMA_BEHIND",
    "JRNL_SCHEMA_NEWER",
    "JRNL_SEQ_AHEAD",
    "REPORT_ACTIVE_CONTENT",
    "REPORT_CHART_TOO_LARGE",
    "REPORT_CURRENCY_MISMATCH",
    "REPORT_DASHBOARD_NAME_DUPLICATE",
    "REPORT_FIGURES_OUT_OF_RANGE",
    "REPORT_LINK_OUTSIDE_WORKSPACE",
    "REPORT_PARTS_OUT_OF_RANGE",
    "REPORT_PART_NO_SOURCE",
    "REPORT_PIE_TOO_MANY_SLICES",
    "REPORT_TABLE_TOO_LARGE",
    "REPORT_TITLE_INVALID",
    "REPORT_TOO_LARGE",
    "RSTEP_IMMUTABLE",
    "RSTEP_LINE_TOO_LONG",
    "RSTEP_PLAN_SIZE_OUT_OF_RANGE",
    "RSTEP_PLAN_VERSION_GAP",
    "START_DUPLICATE",
    "START_LIST_TOO_LONG",
    "START_REASON_MISSING",
    "TREQ_APPROVAL_MISMATCH",
    "TREQ_IMMUTABLE",
    "TREQ_NAME_INVALID",
    "TREQ_UNDECIDED",
];

/// Every failure of this crate. `Display` is the registry code and never carries a value from the
/// caller or the datastore, so logging an error leaks nothing.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// A 2C validation code, permanent.
    #[error("{code}")]
    Invalid { code: &'static str },
    #[error("JRNL_NOT_FOUND")]
    NotFound,
    /// Retryable.
    #[error("JRNL_DATASTORE_UNAVAILABLE")]
    Unavailable,
    #[error("JRNL_DATASTORE_OPEN_FAILED")]
    OpenFailed,
    #[error("JRNL_SCHEMA_NEWER")]
    SchemaNewer {
        module: String,
        found: i64,
        known: i64,
    },
    #[error("JRNL_SCHEMA_BEHIND")]
    SchemaBehind { pending: Vec<(String, i64)> },
    #[error("JRNL_MIGRATION_DUPLICATE")]
    MigrationDuplicate { module: String },
    #[error("JRNL_MIGRATION_CHANGED")]
    MigrationChanged { module: String, version: i64 },
    #[error("JRNL_MIGRATION_FAILED")]
    MigrationFailed { module: String, version: i64 },
    #[error("JRNL_BACKUP_FAILED")]
    BackupFailed,
    #[error("JRNL_EVENT_KIND_INVALID")]
    EventKindInvalid,
    #[error("JRNL_EVENT_TOO_LARGE")]
    EventTooLarge { bytes: usize },
    #[error("JRNL_ENTRY_INVALID")]
    EntryInvalid,
    #[error("JRNL_RESUME_TOO_OLD")]
    ResumeTooOld { first_kept_seq: u64 },
    #[error("JRNL_SEQ_AHEAD")]
    SeqAhead { last_seq: u64 },
    #[error("JRNL_RETENTION_INVALID")]
    RetentionInvalid,
    #[error("JRNL_RETENTION_ABOVE_COMPANY_MAX")]
    RetentionAboveCompanyMax { max_days: u32 },
    #[error("JRNL_EXT_STATE_INVALID")]
    ExternalActionState,
    #[error("JRNL_PLAN_NOT_FOUND")]
    PlanNotFound,
    #[error("JRNL_EXPORT_INVALID")]
    ExportInvalid,
    #[error("SRV_BAD_CURSOR")]
    BadCursor,
}

/// The one error contract of every module error (2C `CodedError`).
impl CodedError for JournalError {
    fn code(&self) -> &'static str {
        match self {
            JournalError::Invalid { code } => code,
            JournalError::NotFound => "JRNL_NOT_FOUND",
            JournalError::Unavailable => "JRNL_DATASTORE_UNAVAILABLE",
            JournalError::OpenFailed => "JRNL_DATASTORE_OPEN_FAILED",
            JournalError::SchemaNewer { .. } => "JRNL_SCHEMA_NEWER",
            JournalError::SchemaBehind { .. } => "JRNL_SCHEMA_BEHIND",
            JournalError::MigrationDuplicate { .. } => "JRNL_MIGRATION_DUPLICATE",
            JournalError::MigrationChanged { .. } => "JRNL_MIGRATION_CHANGED",
            JournalError::MigrationFailed { .. } => "JRNL_MIGRATION_FAILED",
            JournalError::BackupFailed => "JRNL_BACKUP_FAILED",
            JournalError::EventKindInvalid => "JRNL_EVENT_KIND_INVALID",
            JournalError::EventTooLarge { .. } => "JRNL_EVENT_TOO_LARGE",
            JournalError::EntryInvalid => "JRNL_ENTRY_INVALID",
            JournalError::ResumeTooOld { .. } => "JRNL_RESUME_TOO_OLD",
            JournalError::SeqAhead { .. } => "JRNL_SEQ_AHEAD",
            JournalError::RetentionInvalid => "JRNL_RETENTION_INVALID",
            JournalError::RetentionAboveCompanyMax { .. } => "JRNL_RETENTION_ABOVE_COMPANY_MAX",
            JournalError::ExternalActionState => "JRNL_EXT_STATE_INVALID",
            JournalError::PlanNotFound => "JRNL_PLAN_NOT_FOUND",
            JournalError::ExportInvalid => "JRNL_EXPORT_INVALID",
            JournalError::BadCursor => "SRV_BAD_CURSOR",
        }
    }

    /// `true` only for `Unavailable`.
    fn retryable(&self) -> bool {
        matches!(self, JournalError::Unavailable)
    }

    fn repair(&self) -> Option<RepairAction> {
        match self {
            JournalError::Unavailable | JournalError::BackupFailed => Some(RepairAction::TryAgain),
            JournalError::OpenFailed
            | JournalError::SchemaNewer { .. }
            | JournalError::MigrationChanged { .. }
            | JournalError::MigrationFailed { .. } => Some(RepairAction::SendToIt),
            _ => None,
        }
    }
}

impl JournalError {
    /// A 2C rule refused a value; `code` is that rule's registry code.
    pub(crate) fn invalid(code: &'static str) -> JournalError {
        JournalError::Invalid { code }
    }
}

/// A shared-type check refused a value: a cursor becomes `SRV_BAD_CURSOR`, anything else keeps its
/// own registry code (`HASH_NOT_SHA256_HEX`, `INPUT_NAME_INVALID`, …).
impl From<TypesError> for JournalError {
    fn from(error: TypesError) -> Self {
        match error {
            TypesError::BadCursor => JournalError::BadCursor,
            other => JournalError::Invalid { code: other.code() },
        }
    }
}

/// Any datastore failure is `JRNL_DATASTORE_UNAVAILABLE` (retryable). The sqlx error is dropped:
/// its text can hold a value or, for PostgreSQL, part of the connection string.
impl From<sqlx::Error> for JournalError {
    fn from(_: sqlx::Error) -> Self {
        JournalError::Unavailable
    }
}
