//! Check it — restated from `Checker.md` (*Service API commands*, SA-CLOSE-01; SA-RECON-02).
//!
//! The screens send only what names the subject; the program adds the lock file and the source scope
//! before it calls the Checker.

use std::collections::BTreeMap;

use ordin_types::domain::agents::{PartKind, WordingChange};
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

/// A part identifier (`metadata.id`): 1–200 characters (2C `Part.identifier`).
const IDENTIFIER_MAX_CHARS: usize = 200;

/// The id of one recorded check run (UUIDv7; 2C `check_results_ref` is its string).
// SPEC-GAP-21: Checker.md defines `CheckRunId(pub uuid::Uuid)`; this crate depends on no `uuid` and
// ordin-types has no `CheckRunId`, so it is carried as its hyphenated text (the same wire form) and
// `CheckRunRef::check` refuses anything but a version-7 UUID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct CheckRunId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Failure,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "fix", rename_all = "snake_case")]
pub enum Fix {
    /// "Fix it" — fixed code knows the one right answer (ADR-094 item 4).
    FixIt { action: FixAction },
    /// "Ask Ordin to fix it" — judgement needed; the agent maker suggests text.
    AskOrdin,
    /// "Change it yourself" — opens the field.
    ChangeItYourself { field: String },
    /// "Leave it out" / "Leave [tool] out".
    LeaveItOut { what: String },
    /// "Publish to an ordin-format repository" (ADR-087).
    PublishToOrdinFormat,
    /// "Ask IT" / "Ask IT about [tool]" (ADR-012).
    AskIt,
    /// "Ask the agent maker to fix this" (ADR-088 *Conflicts*).
    AskAgentMaker,
    /// "Choose which [class] this agent uses when the workspace doesn't say" (ADR-089).
    ChoosePacks {
        rule_index: u32,
        choices: Vec<String>,
    },
    /// "Let the agent ask" (decision 64).
    LetAgentAsk { rule_index: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FixAction {
    MintIdentifier {
        field: String,
        value: String,
    },
    PointToReplacement {
        field: String,
        from: String,
        to: String,
    },
    AskMissingField {
        field: String,
    },
    DropUnknownField {
        field: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Finding {
    /// Stable machine code (`kind_unknown`, `trust_needs_it_approval`); lower snake case, ≤ 64 characters.
    pub code: String,
    pub severity: Severity,
    /// `metadata.id` of the part the finding concerns, or None for the agent as a whole.
    pub part_identifier: Option<String>,
    /// Fluent message id of the one plain sentence (`check-…`); the screens render it.
    pub message_id: String,
    /// Plain values the sentence needs; never file content.
    pub args: BTreeMap<String, String>,
    pub fixes: Vec<Fix>,
    /// Technical detail for Diagnostics only.
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckSubject {
    RepositoryRead {
        repository: String,
        commit: String,
    },
    AgentDraft {
        agent_draft_id: AgentDraftId,
    },
    PartDraft {
        part_draft_id: PartDraftId,
    },
    Withdrawal {
        part_draft_id: PartDraftId,
    },
    VocabularyChange {
        part_draft_id: PartDraftId,
    },
    InstalledAgent {
        agent_instance_id: AgentInstanceId,
    },
    LibraryItem {
        part_identifier: String,
        version: String,
    },
    Rehearsal {
        rehearsal_id: RehearsalId,
    },
    /// `check_for_install`: an agent about to be added, changed, updated or tried.
    InstallCandidate {
        root_id: String,
        plan_hash: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CheckReport {
    pub check_run_id: CheckRunId,
    pub subject: CheckSubject,
    /// True when no finding has severity Failure.
    pub passed: bool,
    pub findings: Vec<Finding>,
    /// Applied wording changes ("ordin tidied N things"); empty for non-draft subjects.
    pub tidied: Vec<WordingChange>,
    /// "Checked [time], against the version in use" (11 §5.30).
    pub checked_at: Timestamp,
    pub version_checked: String,
    /// Agents that stop when a withdrawal is published (UI-D99), by identifier.
    pub agents_that_stop: Vec<String>,
    /// False only when the publish gate is not met; true for every subject with no publish gate.
    pub publish_gate_met: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CheckInstalledInput {
    pub agent_instance_id: AgentInstanceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CheckLibraryItemInput {
    /// None = the bundled catalogue.
    pub agent_repository_id: Option<AgentRepositoryId>,
    pub kind: PartKind,
    pub identifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CheckRunRef {
    pub check_run_id: CheckRunId,
}

impl CommandInput for CheckInstalledInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for CheckLibraryItemInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.identifier, 1, IDENTIFIER_MAX_CHARS, "/identifier")
    }
}

impl CommandInput for CheckRunRef {
    fn check(&mut self) -> Result<(), InputRejection> {
        ordin_types::id::parse_v7(&self.check_run_id.0)
            .map(|_| ())
            .map_err(|_| rules::reject("/check_run_id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_item_identifier() {
        let mut input = CheckLibraryItemInput {
            agent_repository_id: None,
            kind: PartKind::WorkspaceProfile,
            identifier: "urn:ordin:acme:workspaces:rates".into(),
        };
        assert!(input.check().is_ok());
        for bad in [String::new(), "x".repeat(IDENTIFIER_MAX_CHARS + 1)] {
            input.identifier = bad;
            let refusal = input.check().unwrap_err();
            assert_eq!(
                (refusal.code, refusal.at.as_str()),
                ("SVC_INPUT_INVALID", "/input/identifier")
            );
        }
        assert!(CheckInstalledInput {
            agent_instance_id: AgentInstanceId::new()
        }
        .check()
        .is_ok());
        let mut run = CheckRunRef {
            check_run_id: CheckRunId("0192a1d0-1111-7222-8333-444455556666".into()),
        };
        assert!(run.check().is_ok());
        run.check_run_id = CheckRunId("abc".into());
        assert_eq!(run.check().unwrap_err().at, "/input/check_run_id");
    }

    #[test]
    fn report_example_shape() {
        let json = serde_json::json!({
          "check_run_id": "0192a1d0-1111-7222-8333-444455556666",
          "subject": {"kind": "agent_draft", "agent_draft_id": "0192a1c9-aaaa-7bbb-8ccc-ddddeeee0001"},
          "passed": false,
          "findings": [
            {"code": "trust_needs_it_approval", "severity": "failure",
             "part_identifier": "urn:ordin:acme:tool-bindings:jira", "message_id": "check-trust-needs-it-approval",
             "args": {"tool": "jira-reader"}, "fixes": [{"fix": "leave_it_out", "what": "jira-reader"}, {"fix": "ask_it"}],
             "detail": "not in the trust policy"}
          ],
          "tidied": [{"field": "purpose", "before": "Analyzes contract clauses", "after": "Analyses contract clauses"}],
          "checked_at": "2026-01-10T10:02:11.540Z",
          "version_checked": "draft branch",
          "agents_that_stop": [],
          "publish_gate_met": true
        });
        let report: CheckReport = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap(), json);
    }
}
