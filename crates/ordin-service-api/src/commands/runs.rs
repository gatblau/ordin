//! Runs — restated from `SessionSupervisor.md` (SA-RECON-02).

use ordin_types::prelude::*;
use ordin_types::wire::StartUpReportItem;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::Run;

/// The longest answer a person may give a paused run: the 2C limit on free text passed to a harness
/// (SessionSupervisor.md *Shared Context*, "arguments and free text ≤ 16,384 characters").
const ANSWER_MAX_CHARS: usize = 16_384;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
// spec: ordin-desktop/components/SessionSupervisor#SUP-REC-1
pub struct RunView {
    pub run: Run,
    pub startup: Option<Vec<StartUpReportItem>>,
    /// The run's workspace, whatever its origin.
    pub workspace_id: WorkspaceId,
    /// The task of a task run; `None` for any other origin.
    pub task: Option<ordin_types::domain::runs::Task>,
}

/// `runs.get`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunsGetInput {
    pub run_id: RunId,
}

/// `runs.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunsListInput {
    pub workspace_id: Option<WorkspaceId>,
    pub cursor: Option<Cursor>,
}

/// `runs.stop`: calls `cancel`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunsStopInput {
    pub run_id: RunId,
}

/// `runs.answer`: `resume_with_decision(Answer)` for a run paused for the person.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunsAnswerInput {
    pub run_id: RunId,
    pub text: String,
}

/// `runs.try_again` (OrdinHost.md OH-TA-1): the task of a run that ended `interrupted` or `failed`,
/// given to the same agent again.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
// spec: ordin-desktop/components/ServiceApi#SVC-TA-1
pub struct RunsTryAgainInput {
    pub workspace_id: WorkspaceId,
    pub run_id: RunId,
}

/// What `runs.try_again` did: the new run started, or the conversation asks the person first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunsTryAgainResult {
    /// The task started again as the run `run_id`.
    Started { run_id: RunId },
    /// Nothing started yet; `text` says what the conversation is waiting for.
    Waiting { text: String },
}

impl CommandInput for RunsGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for RunsListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for RunsStopInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for RunsTryAgainInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for RunsAnswerInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.text, 0, ANSWER_MAX_CHARS, "/text")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs() {
        assert!(RunsGetInput {
            run_id: RunId::new()
        }
        .check()
        .is_ok());
        assert!(RunsStopInput {
            run_id: RunId::new()
        }
        .check()
        .is_ok());
        assert!(RunsListInput {
            workspace_id: None,
            cursor: None
        }
        .check()
        .is_ok());
        let mut long = RunsListInput {
            workspace_id: None,
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(long.check().unwrap_err().code, "SRV_BAD_CURSOR");
        assert!(RunsAnswerInput {
            run_id: RunId::new(),
            text: "Thanks".into()
        }
        .check()
        .is_ok());
        let mut big = RunsAnswerInput {
            run_id: RunId::new(),
            text: "x".repeat(16_385),
        };
        assert_eq!(big.check().unwrap_err().at, "/input/text");
        assert!(RunsTryAgainInput {
            workspace_id: WorkspaceId::new(),
            run_id: RunId::new()
        }
        .check()
        .is_ok());
        let unknown = serde_json::from_value::<RunsTryAgainInput>(serde_json::json!({
            "workspace_id": WorkspaceId::new(),
            "run_id": RunId::new(),
            "task": "x"
        }));
        assert!(unknown.is_err(), "unknown fields are refused");
        let started = RunsTryAgainResult::Started {
            run_id: RunId::new(),
        };
        assert_eq!(serde_json::to_value(&started).unwrap()["kind"], "started");
    }
}
