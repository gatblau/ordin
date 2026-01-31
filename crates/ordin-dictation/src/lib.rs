//! Speech on the desktop. The Dictation Service uses the operating system's recogniser; the
//! Transcription Service (TranscriptionService.md, crate feature `transcription`) turns one meeting
//! file into a timed transcript, on this computer only.

#[cfg(feature = "transcription")]
pub mod transcription;
