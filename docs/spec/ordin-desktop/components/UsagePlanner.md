# SPEC: Usage Planner

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-usage-planner` · **Build phase:** 2 · **Programs:** D P A
> **Dependencies:** Organisation Profile, Journal, Runtime Adapter interface

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

## Purpose

The Usage Planner is the policy module that spends the person's plan sparingly (ADR-062; 02:368–370). It chooses the model level — and, through each harness's level-to-model map, the model — for every step from the person's usage settings within the organisation profile's frame; steps a step that fell short up one level, once; estimates a crew's or a large task's use before it starts; decides when an earlier answer may be shown again and when a conversation should be summarised or continued from a summary; and counts each run's model turns, tool calls, level, tokens and cache share, keeping the daily counters that Settings › Using your plan shows as **Today** and that the optional daily pause reads. It decides in fixed code with no model call. The Session Supervisor, the Crew Orchestrator and the Ordin Host call it; it calls none of them (2B-2 D9).

## Approach

Pure functions over a resolved `EffectivePolicy` (SA-RECON-01: the rich choice is `LevelChoice`, because 2C's `ModelChoice` is the `{level, model}` pair a `StartRun` carries; `model_choice()` converts), built from the profile frame and the person's settings and rebuilt whenever either changes, plus three small tables it owns: the person's usage settings, the answer-reuse index and the daily counters, with one more for the turn history estimates are based on. Error prefix: `USAGE_` (2C uses it for usage settings). Rejected: reading the Journal's run records to estimate — 2B-1 rule 5 forbids reading another module's tables, so the planner keeps its own per-work history as runs end. Rejected: asking the harness for a remaining allowance — ordin cannot read it (OQ-3), so nothing here pretends to.

### Closing-round assumptions

- **SA-CLOSE-01** (K8; C-29 conditional) — **When C-29 confirms** that the harness reports context-window use, the Ordin Host passes the Session Supervisor's `context_used_percent(run_id)` × 10 as `context_used_permille` and the Early (≥ 500) or WhenNearlyFull (≥ 800) threshold decides. **Until then** (and whenever no value has been reported) the Ordin Host passes `None` with the number of Ordin turns since the last summary, and `should_summarise` answers true from the 40th turn *(assumed: 40 turns keep a conversation well inside the smallest context window of the supported models)*. `OnlyWhenAsked` always answers false. Not a Gap.

## Shared Context

### From `ordin-types` (2C domain, copied)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetBy { Person, Company }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Setting<T> { pub value: T, pub set_by: SetBy }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsageSettings {
    pub choice: Setting<UsageChoice>,
    /// Kind of work → model level (keys and values below).
    pub model_level_per_kind: Setting<std::collections::BTreeMap<String, String>>,
    /// Three values (2C, decision 80: C-18).
    pub thinking: Setting<ThinkingLevel>,
    pub answer_simple_requests_without_model: Setting<bool>,
    pub reuse_answers: Setting<bool>,
    pub give_agents_only_needed_parts: Setting<bool>,
    /// Three choices (2C, decision 80: C-18).
    pub summarise_long_conversations: Setting<SummariseWhen>,
    pub continue_from_summary_after_break: Setting<bool>,
    /// When Ordin asks before starting estimated work: three choices (2C, decision 80: C-18).
    pub ask_before_estimated_requests: Setting<AskBefore>,
    pub agents_at_same_time: Setting<u32>,
    pub show_usage_on_each_run: Setting<bool>,
    pub daily_pause_requests: Setting<Option<u32>>,
    pub updated_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageChoice { SaveMyPlan, Balanced, BestResults, Custom }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummariseWhen { Early, WhenNearlyFull, OnlyWhenAsked }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskBefore { Heavy, MediumOrHeavy, Never }
// `ThinkingLevel { Low, Automatic, High }`: 2C wire §2, `ordin_types::wire::run` (re-exported by the domain part).

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReusedAnswer {
    pub id: ReusedAnswerId, pub request: String, pub agent: AgentRef, pub workspace_id: WorkspaceId,
    pub input_versions: Vec<InputUsed>, pub source_run_id: RunId,
    pub answered_at: Timestamp, pub shown_again_at: Timestamp, pub expires_at: Timestamp,
    /// The earlier answer, shown again on reuse.
    pub text: String,
}
pub enum AgentRef { AgentInstance { agent_instance_id: AgentInstanceId }, RemoteAgent { remote_agent_id: RemoteAgentId } } // tag "kind"
pub struct InputUsed { pub name: String, pub location: String, pub version_read: String }

// 2C wire §2 (crate ordin-runtime-adapter)
pub enum AdapterKind { ClaudeLocal, CopilotLocal, HostedAgent, SlackThread }   // snake_case
pub struct UsageDelta {
    pub model_turns: u32, pub tool_calls: u32,
    pub input_tokens: Option<u64>, pub output_tokens: Option<u64>, pub cached_input_tokens: Option<u64>,
}
```

