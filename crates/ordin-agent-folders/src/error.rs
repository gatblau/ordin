//! The component's one error type and the codes it owns (AgentFolderManager.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{AgentInstanceId, CodedError, FolderGrantId, RunId};

/// Every code the registry gives this crate.
pub const ALL_CODES: &[&str] = &[
    "AFM_DATASTORE_FAILED",
    "AFM_FOLDER_BUSY",
    "AFM_FOLDER_EXISTS",
    "AFM_FOLDER_NOT_FOUND",
    "AFM_GRANT_INVALID",
    "AFM_IO_FAILED",
    "AFM_ROOT_UNWRITABLE",
    "AFM_SESSION_UNKNOWN",
];

/// Every failure of this crate. `Display` is the registry code and never carries a path or a value
/// from the datastore, so logging an error leaks nothing.
// spec: ordin-desktop/components/AgentFolderManager#SA-RECON-02
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AfmError {
    #[error("AFM_ROOT_UNWRITABLE")]
    RootUnwritable,
    #[error("AFM_FOLDER_EXISTS")]
    FolderExists { id: AgentInstanceId },
    #[error("AFM_FOLDER_NOT_FOUND")]
    FolderNotFound { id: AgentInstanceId },
    #[error("AFM_FOLDER_BUSY")]
    FolderBusy {
        id: AgentInstanceId,
        held: &'static str,
    },
    #[error("AFM_GRANT_INVALID")]
    GrantInvalid { folder_grant_id: FolderGrantId },
    #[error("AFM_SESSION_UNKNOWN")]
    SessionUnknown { run_id: RunId },
    #[error("AFM_IO_FAILED")]
    IoFailed { retryable: bool },
    #[error("AFM_DATASTORE_FAILED")]
    DatastoreFailed { retryable: bool },
}

impl AfmError {
    /// True when the same call may succeed if repeated unchanged later.
    pub fn retryable(&self) -> bool {
        match self {
            AfmError::FolderBusy { .. } => true,
            AfmError::IoFailed { retryable } | AfmError::DatastoreFailed { retryable } => {
                *retryable
            }
            AfmError::RootUnwritable
            | AfmError::FolderExists { .. }
            | AfmError::FolderNotFound { .. }
            | AfmError::GrantInvalid { .. }
            | AfmError::SessionUnknown { .. } => false,
        }
    }

    /// An operating-system failure, retryable (Error Table: file-system operation fails).
    pub(crate) fn io() -> AfmError {
        AfmError::IoFailed { retryable: true }
    }
}

impl CodedError for AfmError {
    fn code(&self) -> &'static str {
        match self {
            AfmError::RootUnwritable => "AFM_ROOT_UNWRITABLE",
            AfmError::FolderExists { .. } => "AFM_FOLDER_EXISTS",
            AfmError::FolderNotFound { .. } => "AFM_FOLDER_NOT_FOUND",
            AfmError::FolderBusy { .. } => "AFM_FOLDER_BUSY",
            AfmError::GrantInvalid { .. } => "AFM_GRANT_INVALID",
            AfmError::SessionUnknown { .. } => "AFM_SESSION_UNKNOWN",
            AfmError::IoFailed { .. } => "AFM_IO_FAILED",
            AfmError::DatastoreFailed { .. } => "AFM_DATASTORE_FAILED",
        }
    }

    fn retryable(&self) -> bool {
        AfmError::retryable(self)
    }
}
