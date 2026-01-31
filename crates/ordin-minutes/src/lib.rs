//! The Minutes Service (MinutesService.md): turns a meeting — a transcript file, a recording, or a
//! meeting recorded with OBS from Ordin — into minutes kept in the Journal. It owns the meeting
//! jobs, the two one-time agreements (downloading the speech model; sending transcript text to the
//! AI service that writes the minutes), the minutes settings, and the recordings and transcripts in
//! Ordin's data folder. Ordin writes the minutes through [`MinutesWriter`], which the desktop
//! program implements, so this crate does not depend on the Ordin Host.
#![deny(unsafe_code)]

mod error;
mod files;
mod service;
mod types;

#[cfg(test)]
mod tests;

pub use error::{MinutesError, ALL_CODES};
pub use service::{MinutesService, MAX_BODY_BYTES, MAX_TRANSCRIPT_CHARS};
pub use types::{
    MinutesJob, MinutesJobPurpose, MinutesJobState, MinutesSettings, MinutesSettingsChange,
    MinutesStatus, MinutesWriter, RecordingView, SpeechModelSetting, WriteMinutesRequest,
};
