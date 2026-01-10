//! Crews — restated from `CrewOrchestrator.md` (`get`, `stop`, `continue`) and `OrdinHost.md` (`start`,
//! `discard_plan`) (SA-RECON-02).

use ordin_types::domain::{CrewRun, Round};
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

/// `crews.get`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewsGetInput {
    pub crew_run_id: CrewRunId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CrewsGetResult {
    pub crew_run: CrewRun,
    pub rounds: Vec<Round>,
}

/// `crews.stop`: `kill`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewsStopInput {
    pub crew_run_id: CrewRunId,
}

/// `crews.continue`: the program obtains the coordinator from the Ordin Host and calls `continue_crew`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewsContinueInput {
    pub crew_run_id: CrewRunId,
}

/// `crews.start` (Ordin Host): the person pressed **Start** on the crew plan.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewsStartInput {
    pub crew_charter_id: CrewCharterId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CrewsStartResult {
    pub crew_run_id: CrewRunId,
}

/// `crews.discard_plan` (Ordin Host).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewsDiscardPlanInput {
    pub crew_charter_id: CrewCharterId,
}

macro_rules! no_rules {
    ($($ty:ty),*) => {$(
        impl CommandInput for $ty {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

no_rules!(
    CrewsGetInput,
    CrewsStopInput,
    CrewsContinueInput,
    CrewsStartInput,
    CrewsDiscardPlanInput
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_have_no_rules_beyond_shape() {
        let id = CrewRunId::new();
        assert!(CrewsGetInput { crew_run_id: id }.check().is_ok());
        assert!(CrewsStopInput { crew_run_id: id }.check().is_ok());
        assert!(CrewsContinueInput { crew_run_id: id }.check().is_ok());
        assert!(CrewsStartInput {
            crew_charter_id: CrewCharterId::new()
        }
        .check()
        .is_ok());
        assert!(CrewsDiscardPlanInput {
            crew_charter_id: CrewCharterId::new()
        }
        .check()
        .is_ok());
        let unknown = serde_json::json!({"crew_run_id": id, "force": true});
        assert!(serde_json::from_value::<CrewsStopInput>(unknown).is_err());
    }
}
