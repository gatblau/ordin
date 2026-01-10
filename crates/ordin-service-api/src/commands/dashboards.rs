//! Dashboards — `dashboards.refresh` restated from `OrdinHost.md`; `dashboards.list`, `keep` and
//! `remove` from `JournalAndArtefactStore.md` (SA-RECON-02). `Dashboard`, `DashboardSource` and
//! `InputUsed` are 2C types from `ordin-types`.

use ordin_types::common::InputUsed;
use ordin_types::{CrewCharterId, Cursor, DashboardId, RunId, WorkspaceId};
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{Dashboard, DashboardSource};

/// What a refresh started (OrdinHost.md).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DashboardRefreshStarted {
    /// A dashboard made by a task: the task ran again with the same inputs.
    Run { run_id: RunId },
    /// A dashboard made by a workflow: its crew plan is shown with the same inputs; the run starts on
    /// `crews.start`.
    CrewPlan { crew_charter_id: CrewCharterId },
}

/// `dashboards.keep`: assembled by the screens from the run the person chose (**Keep as dashboard**).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NewDashboard {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub made_by: DashboardSource,
    pub inputs: Vec<InputUsed>,
    /// The run whose report is kept; its report must exist.
    pub run_id: RunId,
}

/// `dashboards.refresh`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DashboardsRefreshInput {
    pub workspace_id: WorkspaceId,
    pub dashboard_id: DashboardId,
}

/// `dashboards.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DashboardsListInput {
    pub workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

/// `dashboards.remove` (after the person's confirmation in the screens).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DashboardsRemoveInput {
    pub dashboard_id: DashboardId,
}

impl CommandInput for DashboardsRefreshInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for DashboardsListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for NewDashboard {
    fn check(&mut self) -> Result<(), InputRejection> {
        // 2C `Dashboard.name` 1–80 characters (uniqueness is the Journal's).
        rules::text(&self.name, 1, 80, "/name")?;
        // 2C `InputUsed`: name 1–120, location 1–2048, version_read 1–256 characters.
        for (i, input) in self.inputs.iter().enumerate() {
            rules::text(&input.name, 1, 120, &format!("/inputs/{i}/name"))?;
            rules::text(&input.location, 1, 2048, &format!("/inputs/{i}/location"))?;
            rules::text(
                &input.version_read,
                1,
                256,
                &format!("/inputs/{i}/version_read"),
            )?;
        }
        Ok(())
    }
}

impl CommandInput for DashboardsRemoveInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::TaskId;

    fn dashboard(name: &str, input: InputUsed) -> NewDashboard {
        NewDashboard {
            workspace_id: WorkspaceId::new(),
            name: name.to_owned(),
            made_by: DashboardSource::Task {
                task_id: TaskId::new(),
            },
            inputs: vec![input],
            run_id: RunId::new(),
        }
    }

    fn used(name: &str, location: &str, version: &str) -> InputUsed {
        InputUsed {
            name: name.to_owned(),
            location: location.to_owned(),
            version_read: version.to_owned(),
        }
    }

    #[test]
    fn keep_rules() {
        assert!(dashboard("Q3 spend", used("Rates", "place:x", "7"))
            .check()
            .is_ok());
        let cases = [
            (dashboard("", used("a", "b", "c")), "/input/name"),
            (
                dashboard(&"n".repeat(81), used("a", "b", "c")),
                "/input/name",
            ),
            (dashboard("d", used("", "b", "c")), "/input/inputs/0/name"),
            (
                dashboard("d", used("a", "", "c")),
                "/input/inputs/0/location",
            ),
            (
                dashboard("d", used("a", "b", "")),
                "/input/inputs/0/version_read",
            ),
        ];
        for (mut input, at) in cases {
            let err = input.check().unwrap_err();
            assert_eq!((err.code, err.at.as_str()), ("SVC_INPUT_INVALID", at));
        }
    }

    #[test]
    fn other_inputs_and_result() {
        let mut list = DashboardsListInput {
            workspace_id: WorkspaceId::new(),
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(list.check().unwrap_err().code, "SRV_BAD_CURSOR");
        assert!(DashboardsRefreshInput {
            workspace_id: WorkspaceId::new(),
            dashboard_id: DashboardId::new()
        }
        .check()
        .is_ok());
        assert!(DashboardsRemoveInput {
            dashboard_id: DashboardId::new()
        }
        .check()
        .is_ok());
        let run = RunId::new();
        let started = DashboardRefreshStarted::Run { run_id: run };
        assert_eq!(
            serde_json::to_value(started).unwrap(),
            serde_json::json!({"kind": "run", "run_id": run.to_string()})
        );
    }
}
