//! The File Service's one error type and the codes it owns (FileService.md *Errors*, *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::domain::places::PublishedCopyState;
use ordin_types::num::Sha256Hex;
use ordin_types::{CodedError, RepairAction};

use crate::PlaceId;

/// Every code the registry gives this crate (its `FILE_`, `FVER_`, `PLACE_`, `PUB_` and `WF_`
/// rows), sorted. `FILE_AGENT_REFUSED` is a tool failure built by the Agent Loader from an
/// `AgentRefusal`, so no `FileError` variant carries it.
pub const ALL_CODES: &[&str] = &[
    "FILE_AGENT_REFUSED",
    "FILE_BIN_UNAVAILABLE",
    "FILE_CHANGED_MEANWHILE",
    "FILE_COPY_NOT_FOUND",
    "FILE_COPY_TRANSITION_INVALID",
    "FILE_DATASTORE_UNAVAILABLE",
    "FILE_FOLDER_UNREADABLE",
    "FILE_GRANT_HANDOFF_FAILED",
    "FILE_INCOMING_REFUSED",
    "FILE_MOVE_TARGET_EXISTS",
    "FILE_NOT_FOUND",
    "FILE_NOT_GRANTED",
    "FILE_PATH_ESCAPES_PLACE",
    "FILE_PLACE_EXISTS",
    "FILE_PLACE_NOT_FOUND",
    "FILE_PROTECTED_LOCATION",
    "FILE_QUERY_INVALID",
    "FILE_RESTORE_TARGET_EXISTS",
    "FILE_TOO_LARGE",
    "FILE_VERSION_COPY_MISSING",
    "FILE_VERSION_NOT_FOUND",
    "FILE_WATCH_FAILED",
    "FILE_WRITE_FAILED",
    "FVER_LATEST_PRUNE",
    "FVER_PATH_INVALID",
    "FVER_VERSION_GAP",
    "PLACE_NAME_INVALID",
    "PLACE_PATH_INVALID",
    "PUB_AUDIENCE_INVALID",
    "PUB_BRANCH_UNEXPECTED",
    "PUB_FAILURE_REASON_MISMATCH",
    "PUB_FORMAT_DESTINATION_MISMATCH",
    "PUB_FORMAT_MAKER_MISMATCH",
    "WF_BUNDLE_PATH",
    "WF_INVALID_NAMESPACE",
    "WF_INVALID_PATH",
    "WF_NOT_FOUND",
    "WF_STORE_UNAVAILABLE",
    "WF_TOO_LARGE",
];

