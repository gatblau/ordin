//! Artefacts — restated from `JournalAndArtefactStore.md` (SA-RECON-02).
//!
//! `Artefact` and `ArtefactLocation` are 2C types from `ordin-types`; `ArtefactFilter` is the
//! Journal's supporting type.

use ordin_types::{Cursor, RunId};
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{Artefact, ArtefactLocation};

/// The longest artefact path, in bytes (2C `Artefact.path`, `ART_PATH_INVALID`).
const PATH_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ArtefactFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<ArtefactLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producing_run_id: Option<RunId>,
}

/// `artefacts.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ArtefactsListInput {
    pub filter: ArtefactFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// `artefacts.versions`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ArtefactsVersionsInput {
    pub location: ArtefactLocation,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ArtefactsVersionsResult {
    pub items: Vec<Artefact>,
}

impl CommandInput for ArtefactsListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for ArtefactsVersionsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // 2C `Artefact.path`: relative to its folder, 1–4096 bytes, no `..` segment, no NUL.
        if self.path.is_empty()
            || self.path.len() > PATH_MAX_BYTES
            || self.path.starts_with('/')
            || self.path.contains('\0')
            || self.path.split(['/', '\\']).any(|segment| segment == "..")
        {
            return Err(rules::reject("/path"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::PlaceId;

    fn versions(path: &str) -> ArtefactsVersionsInput {
        ArtefactsVersionsInput {
            location: ArtefactLocation::Place {
                place_id: PlaceId::new(),
            },
            path: path.to_owned(),
        }
    }

    #[test]
    fn version_paths() {
        assert!(versions("reports/q3.pdf").check().is_ok());
        for bad in ["", "/abs", "a/../b", "a\0b", &"x".repeat(4097)] {
            let err = versions(bad).check().unwrap_err();
            assert_eq!(
                (err.code, err.at.as_str()),
                ("SVC_INPUT_INVALID", "/input/path"),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn list_cursor() {
        let mut ok = ArtefactsListInput {
            filter: ArtefactFilter::default(),
            cursor: None,
        };
        assert!(ok.check().is_ok());
        let mut long = ArtefactsListInput {
            filter: ArtefactFilter::default(),
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(long.check().unwrap_err().code, "SRV_BAD_CURSOR");
    }
}
