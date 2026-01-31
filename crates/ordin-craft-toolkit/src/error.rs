//! The Craft Toolkit's one error type and the codes it owns (CraftToolkit.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `CRAFT_` and `FTM_` rows).
pub const ALL_CODES: &[&str] = &[
    "CRAFT_ARGUMENTS_INVALID",
    "CRAFT_CONVERSION_UNAVAILABLE",
    "CRAFT_DATASTORE_UNAVAILABLE",
    "CRAFT_DOWNLOAD_FAILED",
    "CRAFT_DUPLICATE",
    "CRAFT_FILES_FAILED",
    "CRAFT_HASH_MISMATCH",
    "CRAFT_INSTALL_FAILED",
    "CRAFT_NOT_APPROVED",
    "CRAFT_NOT_INSTALLED",
    "CRAFT_NO_MCP_MODE",
    "CRAFT_OPEN_FAILED",
    "CRAFT_OPEN_REFUSED",
    "CRAFT_OUTPUT_MISSING",
    "CRAFT_OUTPUT_TOO_LARGE",
    "CRAFT_PATH_NOT_ALLOWED",
    "CRAFT_PLATFORM_UNSUPPORTED",
    "CRAFT_SERVER_START_FAILED",
    "CRAFT_START_BLOCKED",
    "CRAFT_TIMEOUT",
    "CRAFT_TOOL_FAILED",
    "CRAFT_TOOL_NOT_OFFERED",
    "CRAFT_UNVERIFIED_INSTALL",
    "FTM_EXTENSION_DUPLICATE",
    "FTM_EXTENSION_INVALID",
    "FTM_IMMUTABLE",
    "FTM_MEDIA_TYPE_INVALID",
];