2C validation copied: a field with `set_by = company` cannot be changed by the person → `USAGE_FIXED_BY_COMPANY`; every value within the profile's usage section → `USAGE_ABOVE_PROFILE_LIMIT`; `model_level_per_kind` keys and values from the catalogue below → `USAGE_LEVEL_UNKNOWN`; `agents_at_same_time` 1–3 → `USAGE_AGENTS_OUT_OF_RANGE`; `ReusedAnswer.expires_at = answered_at + 7 days` → `REUSE_EXPIRY_INVALID`; reuse refused when any input has a newer version → `REUSE_INPUT_NEWER`; `request` 1–8,000 characters → `REUSE_REQUEST_INVALID`.

### Organisation profile keys read (2D-2)

`usage.plan_mode` (`save`, `balanced`, `best`; default `balanced`); `usage.levels_allowed.<kind>` (kinds `talking`, `coordinating`, `routine`, `documents`, `code`; values `automatic`, `smaller`, `standard`, `most_capable`; default all four); `usage.level_models.<harness>.<level>` (default Claude Code: Haiku, Sonnet, Opus for `smaller`, `standard`, `most_capable`; Copilot `automatic` is Copilot's own selection); `usage.agents_at_once` (default 2, 1–3); `usage.agents_at_once_max` (default 3, 1–3); `usage.defaults.<setting>` (`answer_without_model` true, `reuse_answers` true, `parts_only` true, `summarise` `when_nearly_full`, `continue_from_summary` true, `ask_before` `heavy`, `show_usage` true in workspaces with code, `daily_pause` `none`); `usage.person_may_change[]` (default every usage setting). In the agent profile no profile is read (2D Gaps item 1): `ProfileSnapshot::defaults()` applies and the person's settings do not exist; the agent configuration's model policy decides models (Model Policy component), so only counting and estimates run there.

Organisation Profile API used: `OrganisationProfileService::current() -> Arc<ProfileSnapshot>`, `subscribe() -> watch::Receiver<Arc<ProfileSnapshot>>`, `ProfileSnapshot::defaults()`; the usage keys are the profile document's `usage: UsageSection` (Organisation Profile spec), whose fields carry the key names above.

Journal API used (Journal spec, B2 form, R3): `Datastore` (the program passes `journal.datastore().clone()`; D4), `ModuleMigrations` of `MigrationStep` tuples; `Journal::append_entry(NewJournalEntry { kind, component: "usage_planner", run_id, crew_run_id, workspace_id, summary, data }) -> Result<JournalEntry, JournalError>` ("journal X" in this spec means one such entry); `PruneParticipant { module(), prune(&Datastore, &PruneRequest) }`, `Journal::register_prune_participant`.

### Fixed constants (2D-4 and chosen here)

| Constant | Value | Source |
|---|---|---|
| Reused answer lifetime | 7 days | 2D-4, ADR-062 |
| Task size that triggers an estimate | more than 20 documents | 2D-4 |
| Break after which a conversation continues from a summary | 1 hour | 2D-4 (owner Ordin Host; the planner applies the rule) |
| A typical day's use of the plan | 250 model turns | *assumed here*, SA-USAGE-01: the middle daily-pause choice of ADR-062 |
| Estimate bands | Light: upper bound ≤ 25 turns; Medium: ≤ 100; Heavy: above 100 | *assumed here*, SA-USAGE-02 |
| Ask-before choices (`AskBefore`, 2C) | `Heavy`: ask when the band is Heavy; `MediumOrHeavy`: ask when the band is Medium or Heavy; `Never`: never ask | ADR-062 three choices; 2C C-18 |
| Summarise when the context is at least | `early` 500 ‰, `when_nearly_full` 800 ‰ of the window | *assumed here*, SA-USAGE-03 |
| Turns assumed per agent step with no history | 8 | *assumed here*, SA-USAGE-04 |
| Day boundary of the daily counters | local midnight of the computer | *assumed here* |

## Public Interface

Crate `ordin-usage-planner`; names *chosen here* unless marked 2C or 2D.

```rust
// ModelLevel, ThinkingLevel and ModelChoice { level, model } are 2C wire §2 (`ordin_types::wire::run`);
// SummariseWhen and AskBefore are 2C domain (`ordin_types`); LevelModels and the HarnessModels trait
// are the Runtime Adapter interface's (`ordin_runtime_adapter::{HarnessModels, LevelModels}`).
// All are imported, never redefined here.
use ordin_types::{AskBefore, SummariseWhen};
use ordin_runtime_adapter::{HarnessModels, LevelModels};
/// One owner: the Organisation Profile's `WorkKind` (the key of `UsageSection.levels_allowed`),
/// re-exported so the Session Supervisor and the Crew Orchestrator keep writing `ordin_usage_planner::WorkKind`.
pub use ordin_organisation_profile::WorkKind;   // Talking, Coordinating, Routine, Documents, Code
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateBand { Light, Medium, Heavy }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LevelChoice {
    pub level: ModelLevel,
    /// The harness's model name, or None for the harness's own automatic selection.
    pub model: Option<String>,
    pub thinking: ThinkingLevel,
    /// True when this choice is the one step up after a shortfall.
    pub stepped_up: bool,
}
impl LevelChoice {
    /// The 2C value for `StartRun.model`; `StartRun.thinking` takes `Some(self.thinking)`.
    pub fn model_choice(&self) -> ModelChoice;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Carried to the screens as 2C `CrewPlanBlock.estimate: Option<UsageEstimate>` (the plan's **Estimated use**).
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Today {
    pub model_turns: u32,
    /// Per-mille of today's turns that ran on the Smaller level.
    pub smaller_share_permille: u32,
    pub answered_without_model: u32,
    pub typical_day_permille: u32,
}

/// A change the person makes in Settings › Using your plan; absent fields are unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageSettingsChange {
    pub choice: Option<UsageChoice>,
    pub model_level_per_kind: Option<std::collections::BTreeMap<String, String>>,
    pub thinking: Option<ThinkingLevel>,
    pub answer_simple_requests_without_model: Option<bool>,
    pub reuse_answers: Option<bool>,
    pub give_agents_only_needed_parts: Option<bool>,
    pub summarise_long_conversations: Option<SummariseWhen>,
    pub continue_from_summary_after_break: Option<bool>,
    pub ask_before_estimated_requests: Option<AskBefore>,
    pub agents_at_same_time: Option<u32>,
    pub show_usage_on_each_run: Option<bool>,
    pub daily_pause_requests: Option<Option<u32>>,
}

#[derive(Debug, thiserror::Error)]
pub enum UsageError {
    #[error("set by your company")] FixedByCompany { setting: &'static str },
    #[error("above the company's limit")] AboveProfileLimit { setting: &'static str },
    #[error("unknown level or kind")] LevelUnknown,
    #[error("agents at once out of range")] AgentsOutOfRange,
    #[error("daily pause reached")] DailyPauseReached { turns_today: u32, limit: u32 },
    #[error("answer not reusable")] NotAnswerOnly,
    #[error("request text invalid")] RequestInvalid,
    #[error("datastore unavailable")] Store(#[source] sqlx::Error),
    #[error("journal unavailable")] Journal(#[source] JournalError),
}
impl UsageError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }

pub struct UsagePlanner { /* private */ }
impl UsagePlanner {
    pub const MODULE: &'static str = "usage_planner";
    pub fn migrations() -> ModuleMigrations;   // module "usage_planner" (ModuleMigrations.module)
    /// `harnesses`: the local adapters' `HarnessModels` (Claude Local Adapter, Copilot Local Adapter)
    /// when the program has built them already; empty when it builds the adapters later.
    /// Called by: the desktop and server programs at start.
    pub async fn new(datastore: Datastore, journal: Arc<Journal>, profile: Arc<OrganisationProfileService>,
                     harnesses: Vec<Arc<dyn HarnessModels>>) -> Result<Arc<Self>, UsageError>;
    /// Late binding: replaces the harness list once the program has built its local adapters
    /// (the desktop builds them after the planner). Called by: the desktop and server programs,
    /// once each time their set of local adapters changes.
    pub fn set_harnesses(&self, harnesses: Vec<Arc<dyn HarnessModels>>);

    // Settings (Service API usage.get_settings / usage.update_settings, chosen here)
    pub fn settings(&self) -> UsageSettings;
    pub async fn update_settings(&self, change: UsageSettingsChange) -> Result<UsageSettings, UsageError>;
    // `settings`, `update_settings`: called by the program for the Service API.
    /// Called by: Crew Orchestrator (members running at once never exceed it).
    pub fn agents_at_once(&self) -> u32;
    /// Called by: the desktop and server programs, which pass the value to the MCP Front's
    /// `set_only_needed_parts` at start and after every settings change.
    pub fn parts_only(&self) -> bool;
    /// Called by: Ordin Host (answer simple requests itself).
    pub fn answer_without_model(&self) -> bool;
    /// Called by: the program for the screens (run cards).
    pub fn show_usage_on_each_run(&self) -> bool;

    // Levels
    /// Called by: Session Supervisor (B.3), Ordin Host (`Talking`, `Coordinating`, with the
    /// adapter kind of Ordin's harness).
    pub fn choose(&self, kind: WorkKind, harness: AdapterKind) -> LevelChoice;
    /// One step up after a shortfall; None when already at the top or the next level is not allowed.
    /// Called by: Crew Orchestrator (a saved step tried again).
    pub fn step_up(&self, previous: &LevelChoice, kind: WorkKind, harness: AdapterKind) -> Option<LevelChoice>;

    // Estimates and the daily pause
    /// Called by: Ordin Host, before showing a crew plan or a workflow run, and for a task over
    /// more than 20 documents.
    pub async fn estimate(&self, req: EstimateRequest) -> Result<UsageEstimate, UsageError>;
    /// Called by: Ordin Host, before any work Ordin starts (crew, workflow, delegated task).
    pub async fn may_start_ordin_work(&self) -> Result<(), UsageError>;

    // Reuse — called by: Ordin Host (before delegating a request; after an answer-only run ends).
    pub async fn find_reusable(&self, q: ReuseQuery) -> Result<Option<ReusedAnswer>, UsageError>;
    pub async fn index_answer(&self, a: AnswerToIndex) -> Result<ReusedAnswerId, UsageError>;
    pub async fn record_reuse(&self, id: ReusedAnswerId, run_id: Option<RunId>) -> Result<(), UsageError>;

    // Summaries — called by: Ordin Host. It converts `used / limit` tokens to per-mille itself,
    // and applies the break rule by calling `continue_from_summary` (not by reading the setting).
    /// `turns_since_summary`: Ordin turns of the conversation since its last summary (or its start),
    /// counted by the Ordin Host. Used only when `context_used_permille` is None (SA-CLOSE-01).
    pub fn should_summarise(&self, context_used_permille: Option<u32>, turns_since_summary: u32) -> bool;
    pub fn continue_from_summary(&self, last_message_at: Timestamp, now: Timestamp) -> bool;

    // Counting (Service API usage.today, chosen here)
    /// Called by: Session Supervisor (each `Usage` event).
    pub async fn record_usage(&self, u: RunUsage) -> Result<(), UsageError>;
    /// Called by: Session Supervisor (each run), Crew Orchestrator (each crew run).
    pub async fn record_run_finished(&self, work_key: &str, run_id: RunId, model_turns: u32) -> Result<(), UsageError>;
    /// Called by: Ordin Host (an answer given without a model call).
    pub async fn record_answered_without_model(&self) -> Result<(), UsageError>;
    /// Called by: the program for the Service API `usage.today`.
    pub async fn today(&self) -> Result<Today, UsageError>;
    /// The Journal's pruning hook (Data Model). Called by: the programs.
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant>;
}
```

Service API commands (SA-API-01; *chosen here*): `usage.get_settings` → `UsageSettings`; `usage.update_settings` (input `UsageSettingsChange`) → `UsageSettings`; `usage.today` → `Today`.

### Example

Profile: `plan_mode: balanced`, Claude Code map default. Person: no changes.

- `choose(Routine, ClaudeLocal)` → `{"level":"smaller","model":"haiku","thinking":"automatic","stepped_up":false}`.
- The routine step's result fails its form check: `step_up(…)` → `{"level":"standard","model":"sonnet","thinking":"automatic","stepped_up":true}`; a second `step_up` on that choice returns `None` (one step only).
- `estimate({"work_key":"crew:0192a1d0-…","shape":{"kind":"crew","members":3,"max_rounds":10}})` with no history → min 24, max 250, band `heavy`, `typical_day_permille` 1000, `ask_first` true (balanced asks before heavy).

## Internal Logic

**A. Effective policy.** Rebuilt at `new`, after `update_settings`, and on every profile change (`subscribe`).
1. Company defaults: `plan_mode` → `UsageChoice` (`save` → `SaveMyPlan`, `balanced` → `Balanced`, `best` → `BestResults`); each `usage.defaults.*` value mapped to the 2C field: `summarise` → `summarise_long_conversations` with the same `SummariseWhen` value; `ask_before` → `ask_before_estimated_requests` with the same `AskBefore` value; `thinking` (no profile key) → the choice's table value of A.4; `daily_pause` `none` → `None`, else the number.
2. Person's stored values (table `usage_settings`) apply only for settings named in `usage.person_may_change[]`; each other field takes the company value with `set_by = Company`.
3. Clamp: `agents_at_same_time` to 1..=min(3, `agents_at_once_max`) (ADR-062: 1, 2 or 3 — the 2C upper bound 16 does not apply; recorded as a disagreement); every level in `model_level_per_kind` to `levels_allowed` for its kind (C.3). A stored person's value that the new profile lowers is lowered at use and the setting shows the company maximum (2D chain item 4).
4. Thinking: the choice's table value (Save → Low, Balanced → Automatic, Best → High; Custom → Automatic) unless the person stored a `thinking` value, which then applies. `SummariseWhen` is `summarise_long_conversations` as stored.
5. Harnesses: the list given at `new`, replaced as a whole by `set_harnesses` (held behind an `RwLock`; the policy itself is not rebuilt, since C.4 reads the list at each `choose`).

**B. `update_settings(change)`.**
1. For each present field: not in `person_may_change` → `USAGE_FIXED_BY_COMPANY` (permanent; nothing saved).
2. `model_level_per_kind`: keys in `talking`, `coordinating`, `routine`, `documents`, `code`; values in `automatic`, `smaller`, `standard`, `most_capable` → else `USAGE_LEVEL_UNKNOWN`; a level not allowed for its kind → `USAGE_ABOVE_PROFILE_LIMIT`. Changing it sets `choice = Custom` (ADR-062).
3. `agents_at_same_time` outside 1..=3 → `USAGE_AGENTS_OUT_OF_RANGE`; above `agents_at_once_max` → `USAGE_ABOVE_PROFILE_LIMIT`.
4. `daily_pause_requests` not `None`, 100, 250 or 500 → `USAGE_ABOVE_PROFILE_LIMIT`. `thinking`, `summarise_long_conversations` and `ask_before_estimated_requests` are 2C enums; a value outside them is refused by the command schema before this call.
5. Setting `choice` to a named choice resets `model_level_per_kind` to that choice's table (C.1) and removes a stored `thinking` value (A.4).
6. Upsert the single row of `usage_settings`; rebuild the policy; journal `usage_settings_changed` {fields changed}; return `settings()`.

**C. `choose(kind, harness)`.**
1. Level table (ADR-062): `talking` Smaller/Standard/MostCapable; `coordinating` Standard/Standard/MostCapable; `routine` Smaller/Smaller/Standard; `documents` Standard/Standard/MostCapable; `code` Standard/Standard/MostCapable — for Save/Balanced/Best. `Custom` uses `model_level_per_kind[kind]`.
2. `Automatic` on a harness with no automatic selection (`claude_local`) is read as `Standard` (*assumed here*: Claude Code has no automatic model choice).
3. When the level is not in `levels_allowed[kind]`: the highest allowed level below it; if none, the lowest allowed above it. An empty allowed list → `Standard` with a log `usage_levels_empty` (profile validation should prevent it).
4. Model: `usage.level_models.<harness>.<level>`; absent → the harness's `HarnessModels::default_level_models()` (the adapter kinds map to harnesses `claude_local` → `ClaudeCode`, `copilot_local` → `CopilotCli`; Claude Code's map is `haiku`, `sonnet`, `opus`); a profile model the harness's `accepts_model` refuses → the default map's model for that level, logged `usage_model_not_accepted` {harness, level}; no `HarnessModels` for the harness (none given yet, or none for it) → `None`; `Automatic` on `copilot_local` → `None`; `hosted_agent` and `slack_thread` → `None` (the remote side chooses).
5. Conditional on C-37: where a harness cannot set the model or thinking level per session or sub-agent, the adapter ignores `model`/`thinking` and the start-up report says so (adapter spec); this module's choice does not change.

