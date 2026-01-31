//! Dictation — restated from `DictationService.md` (SA-RECON-02). Desktop only; the text itself reaches
//! the screens as `DictationUpdate` values on the Command Bridge's `service_streams`, which this crate
//! does not define (ServiceApi.md *Streams outside the catalogue*).

use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

use super::identity::v7_text_id;
use super::rules;

v7_text_id!(
    /// One dictation session (UUIDv7, *chosen here* by the owner; never stored).
    DictationSessionId
);

/// `FieldRef`: 1–128 characters (DictationService.md Error Table, `DICT_FIELD_INVALID`).
const FIELD_REF_MAX_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DictationAvailability {
    Available {
        locale: String,
    },
    /// Locale supported, recognition assets not yet installed by the system; starting will request them.
    NeedsAssets {
        locale: String,
    },
    Unavailable {
        reason: DictationUnavailableReason,
        /// Plain sentence in the person's locale.
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DictationUnavailableReason {
    Windows,
    MacosTooOld,
    /// Built for a target with no dictation at all (the ordin server).
    NotOffered,
    LanguageNotSupported,
    MicrophoneDenied,
    SpeechRecognitionDenied,
}

/// Where the text goes; opaque, chosen by the screens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct FieldRef(pub String);

/// `dictation.start`; `DictationUpdate` values follow on the stream.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DictationStartResult {
    pub session_id: DictationSessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DictationGetAvailabilityInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DictationStartInput {
    pub field: FieldRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DictationStopInput {
    pub session_id: DictationSessionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DictationCancelInput {
    pub session_id: DictationSessionId,
}

impl CommandInput for DictationGetAvailabilityInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for DictationStartInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // SPEC-GAP-27: DictationService.md refuses a field "outside 1–128 chars or the allowed
        // characters" but names no allowed characters; only the length is checked here.
        rules::text(&self.field.0, 1, FIELD_REF_MAX_CHARS, "/field")
    }
}

impl CommandInput for DictationStopInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for DictationCancelInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "0192a1d0-2222-7333-8444-555566667777";

    #[test]
    fn start_field() {
        let start = |field: &str| DictationStartInput {
            field: FieldRef(field.into()),
        };
        assert!(start("ordin.message").check().is_ok());
        for bad in ["", &"f".repeat(129)] {
            let rejection = start(bad).check().unwrap_err();
            assert_eq!(
                (rejection.code, rejection.at.as_str()),
                ("SVC_INPUT_INVALID", "/input/field")
            );
        }
    }

    #[test]
    fn sessions_and_shapes() {
        let session_id = DictationSessionId::parse(SESSION).unwrap();
        assert!(DictationStopInput {
            session_id: session_id.clone()
        }
        .check()
        .is_ok());
        assert!(DictationCancelInput { session_id }.check().is_ok());
        assert!(DictationGetAvailabilityInput {}.check().is_ok());
        assert!(serde_json::from_str::<DictationStopInput>(r#"{"session_id":"nope"}"#).is_err());
        let unavailable = DictationAvailability::Unavailable {
            reason: DictationUnavailableReason::Windows,
            message: "Dictation isn't available on Windows.".into(),
        };
        let json = serde_json::to_value(&unavailable).unwrap();
        assert_eq!(json["state"], "unavailable");
        assert_eq!(json["reason"], "windows");
    }
}
