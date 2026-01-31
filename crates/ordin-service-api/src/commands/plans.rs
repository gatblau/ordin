//! Plans — restated from `JournalAndArtefactStore.md` (SA-RECON-02). `RunStep` is the 2C entity.

use ordin_types::RunId;
use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::RunStep;

/// `plans.get`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PlansGetInput {
    pub run_id: RunId,
}

/// The current plan's steps (`current_plan`).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PlansGetResult {
    pub items: Vec<RunStep>,
}

impl CommandInput for PlansGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_shape() {
        let id = RunId::new();
        let mut input: PlansGetInput =
            serde_json::from_value(serde_json::json!({ "run_id": id.to_string() })).unwrap();
        assert!(input.check().is_ok());
        assert!(serde_json::from_value::<PlansGetInput>(
            serde_json::json!({ "run_id": id.to_string(), "x": 1 })
        )
        .is_err());
        let empty = PlansGetResult { items: vec![] };
        assert_eq!(
            serde_json::to_value(empty).unwrap(),
            serde_json::json!({"items": []})
        );
    }
}
