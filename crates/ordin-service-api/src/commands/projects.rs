//! Code (engineering mode) — restated from `ProjectService.md` (SA-RECON-02); the run-check origin from
//! `RulesEngine.md`.
//!
//! The inputs are the method parameters of the Project Service and the results its return types
//! (ProjectService.md *Service API commands*). `Project`, `CodingTask`, `ChangeSet`,
//! `GithubOperationSetting` and `LanguagePlugin` are 2C types and are imported.

use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{
    ChangeSet, CodingTask, GithubOperationSetting, LanguagePlugin, Project,
};
pub use ordin_types::num::Sha256Hex;
pub use ordin_types::time::Timestamp;
pub use ordin_types::{
    AgentInstanceId, ChangeSetEntryId, CodingTaskId, FileVersionId, PlaceId, ProjectId, RunId,
    WorkspaceId,
};

/// An unjournalled stream to the screens (terminal bytes, language-server messages).
// SPEC-GAP-26: the owner types this as `StreamId(pub uuid::Uuid)`; `uuid` is not a dependency of this crate,
// so the id is carried as its 36-character lowercase hyphenated text (any UUID version), checked on read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct StreamId(pub String);

impl<'de> Deserialize<'de> for StreamId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        if is_uuid_text(&raw) {
            Ok(StreamId(raw))
        } else {
            Err(serde::de::Error::custom("not a lowercase hyphenated UUID"))
        }
    }
}

