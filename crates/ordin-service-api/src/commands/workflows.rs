//! Workflows — restated from `OrdinHost.md` *Service API commands* (SA-RECON-02). A version's content is
//! the 2C wire §8 `WorkflowFile` (format 3), imported.

use std::collections::BTreeMap;

use ordin_types::common::AgentRef;
use ordin_types::prelude::*;
use serde::{Deserialize, Deserializer, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{ShareHost, Workflow, WorkflowShare, WorkflowState};
pub use ordin_types::wire::WorkflowFile;

/// A workflow's name: 1–80 characters (`WFF_NAME_INVALID` at the owner).
const NAME_MAX_CHARS: usize = 80;
/// `change_note`: one line, 1–200 characters (`WFF_CHANGE_NOTE_INVALID` at the owner).
const CHANGE_NOTE_MAX_CHARS: usize = 200;
/// `workflows.import.yaml`: at most 256 KB (`WFF_FILE_TOO_LARGE` at the owner).
const YAML_MAX_BYTES: usize = 256 * 1024;

/// A folder of a GitHub or GitLab repository that holds workflow files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryFolder {
    pub host: ShareHost,
    pub repository: String,
    pub folder: String,
}

/// A pull or merge request the Push Service proposes for a shared workflow (UUIDv7).
///
/// SPEC-GAP-8: owned by `PushService.md` (`pub struct SubmissionId(pub uuid::Uuid)`), which no other
/// catalogue row names, so it is restated here; this crate has no `uuid` dependency, so it keeps the
/// canonical 36-character text, checked as every 2C identifier is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(transparent)]
pub struct SubmissionId(String);

impl SubmissionId {
    /// Parses the 36-character form; refuses anything but a lowercase hyphenated v7 UUID.
    pub fn parse(raw: &str) -> Result<Self, ordin_types::TypesError> {
        ordin_types::id::parse_v7(raw).map(|_| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SubmissionId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowSummary {
    pub workflow_id: WorkflowId,
    pub name: String,
    pub state: WorkflowState,
    pub latest_version: Option<u32>,
    pub shared: Option<WorkflowShare>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowView {
    pub workflow: Workflow,
    pub version: Option<u32>,
    /// The version's content, format 3.
    pub file: WorkflowFile,
    /// Newest first.
    pub versions: Vec<VersionLine>,
    /// Per `agents[].key`.
    pub agents: Vec<AgentPresence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct VersionLine {
    pub version: u32,
    pub change_note: String,
    pub saved_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from_version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "presence", rename_all = "snake_case")]
pub enum AgentPresence {
    Present {
        key: String,
        agent: AgentRef,
    },
    OtherVersion {
        key: String,
        agent: AgentRef,
        installed_version: String,
    },
    Missing {
        key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowCheckReport {
    /// Refusals; empty ⇒ may be saved.
    pub problems: Vec<CheckProblem>,
    /// Lowered limits, renamed, missing agents.
    pub notices: Vec<CheckProblem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CheckProblem {
    /// `WFF_…` or `OH_…`.
    pub code: String,
    /// The step's position ("2.1") or a field path ("/limits/max_rounds").
    pub at: String,
    /// One sentence for the person, built from the catalogue (Localisation).
    pub message_id: String,
    pub args: BTreeMap<String, String>,
}

// SPEC-GAP-9: OrdinHost.md step 28 says the reply of `workflows.list_in_repositories` also reports
// `unreachable: [folder]`, while both the owner's command table and the catalogue give the result as
// `Vec<RepositoryWorkflowFile>`; the table's form is restated.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryWorkflowFile {
    pub folder: RepositoryFolder,
    pub file_name: String,
    pub company: bool,
    pub last_changed_by: String,
    pub last_changed_at: Timestamp,
}

// ---- inputs and results, in catalogue order ----

/// `workflows.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsListInput {
    pub workspace_id: WorkspaceId,
    pub cursor: Option<Cursor>,
}

/// `workflows.get`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsGetInput {
    pub workflow_id: WorkflowId,
    pub version: Option<u32>,
}

/// `workflows.check`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsCheckInput {
    pub workspace_id: WorkspaceId,
    pub file: WorkflowFile,
}

/// `workflows.save`; a check failure is refused with `OH_CHECK_FAILED` carrying the report.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsSaveInput {
    pub workflow_id: WorkflowId,
    pub base_version: Option<u32>,
    pub file: WorkflowFile,
    pub change_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsSaveResult {
    pub version: u32,
}

/// `workflows.discard_draft`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsDiscardDraftInput {
    pub workflow_id: WorkflowId,
}

/// `workflows.restore`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsRestoreInput {
    pub workflow_id: WorkflowId,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsRestoreResult {
    pub version: u32,
}

/// `workflows.rename`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsRenameInput {
    pub workflow_id: WorkflowId,
    pub name: String,
}

/// `workflows.duplicate`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsDuplicateInput {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub name: String,
    pub to_workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsDuplicateResult {
    pub workflow_id: WorkflowId,
}

/// `workflows.remove`: a confirmation, not an approval card (D11).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsRemoveInput {
    pub workflow_id: WorkflowId,
    pub confirmed: bool,
}

