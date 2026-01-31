//! The `minutes` group (ServiceApi.md SVC-MIN-1 to SVC-MIN-4; 2C wire §23, WIRE-MIN-6): the inputs of
//! the twenty rows. The job, status, settings, transcript and minutes types are the shared ones of
//! `ordin_types::wire::minutes`, re-exported here. Compiled only with the `minutes` switch, so a build
//! without it has no `minutes.*` row and generates none of these types (SVC-MIN-3).

use ordin_types::{Cursor, MinutesId, MinutesJobId, RecordingId, WorkspaceId};
use serde::{Deserialize, Serialize};

use super::knowledge::KnowledgeExportFormat;
use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::wire::minutes::{
    Minutes, MinutesJob, MinutesJobState, MinutesSettings, MinutesSettingsChange, MinutesSource,
    MinutesStatus, MinutesSummary, ObsState, RecordingView, SpeechModelChoice, SpeechModelInfo,
    SpeechModelSetting, Transcript, TranscriptSourceKind, Utterance,
};

/// The largest minutes body `minutes.save_edit` accepts, in bytes of UTF-8 (1 MiB).
const BODY_MAX_BYTES: usize = 1024 * 1024;

/// `minutes.status`, `minutes.import`, `minutes.jobs`, `minutes.record`.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesWorkspaceInput {
    pub workspace_id: WorkspaceId,
}

/// `minutes.list`: 50 per page, newest first.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesListInput {
    pub workspace_id: WorkspaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// `minutes.get`, `minutes.transcript`, `minutes.remove`, `minutes.delete_recording`.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesIdInput {
    pub minutes_id: MinutesId,
}

/// `minutes.cancel_job`, `minutes.retry`.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesJobInput {
    pub job_id: MinutesJobId,
}

/// `minutes.stop`.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesStopInput {
    pub recording_id: RecordingId,
}

/// `minutes.save_edit`: body 1 byte to 1 MiB, `base_revision` at least 1.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesSaveEditInput {
    pub minutes_id: MinutesId,
    pub body: String,
    pub base_revision: u32,
}

/// `minutes.export`: the program reads the minutes itself; no content or path crosses.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MinutesExportInput {
    pub minutes_id: MinutesId,
    pub format: KnowledgeExportFormat,
}

/// `minutes.save_obs_password`: 1–256 characters. Never logged, journalled or echoed; `Debug` hides it.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-6
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ObsPasswordInput {
    pub password: String,
}

impl std::fmt::Debug for ObsPasswordInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObsPasswordInput")
            .field("password", &"***")
            .finish()
    }
}

impl CommandInput for MinutesWorkspaceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for MinutesListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for MinutesIdInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for MinutesJobInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for MinutesStopInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

// spec: ordin-desktop/components/ServiceApi#SVC-MIN-1
impl CommandInput for MinutesSaveEditInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::bytes(&self.body, 1, BODY_MAX_BYTES, "/body")?;
        if self.base_revision < 1 {
            return Err(rules::reject("/base_revision"));
        }
        Ok(())
    }
}

impl CommandInput for MinutesExportInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-3
impl CommandInput for ObsPasswordInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.password, 1, 256, "/password")
    }
}

/// At least one field; `obs_port` 1024–65535.
// spec: ordin-desktop/phase-2c-shared-types-wire#WIRE-MIN-4
impl CommandInput for MinutesSettingsChange {
    fn check(&mut self) -> Result<(), InputRejection> {
        if self.speech_model.is_none()
            && self.delete_recordings_after_save.is_none()
            && self.obs_port.is_none()
        {
            return Err(rules::reject(""));
        }
        rules::opt_range(self.obs_port, 1024, u16::MAX, "/obs_port")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_is_refused_when_empty_and_never_shown() {
        let mut input = ObsPasswordInput {
            password: String::new(),
        };
        assert_eq!(input.check().unwrap_err().at, "/input/password");
        let secret = ObsPasswordInput {
            password: "fixture-pass".to_owned(),
        };
        assert!(!format!("{secret:?}").contains("fixture-pass"));
        assert!(
            serde_json::from_str::<ObsPasswordInput>(r#"{"password":"x","colour":1}"#).is_err()
        );
    }

    #[test]
    fn a_settings_change_names_at_least_one_field_and_a_port_from_1024() {
        assert_eq!(
            MinutesSettingsChange::default().check().unwrap_err().at,
            "/input"
        );
        let mut low = MinutesSettingsChange {
            obs_port: Some(80),
            ..Default::default()
        };
        assert_eq!(low.check().unwrap_err().at, "/input/obs_port");
        let mut ok = MinutesSettingsChange {
            obs_port: Some(4455),
            ..Default::default()
        };
        assert!(ok.check().is_ok());
    }

    #[test]
    fn an_edit_is_one_byte_to_one_mebibyte_on_a_revision_from_one() {
        let id = MinutesId::new();
        let mut empty = MinutesSaveEditInput {
            minutes_id: id,
            body: String::new(),
            base_revision: 1,
        };
        assert_eq!(empty.check().unwrap_err().at, "/input/body");
        let mut zero = MinutesSaveEditInput {
            minutes_id: id,
            body: "x".to_owned(),
            base_revision: 0,
        };
        assert_eq!(zero.check().unwrap_err().at, "/input/base_revision");
    }
}
