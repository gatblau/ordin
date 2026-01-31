//! The public value types of the Usage Planner (UsagePlanner.md *Public Interface*).

use std::collections::BTreeMap;

use ordin_organisation_profile::{HarnessName, WorkKind};
use ordin_runtime_adapter::UsageDelta;
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::domain::identity::{AskBefore, SummariseWhen, UsageChoice};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::{ModelChoice, ModelLevel, ThinkingLevel};
use ordin_types::{CrewRunId, RunId, WorkspaceId};
use serde::{Deserialize, Deserializer, Serialize};

/// Reads `null` as `Some(None)` and a value as `Some(Some(v))`; an absent field stays `None`
/// through `#[serde(default)]`. This is what lets `Option<Option<u32>>` survive a round trip.
pub(crate) fn double_option<'de, D>(deserializer: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<u32>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateBand {
    Light,
    Medium,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LevelChoice {
    pub level: ModelLevel,
    /// The harness's model name, or None for the harness's own automatic selection.
    pub model: Option<String>,
    pub thinking: ThinkingLevel,
    /// The person's effort for this kind of work on this harness, in the harness's own words (for
    /// Claude Code, a `--effort` value); `None` leaves it to the harness (SA-AGM-01).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// True when this choice is the one step up after a shortfall.
    pub stepped_up: bool,
}

impl LevelChoice {
    /// The 2C value for `StartRun.model`; `StartRun.thinking` takes `Some(self.thinking)`.
    pub fn model_choice(&self) -> ModelChoice {
        ModelChoice {
            level: self.level,
            model: self.model.clone(),
        }
    }
}

/// Settings › Agents and models (amended January 2026; user decision; Approved by the user January
/// 2026): the person's own model for each level and effort for each kind of work, per harness. An
/// absent entry falls to the organisation profile's `usage.level_models`, then the harness's
/// default; an absent effort leaves it to the harness.
// spec: ordin-desktop/components/UsagePlanner#SA-AGM-01
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PersonalModelChoices {
    #[serde(default)]
    pub models: BTreeMap<HarnessName, BTreeMap<ModelLevel, String>>,
    #[serde(default)]
    pub effort: BTreeMap<HarnessName, BTreeMap<WorkKind, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EstimateRequest {
    /// Identifies "the same work" for history: "workflow:<id>:<version>", "crew:<charter id>",
    /// or "task:<agent instance id>:<command or 'free_text'>".
    pub work_key: String,
    pub shape: WorkShape,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkShape {
    /// A crew Ordin coordinates as it goes: limits from the charter.
    Crew { members: u32, max_rounds: u32 },
    /// A saved workflow: counts walked from its outline at each block's maximum.
    Workflow { agent_steps: u32, ordin_turns: u32 },
    /// A task over documents.
    Task { documents: u32, total_bytes: u64 },
}

/// Carried to the screens as 2C `CrewPlanBlock.estimate: Option<UsageEstimate>` (the plan's **Estimated use**).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsageEstimate {
    pub band: EstimateBand,
    pub min_turns: u32,
    pub max_turns: u32,
    /// Share of a typical day's use, in per-mille of 250 turns, for "about a tenth of a typical day".
    pub typical_day_permille: u32,
    pub based_on: Vec<EstimateBasis>,
    /// True when the band reaches the person's "Ask before work estimated as" choice.
    pub ask_first: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EstimateBasis {
    EarlierRuns { count: u32 },
    Limits { max_rounds: u32 },
    Members { count: u32 },
    Steps { count: u32 },
    Documents { count: u32, total_bytes: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReuseQuery {
    pub request: String,
    pub agent: AgentRef,
    pub workspace_id: WorkspaceId,
    pub input_versions: Vec<InputUsed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AnswerToIndex {
    pub query: ReuseQuery,
    pub source_run_id: RunId,
    pub answered_at: Timestamp,
    /// The answer the run ended with (its final message); stored as `ReusedAnswer.text`.
    pub text: String,
    /// What the run did besides answering; any non-zero value refuses indexing (ADR-062:
    /// "never an action, a change, a push or an approval").
    pub external_actions: u32,
    pub files_changed: u32,
    pub approvals: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunUsage {
    pub run_id: RunId,
    pub crew_run_id: Option<CrewRunId>,
    pub round: Option<u32>,
    pub level: ModelLevel,
    pub delta: UsageDelta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Today {
    pub model_turns: u32,
    /// Per-mille of today's turns that ran on the Smaller level.
    pub smaller_share_permille: u32,
    pub answered_without_model: u32,
    pub typical_day_permille: u32,
}

/// A change the person makes in Settings > Using your plan; absent fields are unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageSettingsChange {
    pub choice: Option<UsageChoice>,
    pub model_level_per_kind: Option<BTreeMap<String, String>>,
    pub thinking: Option<ThinkingLevel>,
    pub answer_simple_requests_without_model: Option<bool>,
    pub reuse_answers: Option<bool>,
    pub give_agents_only_needed_parts: Option<bool>,
    pub summarise_long_conversations: Option<SummariseWhen>,
    pub continue_from_summary_after_break: Option<bool>,
    pub ask_before_estimated_requests: Option<AskBefore>,
    pub agents_at_same_time: Option<u32>,
    pub show_usage_on_each_run: Option<bool>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub daily_pause_requests: Option<Option<u32>>,
}
