//! Runs — restated from `SessionSupervisor.md` (SA-RECON-02).

use ordin_types::prelude::*;
use ordin_types::wire::{ContentRef, StartUpReportItem};
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

/// `runs.start_task` (OrdinHost.md OH-TF-1): an agent's task started from its form (ADR-119).
// spec: ordin-desktop/components/ServiceApi#SVC-TF-1
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunsStartTaskInput {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    /// `TaskForm.command_id` (PackageCatalogue.md PKG-TF-1), 1–300 characters.
    pub command_id: String,
    /// 0–8 values; at most one per input name. A task without inputs takes one value named
    /// `arguments` of kind `text`.
    pub values: Vec<TaskInputValue>,
    /// The waiting form Ordin opened for this task (2C wire WIRE-TF-1), when the form was sent from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<BlockId>,
}

/// One field's value of a task's form.
// SPEC-GAP-28: ServiceApi.md SVC-TF-1 says unknown fields of `RunsStartTaskInput` are refused and
// gives `TaskInputValue` a `#[serde(flatten)]` value; serde cannot refuse unknown keys of a struct
// holding a flattened field (as SPEC-GAP-3), so an unknown key inside one value is ignored, not refused.
// spec: ordin-desktop/components/ServiceApi#SVC-TF-1
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TaskInputValue {
    /// `^[a-z][a-z0-9_]{0,31}$`.
    pub name: String,
    #[serde(flatten)]
    pub value: TaskValue,
}

/// Short text, long text, number and date travel as `text`; the core reads number and date from it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskValue {
    /// 0–4,000 characters.
    Text { text: String },
    /// `PlaceFile` only.
    Document { document: ContentRef },
    /// 0–20, each `PlaceFile`.
    Documents { documents: Vec<ContentRef> },
    /// 0–60 characters.
    Choice { choice: String },
}

/// What `runs.start_task` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunsStartTaskResult {
    /// The task started; the screen opens `run:<run_id>`.
    Started { run_id: RunId },
    /// Nothing started: the conversation asks the person first. `text` is written for Ordin and is
    /// not shown.
    Waiting { text: String },
    /// Nothing started: the values break the task's inputs. One entry per field, in input order.
    Refused { problems: Vec<TaskInputProblem> },
}

/// One field's problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TaskInputProblem {
    pub name: String,
    pub problem: TaskProblem,
    /// For `document_gone`: the file name of the document that is no longer in the workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskProblem {
    RequiredEmpty,
    TooLong,
    NotANumber,
    NotADate,
    NotAChoice,
    DocumentGone,
    TooFewDocuments,
    TooManyDocuments,
    WrongKind,
    UnknownInput,
}

/// The most values a task takes (AgentModel.md AM-TF-2 `TASK_INPUTS_MAX`).
const TASK_VALUES_MAX: usize = 8;
/// `TaskValue::Text`, at most 4,000 characters (AM-TF-2 `LONG_TEXT_MAX`).
const TASK_TEXT_MAX: usize = 4_000;
const TASK_CHOICE_MAX: usize = 60;
const TASK_DOCUMENTS_MAX: usize = 20;
const TASK_COMMAND_ID_MAX: usize = 300;

fn task_input_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    first_ok
        && name.chars().count() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn place_file(item: &ContentRef, at: &str) -> Result<(), InputRejection> {
    match item {
        ContentRef::PlaceFile { relative_path, .. } => {
            rules::relative_path(relative_path, &format!("{at}/relative_path"))
        }
        _ => Err(rules::reject(at)),
    }
}

// spec: ordin-desktop/components/ServiceApi#SVC-TF-1
impl CommandInput for RunsStartTaskInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.command_id, 1, TASK_COMMAND_ID_MAX, "/command_id")?;
        rules::count(&self.values, 0, TASK_VALUES_MAX, "/values")?;
        for (i, value) in self.values.iter().enumerate() {
            let at = format!("/values/{i}");
            if !task_input_name(&value.name) {
                return Err(rules::reject(&format!("{at}/name")));
            }
            if self.values[..i].iter().any(|v| v.name == value.name) {
                return Err(rules::reject(&format!("{at}/name")));
            }
            match &value.value {
                TaskValue::Text { text } => {
                    rules::text(text, 0, TASK_TEXT_MAX, &format!("{at}/text"))?
                }
                TaskValue::Choice { choice } => {
                    rules::text(choice, 0, TASK_CHOICE_MAX, &format!("{at}/choice"))?
                }
                TaskValue::Document { document } => {
                    place_file(document, &format!("{at}/document"))?
                }
                TaskValue::Documents { documents } => {
                    rules::count(documents, 0, TASK_DOCUMENTS_MAX, &format!("{at}/documents"))?;
                    for (j, item) in documents.iter().enumerate() {
                        place_file(item, &format!("{at}/documents/{j}"))?;
                    }
                }
            }
        }
        Ok(())
    }
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
