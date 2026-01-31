//! The public types of the Transcription Service (TranscriptionService.md *Public Interface*; 2C
//! wire §23, WIRE-MIN-5).

use serde::{Deserialize, Serialize};

/// Defined once in the shared types (2C wire §23, WIRE-MIN-4 and WIRE-MIN-5).
pub use ordin_types::wire::minutes::{
    SpeechModelChoice, SpeechModelInfo, Transcript, TranscriptSourceKind, Utterance,
};

/// How the audio tracks of a recording become speakers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeakerTracks {
    /// Every track mixed to one; no speaker label (imported audio).
    Mixed,
    /// Ordin's own recordings: track 1 → "You", track 2 → "Others".
    YouAndOthers,
}

/// Progress of `transcribe_audio`, sent at least every second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum TranscribeProgress {
    Decoding { done_ms: u64, total_ms: u64 },
    Recognising { done_ms: u64, total_ms: u64 },
}

/// Progress of `download_model`, sent every megabyte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub received_mb: u32,
    pub total_mb: u32,
}
