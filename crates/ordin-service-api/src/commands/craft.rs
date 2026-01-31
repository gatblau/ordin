//! Edit in an app — restated from `CraftToolkit.md` (SA-RECON-02).
//!
//! Opening a document in its ArtCraft app or the system's default app, and the installed apps and
//! conversions the screens show. `CraftInstallation` is a 2C type and is imported.

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::CraftInstallation;

/// A path relative to its place: at most 4,096 bytes (2C `FileVersion.path`).
const PLACE_PATH_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenOutcome {
    /// Opened in the mapped ArtCraft app's window.
    OpenedInApp { app: String },
    /// No ArtCraft app is mapped or installed: the screens ask "No editor for this kind of file — open
    /// it in your usual app?" (11 §6).
    NoEditor,
    /// Opened with the system's default app.
    OpenedWithSystemDefault,
}

/// `craft.open` (**Edit in [app]**).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftOpenInput {
    pub place_id: PlaceId,
    pub path: String,
}

/// `craft.open_with_system_default` (**Open in your usual app**).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftOpenWithSystemDefaultInput {
    pub place_id: PlaceId,
    pub path: String,
}

/// `craft.installations`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftInstallationsInput {}

/// `craft.conversions`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftConversionsInput {}

/// `craft.conversions`: which copy formats can be made now (C-28).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CraftConversionsResult {
    pub word: bool,
    pub pdf: bool,
    pub slides: bool,
}

/// A path relative to its place: 1–4,096 bytes, no leading `/`, no `..` segment.
fn place_path(value: &str) -> Result<(), InputRejection> {
    rules::bytes(value, 1, PLACE_PATH_MAX_BYTES, "/path")?;
    if value.starts_with('/') || value.split(['/', '\\']).any(|segment| segment == "..") {
        return Err(rules::reject("/path"));
    }
    Ok(())
}

impl CommandInput for CraftOpenInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        place_path(&self.path)
    }
}

impl CommandInput for CraftOpenWithSystemDefaultInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        place_path(&self.path)
    }
}

impl CommandInput for CraftInstallationsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for CraftConversionsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_paths() {
        let place_id = PlaceId::new();
        assert!(CraftOpenInput {
            place_id,
            path: "Q3/logo.png".into()
        }
        .check()
        .is_ok());
        for bad in ["", "/a", "a/../b"] {
            let err = CraftOpenWithSystemDefaultInput {
                place_id,
                path: bad.into(),
            }
            .check()
            .unwrap_err();
            assert_eq!(
                (err.code, err.at.as_str()),
                ("SVC_INPUT_INVALID", "/input/path")
            );
        }
        assert!(CraftOpenWithSystemDefaultInput {
            place_id,
            path: "a".into()
        }
        .check()
        .is_ok());
        assert!(CraftInstallationsInput {}.check().is_ok());
        assert!(CraftConversionsInput {}.check().is_ok());
    }

    #[test]
    fn outcome_shape() {
        let outcome = OpenOutcome::OpenedInApp {
            app: "photocraft".into(),
        };
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            serde_json::json!({"kind": "opened_in_app", "app": "photocraft"})
        );
        assert_eq!(
            serde_json::to_value(OpenOutcome::NoEditor).unwrap(),
            serde_json::json!({"kind": "no_editor"})
        );
    }
}