**D. `step_up(previous, kind, harness)`.** `previous.stepped_up` → `None` (once only, ADR-062). Next level: Smaller → Standard → MostCapable; Automatic → Standard; MostCapable → `None`. Not allowed for the kind → `None`. Otherwise return the choice with `stepped_up = true`; journal `usage_stepped_up` {kind, from, to}. The person's **Try again with a more capable model** is not this call; it starts a new run at the next level chosen by the caller.

**E. `estimate(req)`.**
1. History: the last 5 rows of `usage_work_history` for `work_key`. With ≥ 1 row: `min_turns` = smallest, `max_turns` = largest; `based_on = [EarlierRuns { count }]`.
2. Without history, from the shape: `Crew` → min = members × 8, max = max_rounds × (members × 8 + 1) capped at 30 × 9 × 8; `Workflow` → min = agent_steps × 8 + ordin_turns, max = agent_steps × 8 × 2 + ordin_turns (a step tried twice); `Task` → min = 4 + documents / 2, max = 4 + documents × 2. `based_on` lists the shape's facts.
3. Band from `max_turns` (Light ≤ 25 < Medium ≤ 100 < Heavy). `typical_day_permille = max_turns × 1000 / 250`, capped at 10,000.
4. `ask_first` from `ask_before_estimated_requests`: `Heavy` → band is Heavy; `MediumOrHeavy` → band is Medium or Heavy; `Never` → false.
5. Estimates are offered for crews, workflows and tasks over more than 20 documents; a `Task` with ≤ 20 documents returns band Light with `ask_first = false` without arithmetic (2D-4).

