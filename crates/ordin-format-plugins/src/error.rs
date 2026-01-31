//! The crate's errors (FormatPlugins.md *Public Interface* `FormatError`, *Error Table*).
//!
//! Every error is a returned value of an in-process API. `Display` is the bare code, so a caller that
//! logs the error never writes file content.

use ordin_types::CodedError;

use crate::types::FormatId;

/// Every code this crate owns (ErrorHandling.md *Internal Logic* 11).
pub const ALL_CODES: &[&str] = &[
    "FMT_COMMIT_INVALID",
    "FMT_KIND_NOT_PROPOSABLE",
    "FMT_NOT_RECOGNISED",
    "FMT_PACKAGE_MANIFEST_INVALID",
    "FMT_PART_INVALID",
    "FMT_PATH_INVALID",
    "FMT_ROOT_NOT_SINGLE",
    "FMT_TREE_TOO_LARGE",
    "FMT_VERSION_UNTESTED",
    "FMT_WRITE_UNSUPPORTED",
];

// spec: ordin-desktop/components/FormatPlugins#SA-RECON-01
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("FMT_TREE_TOO_LARGE")]
    TreeTooLarge { files: u32, bytes: u64 },
    #[error("FMT_PATH_INVALID")]
    PathInvalid { path: String },
    #[error("FMT_NOT_RECOGNISED")]
    NotRecognised,
    #[error("FMT_PACKAGE_MANIFEST_INVALID")]
    PackageManifestInvalid { path: String, line: Option<u32> },
    #[error("FMT_WRITE_UNSUPPORTED")]
    WriteUnsupported { format: FormatId },
    #[error("FMT_VERSION_UNTESTED")]
    VersionUntested { declared: String },
    #[error("FMT_PART_INVALID")]
    PartInvalid { identifier: String, code: String },
    #[error("FMT_KIND_NOT_PROPOSABLE")]
    KindNotProposable { path: String },
    #[error("FMT_ROOT_NOT_SINGLE")]
    RootNotSingle { found: u32 },
    #[error("FMT_COMMIT_INVALID")]
    CommitInvalid { commit: String },
}

impl FormatError {
    /// The registry code; equal to the `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            FormatError::TreeTooLarge { .. } => "FMT_TREE_TOO_LARGE",
            FormatError::PathInvalid { .. } => "FMT_PATH_INVALID",
            FormatError::NotRecognised => "FMT_NOT_RECOGNISED",
            FormatError::PackageManifestInvalid { .. } => "FMT_PACKAGE_MANIFEST_INVALID",
            FormatError::WriteUnsupported { .. } => "FMT_WRITE_UNSUPPORTED",
            FormatError::VersionUntested { .. } => "FMT_VERSION_UNTESTED",
            FormatError::PartInvalid { .. } => "FMT_PART_INVALID",
            FormatError::KindNotProposable { .. } => "FMT_KIND_NOT_PROPOSABLE",
            FormatError::RootNotSingle { .. } => "FMT_ROOT_NOT_SINGLE",
            FormatError::CommitInvalid { .. } => "FMT_COMMIT_INVALID",
        }
    }
}

/// Every row of the Error Table is `Err(permanent)`.
impl CodedError for FormatError {
    fn code(&self) -> &'static str {
        FormatError::code(self)
    }
    fn retryable(&self) -> bool {
        false
    }
}
