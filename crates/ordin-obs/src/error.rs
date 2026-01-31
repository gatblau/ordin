//! The adapter's one error type (ObsRecorderAdapter.md *Error Table*).

use ordin_types::{CodedError, RepairAction};

/// Every `OBS_` code this crate owns.
pub const ALL_CODES: &[&str] = &[
    "OBS_ALREADY_RECORDING",
    "OBS_NOT_INSTALLED",
    "OBS_NOT_REACHABLE",
    "OBS_NOT_RECORDING",
    "OBS_OUTPUT_MISSING",
    "OBS_PASSWORD_NEEDED",
    "OBS_PASSWORD_REFUSED",
    "OBS_PORT_INVALID",
    "OBS_REQUEST_FAILED",
    "OBS_SCENE_FAILED",
    "OBS_TIMEOUT",
    "OBS_VERSION_UNSUPPORTED",
];

/// `Display` never carries a password, an authentication string or a path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ObsError {
    #[error("not installed")]
    NotInstalled,
    #[error("not reachable")]
    NotReachable,
    #[error("password needed")]
    PasswordNeeded,
    #[error("password refused")]
    PasswordRefused,
    #[error("version unsupported")]
    VersionUnsupported,
    #[error("already recording")]
    AlreadyRecording,
    #[error("not recording")]
    NotRecording,
    #[error("request failed: {0}")]
    Request(u16),
    #[error("no answer in time")]
    Timeout,
    #[error("scene not prepared")]
    SceneFailed,
    #[error("output missing")]
    OutputMissing,
    #[error("port invalid")]
    PortInvalid,
}

impl CodedError for ObsError {
    fn code(&self) -> &'static str {
        match self {
            ObsError::NotInstalled => "OBS_NOT_INSTALLED",
            ObsError::NotReachable => "OBS_NOT_REACHABLE",
            ObsError::PasswordNeeded => "OBS_PASSWORD_NEEDED",
            ObsError::PasswordRefused => "OBS_PASSWORD_REFUSED",
            ObsError::VersionUnsupported => "OBS_VERSION_UNSUPPORTED",
            ObsError::AlreadyRecording => "OBS_ALREADY_RECORDING",
            ObsError::NotRecording => "OBS_NOT_RECORDING",
            ObsError::Request(_) => "OBS_REQUEST_FAILED",
            ObsError::Timeout => "OBS_TIMEOUT",
            ObsError::SceneFailed => "OBS_SCENE_FAILED",
            ObsError::OutputMissing => "OBS_OUTPUT_MISSING",
            ObsError::PortInvalid => "OBS_PORT_INVALID",
        }
    }

    /// True for nothing answering, a refused request, no answer in time and a scene not prepared.
    fn retryable(&self) -> bool {
        matches!(
            self,
            ObsError::NotReachable
                | ObsError::Request(_)
                | ObsError::Timeout
                | ObsError::SceneFailed
        )
    }

    fn repair(&self) -> Option<RepairAction> {
        self.retryable().then_some(RepairAction::TryAgain)
    }
}
