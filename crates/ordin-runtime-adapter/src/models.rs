//! The level-to-model contract the Usage Planner reaches the harness adapters through
//! (2B-2 D9; 2C wire §2, decision 80: C-11).

use crate::{HarnessKind, ModelLevel};

/// A harness's map from level to model name. A level absent from the map uses the harness's
/// automatic selection.
pub type LevelModels = std::collections::BTreeMap<ModelLevel, String>;

/// Implemented by each local harness adapter (Claude Local, Copilot Local) and given to the
/// Usage Planner by the program (2B-2 D9). Remote adapters do not implement it: a remote
/// agent's model is the remote side's.
///
/// C-37 fallback: until the item is confirmed for a harness, its adapter answers `false` from
/// `per_session_model`, ignores `StartRun.model` and the run records `ModelChoice { level,
/// model: None }`.
// spec: ordin-desktop/components/RuntimeAdapterInterface#C-37
pub trait HarnessModels: Send + Sync {
    fn harness(&self) -> HarnessKind;
    /// The map used when the organisation profile has no `usage.level_models.<harness>` entry.
    fn default_level_models(&self) -> LevelModels;
    /// True when the harness accepts a model per session (C-37). When false, `StartRun.model`
    /// is ignored by the adapter and the run records `ModelChoice { level, model: None }`.
    fn per_session_model(&self) -> bool;
    /// True when `name` is a model this harness version accepts; false makes the Usage Planner
    /// fall back to the harness's automatic selection and journal `RA_MODEL_UNKNOWN`.
    fn accepts_model(&self, name: &str) -> bool;
}
