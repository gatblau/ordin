//! The `HarnessModels` the program gives the Usage Planner (CopilotLocalAdapter.md *Public
//! Interface*; RuntimeAdapterInterface.md, O63).

use ordin_runtime_adapter::{HarnessKind, HarnessModels, LevelModels, ModelLevel};

/// Longest model name a session is given.
const MAX_MODEL_NAME: usize = 100;
/// Copilot's own selection (2D-2).
const AUTOMATIC: &str = "automatic";

pub(crate) struct CopilotModels;

impl HarnessModels for CopilotModels {
    fn harness(&self) -> HarnessKind {
        HarnessKind::CopilotCli
    }

    /// 2D's default `usage.level_models.copilot`: `automatic` for every level.
    fn default_level_models(&self) -> LevelModels {
        LevelModels::from([
            (ModelLevel::Smaller, AUTOMATIC.to_owned()),
            (ModelLevel::Standard, AUTOMATIC.to_owned()),
            (ModelLevel::MostCapable, AUTOMATIC.to_owned()),
        ])
    }

    /// `SessionOptions.model`; `None` is Copilot's automatic selection.
    fn per_session_model(&self) -> bool {
        true
    }

    /// `automatic`, or 1–100 characters of `[A-Za-z0-9._-]`.
    fn accepts_model(&self, name: &str) -> bool {
        name == AUTOMATIC
            || ((1..=MAX_MODEL_NAME).contains(&name.chars().count())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
    }
}
