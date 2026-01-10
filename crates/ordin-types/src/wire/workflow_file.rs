//! 2C part 2 §8 Workflow file, format 3.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- WorkflowFile ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFile {
    /// Format version; this release writes 3 and reads 1, 2 and 3.
    pub format: u8,
    pub name: String,
    pub purpose: String,
    pub inputs: Vec<WorkflowFileInput>,
    pub agents: Vec<WorkflowFileAgent>,
    pub steps: Vec<WorkflowFileStep>,
    pub limits: WorkflowFileLimits,
    /// One line of text ("Saved from the run of 16 Oct").
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFileInput {
    pub label: String,
    pub kind: WorkflowInputKind,
    pub required: bool,
}

/// The only definition (amended January 2026, decision 80: C-1); the domain part's `WorkflowInput.kind` re-exports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum WorkflowInputKind {
    Document,
    Text,
    /// A list a `for_each` block can go through (ADR-065); see HLD inconsistency I-1.
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFileAgent {
    /// Short local name the steps use, unique in the file ("analyst").
    pub key: String,
    /// Format 3 (OQ-47): the agent repository's profile key, or its address for one
    /// the person added.
    pub repository: AgentRepositoryRef,
    /// The agent's identifier, a `urn:ordin:` WorkspaceProfile identifier.
    pub id: String,
    pub version: String,
    /// The agent's display name.
    pub name: String,
    pub role: String,
}

// SPEC-GAP-3: 2C derives `PartialEq` on the domain entities that hold this type (`WorkflowVersion`, `WorkflowAgent`)
// but not on this wire type, so those derives cannot compile as written; `PartialEq` is added here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRepositoryRef {
    ProfileKey(String),
    Address(String),
}

// SPEC-GAP-3: 2C derives `PartialEq` on the domain entities that hold this type (`WorkflowVersion`, `WorkflowAgent`)
// but not on this wire type, so those derives cannot compile as written; `PartialEq` is added here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFileLimits {
    pub max_rounds: u16,
    pub wall_clock_minutes: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
    pub agents_at_once: u8,
    pub failure_repeat_limit: u8,
    /// Agent keys allowed to take externally visible actions.
    pub external_action_agents: Vec<String>,
}

// ---- WorkflowFileStep ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowFileStep {
    Agent {
        /// Display number: "3", "2.1", "3a"; steps sharing a number with letters run at the same time.
        position: String,
        agent: String,
        line: String,
        uses: Vec<StepUse>,
        asks_first: bool,
        ordin_checks: bool,
        #[serde(default)]
        if_it_fails: IfItFails,
    },
    If {
        position: String,
        decided_by: Decider,
        paths: Vec<NamedPath>,
    },
    #[serde(rename = "for_each")]
    Each {
        position: String,
        list: ListSource,
        max_items: u8,
        steps: Vec<WorkflowFileStep>,
    },
    #[serde(rename = "repeat_until")]
    Until {
        position: String,
        decided_by: Decider,
        max_times: u8,
        steps: Vec<WorkflowFileStep>,
    },
    #[serde(rename = "run_workflow")]
    Call {
        position: String,
        /// The name of a workflow saved in the same workspace.
        workflow: String,
    },
}

/// `wire::workflow_file::NamedPath` — distinct from the domain part's `domain::runs::NamedPath`
/// (the saved workflow's); the Ordin Host converts between them (amended January 2026, decision 80: C-1, namespaced).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NamedPath {
    pub name: String,
    pub steps: Vec<WorkflowFileStep>,
}

/// `wire::workflow_file::StepUse` — distinct from `domain::runs::StepUse` (amended January 2026, decision 80: C-1, namespaced).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepUse {
    Input(String),
    Step(String),
    /// Inside a for_each: the current item.
    Item(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ListSource {
    Input(String),
    Step(String),
}

/// The only definition (amended January 2026, decision 80: C-1); the domain part's `AgentStep.if_it_fails` re-exports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum IfItFails {
    #[default]
    OrdinTakesOver,
    AskMe,
    SkipIt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "by", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decider {
    /// Fixed code, for what the application can know for certain (ADR-065).
    Application {
        check: ApplicationCheck,
        step: String,
    },
    Ordin {
        words: String,
    },
    /// The person's answer ("Ask me" is an `if` decided by the person; path names are the choices).
    Person {
        words: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ApplicationCheck {
    ListHasItems,
    StepFailed,
    CheckFoundShortfall,
    DocumentChanged,
}