/// Every failure of this crate. `Display` is the code alone: it never carries a path, a name or
/// file content.
// spec: ordin-desktop/components/FileService#SA-ERR-01
#[derive(Debug, Clone, thiserror::Error)]
pub enum FileError {
    #[error("FILE_PLACE_NOT_FOUND")]
    PlaceNotFound,
    #[error("FILE_NOT_FOUND")]
    NotFound,
    #[error("FILE_PROTECTED_LOCATION")]
    ProtectedLocation,
    #[error("FILE_FOLDER_UNREADABLE")]
    FolderUnreadable,
    #[error("FILE_PLACE_EXISTS")]
    PlaceExists { existing: PlaceId },
    #[error("FILE_PATH_ESCAPES_PLACE")]
    PathEscapesPlace,
    #[error("FILE_TOO_LARGE")]
    TooLarge { limit_bytes: u64 },
    #[error("FILE_CHANGED_MEANWHILE")]
    ChangedMeanwhile { current: Sha256Hex },
    #[error("FILE_WRITE_FAILED")]
    WriteFailed,
    #[error("FILE_BIN_UNAVAILABLE")]
    BinUnavailable,
    #[error("FILE_VERSION_NOT_FOUND")]
    VersionNotFound,
    #[error("FILE_VERSION_COPY_MISSING")]
    VersionCopyMissing,
    #[error("FILE_COPY_NOT_FOUND")]
    CopyNotFound,
    #[error("FILE_COPY_TRANSITION_INVALID")]
    CopyTransitionInvalid {
        from: PublishedCopyState,
        to: PublishedCopyState,
    },
    #[error("FILE_QUERY_INVALID")]
    QueryInvalid,
    #[error("FILE_GRANT_HANDOFF_FAILED")]
    GrantHandoffFailed,
    #[error("FILE_WATCH_FAILED")]
    WatchFailed,
    #[error("FILE_DATASTORE_UNAVAILABLE")]
    DatastoreUnavailable,
    #[error("FILE_NOT_GRANTED")]
    NotGranted,
    #[error("FILE_INCOMING_REFUSED")]
    IncomingRefused { reason: String },
    #[error("FILE_RESTORE_TARGET_EXISTS")]
    RestoreTargetExists,
    /// ADR-104 (Amendment January 2026 — Moving a file).
    #[error("FILE_MOVE_TARGET_EXISTS")]
    MoveTargetExists,
    /// A 2C validation code (`PLACE_*`, `FVER_*`, `PUB_*`), or `SRV_BAD_CURSOR` for a cursor the
    /// shared paging helper refused, carried as its string.
    #[error("{0}")]
    Invalid(&'static str),
    /// Work folder (WR-FileTree codes); they keep wrightrun's names.
    // spec: ordin-desktop/components/FileService#SA-RECON-08
    #[error("{0}")]
    WorkFolder(&'static str),
}

impl FileError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            FileError::PlaceNotFound => "FILE_PLACE_NOT_FOUND",
            FileError::NotFound => "FILE_NOT_FOUND",
            FileError::ProtectedLocation => "FILE_PROTECTED_LOCATION",
            FileError::FolderUnreadable => "FILE_FOLDER_UNREADABLE",
            FileError::PlaceExists { .. } => "FILE_PLACE_EXISTS",
            FileError::PathEscapesPlace => "FILE_PATH_ESCAPES_PLACE",
            FileError::TooLarge { .. } => "FILE_TOO_LARGE",
            FileError::ChangedMeanwhile { .. } => "FILE_CHANGED_MEANWHILE",
            FileError::WriteFailed => "FILE_WRITE_FAILED",
            FileError::BinUnavailable => "FILE_BIN_UNAVAILABLE",
            FileError::VersionNotFound => "FILE_VERSION_NOT_FOUND",
            FileError::VersionCopyMissing => "FILE_VERSION_COPY_MISSING",
            FileError::CopyNotFound => "FILE_COPY_NOT_FOUND",
            FileError::CopyTransitionInvalid { .. } => "FILE_COPY_TRANSITION_INVALID",
            FileError::QueryInvalid => "FILE_QUERY_INVALID",
            FileError::GrantHandoffFailed => "FILE_GRANT_HANDOFF_FAILED",
            FileError::WatchFailed => "FILE_WATCH_FAILED",
            FileError::DatastoreUnavailable => "FILE_DATASTORE_UNAVAILABLE",
            FileError::NotGranted => "FILE_NOT_GRANTED",
            FileError::IncomingRefused { .. } => "FILE_INCOMING_REFUSED",
            FileError::RestoreTargetExists => "FILE_RESTORE_TARGET_EXISTS",
            FileError::MoveTargetExists => "FILE_MOVE_TARGET_EXISTS",
            FileError::Invalid(code) | FileError::WorkFolder(code) => code,
        }
    }

    /// The Error Table's status column: `Err(retryable)` rows. `FILE_GRANT_HANDOFF_FAILED` is
    /// retryable whatever the Agent Folder Manager's error was; the variant carries no field for
    /// that class.
    pub fn retryable(&self) -> bool {
        match self {
            FileError::ChangedMeanwhile { .. }
            | FileError::WriteFailed
            | FileError::GrantHandoffFailed
            | FileError::WatchFailed
            | FileError::DatastoreUnavailable => true,
            FileError::Invalid(code) => *code == "FVER_VERSION_GAP",
            FileError::WorkFolder(code) => *code == "WF_STORE_UNAVAILABLE",
            _ => false,
        }
    }
}

impl CodedError for FileError {
    fn code(&self) -> &'static str {
        FileError::code(self)
    }

    fn retryable(&self) -> bool {
        FileError::retryable(self)
    }

    /// The `action` column of the Error Table.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            FileError::VersionCopyMissing => None,
            FileError::ChangedMeanwhile { .. }
            | FileError::WriteFailed
            | FileError::DatastoreUnavailable => Some(RepairAction::TryAgain),
            FileError::Invalid("FVER_VERSION_GAP") | FileError::Invalid("SRV_BAD_CURSOR") => {
                Some(RepairAction::TryAgain)
            }
            _ => None,
        }
    }
}

/// Any statement failure on this module's tables is `FILE_DATASTORE_UNAVAILABLE`.
impl From<sqlx::Error> for FileError {
    fn from(error: sqlx::Error) -> Self {
        tracing::warn!(
            component = "files",
            error_class = db_class(&error),
            "files.datastore_failed"
        );
        FileError::DatastoreUnavailable
    }
}

/// A short class for the log; never the statement or a value.
pub(crate) fn db_class(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::PoolClosed | sqlx::Error::PoolTimedOut => "pool",
        sqlx::Error::Io(_) => "io",
        _ => "other",
    }
}

/// The shared paging helper's refusal of a cursor (`SRV_BAD_CURSOR`).
impl From<ordin_types::TypesError> for FileError {
    fn from(error: ordin_types::TypesError) -> Self {
        FileError::Invalid(error.code())
    }
}
