//! How Ordin talks — restated from `OrdinHost.md` *Voice settings* (SA-RECON-02). `VoiceSettings` is a
//! 2C type, imported.

use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{VoiceLength, VoiceRegister, VoiceSettings, VoiceTone};

/// `notes` ≤ 300 characters (`VOICE_NOTES_TOO_LONG` at the owner).
const NOTES_MAX_CHARS: usize = 300;
/// `name_to_use` ≤ 60 characters (`VOICE_NAME_TOO_LONG` at the owner); an empty string is stored as `None`.
const NAME_MAX_CHARS: usize = 60;

/// `voice.get`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VoiceGetInput {}

/// `voice.update`: each absent field is left unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VoiceUpdateInput {
    pub tone: Option<VoiceTone>,
    pub register: Option<VoiceRegister>,
    pub length: Option<VoiceLength>,
    pub name_to_use: Option<String>,
    pub notes: Option<String>,
}

impl CommandInput for VoiceGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for VoiceUpdateInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::opt_text(&self.name_to_use, 0, NAME_MAX_CHARS, "/name_to_use")?;
        rules::opt_text(&self.notes, 0, NOTES_MAX_CHARS, "/notes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_limits() {
        assert!(VoiceGetInput {}.check().is_ok());
        assert!(VoiceUpdateInput {
            tone: Some(VoiceTone::Warm),
            ..Default::default()
        }
        .check()
        .is_ok());
        let mut name = VoiceUpdateInput {
            name_to_use: Some("x".repeat(61)),
            ..Default::default()
        };
        assert_eq!(name.check().unwrap_err().at, "/input/name_to_use");
        let mut notes = VoiceUpdateInput {
            notes: Some("x".repeat(301)),
            ..Default::default()
        };
        assert_eq!(notes.check().unwrap_err().at, "/input/notes");
    }
}
