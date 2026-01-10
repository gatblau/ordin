//! Making agents and parts — restated from `AuthoringService.md` (*Service API commands*, ruling R1;
//! SA-RECON-02).
//!
//! The drafts themselves (`AgentDraft`, `PartDraft`, `Rehearsal`) are 2C types; this module restates the
//! Authoring Service's inputs and views. `authoring.publish` reaches the service with a `PublishContext`
//! the program adds; the screens send only the `DraftRef`.

use std::collections::BTreeMap;

use ordin_types::domain::agents::{
    AgentDraft, DraftJob, PartDraft, PartKind, ReviewDraftState, VocabularyAxis, WordingChange,
};
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::checker::Finding;
use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::agents::Rehearsal;
pub use ordin_types::{AgentDraftId, AgentInstanceId, PartDraftId, PartId};

/// `MakeAgentInput.request`: at most 8,000 characters.
const REQUEST_MAX_CHARS: usize = 8000;
/// `MakeAgentInput.requirements`: 0–50 items.
const REQUIREMENTS_MAX: usize = 50;
/// `RehearseInput.situations`: 1–20.
const SITUATIONS_MAX: usize = 20;
/// `WithdrawInput.reason`: 1–1,000 characters.
const REASON_MAX_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftRef {
    Agent { id: AgentDraftId },
    Part { id: PartDraftId },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MakeAgentInput {
    pub workspace_id: WorkspaceId,
    /// At most 8,000 characters.
    pub request: String,
    /// 0–50 items.
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UpdateAgentDraftInput {
    pub id: AgentDraftId,
    pub name: Option<String>,
    pub purpose: Option<String>,
    pub jobs: Option<Vec<DraftJob>>,
    pub tasks: Option<Vec<String>>,
    pub will: Option<Vec<String>>,
    pub wont: Option<Vec<String>>,
    pub needs: Option<Vec<String>>,
    pub reused_parts: Option<Vec<PartId>>,
    pub fixed_packs: Option<Vec<FixedPackChoice>>,
    /// Which tasks carry out each job: key = index into `jobs` (0-based), value = task texts from `tasks`.
    pub job_tasks: Option<BTreeMap<u32, Vec<String>>>,
}

/// The optional WorkspaceProfile field `fixedPracticePacks` (SA-NAME-07): fixed packs, or a rule left
/// for the agent to ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FixedPackChoice {
    Fixed { rule: String, packs: Vec<String> },
    AgentAsks { rule: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RehearseInput {
    pub id: AgentDraftId,
    pub workspace_id: WorkspaceId,
    /// 1–20.
    pub situations: Vec<SituationInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SituationInput {
    pub description: String,
    pub documents: Vec<String>,
    pub expectations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TryItInput {
    pub id: AgentDraftId,
    pub workspace_id: WorkspaceId,
}

/// The wire form of `set_publish_to(id, repository)` (*chosen here* by the owner).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SetPublishToInput {
    pub id: AgentDraftId,
    pub repository: AgentRepositoryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NewPartInput {
    pub agent_repository_id: AgentRepositoryId,
    pub kind: PartKind,
    pub name: String,
    pub workspace_links_code: bool,
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChangePartInput {
    pub part: PartId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WithdrawInput {
    pub part: PartId,
    /// 1–1,000 characters.
    pub reason: String,
    pub replacement: Option<PartId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VocabularyChangeInput {
    pub agent_repository_id: AgentRepositoryId,
    pub axis: VocabularyAxis,
    pub change: VocabularyEdit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VocabularyEdit {
    Add {
        code: String,
        label: String,
        description: String,
    },
    Retire {
        code: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UpdatePartDraftInput {
    pub id: PartDraftId,
    pub fields: serde_json::Value,
    pub changes_what_part_does: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SuggestWordingInput {
    pub id: PartDraftId,
    pub field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ImportInput {
    pub source: std::path::PathBuf,
    pub as_part: Option<PartKind>,
    pub agent_repository_id: AgentRepositoryId,
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApplyFixInput {
    pub draft: DraftRef,
    pub finding_code: String,
    pub fix: FixChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FixChoice {
    FixIt,
    AskOrdin,
    LeaveItOut,
    AskAgentMaker,
    PublishToOrdinFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DraftFilter {
    pub agent_repository_id: Option<AgentRepositoryId>,
    pub open_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AgentDraftView {
    /// Carries `field_authors`.
    pub draft: AgentDraft,
    pub job_tasks: BTreeMap<u32, Vec<String>>,
    pub checks: Vec<Finding>,
    pub tidied: Vec<WordingChange>,
    pub publish_to: Vec<AgentRepositoryId>,
    pub can_try: bool,
    pub can_publish: bool,
    pub reviewer_words: Vec<String>,
    /// Code workspaces only.
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PartDraftView {
    pub draft: PartDraft,
    pub checks: Vec<Finding>,
    pub tidied: Vec<WordingChange>,
    pub agents_that_stop: Vec<String>,
    pub used_by: Vec<PartId>,
    pub form: serde_json::Value,
    pub writing_guide: Option<String>,
    pub can_publish: bool,
    pub reviewer_words: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DraftSummary {
    pub draft: DraftRef,
    pub name: String,
    pub state: ReviewDraftState,
    pub repository: String,
    pub updated_at: Timestamp,
}

// ---- checks ----

impl CommandInput for MakeAgentInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.request, 0, REQUEST_MAX_CHARS, "/request")?;
        rules::count(&self.requirements, 0, REQUIREMENTS_MAX, "/requirements")
    }
}

impl CommandInput for RehearseInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::count(&self.situations, 1, SITUATIONS_MAX, "/situations")
    }
}

impl CommandInput for WithdrawInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.reason, 1, REASON_MAX_CHARS, "/reason")
    }
}

/// Inputs whose limits the owner applies with its own codes (draft field ranges `ADRAFT_*`,
/// `PDRAFT_FIELDS_TOO_LARGE`, the vocabulary rules), or that hold only identifiers and closed enums.
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
    AgentDraftId,
    UpdateAgentDraftInput,
    TryItInput,
    SetPublishToInput,
    NewPartInput,
    ChangePartInput,
    PartId,
    VocabularyChangeInput,
    UpdatePartDraftInput,
    SuggestWordingInput,
    ImportInput,
    ApplyFixInput,
    DraftRef,
    DraftFilter,
    PartDraftId,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_agent_limits() {
        let mut input = MakeAgentInput {
            workspace_id: WorkspaceId::new(),
            request: "Check supplier invoices".into(),
            requirements: vec!["Reads PDF invoices".into()],
        };
        assert!(input.check().is_ok());
        input.request = "x".repeat(REQUEST_MAX_CHARS + 1);
        assert_eq!(input.check().unwrap_err().at, "/input/request");
        input.request = String::new();
        input.requirements = vec![String::new(); REQUIREMENTS_MAX + 1];
        let refusal = input.check().unwrap_err();
        assert_eq!(
            (refusal.code, refusal.at.as_str()),
            ("SVC_INPUT_INVALID", "/input/requirements")
        );
    }

    #[test]
    fn rehearse_and_withdraw_limits() {
        let situation = SituationInput {
            description: "An invoice".into(),
            documents: vec![],
            expectations: vec!["Flags the line".into()],
        };
        let mut rehearse = RehearseInput {
            id: AgentDraftId::new(),
            workspace_id: WorkspaceId::new(),
            situations: vec![],
        };
        assert_eq!(rehearse.check().unwrap_err().at, "/input/situations");
        rehearse.situations = vec![situation; SITUATIONS_MAX];
        assert!(rehearse.check().is_ok());
        let mut withdraw = WithdrawInput {
            part: PartId::new(),
            reason: String::new(),
            replacement: None,
        };
        assert_eq!(withdraw.check().unwrap_err().at, "/input/reason");
        withdraw.reason = "Replaced by the new rate rule".into();
        assert!(withdraw.check().is_ok());
        assert!(AgentDraftId::new().check().is_ok());
        assert!(DraftRef::Part {
            id: PartDraftId::new()
        }
        .check()
        .is_ok());
    }

    #[test]
    fn apply_fix_example_shape() {
        let json = serde_json::json!({ "draft": { "kind": "agent", "id": "0192f0a2-1d4e-7f00-8a11-22b3c4d5e6f7" },
            "finding_code": "id_grammar_invalid", "fix": "fix_it" });
        let input: ApplyFixInput = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&input).unwrap(), json);
        let mut extra = json;
        extra["draft"]["force"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ApplyFixInput>(extra).is_err());
    }
}