/// Every failure of this crate (Public Interface).
#[derive(Debug, Clone, thiserror::Error)]
pub enum CraftError {
    #[error("CRAFT_NOT_APPROVED")]
    NotApproved,
    #[error("CRAFT_NOT_INSTALLED")]
    NotInstalled,
    #[error("CRAFT_DOWNLOAD_FAILED")]
    DownloadFailed,
    #[error("CRAFT_HASH_MISMATCH")]
    HashMismatch,
    #[error("CRAFT_PLATFORM_UNSUPPORTED")]
    PlatformUnsupported,
    #[error("CRAFT_INSTALL_FAILED")]
    InstallFailed,
    #[error("CRAFT_START_BLOCKED")]
    StartBlocked { program: String, reason: String },
    #[error("CRAFT_NO_MCP_MODE")]
    NoMcpMode,
    #[error("CRAFT_SERVER_START_FAILED")]
    ServerStartFailed,
    #[error("CRAFT_TOOL_NOT_OFFERED")]
    ToolNotOffered,
    #[error("CRAFT_PATH_NOT_ALLOWED")]
    PathNotAllowed,
    #[error("CRAFT_ARGUMENTS_INVALID")]
    ArgumentsInvalid,
    #[error("CRAFT_TIMEOUT")]
    Timeout,
    #[error("CRAFT_TOOL_FAILED")]
    ToolFailed,
    #[error("CRAFT_OUTPUT_MISSING")]
    OutputMissing,
    #[error("CRAFT_OUTPUT_TOO_LARGE")]
    OutputTooLarge,
    #[error("CRAFT_CONVERSION_UNAVAILABLE")]
    ConversionUnavailable,
    #[error("CRAFT_OPEN_FAILED")]
    OpenFailed,
    #[error("CRAFT_OPEN_REFUSED")]
    OpenRefused,
    #[error("CRAFT_FILES_FAILED")]
    FilesFailed,
    #[error("CRAFT_DATASTORE_UNAVAILABLE")]
    DatastoreUnavailable,
    /// `CRAFT_DUPLICATE`, `CRAFT_UNVERIFIED_INSTALL`, `FTM_*`.
    #[error("{0}")]
    Invalid(&'static str),
}

impl CraftError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            CraftError::NotApproved => "CRAFT_NOT_APPROVED",
            CraftError::NotInstalled => "CRAFT_NOT_INSTALLED",
            CraftError::DownloadFailed => "CRAFT_DOWNLOAD_FAILED",
            CraftError::HashMismatch => "CRAFT_HASH_MISMATCH",
            CraftError::PlatformUnsupported => "CRAFT_PLATFORM_UNSUPPORTED",
            CraftError::InstallFailed => "CRAFT_INSTALL_FAILED",
            CraftError::StartBlocked { .. } => "CRAFT_START_BLOCKED",
            CraftError::NoMcpMode => "CRAFT_NO_MCP_MODE",
            CraftError::ServerStartFailed => "CRAFT_SERVER_START_FAILED",
            CraftError::ToolNotOffered => "CRAFT_TOOL_NOT_OFFERED",
            CraftError::PathNotAllowed => "CRAFT_PATH_NOT_ALLOWED",
            CraftError::ArgumentsInvalid => "CRAFT_ARGUMENTS_INVALID",
            CraftError::Timeout => "CRAFT_TIMEOUT",
            CraftError::ToolFailed => "CRAFT_TOOL_FAILED",
            CraftError::OutputMissing => "CRAFT_OUTPUT_MISSING",
            CraftError::OutputTooLarge => "CRAFT_OUTPUT_TOO_LARGE",
            CraftError::ConversionUnavailable => "CRAFT_CONVERSION_UNAVAILABLE",
            CraftError::OpenFailed => "CRAFT_OPEN_FAILED",
            CraftError::OpenRefused => "CRAFT_OPEN_REFUSED",
            CraftError::FilesFailed => "CRAFT_FILES_FAILED",
            CraftError::DatastoreUnavailable => "CRAFT_DATASTORE_UNAVAILABLE",
            CraftError::Invalid(code) => code,
        }
    }

    /// `Err(retryable)` rows of the Error Table; every other row is permanent.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            CraftError::NotInstalled
                | CraftError::DownloadFailed
                | CraftError::ServerStartFailed
                | CraftError::Timeout
                | CraftError::OpenFailed
                | CraftError::FilesFailed
                | CraftError::DatastoreUnavailable
        )
    }

    /// The Error Table's sentence. `CRAFT_TOOL_FAILED` carries the app's own message instead, and
    /// the rows answered `— (internal)` or by a start-up report item have no sentence of their own.
    pub(crate) fn sentence(&self) -> String {
        match self {
            CraftError::NotApproved => "Your company hasn't approved this app.".to_owned(),
            CraftError::NotInstalled => {
                "This app isn't installed yet. ordin will install it.".to_owned()
            }
            CraftError::DownloadFailed => "ordin couldn't download this app.".to_owned(),
            CraftError::HashMismatch => {
                "This app's download didn't pass ordin's safety check.".to_owned()
            }
            CraftError::PlatformUnsupported => {
                "This app isn't available for this computer yet.".to_owned()
            }
            CraftError::InstallFailed => "ordin couldn't install this app.".to_owned(),
            CraftError::StartBlocked { program, .. } => {
                format!("Your company's settings stopped {program} from opening on this computer.")
            }
            CraftError::NoMcpMode => "This tool isn't available to you.".to_owned(),
            CraftError::ServerStartFailed => "The app didn't start.".to_owned(),
            CraftError::ToolNotOffered => "This tool isn't available to you.".to_owned(),
            CraftError::PathNotAllowed => {
                "That file is outside the folders you can use.".to_owned()
            }
            CraftError::ArgumentsInvalid => "Give the file as a full path.".to_owned(),
            CraftError::Timeout => "The app took too long and was stopped.".to_owned(),
            CraftError::ToolFailed => "The app returned an error.".to_owned(),
            CraftError::OutputMissing => "The app didn't write the file.".to_owned(),
            CraftError::OutputTooLarge => "The file the app wrote is too large.".to_owned(),
            CraftError::ConversionUnavailable => {
                "Word, PDF and slides copies aren't ready yet.".to_owned()
            }
            CraftError::OpenFailed => "ordin couldn't open this file.".to_owned(),
            CraftError::OpenRefused => {
                "ordin won't open this file, because the system could run it.".to_owned()
            }
            CraftError::FilesFailed => "ordin couldn't save the file.".to_owned(),
            CraftError::DatastoreUnavailable => "ordin couldn't reach its own records.".to_owned(),
            CraftError::Invalid(_) => "ordin couldn't use this app.".to_owned(),
        }
    }
}

impl CodedError for CraftError {
    fn code(&self) -> &'static str {
        CraftError::code(self)
    }

    fn retryable(&self) -> bool {
        CraftError::retryable(self)
    }

    /// The Error Table's `action` column.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            CraftError::NotInstalled
            | CraftError::DownloadFailed
            | CraftError::OpenFailed
            | CraftError::DatastoreUnavailable => Some(RepairAction::TryAgain),
            CraftError::HashMismatch
            | CraftError::InstallFailed
            | CraftError::StartBlocked { .. } => None,
            _ => None,
        }
    }
}

/// Any datastore failure is `CRAFT_DATASTORE_UNAVAILABLE` (retryable).
impl From<sqlx::Error> for CraftError {
    fn from(_: sqlx::Error) -> Self {
        CraftError::DatastoreUnavailable
    }
}