**F. `may_start_ordin_work()`.** `daily_pause_requests = Some(limit)` and today's `model_turns ≥ limit` → `USAGE_DAILY_PAUSE_REACHED` (permanent for today; the Ordin Host shows the pause). Work the person starts directly is not paused (ADR-062: "Pause work Ordin starts").

**G. Reuse.**
1. `normalise(request)`: trim, collapse runs of white space to one space, Unicode case fold; 1–8,000 characters before normalising → else `REUSE_REQUEST_INVALID`.
2. `find_reusable(q)`: `reuse_answers` off → `None`. Select rows with the same normalised request digest (SHA-256 hex), agent and workspace, `expires_at > now`, most recent first. A row matches when its `input_versions` and `q.input_versions` hold the same set of `(name, location)` and every `version_read` is equal; a newer version on any input → skip (`REUSE_INPUT_NEWER` is logged, not returned). First match returned.
3. `index_answer(a)`: any of `external_actions`, `files_changed`, `approvals` > 0 → `USAGE_NOT_ANSWER_ONLY` (permanent). Insert with `text = a.text`, `expires_at = answered_at + 7 days`, `shown_again_at = answered_at`.
4. `record_reuse(id, run_id)`: update `shown_again_at`; journal `usage_answer_reused` {reused_answer_id, source_run_id} (04:188: each reuse is also journalled); increment `answered_without_model` today.
5. Rows past `expires_at` are deleted at start and once a day.

