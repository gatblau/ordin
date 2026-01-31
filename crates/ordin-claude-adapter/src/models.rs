//! The `HarnessModels` the program gives the Usage Planner (RuntimeAdapterInterface.md, O63).

use ordin_runtime_adapter::{HarnessKind, HarnessModels, LevelModels, ModelLevel};

/// Longest model name `--model` is given (ClaudeLocalAdapter.md *Public Interface*).
const MAX_MODEL_NAME: usize = 100;

pub(crate) struct ClaudeModels;

impl HarnessModels for ClaudeModels {
    fn harness(&self) -> HarnessKind {
        HarnessKind::ClaudeCode
    }

    /// 2D's default `usage.level_models.claude_code`.
    fn default_level_models(&self) -> LevelModels {
        LevelModels::from([
            (ModelLevel::Smaller, "haiku".to_owned()),
            (ModelLevel::Standard, "sonnet".to_owned()),
            (ModelLevel::MostCapable, "opus".to_owned()),
        ])
    }

    // spec: ordin-desktop/components/ClaudeLocalAdapter#C-37
    fn per_session_model(&self) -> bool {
        true
    }

    fn accepts_model(&self, name: &str) -> bool {
        (1..=MAX_MODEL_NAME).contains(&name.chars().count())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    }
}
