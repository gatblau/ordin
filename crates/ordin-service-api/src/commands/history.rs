//! History retention — restated from `JournalAndArtefactStore.md` (SA-RECON-02).
//!
//! `history.set_days` checks no range here: a value under 1 must reach the Journal, whose
//! `JRNL_RETENTION_INVALID` sentence ("Enter a whole number of days, 1 or more.") the person sees.

use ordin_types::time::Timestamp;
use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RetentionSetting {
    pub days: u32,
    /// The company maximum when the organisation profile sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_max_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_pruned_at: Option<Timestamp>,
}

/// `history.get`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HistoryGetInput {}

/// `history.set_days`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HistorySetDaysInput {
    pub days: u32,
}

impl CommandInput for HistoryGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for HistorySetDaysInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        // The Journal's example reply.
        let reply: RetentionSetting = serde_json::from_value(serde_json::json!(
            {"days": 120, "company_max_days": 365, "last_pruned_at": "2026-01-10T04:00:01.004Z"}
        ))
        .unwrap();
        assert_eq!(reply.days, 120);
        let bare = RetentionSetting {
            days: 90,
            company_max_days: None,
            last_pruned_at: None,
        };
        assert_eq!(
            serde_json::to_value(bare).unwrap(),
            serde_json::json!({"days": 90})
        );
        assert!(HistoryGetInput {}.check().is_ok());
        // Zero passes here and is refused by the Journal with its own code.
        assert!(HistorySetDaysInput { days: 0 }.check().is_ok());
        assert!(serde_json::from_value::<HistoryGetInput>(serde_json::json!({"x": 1})).is_err());
    }
}