**H. Summaries.** `should_summarise(p, turns)`: `OnlyWhenAsked` → false; `p = None` (context use unknown: C-29 not confirmed, or the harness sent no value) → `turns ≥ 40` (`SUMMARISE_AFTER_TURNS = 40`, *assumed*, SA-CLOSE-01), whatever the Early/WhenNearlyFull choice; `p = Some(_)` (only once C-29 confirms the harness reports context use) → Early: `p ≥ 500`; WhenNearlyFull: `p ≥ 800`, and `turns` is ignored. A `p` above 1,000 is treated as 1,000. `continue_from_summary(last, now)`: setting on and `now − last > 1 hour`.

**I. Counting.**
1. `record_usage(u)`: upsert today's row of `usage_daily` adding `model_turns`, `tool_calls`, `turns_on_smaller` (when `level = Smaller`), `tokens` (input + output where reported), `tokens_from_cache`. Journal `usage_run_counted` {run_id, crew_run_id?, round?, level, model_turns, tool_calls, tokens?, cached?} (ADR-062 "What is counted"). Conditional on C-37: tokens and cache share are recorded only when the harness reports them; absent values stay absent, never zero.
2. `record_run_finished(work_key, run_id, turns)`: insert into `usage_work_history`; keep the newest 20 rows per key.
3. `today()`: today's row (zeros when absent); `smaller_share_permille = turns_on_smaller × 1000 / model_turns` (0 when no turns); `typical_day_permille = model_turns × 1000 / 250`.
4. A store error → `USAGE_STORE_UNAVAILABLE` (retryable). The Session Supervisor logs it and the run continues: counting never stops work.

