//! The Minutes Service's one error type (MinutesService.md *Public Interface* and *Error Table*).

use ordin_dictation::transcription::TranscriptionError;
use ordin_journal::JournalError;
use ordin_obs::ObsError;
use ordin_types::{CodedError, RepairAction};

/// Every `MIN_` code this crate owns.
pub const ALL_CODES: &[&str] = &[
    "MIN_BODY_INVALID",
    "MIN_EDIT_CONFLICT",
    "MIN_JOB_FINISHED",
    "MIN_JOB_NOT_FOUND",
    "MIN_JOB_NOT_READY",
    "MIN_NOT_FOUND",
    "MIN_RECORDING_IN_PROGRESS",
    "MIN_RECORDING_NOT_FOUND",
    "MIN_SETTING_INVALID",
    "MIN_STORE_FAILED",
    "MIN_TRANSCRIPT_TOO_LONG",
    "MIN_WORKSPACE_UNKNOWN",
    "MIN_WRITE_FAILED",
];

/// `Display` never carries a file name, a path, transcript text or a minutes body.
#[derive(Debug, thiserror::Error)]
pub enum MinutesError {
    #[error("minutes not found")]
    NotFound,
    #[error("workspace unknown")]
    WorkspaceUnknown,
    #[error("job not found")]
    JobNotFound,
    #[error("job already finished")]
    JobFinished,
    /// `transcription` of a minutes job, or of a job not yet ready (MIN-TR-3).
    #[error("job not ready")]
    JobNotReady,
    #[error("recording not found")]
    RecordingNotFound,
    #[error("already recording")]
    RecordingInProgress,
    #[error("edited elsewhere")]
    EditConflict,
    #[error("body invalid")]
    BodyInvalid,
    #[error("setting invalid")]
    SettingInvalid,
    #[error("transcript too long")]
    TranscriptTooLong,
    #[error("storage failed")]
    StoreFailed,
    #[error("minutes not written")]
    WriteFailed,
    #[error(transparent)]
    Transcription(#[from] TranscriptionError),
    #[error(transparent)]
    Recorder(#[from] ObsError),
    #[error("journal: {0}")]
    Journal(JournalError),
}

impl From<JournalError> for MinutesError {
    fn from(error: JournalError) -> Self {
        MinutesError::Journal(error)
    }
}

impl CodedError for MinutesError {
    fn code(&self) -> &'static str {
        match self {
            MinutesError::NotFound => "MIN_NOT_FOUND",
            MinutesError::WorkspaceUnknown => "MIN_WORKSPACE_UNKNOWN",
            MinutesError::JobNotFound => "MIN_JOB_NOT_FOUND",
            MinutesError::JobFinished => "MIN_JOB_FINISHED",
            MinutesError::JobNotReady => "MIN_JOB_NOT_READY",
            MinutesError::RecordingNotFound => "MIN_RECORDING_NOT_FOUND",
            MinutesError::RecordingInProgress => "MIN_RECORDING_IN_PROGRESS",
            MinutesError::EditConflict => "MIN_EDIT_CONFLICT",
            MinutesError::BodyInvalid => "MIN_BODY_INVALID",
            MinutesError::SettingInvalid => "MIN_SETTING_INVALID",
            MinutesError::TranscriptTooLong => "MIN_TRANSCRIPT_TOO_LONG",
            MinutesError::StoreFailed => "MIN_STORE_FAILED",
            MinutesError::WriteFailed => "MIN_WRITE_FAILED",
            MinutesError::Transcription(e) => e.code(),
            MinutesError::Recorder(e) => e.code(),
            MinutesError::Journal(e) => e.code(),
        }
    }

    /// `MIN_STORE_FAILED` and `MIN_WRITE_FAILED`; a wrapped error keeps its owner's flag.
    fn retryable(&self) -> bool {
        match self {
            MinutesError::StoreFailed | MinutesError::WriteFailed => true,
            MinutesError::Transcription(e) => e.retryable(),
            MinutesError::Recorder(e) => e.retryable(),
            MinutesError::Journal(e) => e.retryable(),
            _ => false,
        }
    }

    fn repair(&self) -> Option<RepairAction> {
        match self {
            MinutesError::StoreFailed | MinutesError::WriteFailed => Some(RepairAction::TryAgain),
            MinutesError::Transcription(e) => e.repair(),
            MinutesError::Recorder(e) => e.repair(),
            MinutesError::Journal(e) => e.repair(),
            _ => None,
        }
    }
}
