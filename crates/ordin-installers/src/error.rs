//! The crate's one error type and the codes it owns (Installers.md *Public Interface* `InstallError`,
//! *Error Table*; ErrorHandling.md *Internal Logic* 11).
//!
//! `Display` is the bare code, so a caller that logs the error never writes a path or file content.

use ordin_types::domain::HarnessKind;
use ordin_types::CodedError;

/// Every code this crate owns, in registry order.
pub const ALL_CODES: &[&str] = &[
    "INST_BUNDLE_UNAVAILABLE",
    "INST_FOLDER_NOT_EMPTY",
    "INST_FOLDER_UNAVAILABLE",
    "INST_HARNESS_UNSUPPORTED",
    "INST_IMPORT_CYCLE",
    "INST_LOOP_UNSUPPORTED",
    "INST_NOT_INSTALLED",
    "INST_PATH_INVALID",
    "INST_RECORD_INVALID",
    "INST_REPAIR_FAILED",
    "INST_WRITE_FAILED",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    #[error("INST_HARNESS_UNSUPPORTED")]
    HarnessUnsupported { harness: HarnessKind },
    #[error("INST_FOLDER_UNAVAILABLE")]
    FolderUnavailable { code: &'static str, retryable: bool },
    #[error("INST_FOLDER_NOT_EMPTY")]
    FolderNotEmpty,
    #[error("INST_NOT_INSTALLED")]
    NotInstalled,
    #[error("INST_BUNDLE_UNAVAILABLE")]
    BundleUnavailable { code: String, retryable: bool },
    #[error("INST_LOOP_UNSUPPORTED")]
    LoopUnsupported {
        strategy: String,
        missing_capability: Option<String>,
    },
    #[error("INST_IMPORT_CYCLE")]
    ImportCycle { file: String },
    #[error("INST_PATH_INVALID")]
    PathInvalid { path: String },
    #[error("INST_WRITE_FAILED")]
    WriteFailed { retryable: bool },
    #[error("INST_RECORD_INVALID")]
    RecordInvalid,
    #[error("INST_REPAIR_FAILED")]
    RepairFailed { code: String },
}

impl InstallError {
    /// The registry code; equal to the `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            InstallError::HarnessUnsupported { .. } => "INST_HARNESS_UNSUPPORTED",
            InstallError::FolderUnavailable { .. } => "INST_FOLDER_UNAVAILABLE",
            InstallError::FolderNotEmpty => "INST_FOLDER_NOT_EMPTY",
            InstallError::NotInstalled => "INST_NOT_INSTALLED",
            InstallError::BundleUnavailable { .. } => "INST_BUNDLE_UNAVAILABLE",
            InstallError::LoopUnsupported { .. } => "INST_LOOP_UNSUPPORTED",
            InstallError::ImportCycle { .. } => "INST_IMPORT_CYCLE",
            InstallError::PathInvalid { .. } => "INST_PATH_INVALID",
            InstallError::WriteFailed { .. } => "INST_WRITE_FAILED",
            InstallError::RecordInvalid => "INST_RECORD_INVALID",
            InstallError::RepairFailed { .. } => "INST_REPAIR_FAILED",
        }
    }

    /// True when the same call may succeed if repeated unchanged later: a busy folder and a failed
    /// file operation (Error Table `Err(retryable)` rows); every other row is permanent.
    pub fn retryable(&self) -> bool {
        match self {
            InstallError::FolderUnavailable { retryable, .. }
            | InstallError::BundleUnavailable { retryable, .. }
            | InstallError::WriteFailed { retryable } => *retryable,
            InstallError::HarnessUnsupported { .. }
            | InstallError::FolderNotEmpty
            | InstallError::NotInstalled
            | InstallError::LoopUnsupported { .. }
            | InstallError::ImportCycle { .. }
            | InstallError::PathInvalid { .. }
            | InstallError::RecordInvalid
            | InstallError::RepairFailed { .. } => false,
        }
    }

    /// An I/O failure while writing, renaming or deleting (Error Table, retryable row).
    pub(crate) fn io() -> InstallError {
        InstallError::WriteFailed { retryable: true }
    }
}

impl CodedError for InstallError {
    fn code(&self) -> &'static str {
        InstallError::code(self)
    }
    fn retryable(&self) -> bool {
        InstallError::retryable(self)
    }
}

/// Step 8: a lease the Agent Folder Manager refused. `AFM_FOLDER_BUSY` is retryable, a missing
/// folder is not; any other folder failure keeps its own code and retry flag.
pub(crate) fn folder_unavailable(error: &ordin_agent_folders::AfmError) -> InstallError {
    InstallError::FolderUnavailable {
        code: CodedError::code(error),
        retryable: error.retryable(),
    }
}

/// Step 8: a bundle the Resolver and Compiler cannot open is never retried by this crate.
pub(crate) fn bundle_unavailable(error: &ordin_resolver::ResolveError) -> InstallError {
    InstallError::BundleUnavailable {
        code: error.code().to_owned(),
        retryable: false,
    }
}
