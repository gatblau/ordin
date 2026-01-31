//! Meeting minutes kept by Ordin (2C wire §23, WIRE-MIN-2). The Journal owns the row; Ordin Host,
//! the Minutes Service, the desktop program and the screens read it.
//!
//! The job, status, settings and transcript types of §23 (WIRE-MIN-3 to WIRE-MIN-5) are defined here
//! too, once; the Transcription Service, the OBS Recorder Adapter and the Minutes Service re-export
//! them, and the Service API generates them only in a build with the `minutes` switch.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::id::{MinutesId, MinutesJobId, RecordingId, RunId, WorkspaceId};
use crate::time::Timestamp;

/// Where the minutes came from.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MinutesSource {
    TranscriptFile,
    AudioFile,
    Recording,
}

/// One meeting's minutes.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-2
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Minutes {
    pub id: MinutesId,
    pub workspace_id: WorkspaceId,
    /// 1–120 characters, no control character.
    pub title: String,
    pub source: MinutesSource,
    /// The file's name only, 1–255 characters.
    pub source_name: String,
    /// Markdown, 1 byte to 1 MiB.
    pub body: String,
    /// `None` once the recording is deleted, and for imported files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<RecordingId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<RunId>,
    /// At least 1.
    pub revision: u32,
    pub kept_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<Timestamp>,
}

/// One line of the Minutes list.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-2
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesSummary {
    pub id: MinutesId,
    pub title: String,
    pub source: MinutesSource,
    pub kept_at: Timestamp,
}

/// Where a job is.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-3
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MinutesJobState {
    Queued,
    WaitingForModel,
    DownloadingModel {
        received_mb: u32,
        total_mb: u32,
    },
    Reading,
    Transcribing {
        done_ms: u64,
        total_ms: u64,
    },
    WaitingForNotice,
    Writing,
    Done {
        minutes_id: MinutesId,
    },
    /// A job of purpose `transcript`: the text is ready for Ordin (WIRE-MIN-7).
    TranscriptReady,
    Failed {
        code: String,
        message: String,
    },
    Cancelled,
}

impl MinutesJobState {
    /// Done, failed or cancelled.
    pub fn finished(&self) -> bool {
        matches!(
            self,
            MinutesJobState::Done { .. }
                | MinutesJobState::TranscriptReady
                | MinutesJobState::Failed { .. }
                | MinutesJobState::Cancelled
        )
    }

    /// The `state` tag, for log lines.
    pub fn name(&self) -> &'static str {
        match self {
            MinutesJobState::Queued => "queued",
            MinutesJobState::WaitingForModel => "waiting_for_model",
            MinutesJobState::DownloadingModel { .. } => "downloading_model",
            MinutesJobState::Reading => "reading",
            MinutesJobState::Transcribing { .. } => "transcribing",
            MinutesJobState::WaitingForNotice => "waiting_for_notice",
            MinutesJobState::Writing => "writing",
            MinutesJobState::Done { .. } => "done",
            MinutesJobState::TranscriptReady => "transcript_ready",
            MinutesJobState::Failed { .. } => "failed",
            MinutesJobState::Cancelled => "cancelled",
        }
    }
}

/// One meeting being turned into minutes.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-3
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MinutesJob {
    pub id: MinutesJobId,
    pub workspace_id: WorkspaceId,
    /// The file's name only.
    pub source_name: String,
    pub source: MinutesSource,
    pub state: MinutesJobState,
    pub created_at: Timestamp,
    /// Minutes, or text for Ordin; absent in JSON reads as minutes (WIRE-MIN-7).
    #[serde(default)]
    pub purpose: MinutesJobPurpose,
}

/// What a job makes: minutes, or a recording turned into text for Ordin.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MinutesJobPurpose {
    #[default]
    Minutes,
    Transcript,
}

/// Which speech model: the larger (more accurate, about 1.5 GB) or the smaller (faster, about
/// 470 MB). There is no fixed default: the machine recommends one (TRX-MACH-1).
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpeechModelChoice {
    Larger,
    Smaller,
}

impl SpeechModelChoice {
    /// The serialised name.
    pub fn as_str(self) -> &'static str {
        match self {
            SpeechModelChoice::Larger => "larger",
            SpeechModelChoice::Smaller => "smaller",
        }
    }
}

/// The person's speech model setting: the machine's recommendation, or their own choice, which
/// wins over it.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-8
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SpeechModelSetting {
    Recommended,
    Larger,
    Smaller,
}