/// `workflows.export`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsExportInput {
    pub workflow_id: WorkflowId,
    pub version: u32,
}

/// The screens write it where the person chooses, with `files.save_export`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsExportResult {
    pub file_name: String,
    /// At most 256 KB.
    pub yaml: String,
}

/// `workflows.import`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsImportInput {
    pub workspace_id: WorkspaceId,
    pub file_name: String,
    pub yaml: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsImportResult {
    pub workflow_id: WorkflowId,
    pub check: WorkflowCheckReport,
}

/// `workflows.list_in_repositories`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsListInRepositoriesInput {
    pub workspace_id: WorkspaceId,
}

/// `workflows.add_from_repository`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsAddFromRepositoryInput {
    pub workspace_id: WorkspaceId,
    pub host: ShareHost,
    pub repository: String,
    pub folder: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsAddFromRepositoryResult {
    pub workflow_id: WorkflowId,
    pub check: WorkflowCheckReport,
}

/// `workflows.share`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsShareInput {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub folder: RepositoryFolder,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsShareResult {
    pub submission_id: SubmissionId,
}

/// `workflows.send_back`: a share of the latest version to the shared folder.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsSendBackInput {
    pub workflow_id: WorkflowId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsSendBackResult {
    pub submission_id: SubmissionId,
}

/// `workflows.use_newer`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsUseNewerInput {
    pub workflow_id: WorkflowId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsUseNewerResult {
    pub version: u32,
}

/// `workflows.keep_as_own`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsKeepAsOwnInput {
    pub workflow_id: WorkflowId,
}

/// `workflows.run`: shows the crew plan; the run starts on `crews.start`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsRunInput {
    pub workflow_id: WorkflowId,
    pub version: Option<u32>,
    pub request: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkflowsRunResult {
    pub crew_charter_id: CrewCharterId,
}

// ---- checks ----

fn name(value: &str) -> Result<(), InputRejection> {
    rules::text(value.trim(), 1, NAME_MAX_CHARS, "/name")
}

impl CommandInput for WorkflowsListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for WorkflowsSaveInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.change_note, 1, CHANGE_NOTE_MAX_CHARS, "/change_note")?;
        if self.change_note.contains('\n') {
            return Err(rules::reject("/change_note"));
        }
        Ok(())
    }
}

impl CommandInput for WorkflowsRenameInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        name(&self.name)
    }
}

impl CommandInput for WorkflowsDuplicateInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        name(&self.name)
    }
}

impl CommandInput for WorkflowsImportInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::file_name(&self.file_name, "/file_name")?;
        rules::bytes(&self.yaml, 0, YAML_MAX_BYTES, "/yaml")
    }
}

impl CommandInput for WorkflowsAddFromRepositoryInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::file_name(&self.file_name, "/file_name")
    }
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
    WorkflowsGetInput,
    WorkflowsCheckInput,
    WorkflowsDiscardDraftInput,
    WorkflowsRestoreInput,
    WorkflowsRemoveInput,
    WorkflowsExportInput,
    WorkflowsListInRepositoriesInput,
    WorkflowsShareInput,
    WorkflowsSendBackInput,
    WorkflowsUseNewerInput,
    WorkflowsKeepAsOwnInput,
    WorkflowsRunInput
);

#[cfg(test)]
mod tests {
    use super::*;

    fn at(result: Result<(), InputRejection>) -> Option<String> {
        result.err().map(|r| r.at)
    }

