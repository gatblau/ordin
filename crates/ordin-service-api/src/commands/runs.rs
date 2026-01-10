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
pub struct RunView {
    pub run: Run,
    pub startup: Option<Vec<StartUpReportItem>>,
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
    }
}