impl SpeechModelSetting {
    /// The model in use: `recommended` for `Recommended`, else the chosen one.
    pub fn resolve(self, recommended: SpeechModelChoice) -> SpeechModelChoice {
        match self {
            SpeechModelSetting::Recommended => recommended,
            SpeechModelSetting::Larger => SpeechModelChoice::Larger,
            SpeechModelSetting::Smaller => SpeechModelChoice::Smaller,
        }
    }
}

/// One speech model as the consent prompt shows it.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SpeechModelInfo {
    pub choice: SpeechModelChoice,
    /// From `speech-models.json`, e.g. "ggml-medium.bin".
    pub file_name: String,
    /// Shown in the consent prompt.
    pub size_mb: u32,
    /// The models folder, shown in the consent prompt; the one path that crosses the boundary.
    pub folder: PathBuf,
    /// The file exists; it is written only after its checksum matched.
    pub present: bool,
}

/// What OBS can do for Ordin now.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ObsState {
    NotInstalled,
    /// Installed, but nothing answers on the port (OBS closed, or its WebSocket server off).
    NotReachable,
    /// The server asks for a password and none is stored.
    PasswordNeeded,
    /// The stored password was refused.
    PasswordRefused,
    /// OBS older than 28 or a WebSocket RPC version other than 1.
    VersionUnsupported {
        obs_version: String,
    },
    Ready {
        obs_version: String,
        others_heard: bool,
    },
    Recording {
        started_at: Timestamp,
        others_heard: bool,
    },
}

/// The settings the person changes.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MinutesSettings {
    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-8
    pub speech_model: SpeechModelSetting,
    pub delete_recordings_after_save: bool,
    /// 1024–65535.
    pub obs_port: u16,
}

/// Each field optional; at least one.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MinutesSettingsChange {
    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-8
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech_model: Option<SpeechModelSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_recordings_after_save: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obs_port: Option<u16>,
}

/// The running recording.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RecordingView {
    pub id: RecordingId,
    pub workspace_id: WorkspaceId,
    pub started_at: Timestamp,
    pub others_heard: bool,
}

/// What the Minutes screens show.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MinutesStatus {
    pub settings: MinutesSettings,
    /// The model in use: the recommendation for `recommended`, else the chosen one.
    pub model: SpeechModelInfo,
    /// What this computer handles well (TRX-MACH-3).
    // spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-8
    pub recommended_model: SpeechModelChoice,
    pub model_consent_given: bool,
    pub writer_notice_seen: bool,
    /// The AI service named in the notice.
    pub writer_name: String,
    pub recorder: ObsState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<RecordingView>,
}

/// Where a transcript came from.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-5
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSourceKind {
    TranscriptFile,
    AudioFile,
    Recording,
}

/// One timed piece of speech.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-5
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Utterance {
    pub start_ms: u64,
    /// At least `start_ms`.
    pub end_ms: u64,
    /// 1–80 characters; `None` when the source names no speaker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// 1–4,000 characters, trimmed, single spaces.
    pub text: String,
}

/// A meeting as timed utterances.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-5
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Transcript {
    /// Always 1.
    pub format: u32,
    pub source_kind: TranscriptSourceKind,
    /// The file name only, 1–255 characters.
    pub source_name: String,
    /// BCP 47 tag detected by the model; `None` for transcript files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub duration_ms: u64,
    /// Ordered by `start_ms`, then speaker; at least one.
    pub utterances: Vec<Utterance>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_round_trip_in_snake_case_and_refuse_unknown_fields() {
        let minutes = Minutes {
            id: MinutesId::new(),
            workspace_id: WorkspaceId::new(),
            title: "Supplier call".to_owned(),
            source: MinutesSource::TranscriptFile,
            source_name: "meeting.vtt".to_owned(),
            body: "## Summary".to_owned(),
            recording: None,
            run_id: None,
            revision: 1,
            kept_at: Timestamp::now(),
            edited_at: None,
        };
        let value = serde_json::to_value(&minutes).unwrap();
        assert_eq!(value["source"], "transcript_file");
        assert!(value.get("recording").is_none());
        assert!(value.get("edited_at").is_none());
        let back: Minutes = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(back, minutes);
        let mut extra = value;
        extra["colour"] = serde_json::json!("red");
        assert!(serde_json::from_value::<Minutes>(extra).is_err());
        for (source, name) in [
            (MinutesSource::AudioFile, "audio_file"),
            (MinutesSource::Recording, "recording"),
        ] {
            assert_eq!(serde_json::to_value(source).unwrap(), name);
        }
    }
}