/// True for a 36-character lowercase hyphenated UUID of any version.
fn is_uuid_text(raw: &str) -> bool {
    raw.len() == 36
        && raw.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

// ---- results ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StatusEntry {
    pub path: String,
    /// "modified", "new", "deleted", "renamed", "conflicted".
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CommitSummary {
    pub commit: String,
    pub author: String,
    pub at: Timestamp,
    pub subject: String,
}

/// Result of the engineer's `push`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PushOutcome {
    pub commit: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReviewTask {
    pub task: CodingTask,
    pub change_set: Option<ChangeSet>,
    pub new_problems: Vec<Problem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Problem {
    pub file: String,
    pub line: u32,
    pub severity: String,
    pub code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryDecision {
    Accept,
    Reject,
    KeepYours,
    UseTheAgents,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionOutcome {
    Applied { file_version_id: FileVersionId },
    Dropped,
    Conflict { yours: String, agents: String },
}

/// Who asked for a command (Rules Engine's `CommandOrigin`, restated from `RulesEngine.md`).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandOrigin {
    Person,
    Run {
        run_id: RunId,
        agent_instance_id: AgentInstanceId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProjectCheckResult {
    pub command: String,
    pub exit_code: Option<i32>,
    pub output: String,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct EditorSession {
    pub stream: StreamId,
    pub language: Option<String>,
    pub server_line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Ide {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

/// One followed original's change in a repository (ADR-069).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryOriginChange {
    pub place_id: PlaceId,
    pub path: String,
    pub branch: String,
    pub commit: String,
    pub author: String,
    pub committed_at: Timestamp,
}

/// The Project Service's `fetch_followed` result (`projects.check_now`).
// Named apart from the Catalogue Service's `FetchOutcome` (ProjectService.md, amended January 2026).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProjectFetchOutcome {
    pub place_id: PlaceId,
    pub branch: String,
    pub commit: String,
    pub checked_at: Timestamp,
    pub changes: Vec<RepositoryOriginChange>,
}

// ---- inputs (mechanical names, SA-RECON-02) ----

/// Declares one input struct with `deny_unknown_fields`.
macro_rules! input {
    ($(#[$doc:meta])* $name:ident { $($(#[$fdoc:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        pub struct $name { $($(#[$fdoc])* pub $field: $ty),* }
    };
}

input!(/// `projects.open` (`open_project`).
    ProjectsOpenInput { place_id: PlaceId, workspace_id: WorkspaceId });
input!(/// `projects.close` (`close_project`).
    ProjectsCloseInput { project_id: ProjectId });
input!(/// `projects.status`.
    ProjectsStatusInput { project_id: ProjectId });
input!(/// `projects.diff`.
    ProjectsDiffInput { project_id: ProjectId, path: Option<String> });
input!(/// `projects.history`.
    ProjectsHistoryInput { project_id: ProjectId, path: Option<String>, limit: u32 });
input!(/// `projects.branches`.
    ProjectsBranchesInput { project_id: ProjectId });
input!(/// `projects.create_branch`.
    ProjectsCreateBranchInput { project_id: ProjectId, name: String });
input!(/// `projects.switch_branch`.
    ProjectsSwitchBranchInput { project_id: ProjectId, name: String });
input!(/// `projects.commit`.
    ProjectsCommitInput { project_id: ProjectId, message: String, terminal: Option<StreamId> });
input!(/// `projects.push`.
    ProjectsPushInput { project_id: ProjectId, terminal: Option<StreamId> });
input!(/// `projects.save_file` (9 MiB body).
ProjectsSaveFileInput {
    project_id: ProjectId,
    path: String,
    text: String,
    expected_hash: Option<Sha256Hex>,
});
input!(/// `projects.review`.
    ProjectsReviewInput { project_id: ProjectId });
input!(/// `projects.decide_entry`.
    ProjectsDecideEntryInput { entry: ChangeSetEntryId, decision: EntryDecision });
input!(/// `projects.keep_attempt`.
    ProjectsKeepAttemptInput { task: CodingTaskId });
input!(/// `projects.run_check`.
ProjectsRunCheckInput {
    project_id: ProjectId,
    task: Option<CodingTaskId>,
    command: String,
    origin: CommandOrigin,
});
input!(/// `projects.git_host_settings`.
    ProjectsGitHostSettingsInput { project_id: ProjectId });
input!(/// `projects.set_git_host_approval`.
    ProjectsSetGitHostApprovalInput { project_id: ProjectId, kind: String, approval_required: bool });
input!(/// `projects.language_plugins`.
    ProjectsLanguagePluginsInput {});
input!(/// `projects.configure_language`.
ProjectsConfigureLanguageInput {
    language: String,
    chosen_server: Option<String>,
    enabled: bool,
    server_settings: serde_json::Value,
});
input!(/// `projects.restart_language_server`.
    ProjectsRestartLanguageServerInput { language: String });
input!(/// `projects.editor_open`.
    ProjectsEditorOpenInput { project_id: ProjectId, path: String });
input!(/// `projects.lsp_send`.
    ProjectsLspSendInput { session: StreamId, message: serde_json::Value });
input!(/// `projects.editor_close`.
    ProjectsEditorCloseInput { session: StreamId });
input!(/// `projects.ides`.
    ProjectsIdesInput {});
input!(/// `projects.set_ide_enabled`.
    ProjectsSetIdeEnabledInput { ide_id: String, enabled: bool });
input!(/// `projects.open_in_ide`.
    ProjectsOpenInIdeInput { project_id: ProjectId, ide_id: String, path: Option<String> });
input!(/// `projects.terminal_open`.
    ProjectsTerminalOpenInput { project_id: ProjectId, cols: u16, rows: u16 });
input!(/// `projects.terminal_input`.
    // SPEC-GAP-24: the ServiceApi.md row writes `bytes` untyped (which reads as `String`), while the
    // owner's `terminal_input(terminal, bytes: Vec<u8>)` takes bytes; the owner's type is restated, so
    // `bytes` is a JSON array of numbers and terminal control bytes are not refused as text.
    ProjectsTerminalInputInput { terminal: StreamId, bytes: Vec<u8> });
input!(/// `projects.terminal_resize`.
    ProjectsTerminalResizeInput { terminal: StreamId, cols: u16, rows: u16 });
input!(/// `projects.terminal_close`.
    ProjectsTerminalCloseInput { terminal: StreamId });
input!(/// `projects.check_now` (`fetch_followed` for one original).
    ProjectsCheckNowInput { repo_place: PlaceId, branch: String });
input!(/// `projects.set_check_commands` (SA-CLOSE-03 of the owner; ServiceApi.md SA-CLOSE-07).
    ProjectsSetCheckCommandsInput { project_id: ProjectId, commands: Vec<String> });

// ---- checks ----

/// The SA-GH-01 operation kinds.
const OPERATION_KINDS: &[&str] = &[
    "read",
    "comment",
    "open_pull_request",
    "update_pull_request",
    "request_review",
    "push_branch",
    "create_branch",
    "delete_branch",
    "label",
    "merge",
    "release",
    "close",
];
/// `Project.check_commands`: at most 20, each at most 1,000 characters.
const MAX_CHECK_COMMANDS: usize = 20;
const MAX_CHECK_COMMAND_CHARS: usize = 1000;
/// `LanguagePlugin.server_settings`: a JSON object of at most 64 KiB.
const MAX_SERVER_SETTINGS_BYTES: usize = 64 * 1024;

impl CommandInput for ProjectsSetCheckCommandsInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // The owner trims, drops empty commands and duplicates, then applies the limits (Internal Logic 6);
        // the same reading is applied here so a list the owner would accept is never refused.
        let mut kept: Vec<&str> = Vec::new();
        for (i, command) in self.commands.iter().enumerate() {
            let trimmed = command.trim();
            rules::text(
                trimmed,
                0,
                MAX_CHECK_COMMAND_CHARS,
                &format!("/commands/{i}"),
            )?;
            if !trimmed.is_empty() && !kept.contains(&trimmed) {
                kept.push(trimmed);
            }
        }
        rules::count(&kept, 0, MAX_CHECK_COMMANDS, "/commands")
    }
}

impl CommandInput for ProjectsRunCheckInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // The Rules Engine's `CommandRequest.command`: 1–8,000 characters.
        rules::text(&self.command, 1, 8000, "/command")
    }
}

impl CommandInput for ProjectsSetGitHostApprovalInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        if !OPERATION_KINDS.contains(&self.kind.as_str()) {
            return Err(rules::reject("/kind"));
        }
        Ok(())
    }
}

impl CommandInput for ProjectsConfigureLanguageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.language, 1, 40, "/language")?;
        let size = serde_json::to_vec(&self.server_settings)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if !self.server_settings.is_object() || size > MAX_SERVER_SETTINGS_BYTES {
            return Err(rules::reject("/server_settings"));
        }
        Ok(())
    }
}

impl CommandInput for ProjectsRestartLanguageServerInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.language, 1, 40, "/language")
    }
}

/// Inputs with no stateless rule beyond their shape.
macro_rules! no_rules {
    ($($ty:ty),* $(,)?) => {$(
        impl CommandInput for $ty {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

no_rules!(
    ProjectsOpenInput,
    ProjectsCloseInput,
    ProjectsStatusInput,
    ProjectsDiffInput,
    ProjectsHistoryInput,
    ProjectsBranchesInput,
    ProjectsCreateBranchInput,
    ProjectsSwitchBranchInput,
    ProjectsCommitInput,
    ProjectsPushInput,
    ProjectsSaveFileInput,
    ProjectsReviewInput,
    ProjectsDecideEntryInput,
    ProjectsKeepAttemptInput,
    ProjectsGitHostSettingsInput,
    ProjectsLanguagePluginsInput,
    ProjectsEditorOpenInput,
    ProjectsLspSendInput,
    ProjectsEditorCloseInput,
    ProjectsIdesInput,
    ProjectsSetIdeEnabledInput,
    ProjectsOpenInIdeInput,
    ProjectsTerminalOpenInput,
    ProjectsTerminalInputInput,
    ProjectsTerminalResizeInput,
    ProjectsTerminalCloseInput,
    ProjectsCheckNowInput,
);

#[cfg(test)]
mod tests {
    use super::*;

    fn at(r: Result<(), InputRejection>) -> String {
        let e = r.unwrap_err();
        assert_eq!(e.code, "SVC_INPUT_INVALID");
        e.at
    }

    #[test]
    fn check_commands() {
        let input = |commands: Vec<String>| ProjectsSetCheckCommandsInput {
            project_id: ProjectId::new(),
            commands,
        };
        assert!(
            input(vec!["cargo test".into(), " cargo test ".into(), "".into()])
                .check()
                .is_ok()
        );
        // 25 entries, but only 20 distinct non-empty ones after trimming.
        let mut many: Vec<String> = (0..20).map(|i| format!("check {i}")).collect();
        many.extend((0..5).map(|_| "check 0 ".to_owned()));
        assert!(input(many).check().is_ok());
        let too_many: Vec<String> = (0..21).map(|i| format!("check {i}")).collect();
        assert_eq!(at(input(too_many).check()), "/input/commands");
        assert_eq!(
            at(input(vec!["ok".into(), "x".repeat(1001)]).check()),
            "/input/commands/1"
        );
    }

    #[test]
    fn other_rules() {
        let mut run = ProjectsRunCheckInput {
            project_id: ProjectId::new(),
            task: None,
            command: String::new(),
            origin: CommandOrigin::Person,
        };
        assert_eq!(at(run.check()), "/input/command");
        run.command = "cargo test".into();
        assert!(run.check().is_ok());

        let mut approval = ProjectsSetGitHostApprovalInput {
            project_id: ProjectId::new(),
            kind: "merge".into(),
            approval_required: true,
        };
        assert!(approval.check().is_ok());
        approval.kind = "force_push".into();
        assert_eq!(at(approval.check()), "/input/kind");

        let mut language = ProjectsConfigureLanguageInput {
            language: "rust".into(),
            chosen_server: None,
            enabled: true,
            server_settings: serde_json::json!({}),
        };
        assert!(language.check().is_ok());
        language.server_settings = serde_json::json!([1]);
        assert_eq!(at(language.check()), "/input/server_settings");
        language.server_settings = serde_json::json!({"x": "y".repeat(70_000)});
        assert_eq!(at(language.check()), "/input/server_settings");
        language.language = String::new();
        assert_eq!(at(language.check()), "/input/language");

        let mut restart = ProjectsRestartLanguageServerInput {
            language: "x".repeat(41),
        };
        assert_eq!(at(restart.check()), "/input/language");
        restart.language = "go".into();
        assert!(restart.check().is_ok());
        assert!(ProjectsIdesInput {}.check().is_ok());
    }

    #[test]
    fn shapes() {
        assert_eq!(
            serde_json::to_value(EntryDecision::KeepYours).unwrap(),
            serde_json::json!({"kind": "keep_yours"})
        );
        assert_eq!(
            serde_json::to_value(DecisionOutcome::Dropped).unwrap(),
            serde_json::json!({"kind": "dropped"})
        );
        let origin: CommandOrigin =
            serde_json::from_value(serde_json::json!({"kind": "person"})).unwrap();
        assert!(matches!(origin, CommandOrigin::Person));
        let input: ProjectsTerminalInputInput = serde_json::from_value(serde_json::json!({
            "terminal": "00000000-0000-0000-0000-000000000000", "bytes": [3, 27, 91]
        }))
        .unwrap();
        assert_eq!(input.bytes, vec![3, 27, 91]);
        assert!(serde_json::from_value::<StreamId>(serde_json::json!("x")).is_err());
        assert!(serde_json::from_value::<ProjectsIdesInput>(serde_json::json!({"x": 1})).is_err());
    }
}
