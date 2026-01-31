//! The adapter's public data (ObsRecorderAdapter.md *Public Interface*).

use std::path::PathBuf;

use ordin_types::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Defined once in the shared types (2C wire §23, WIRE-MIN-4).
pub use ordin_types::wire::minutes::ObsState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordingStarted {
    pub started_at: Timestamp,
    pub others_heard: bool,
    /// OBS's recording folder (`GetRecordDirectory`), read before StartRecord; the Minutes Service
    /// keeps it to find the file when OBS is closed during the recording. Never logged and never
    /// sent to the screens.
    pub record_folder: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordingFinished {
    pub output_path: PathBuf,
    pub duration_ms: u64,
}