    #[test]
    fn names_notes_and_files() {
        let id = WorkflowId::new();
        assert_eq!(
            at(WorkflowsRenameInput {
                workflow_id: id,
                name: "Quarterly review".into()
            }
            .check()),
            None
        );
        assert_eq!(
            at(WorkflowsRenameInput {
                workflow_id: id,
                name: "  ".into()
            }
            .check())
            .as_deref(),
            Some("/input/name")
        );
        let dup = |name: String| {
            WorkflowsDuplicateInput {
                workflow_id: id,
                version: 1,
                name,
                to_workspace_id: WorkspaceId::new(),
            }
            .check()
        };
        assert_eq!(at(dup("x".repeat(81))).as_deref(), Some("/input/name"));
        let import = |file_name: &str, yaml: String| {
            WorkflowsImportInput {
                workspace_id: WorkspaceId::new(),
                file_name: file_name.into(),
                yaml,
            }
            .check()
        };
        assert_eq!(at(import("a.workflow.yaml", "name: a".into())), None);
        assert_eq!(
            at(import("a/b.yaml", String::new())).as_deref(),
            Some("/input/file_name")
        );
        assert_eq!(
            at(import("a.yaml", "x".repeat(256 * 1024 + 1))).as_deref(),
            Some("/input/yaml")
        );
        let add = |file_name: &str| {
            WorkflowsAddFromRepositoryInput {
                workspace_id: WorkspaceId::new(),
                host: ShareHost::Github,
                repository: "acme/flows".into(),
                folder: String::new(),
                file_name: file_name.into(),
            }
            .check()
        };
        assert_eq!(at(add("")).as_deref(), Some("/input/file_name"));
        assert_eq!(at(add("q.workflow.yaml")), None);
    }

    #[test]
    fn save_change_note() {
        let file = WorkflowFile {
            format: 3,
            name: "Quarterly review".into(),
            purpose: "Compare the quarter's quotes".into(),
            inputs: vec![],
            agents: vec![],
            steps: vec![],
            limits: ordin_types::wire::WorkflowFileLimits {
                max_rounds: 10,
                wall_clock_minutes: 60,
                request_budget: None,
                agents_at_once: 4,
                failure_repeat_limit: 2,
                external_action_agents: vec![],
            },
            origin: "Saved from the run of 16 Oct".into(),
        };
        let save = |note: &str| {
            WorkflowsSaveInput {
                workflow_id: WorkflowId::new(),
                base_version: None,
                file: file.clone(),
                change_note: note.into(),
            }
            .check()
        };
        assert_eq!(at(save("First version")), None);
        assert_eq!(at(save("")).as_deref(), Some("/input/change_note"));
        assert_eq!(
            at(save("two\nlines")).as_deref(),
            Some("/input/change_note")
        );
        let mut check = WorkflowsCheckInput {
            workspace_id: WorkspaceId::new(),
            file,
        };
        assert!(check.check().is_ok());
    }

    #[test]
    fn list_cursor_and_ids() {
        let mut list = WorkflowsListInput {
            workspace_id: WorkspaceId::new(),
            cursor: Some(Cursor::encode(&"x".repeat(900))),
        };
        assert_eq!(list.check().unwrap_err().code, "SRV_BAD_CURSOR");
        let id = "0192a1c0-0d1e-7f20-8a31-425364758697";
        let submission = SubmissionId::parse(id).unwrap();
        assert_eq!(submission.as_str(), id);
        assert!(serde_json::from_value::<SubmissionId>(serde_json::json!("nope")).is_err());
        let wid = WorkflowId::new();
        assert!(WorkflowsGetInput {
            workflow_id: wid,
            version: None
        }
        .check()
        .is_ok());
        assert!(WorkflowsDiscardDraftInput { workflow_id: wid }
            .check()
            .is_ok());
        assert!(WorkflowsRestoreInput {
            workflow_id: wid,
            version: 2
        }
        .check()
        .is_ok());
        assert!(WorkflowsRemoveInput {
            workflow_id: wid,
            confirmed: true
        }
        .check()
        .is_ok());
        assert!(WorkflowsExportInput {
            workflow_id: wid,
            version: 1
        }
        .check()
        .is_ok());
        assert!(WorkflowsListInRepositoriesInput {
            workspace_id: WorkspaceId::new()
        }
        .check()
        .is_ok());
        let folder = RepositoryFolder {
            host: ShareHost::Gitlab,
            repository: "acme/flows".into(),
            folder: "shared".into(),
        };
        assert!(WorkflowsShareInput {
            workflow_id: wid,
            version: 1,
            folder
        }
        .check()
        .is_ok());
        assert!(WorkflowsSendBackInput { workflow_id: wid }.check().is_ok());
        assert!(WorkflowsUseNewerInput { workflow_id: wid }.check().is_ok());
        assert!(WorkflowsKeepAsOwnInput { workflow_id: wid }.check().is_ok());
        assert!(WorkflowsRunInput {
            workflow_id: wid,
            version: None,
            request: None
        }
        .check()
        .is_ok());
    }

    #[test]
    fn presence_tag() {
        let json = serde_json::json!({"presence": "missing", "key": "analyst"});
        let presence: AgentPresence = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(presence).unwrap(), json);
    }
}