## Data Model

Module `usage_planner`. SQLite on the desktop, the personal server and the single shape; PostgreSQL in the scaled shape holds only `usage_daily` and `usage_work_history` (the agent profile has no person's settings and no reuse, ADR-097 item 8 lists no Ordin there).

```sql
-- SQLite, migration 1
CREATE TABLE usage_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),          -- one row per installation (2C)
    settings TEXT NOT NULL,                         -- JSON: the person's own values only, by 2C field name
    updated_at TEXT NOT NULL
);
CREATE TABLE reused_answers (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    request TEXT NOT NULL CHECK (length(request) BETWEEN 1 AND 8000),
    request_digest TEXT NOT NULL CHECK (length(request_digest) = 64),
    agent TEXT NOT NULL,                            -- JSON AgentRef
    agent_key TEXT NOT NULL,                        -- "<kind>:<id>" for lookup
    workspace_id TEXT NOT NULL,
    input_versions TEXT NOT NULL,                   -- JSON Vec<InputUsed>
    source_run_id TEXT NOT NULL,
    text TEXT NOT NULL,                             -- ReusedAnswer.text, the earlier answer
    answered_at TEXT NOT NULL,
    shown_again_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX reused_answers_lookup ON reused_answers (request_digest, agent_key, workspace_id, expires_at);
CREATE TABLE usage_daily (
    day TEXT PRIMARY KEY,                           -- YYYY-MM-DD, local date
    model_turns INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    turns_on_smaller INTEGER NOT NULL DEFAULT 0,
    answered_without_model INTEGER NOT NULL DEFAULT 0,
    tokens INTEGER,
    tokens_from_cache INTEGER
);
CREATE TABLE usage_work_history (
    work_key TEXT NOT NULL CHECK (length(work_key) BETWEEN 1 AND 200),
    run_id TEXT NOT NULL,
    model_turns INTEGER NOT NULL CHECK (model_turns >= 0),
    finished_at TEXT NOT NULL,
    PRIMARY KEY (work_key, run_id)
);
```

```sql
-- PostgreSQL, migration 1 (scaled shape)
CREATE TABLE usage_daily (day date PRIMARY KEY, model_turns bigint NOT NULL DEFAULT 0, tool_calls bigint NOT NULL DEFAULT 0,
  turns_on_smaller bigint NOT NULL DEFAULT 0, answered_without_model bigint NOT NULL DEFAULT 0, tokens bigint, tokens_from_cache bigint);
CREATE TABLE usage_work_history (work_key text NOT NULL, run_id uuid NOT NULL, model_turns integer NOT NULL CHECK (model_turns >= 0),
  finished_at timestamptz NOT NULL, PRIMARY KEY (work_key, run_id));
```

`usage_daily` rows older than 400 days are deleted at start (*chosen here*). Through the Journal's `PruneParticipant` (`module()` = `usage_planner`), `usage_work_history` rows finished before `request.cutoff` (never a run in `request.runs_in_progress`) and `reused_answers` rows answered before it are deleted; the daily counters are aggregates with no run content and keep their 400-day rule. A store error → `PruneFailure { code: "USAGE_STORE_UNAVAILABLE" }`. Migrations are `MigrationStep` tuples (R3). No row holds an answer's text: `reused_answers.request` is the request, and the answer stays in the source run's artefacts (04:188).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The person changes a setting the company fixed (B.1) | `Err(permanent)`; Service API reply `kind: error` | `USAGE_FIXED_BY_COMPANY` | `{"code":"USAGE_FIXED_BY_COMPANY","message":"Set by your company."}` |
| A value above the company's frame (B.2–B.4) | `Err(permanent)` | `USAGE_ABOVE_PROFILE_LIMIT` | `{"code":"USAGE_ABOVE_PROFILE_LIMIT","message":"Your company allows at most 2 agents at the same time."}` |
| Unknown kind or level (B.2) | `Err(permanent)` | `USAGE_LEVEL_UNKNOWN` | `UsageError::LevelUnknown` |
| Agents at once outside 1–3 (B.3) | `Err(permanent)` | `USAGE_AGENTS_OUT_OF_RANGE` | `UsageError::AgentsOutOfRange` |
| The daily pause is reached (F) | `Err(permanent)` | `USAGE_DAILY_PAUSE_REACHED` | `UsageError::DailyPauseReached { turns_today, limit }` |
| Indexing a run that acted, changed or was approved (G.3) | `Err(permanent)` | `USAGE_NOT_ANSWER_ONLY` | `UsageError::NotAnswerOnly` |
| Request text empty or over 8,000 characters (G.1) | `Err(permanent)` | `REUSE_REQUEST_INVALID` | `UsageError::RequestInvalid` |
| Store unavailable | `Err(retryable)` | `USAGE_STORE_UNAVAILABLE` | `UsageError::Store`; Settings shows "Couldn't save this setting. Try again." with action `try_again` |
| Journal unavailable while recording | `Err(retryable)` | `USAGE_JOURNAL_UNAVAILABLE` | `UsageError::Journal`; counters are kept, the journal line is retried once |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Usage Planner

  Scenario: Happy path — Balanced chooses Smaller for routine work on Claude Code
    Given the profile's plan mode is balanced and no person's changes
    When choose is called for routine work on claude_local
    Then the level is smaller, the model "haiku" and thinking automatic

  Scenario: The harness's default map applies when the profile names no model
    Given the profile has no usage.level_models entry for claude_code
    And the Claude Local Adapter's HarnessModels maps standard to "sonnet"
    When choose is called for documents on claude_local
    Then the model is "sonnet" and model_choice() gives level standard with model "sonnet"

  Scenario: A profile model the harness does not accept
    Given usage.level_models.claude_code.standard is "sonnet-9" and the harness does not accept it
    When choose is called for documents on claude_local
    Then the model is the harness default for standard and usage_model_not_accepted is logged

  Scenario: Step up once, never twice
    Given a routine step ran on smaller and its result failed its check
    When step_up is called
    Then the level is standard with stepped_up true
    When step_up is called again on that choice
    Then it returns none

  Scenario: Company limits the levels for code
    Given usage.levels_allowed.code is [smaller, standard]
    And the person's choice is best_results
    When choose is called for code
    Then the level is standard

  Scenario: A setting the company fixed
    Given usage.person_may_change does not list reuse_answers
    When the person turns reuse_answers off
    Then update_settings returns USAGE_FIXED_BY_COMPANY and nothing is saved

  Scenario: Agents at once above the company maximum
    Given usage.agents_at_once_max is 2
    When the person sets agents_at_same_time to 3
    Then update_settings returns USAGE_ABOVE_PROFILE_LIMIT

  Scenario: Changing a level makes the choice Custom
    When the person sets talking to most_capable
    Then the choice is custom

  Scenario: Estimate from history
    Given three earlier runs of "workflow:W1:3" used 18, 22 and 30 model turns
    When estimate is called for "workflow:W1:3"
    Then min is 18, max is 30, the band is medium
    And based_on is earlier runs, count 3

  Scenario: Heavy estimate asks first under Balanced
    Given no history for a crew of 3 members with 10 rounds
    When estimate is called
    Then the band is heavy and ask_first is true

  Scenario: A small task is not estimated
    When estimate is called for a task over 12 documents
    Then the band is light and ask_first is false

  Scenario: Reuse an unchanged answer within 7 days
    Given an answer "Kestrel is cheapest." indexed 2 days ago for "Compare the Q3 quotes" by "analyst" in "Sales" over Quote.pdf version 3
    When find_reusable is called with the same request in different letter case over version 3
    Then the earlier answer is returned with text "Kestrel is cheapest."
    When record_reuse is called
    Then answered_without_model today rises by 1 and usage_answer_reused is journalled

  Scenario: Summarise after 40 turns when context use is unknown
    Given the summary setting is Early
    When should_summarise is called with no context use and 40 turns since the last summary
    Then it returns true
    And with no context use and 39 turns it returns false

  Scenario: Context use decides once C-29 is confirmed
    Given the summary setting is WhenNearlyFull
    When should_summarise is called with context use 820 per-mille and 3 turns
    Then it returns true

  Scenario: Only when asked never summarises by itself
    Given the summary setting is OnlyWhenAsked
    When should_summarise is called with no context use and 100 turns
    Then it returns false

  Scenario: No reuse when an input changed
    Given the same indexed answer
    When find_reusable is called over Quote.pdf version 4
    Then it returns none

  Scenario: Never index an action
    When index_answer is called for a run that made 1 external action
    Then it returns USAGE_NOT_ANSWER_ONLY

  Scenario: Daily pause stops Ordin's work, not the person's
    Given daily_pause_requests is 100 and 100 model turns were counted today
    When may_start_ordin_work is called
    Then it returns USAGE_DAILY_PAUSE_REACHED

  Scenario: Counting when the harness reports no tokens
    When record_usage is called with 4 model turns and no token figures
    Then today's model_turns rises by 4 and tokens stay absent

  Scenario: Continue from a summary after a break
    Given continue_from_summary_after_break is on
    When the last message was 61 minutes ago
    Then continue_from_summary returns true

  Scenario: Harnesses bound after start
    Given the planner was built with no harnesses
    When choose is called for documents on claude_local
    Then the model is none
    When the program calls set_harnesses with the Claude Local Adapter's HarnessModels
    And choose is called for documents on claude_local
    Then the model is "sonnet"

  Scenario: Ask before medium or heavy work
    Given ask_before_estimated_requests is medium_or_heavy
    When estimate gives the band medium
    Then ask_first is true

  Scenario: Store unavailable while saving a setting
    Given the database file cannot be written
    When update_settings is called
    Then it returns USAGE_STORE_UNAVAILABLE marked retryable
```

## Performance / Security / Observability

- **Performance targets:** `choose` and `step_up` are in-memory, p95 ≤ 50 µs. `estimate` p95 ≤ 5 ms; `find_reusable` p95 ≤ 5 ms with 10,000 index rows; `record_usage` p95 ≤ 5 ms. Settings commands answer within A-03's 150 ms. Memory: the effective policy under 16 KiB.
- **Security:** In-process API only; Service API commands arrive through the Command Bridge or the Remote API, checked against the command schema (`deny_unknown_fields`). The person can change only what the profile lets them, clamped to the profile and the application maximum. No secret is held. Never logged: the request text of a reused answer, input locations, model output. Model names are shown only to people who code and in Diagnostics (11 §3); this module returns them but the screens decide where they appear.
- **Observability:** log events (target `ordin_usage_planner`): `usage_level_chosen` {kind, harness, level} at debug; `usage_stepped_up` {kind, from, to}; `usage_settings_changed` {fields}; `usage_daily_pause_reached` {turns_today, limit}; `usage_answer_reused` {reused_answer_id}; `usage_levels_empty` {kind}. Journal records: `usage_run_counted`, `usage_answer_reused`, `usage_stepped_up`, `usage_settings_changed`. No metric of its own (Metrics.md N-1: only the 50 families of the closed list exist (49 before ADR-107); model turns appear as the Session Supervisor's span `ordin.model_turn`).

Reconciliation assumptions (January 2026):
- **SA-RECON-01** — 2C owns `ModelLevel`, `ThinkingLevel`, `ModelChoice` and `HarnessModels`; this crate's rich result is `LevelChoice` (Approach).
- **SA-RECON-02** — R1: the Service API catalogue adopts `usage.get_settings`, `usage.update_settings` and `usage.today` with the results above (`Today` in per-mille).
- **SA-RECON-03** — R3, R20: journal records go through `append_entry`; the tables live in the Journal's `Datastore`; history is pruned through `PruneParticipant`.

## Gaps

None.
