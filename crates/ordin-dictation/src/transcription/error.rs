//! The Transcription Service's one error type (TranscriptionService.md *Error Table*).

use ordin_types::{CodedError, RepairAction};

/// Every `TRX_` code this module owns.
pub const ALL_CODES: &[&str] = &[
    "TRX_AUDIO_UNDECODABLE",
    "TRX_BUSY",
    "TRX_CANCELLED",
    "TRX_ENGINE_FAILED",
    "TRX_FILE_TOO_LARGE",
    "TRX_FILE_UNREADABLE",
    "TRX_FORMAT_UNSUPPORTED",
    "TRX_MODEL_CHECKSUM",
    "TRX_MODEL_DOWNLOAD_FAILED",
    "TRX_MODEL_MISSING",
    "TRX_NO_SPACE",
    "TRX_TRANSCRIPT_EMPTY",
    "TRX_TRANSCRIPT_MALFORMED",
];

/// `Display` never carries a file name, a path or any transcript text.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptionError {
    #[error("format not supported")]
    FormatUnsupported,
    #[error("file unreadable")]
    FileUnreadable,
    #[error("file too large")]
    FileTooLarge,
    #[error("transcript malformed")]
    TranscriptMalformed,
    #[error("no speech")]
    Empty,
    #[error("audio undecodable")]
    AudioUndecodable,
    #[error("model missing")]
    ModelMissing,
    #[error("download failed")]
    DownloadFailed,
    #[error("checksum mismatch")]
    ChecksumMismatch,
    #[error("no space")]
    NoSpace,
    #[error("cancelled")]
    Cancelled,
    #[error("engine failed: {0}")]
    Engine(i32),
    #[error("busy")]
    Busy,
}

impl CodedError for TranscriptionError {
    fn code(&self) -> &'static str {
        match self {
            TranscriptionError::FormatUnsupported => "TRX_FORMAT_UNSUPPORTED",
            TranscriptionError::FileUnreadable => "TRX_FILE_UNREADABLE",
            TranscriptionError::FileTooLarge => "TRX_FILE_TOO_LARGE",
            TranscriptionError::TranscriptMalformed => "TRX_TRANSCRIPT_MALFORMED",
            TranscriptionError::Empty => "TRX_TRANSCRIPT_EMPTY",
            TranscriptionError::AudioUndecodable => "TRX_AUDIO_UNDECODABLE",
            TranscriptionError::ModelMissing => "TRX_MODEL_MISSING",
            TranscriptionError::DownloadFailed => "TRX_MODEL_DOWNLOAD_FAILED",
            TranscriptionError::ChecksumMismatch => "TRX_MODEL_CHECKSUM",
            TranscriptionError::NoSpace => "TRX_NO_SPACE",
            TranscriptionError::Cancelled => "TRX_CANCELLED",
            TranscriptionError::Engine(_) => "TRX_ENGINE_FAILED",
            TranscriptionError::Busy => "TRX_BUSY",
        }
    }

    /// True for a failed download, a damaged download, an engine failure and a busy service.
    fn retryable(&self) -> bool {
        matches!(
            self,
            TranscriptionError::DownloadFailed
                | TranscriptionError::ChecksumMismatch
                | TranscriptionError::Engine(_)
                | TranscriptionError::Busy
        )
    }

    fn repair(&self) -> Option<RepairAction> {
        self.retryable().then_some(RepairAction::TryAgain)
    }
}
