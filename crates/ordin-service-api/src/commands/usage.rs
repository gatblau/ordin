//! Using your plan — restated from `UsagePlanner.md` (SA-RECON-02).
//!
//! `usage.get_settings` and `usage.update_settings` answer the 2C `UsageSettings`; `usage.today` answers
//! the planner's `Today` (per-mille, no floats on the wire).

use std::collections::BTreeMap;

use ordin_types::domain::{AskBefore, SummariseWhen, UsageChoice};
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

pub use ordin_types::domain::UsageSettings;

use crate::api::{CommandInput, InputRejection};

/// Today's use of the plan.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Today {
    pub model_turns: u32,
    /// Per-mille of today's turns that ran on the Smaller level.
    pub smaller_share_permille: u32,
    pub answered_without_model: u32,
    pub typical_day_permille: u32,
}

/// A change the person makes in Settings › Using your plan; absent fields are unchanged.
///
/// The owner's rules on these values (the company's frame, agents at once 1–3, a known kind or level)
/// carry its own codes (`USAGE_FIXED_BY_COMPANY`, `USAGE_ABOVE_PROFILE_LIMIT`,
/// `USAGE_AGENTS_OUT_OF_RANGE`, `USAGE_LEVEL_UNKNOWN`), whose sentences name the limit, so this crate
/// refuses only by shape.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageSettingsChange {
    pub choice: Option<UsageChoice>,
    pub model_level_per_kind: Option<BTreeMap<String, String>>,
    pub thinking: Option<ThinkingLevel>,
    pub answer_simple_requests_without_model: Option<bool>,
    pub reuse_answers: Option<bool>,
    pub give_agents_only_needed_parts: Option<bool>,
    pub summarise_long_conversations: Option<SummariseWhen>,
    pub continue_from_summary_after_break: Option<bool>,
    pub ask_before_estimated_requests: Option<AskBefore>,
    pub agents_at_same_time: Option<u32>,
    pub show_usage_on_each_run: Option<bool>,
    // SPEC-GAP-16: the owner types this `Option<Option<u32>>` with no serde attribute, so JSON `null`
    // ("no daily pause") reads as "unchanged" and cannot be told from an absent key; restated as written.
    pub daily_pause_requests: Option<Option<u32>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageGetSettingsInput {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageTodayInput {}

impl CommandInput for UsageGetSettingsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for UsageTodayInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for UsageSettingsChange {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inputs() {
        let mut change: UsageSettingsChange =
            serde_json::from_value(json!({ "agents_at_same_time": 2, "reuse_answers": false }))
                .expect("change");
        assert!(change.check().is_ok());
        assert!(serde_json::from_value::<UsageSettingsChange>(json!({ "plan": "pro" })).is_err());
        assert!(UsageGetSettingsInput {}.check().is_ok());
        assert!(UsageTodayInput {}.check().is_ok());
        assert!(serde_json::from_value::<UsageTodayInput>(json!({ "x": 1 })).is_err());
    }

    #[test]
    fn today_shape() {
        let today = Today {
            model_turns: 40,
            smaller_share_permille: 250,
            answered_without_model: 3,
            typical_day_permille: 160,
        };
        let out = serde_json::to_value(&today).expect("serialise");
        assert_eq!(out["smaller_share_permille"], 250);
    }
}
