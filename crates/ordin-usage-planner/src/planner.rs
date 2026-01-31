//! The Usage Planner (UsagePlanner.md *Internal Logic* A to I).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use ordin_journal::{
    Datastore, Journal, MigrationStep, ModuleMigrations, NewJournalEntry, PruneFailure,
    PruneParticipant, PruneRequest,
};
use ordin_organisation_profile::{
    HarnessName, OrganisationProfileService, ProfileSnapshot, WorkKind,
};
use ordin_runtime_adapter::{HarnessKind, HarnessModels};
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::domain::identity::{AskBefore, SummariseWhen, UsageChoice, UsageSettings};
use ordin_types::domain::runs::ReusedAnswer;
use ordin_types::time::Timestamp;
use ordin_types::wire::run::{AdapterKind, ModelLevel};
use ordin_types::{CodedError, CrewRunId, ReusedAnswerId, RunId, WorkspaceId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::error::UsageError;
use crate::log;
use crate::policy::{clamp_level, kind_text, level_text, table_map, Policy, AGENTS_MAX};
use crate::store::{self, DailyAdd, ReusedRow};
use crate::types::{
    AnswerToIndex, EstimateBand, EstimateBasis, EstimateRequest, LevelChoice, ReuseQuery, RunUsage,
    Today, UsageEstimate, UsageSettingsChange, WorkShape,
};

const SQLITE_MIGRATIONS: &[MigrationStep] = &[(
    1,
    "create usage planner tables",
    include_str!("sql/sqlite_v1.sql"),
)];
const POSTGRES_MIGRATIONS: &[MigrationStep] = &[(
    1,
    "create usage planner history tables",
    include_str!("sql/postgres_v1.sql"),
)];

/// Reused answer lifetime (Fixed constants: 7 days).
const REUSE_DAYS: i64 = 7;
/// A task over more than this many documents is estimated (Fixed constants).
const ESTIMATE_DOCUMENTS: u32 = 20;
/// Break after which a conversation continues from a summary (Fixed constants: 1 hour).
const BREAK_MILLIS: i64 = 3_600_000;
/// A typical day's use of the plan (Fixed constants, SA-USAGE-01).
// spec: ordin-desktop/components/UsagePlanner#SA-USAGE-01
const TYPICAL_DAY_TURNS: u64 = 250;
/// Estimate bands (Fixed constants, SA-USAGE-02): Light up to this many turns, Medium up to the next.
// spec: ordin-desktop/components/UsagePlanner#SA-USAGE-02
const LIGHT_MAX_TURNS: u32 = 25;
const MEDIUM_MAX_TURNS: u32 = 100;
/// Summarise when the context is at least this many per-mille of the window (SA-USAGE-03).
// spec: ordin-desktop/components/UsagePlanner#SA-USAGE-03
const EARLY_PERMILLE: u32 = 500;
const NEARLY_FULL_PERMILLE: u32 = 800;
/// SA-CLOSE-01: Ordin turns after which a conversation is summarised when context use is unknown.
// spec: ordin-desktop/components/UsagePlanner#SA-CLOSE-01
const SUMMARISE_AFTER_TURNS: u32 = 40;
/// SA-USAGE-04: turns assumed per agent step with no history.
// spec: ordin-desktop/components/UsagePlanner#SA-USAGE-04
const TURNS_PER_STEP: u64 = 8;
/// E.2: the crew maximum cap, 30 rounds of 9 members at 8 turns.
const CREW_MAX_CAP: u64 = 30 * 9 * 8;
/// `usage_daily` rows older than this many days are deleted at start.
const DAILY_KEEP_DAYS: i64 = 400;
/// `ReusedAnswer.request` limit before normalising (2C).
const REQUEST_MAX_CHARS: usize = 8_000;
/// The wait before the one retry of a failed journal line.
const JOURNAL_RETRY: Duration = Duration::from_millis(200);

/// Where the planner reads the profile snapshot from; the organisation profile service in the
/// programs, a fixed snapshot in tests.
pub(crate) trait SnapshotSource: Send + Sync {
    fn current(&self) -> Arc<ProfileSnapshot>;
}

impl SnapshotSource for OrganisationProfileService {
    fn current(&self) -> Arc<ProfileSnapshot> {
        OrganisationProfileService::current(self)
    }
}

struct State {
    stored: UsageSettingsChange,
    updated_at: Timestamp,
    policy: Arc<Policy>,
}

pub struct UsagePlanner {
    pool: SqlitePool,
    journal: Arc<Journal>,
    source: Arc<dyn SnapshotSource>,
    harnesses: RwLock<Vec<Arc<dyn HarnessModels>>>,
    state: Mutex<State>,
    /// One settings change at a time.
    update_lock: tokio::sync::Mutex<()>,
    /// The local day of the last expiry sweep (G.5).
    pub(crate) swept_on: Mutex<String>,
}

/// A poisoned lock still holds consistent data here (every critical section is a plain assignment).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The local date of the computer, the key of `usage_daily` (Fixed constants).
fn day_key() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn clamp_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The profile and registry names of a local harness; `None` for a remote adapter.
fn harness_of(adapter: AdapterKind) -> Option<(HarnessName, HarnessKind)> {
    match adapter {
        AdapterKind::ClaudeLocal => Some((HarnessName::ClaudeCode, HarnessKind::ClaudeCode)),
        AdapterKind::CopilotLocal => Some((HarnessName::Copilot, HarnessKind::CopilotCli)),
        AdapterKind::HostedAgent | AdapterKind::SlackThread => None,
    }
}

fn adapter_text(adapter: AdapterKind) -> &'static str {
    match adapter {
        AdapterKind::ClaudeLocal => "claude_local",
        AdapterKind::CopilotLocal => "copilot_local",
        AdapterKind::HostedAgent => "hosted_agent",
        AdapterKind::SlackThread => "slack_thread",
    }
}

fn agent_key(agent: &AgentRef) -> String {
    match agent {
        AgentRef::AgentInstance { agent_instance_id } => {
            format!("agent_instance:{agent_instance_id}")
        }
        AgentRef::RemoteAgent { remote_agent_id } => format!("remote_agent:{remote_agent_id}"),
    }
}

/// G.1: the request must be 1 to 8,000 characters before normalising.
fn check_request(request: &str) -> Result<(), UsageError> {
    if (1..=REQUEST_MAX_CHARS).contains(&request.chars().count()) {
        Ok(())
    } else {
        Err(UsageError::RequestInvalid)
    }
}

/// G.1: trim, collapse white space to one space, case fold, then SHA-256 as lower-case hex.
// SPEC-GAP-4: the spec asks for a "Unicode case fold"; the standard library offers full lower-casing
// only, which is used here. The two differ for a few letters (for example the German sharp s).
pub(crate) fn request_digest(request: &str) -> String {
    let normalised = request
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    format!("{:x}", Sha256::digest(normalised.as_bytes()))
}

fn present_fields(change: &UsageSettingsChange) -> Vec<&'static str> {
    let mut fields = Vec::new();
    let mut add = |present: bool, name: &'static str| {
        if present {
            fields.push(name);
        }
    };
    add(change.choice.is_some(), "choice");
    add(
        change.model_level_per_kind.is_some(),
        "model_level_per_kind",
    );
    add(change.thinking.is_some(), "thinking");
    add(
        change.answer_simple_requests_without_model.is_some(),
        "answer_simple_requests_without_model",
    );
    add(change.reuse_answers.is_some(), "reuse_answers");
    add(
        change.give_agents_only_needed_parts.is_some(),
        "give_agents_only_needed_parts",
    );
    add(
        change.summarise_long_conversations.is_some(),
        "summarise_long_conversations",
    );
    add(
        change.continue_from_summary_after_break.is_some(),
        "continue_from_summary_after_break",
    );
    add(
        change.ask_before_estimated_requests.is_some(),
        "ask_before_estimated_requests",
    );
    add(change.agents_at_same_time.is_some(), "agents_at_same_time");
    add(
        change.show_usage_on_each_run.is_some(),
        "show_usage_on_each_run",
    );
    add(
        change.daily_pause_requests.is_some(),
        "daily_pause_requests",
    );
    fields
}

impl UsagePlanner {
    pub const MODULE: &'static str = "usage_planner";

    // spec: ordin-desktop/components/UsagePlanner#SA-RECON-03
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: SQLITE_MIGRATIONS,
            postgres: POSTGRES_MIGRATIONS,
        }
    }

    /// Opens the store, loads the person's settings, builds the effective policy and clears what has
    /// expired (A, G.5, Data Model).
    pub async fn new(
        datastore: Datastore,
        journal: Arc<Journal>,
        profile: Arc<OrganisationProfileService>,
        harnesses: Vec<Arc<dyn HarnessModels>>,
    ) -> Result<Arc<Self>, UsageError> {
        Self::build(datastore, journal, profile, harnesses).await
    }

    pub(crate) async fn build(
        datastore: Datastore,
        journal: Arc<Journal>,
        source: Arc<dyn SnapshotSource>,
        harnesses: Vec<Arc<dyn HarnessModels>>,
    ) -> Result<Arc<Self>, UsageError> {
        // SPEC-GAP-5: the Data Model gives `usage_daily` and `usage_work_history` a PostgreSQL slice
        // (scaled shape) but the Internal Logic statements are written once, for SQLite. Release 1
        // builds the desktop profile only, so a PostgreSQL datastore is refused as an unavailable
        // store until the agent profile is built.
        let pool = match datastore {
            Datastore::Sqlite(pool) => pool,
            Datastore::Postgres(_) => {
                return Err(UsageError::Store(sqlx::Error::Configuration(
                    "the usage planner store is built for SQLite only".into(),
                )))
            }
        };
        let (stored, updated_at) = match store::load_settings(&pool).await? {
            Some((json, updated)) => (
                serde_json::from_str::<UsageSettingsChange>(&json)
                    .map_err(|_| UsageError::corrupt("usage settings"))?,
                store::parse_ts(&updated)?,
            ),
            None => (
                UsageSettingsChange::default(),
                Timestamp::from_unix_millis(0).unwrap_or_else(|_| Timestamp::now()),
            ),
        };
        let policy = Arc::new(Policy::build(source.current(), &stored, updated_at));
        store::delete_expired(&pool, &Timestamp::now().to_wire()).await?;
        let cutoff = (chrono::Local::now() - chrono::TimeDelta::days(DAILY_KEEP_DAYS))
            .format("%Y-%m-%d")
            .to_string();
        store::delete_old_days(&pool, &cutoff).await?;
        Ok(Arc::new(UsagePlanner {
            pool,
            journal,
            source,
            harnesses: RwLock::new(harnesses),
            state: Mutex::new(State {
                stored,
                updated_at,
                policy,
            }),
            update_lock: tokio::sync::Mutex::new(()),
            swept_on: Mutex::new(day_key()),
        }))
    }

    /// Late binding (A.5): replaces the harness list as a whole; the policy is not rebuilt.
    pub fn set_harnesses(&self, harnesses: Vec<Arc<dyn HarnessModels>>) {
        *self
            .harnesses
            .write()
            .unwrap_or_else(PoisonError::into_inner) = harnesses;
    }

    /// The policy in force: rebuilt when the profile snapshot has changed since the last call (A).
    // spec: ordin-desktop/components/UsagePlanner#SA-RECON-01
    fn policy(&self) -> Arc<Policy> {
        let current = self.source.current();
        let mut state = lock(&self.state);
        if !Arc::ptr_eq(&state.policy.snapshot, &current) {
            state.policy = Arc::new(Policy::build(current, &state.stored, state.updated_at));
        }
        state.policy.clone()
    }

    // ---- settings

    pub fn settings(&self) -> UsageSettings {
        self.policy().settings.clone()
    }

    pub fn agents_at_once(&self) -> u32 {
        self.policy().settings.agents_at_same_time.value
    }

    pub fn parts_only(&self) -> bool {
        self.policy().settings.give_agents_only_needed_parts.value
    }

    pub fn answer_without_model(&self) -> bool {
        self.policy()
            .settings
            .answer_simple_requests_without_model
            .value
    }

    pub fn show_usage_on_each_run(&self) -> bool {
        self.policy().settings.show_usage_on_each_run.value
    }

    /// B. Validates the change against the profile frame, saves the person's values and rebuilds the policy.
    pub async fn update_settings(
        &self,
        change: UsageSettingsChange,
    ) -> Result<UsageSettings, UsageError> {
        let _one_at_a_time = self.update_lock.lock().await;
        let policy = self.policy();
        let fields = present_fields(&change);
        // B.1
        for field in &fields {
            if !policy.person_may_change(field) {
                return Err(UsageError::FixedByCompany { setting: field });
            }
        }
        let mut candidate = lock(&self.state).stored.clone();

        // B.5: a named choice resets the per-kind levels and drops a stored thinking value.
        if let Some(choice) = change.choice {
            match choice {
                UsageChoice::Custom => {
                    if candidate.model_level_per_kind.is_none() {
                        candidate.model_level_per_kind =
                            Some(table_map(policy.settings.choice.value));
                    }
                }
                _ => {
                    candidate.model_level_per_kind = None;
                    candidate.thinking = None;
                }
            }
            candidate.choice = Some(choice);
        }
        if let Some(thinking) = change.thinking {
            candidate.thinking = Some(thinking);
        }
        // B.2
        if let Some(levels) = &change.model_level_per_kind {
            let mut merged = candidate.model_level_per_kind.clone().unwrap_or_else(|| {
                table_map(candidate.choice.unwrap_or(policy.settings.choice.value))
            });
            for (key, value) in levels {
                let kind = crate::policy::parse_kind(key).ok_or(UsageError::LevelUnknown)?;
                let level = crate::policy::parse_level(value).ok_or(UsageError::LevelUnknown)?;
                if !policy.allowed(kind).contains(&level) {
                    return Err(UsageError::AboveProfileLimit {
                        setting: "model_level_per_kind",
                    });
                }
                merged.insert(key.clone(), value.clone());
            }
            candidate.model_level_per_kind = Some(merged);
            candidate.choice = Some(UsageChoice::Custom);
        }
        // B.3
        if let Some(agents) = change.agents_at_same_time {
            if !(1..=AGENTS_MAX).contains(&agents) {
                return Err(UsageError::AgentsOutOfRange);
            }
            let max = policy
                .snapshot
                .document
                .usage
                .agents_at_once_max
                .clamp(1, AGENTS_MAX);
            if agents > max {
                return Err(UsageError::AboveProfileLimit {
                    setting: "agents_at_same_time",
                });
            }
            candidate.agents_at_same_time = Some(agents);
        }
        // B.4
        if let Some(pause) = change.daily_pause_requests {
            if matches!(pause, Some(limit) if ![100, 250, 500].contains(&limit)) {
                return Err(UsageError::AboveProfileLimit {
                    setting: "daily_pause_requests",
                });
            }
            candidate.daily_pause_requests = Some(pause);
        }
        if let Some(value) = change.answer_simple_requests_without_model {
            candidate.answer_simple_requests_without_model = Some(value);
        }
        if let Some(value) = change.reuse_answers {
            candidate.reuse_answers = Some(value);
        }
        if let Some(value) = change.give_agents_only_needed_parts {
            candidate.give_agents_only_needed_parts = Some(value);
        }
        if let Some(value) = change.summarise_long_conversations {
            candidate.summarise_long_conversations = Some(value);
        }
        if let Some(value) = change.continue_from_summary_after_break {
            candidate.continue_from_summary_after_break = Some(value);
        }
        if let Some(value) = change.ask_before_estimated_requests {
            candidate.ask_before_estimated_requests = Some(value);
        }
        if let Some(value) = change.show_usage_on_each_run {
            candidate.show_usage_on_each_run = Some(value);
        }

        // B.6
        let updated_at = Timestamp::now();
        let json =
            serde_json::to_string(&candidate).map_err(|_| UsageError::corrupt("usage settings"))?;
        store::save_settings(&self.pool, &json, &updated_at.to_wire()).await?;
        {
            let mut state = lock(&self.state);
            state.stored = candidate;
            state.updated_at = updated_at;
            state.policy = Arc::new(Policy::build(
                self.source.current(),
                &state.stored,
                state.updated_at,
            ));
        }
        log::settings_changed(&fields.join(","));
        self.journal_entry(
            "usage_settings_changed",
            None,
            None,
            "The person changed their usage settings.".to_owned(),
            json!({ "fields": fields }),
        )
        .await?;
        Ok(self.settings())
    }

    // ---- levels

    /// C. The level, model and thinking for one step of `kind` on `harness`.
    pub fn choose(&self, kind: WorkKind, harness: AdapterKind) -> LevelChoice {
        let policy = self.policy();
        let mut level = policy.level_for(kind);
        // C.2: Claude Code has no automatic model choice.
        if level == ModelLevel::Automatic && harness == AdapterKind::ClaudeLocal {
            level = ModelLevel::Standard;
        }
        // C.3
        let allowed = policy.allowed(kind);
        level = match clamp_level(&allowed, level) {
            Some(clamped) => clamped,
            None => {
                log::levels_empty(kind_text(kind));
                ModelLevel::Standard
            }
        };
        let model = self.model_for(&policy, harness, level);
        log::level_chosen(kind_text(kind), adapter_text(harness), level_text(level));
        LevelChoice {
            level,
            model,
            thinking: policy.settings.thinking.value,
            stepped_up: false,
        }
    }

    /// C.4: the profile's model for the harness and level, else the harness's default map.
    fn model_for(
        &self,
        policy: &Policy,
        adapter: AdapterKind,
        level: ModelLevel,
    ) -> Option<String> {
        let (name, kind) = harness_of(adapter)?;
        if level == ModelLevel::Automatic && adapter == AdapterKind::CopilotLocal {
            return None;
        }
        let models = self
            .harnesses
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|candidate| candidate.harness() == kind)
            .cloned()?;
        let default = models.default_level_models().get(&level).cloned();
        let named = policy
            .snapshot
            .document
            .usage
            .level_models
            .get(&name)
            .and_then(|by_level| by_level.get(&level));
        match named {
            Some(model) if models.accepts_model(model) => Some(model.clone()),
            Some(_) => {
                log::model_not_accepted(adapter_text(adapter), level_text(level));
                default
            }
            None => default,
        }
    }

    /// D. One step up after a shortfall, once.
    pub fn step_up(
        &self,
        previous: &LevelChoice,
        kind: WorkKind,
        harness: AdapterKind,
    ) -> Option<LevelChoice> {
        if previous.stepped_up {
            return None;
        }
        let next = match previous.level {
            ModelLevel::Automatic | ModelLevel::Smaller => ModelLevel::Standard,
            ModelLevel::Standard => ModelLevel::MostCapable,
            ModelLevel::MostCapable => return None,
        };
        let policy = self.policy();
        if !policy.allowed(kind).contains(&next) {
            return None;
        }
        let model = self.model_for(&policy, harness, next);
        log::stepped_up(
            kind_text(kind),
            level_text(previous.level),
            level_text(next),
        );
        self.journal_in_background(
            "usage_stepped_up",
            "A step was tried again on a more capable level.".to_owned(),
            json!({
                "kind": kind_text(kind),
                "from": level_text(previous.level),
                "to": level_text(next),
            }),
        );
        Some(LevelChoice {
            level: next,
            model,
            thinking: previous.thinking,
            stepped_up: true,
        })
    }

    // ---- estimates and the daily pause

    /// E.
    pub async fn estimate(&self, req: EstimateRequest) -> Result<UsageEstimate, UsageError> {
        let ask = self.policy().settings.ask_before_estimated_requests.value;
        // E.5
        if let WorkShape::Task {
            documents,
            total_bytes,
        } = req.shape
        {
            if documents <= ESTIMATE_DOCUMENTS {
                // SPEC-GAP-6: E.5 gives a small task the band Light without arithmetic and names no
                // turn figures; the minimum, maximum and day share are 0.
                return Ok(UsageEstimate {
                    band: EstimateBand::Light,
                    min_turns: 0,
                    max_turns: 0,
                    typical_day_permille: 0,
                    based_on: vec![EstimateBasis::Documents {
                        count: documents,
                        total_bytes,
                    }],
                    ask_first: false,
                });
            }
        }
        // E.1
        let history = store::recent_history(&self.pool, &req.work_key).await?;
        let (min_turns, max_turns, based_on) =
            if let (Some(min), Some(max)) = (history.iter().min(), history.iter().max()) {
                (
                    to_u32(*min),
                    to_u32(*max),
                    vec![EstimateBasis::EarlierRuns {
                        count: clamp_u32(history.len() as u64),
                    }],
                )
            } else {
                // E.2
                match req.shape {
                    WorkShape::Crew {
                        members,
                        max_rounds,
                    } => {
                        let per_round = u64::from(members) * TURNS_PER_STEP;
                        let max = (u64::from(max_rounds) * (per_round + 1)).min(CREW_MAX_CAP);
                        (
                            clamp_u32(per_round),
                            clamp_u32(max),
                            vec![
                                EstimateBasis::Members { count: members },
                                EstimateBasis::Limits { max_rounds },
                            ],
                        )
                    }
                    WorkShape::Workflow {
                        agent_steps,
                        ordin_turns,
                    } => {
                        let steps = u64::from(agent_steps) * TURNS_PER_STEP;
                        (
                            clamp_u32(steps + u64::from(ordin_turns)),
                            clamp_u32(steps * 2 + u64::from(ordin_turns)),
                            vec![EstimateBasis::Steps { count: agent_steps }],
                        )
                    }
                    WorkShape::Task {
                        documents,
                        total_bytes,
                    } => (
                        clamp_u32(4 + u64::from(documents) / 2),
                        clamp_u32(4 + u64::from(documents) * 2),
                        vec![EstimateBasis::Documents {
                            count: documents,
                            total_bytes,
                        }],
                    ),
                }
            };
        // E.3
        let band = if max_turns <= LIGHT_MAX_TURNS {
            EstimateBand::Light
        } else if max_turns <= MEDIUM_MAX_TURNS {
            EstimateBand::Medium
        } else {
            EstimateBand::Heavy
        };
        let typical_day_permille =
            clamp_u32((u64::from(max_turns) * 1000 / TYPICAL_DAY_TURNS).min(10_000));
        // E.4
        let ask_first = match ask {
            AskBefore::Heavy => band == EstimateBand::Heavy,
            AskBefore::MediumOrHeavy => band >= EstimateBand::Medium,
            AskBefore::Never => false,
        };
        Ok(UsageEstimate {
            band,
            min_turns,
            max_turns,
            typical_day_permille,
            based_on,
            ask_first,
        })
    }

    /// F. Refuses work Ordin starts once today's model turns reach the daily pause.
    pub async fn may_start_ordin_work(&self) -> Result<(), UsageError> {
        let Some(limit) = self.policy().settings.daily_pause_requests.value else {
            return Ok(());
        };
        let turns_today = to_u32(store::daily(&self.pool, &day_key()).await?.model_turns);
        if turns_today >= limit {
            log::daily_pause_reached(turns_today, limit);
            return Err(UsageError::DailyPauseReached { turns_today, limit });
        }
        Ok(())
    }

    // ---- reuse

    /// G.5: rows past `expires_at` go once a day.
    async fn sweep_if_due(&self) -> Result<(), UsageError> {
        let day = day_key();
        if *lock(&self.swept_on) == day {
            return Ok(());
        }
        store::delete_expired(&self.pool, &Timestamp::now().to_wire()).await?;
        *lock(&self.swept_on) = day;
        Ok(())
    }

    /// G.2.
    pub async fn find_reusable(&self, q: ReuseQuery) -> Result<Option<ReusedAnswer>, UsageError> {
        check_request(&q.request)?;
        if !self.policy().settings.reuse_answers.value {
            return Ok(None);
        }
        self.sweep_if_due().await?;
        let rows = store::reuse_candidates(
            &self.pool,
            &request_digest(&q.request),
            &agent_key(&q.agent),
            &q.workspace_id.to_string(),
            &Timestamp::now().to_wire(),
        )
        .await?;
        let wanted = input_map(&q.input_versions);
        for row in rows {
            let held: Vec<InputUsed> = serde_json::from_str(&row.input_versions)
                .map_err(|_| UsageError::corrupt("input versions"))?;
            let held = input_map(&held);
            if !held.keys().eq(wanted.keys()) {
                continue;
            }
            if held != wanted {
                log::reuse_input_newer();
                continue;
            }
            return reused_answer(row).map(Some);
        }
        Ok(None)
    }

    /// G.3.
    pub async fn index_answer(&self, a: AnswerToIndex) -> Result<ReusedAnswerId, UsageError> {
        check_request(&a.query.request)?;
        if a.external_actions > 0 || a.files_changed > 0 || a.approvals > 0 {
            return Err(UsageError::NotAnswerOnly);
        }
        self.sweep_if_due().await?;
        let id = ReusedAnswerId::new();
        let expires_at = Timestamp(a.answered_at.0 + chrono::TimeDelta::days(REUSE_DAYS));
        let row = ReusedRow {
            id: id.to_string(),
            request: a.query.request.clone(),
            agent: serde_json::to_string(&a.query.agent)
                .map_err(|_| UsageError::corrupt("agent"))?,
            workspace_id: a.query.workspace_id.to_string(),
            input_versions: serde_json::to_string(&a.query.input_versions)
                .map_err(|_| UsageError::corrupt("input versions"))?,
            source_run_id: a.source_run_id.to_string(),
            text: a.text,
            answered_at: a.answered_at.to_wire(),
            shown_again_at: a.answered_at.to_wire(),
            expires_at: expires_at.to_wire(),
        };
        store::insert_reused(
            &self.pool,
            &row,
            &request_digest(&a.query.request),
            &agent_key(&a.query.agent),
        )
        .await?;
        Ok(id)
    }

    /// G.4. An id no row has changes nothing.
    // SPEC-GAP-7: G.4 and the Error Table give no row for an unknown reused-answer id; the call
    // returns Ok and records nothing.
    pub async fn record_reuse(
        &self,
        id: ReusedAnswerId,
        run_id: Option<RunId>,
    ) -> Result<(), UsageError> {
        let id_text = id.to_string();
        let Some(source) =
            store::touch_reused(&self.pool, &id_text, &Timestamp::now().to_wire()).await?
        else {
            return Ok(());
        };
        store::add_daily(
            &self.pool,
            &day_key(),
            DailyAdd {
                answered_without_model: 1,
                ..DailyAdd::default()
            },
        )
        .await?;
        log::answer_reused(&id_text);
        self.journal_entry(
            "usage_answer_reused",
            run_id,
            None,
            "An earlier answer was shown again.".to_owned(),
            json!({ "reused_answer_id": id_text, "source_run_id": source }),
        )
        .await
    }

    // ---- summaries

    /// H.
    // spec: ordin-desktop/components/UsagePlanner#SA-CLOSE-01
    pub fn should_summarise(
        &self,
        context_used_permille: Option<u32>,
        turns_since_summary: u32,
    ) -> bool {
        let when = self.policy().settings.summarise_long_conversations.value;
        if when == SummariseWhen::OnlyWhenAsked {
            return false;
        }
        match context_used_permille {
            None => turns_since_summary >= SUMMARISE_AFTER_TURNS,
            Some(used) => {
                let used = used.min(1_000);
                match when {
                    SummariseWhen::Early => used >= EARLY_PERMILLE,
                    _ => used >= NEARLY_FULL_PERMILLE,
                }
            }
        }
    }

    pub fn continue_from_summary(&self, last_message_at: Timestamp, now: Timestamp) -> bool {
        self.policy()
            .settings
            .continue_from_summary_after_break
            .value
            && now.unix_millis() - last_message_at.unix_millis() > BREAK_MILLIS
    }

    // ---- counting

    /// I.1.
    pub async fn record_usage(&self, u: RunUsage) -> Result<(), UsageError> {
        let tokens = match (u.delta.input_tokens, u.delta.output_tokens) {
            (None, None) => None,
            (input, output) => Some(to_i64(
                input.unwrap_or(0).saturating_add(output.unwrap_or(0)),
            )),
        };
        let cached = u.delta.cached_input_tokens.map(to_i64);
        store::add_daily(
            &self.pool,
            &day_key(),
            DailyAdd {
                model_turns: i64::from(u.delta.model_turns),
                tool_calls: i64::from(u.delta.tool_calls),
                turns_on_smaller: if u.level == ModelLevel::Smaller {
                    i64::from(u.delta.model_turns)
                } else {
                    0
                },
                answered_without_model: 0,
                tokens,
                tokens_from_cache: cached,
            },
        )
        .await?;
        let mut data = serde_json::Map::new();
        data.insert("run_id".into(), json!(u.run_id.to_string()));
        if let Some(crew) = u.crew_run_id {
            data.insert("crew_run_id".into(), json!(crew.to_string()));
        }
        if let Some(round) = u.round {
            data.insert("round".into(), json!(round));
        }
        data.insert("level".into(), json!(level_text(u.level)));
        data.insert("model_turns".into(), json!(u.delta.model_turns));
        data.insert("tool_calls".into(), json!(u.delta.tool_calls));
        if let Some(tokens) = tokens {
            data.insert("tokens".into(), json!(tokens));
        }
        if let Some(cached) = cached {
            data.insert("cached".into(), json!(cached));
        }
        self.journal_entry(
            "usage_run_counted",
            Some(u.run_id),
            u.crew_run_id,
            "A run's model use was counted.".to_owned(),
            Value::Object(data),
        )
        .await
    }

    /// I.2.
    pub async fn record_run_finished(
        &self,
        work_key: &str,
        run_id: RunId,
        model_turns: u32,
    ) -> Result<(), UsageError> {
        store::insert_history(
            &self.pool,
            work_key,
            &run_id.to_string(),
            i64::from(model_turns),
            &Timestamp::now().to_wire(),
        )
        .await?;
        Ok(())
    }

    pub async fn record_answered_without_model(&self) -> Result<(), UsageError> {
        store::add_daily(
            &self.pool,
            &day_key(),
            DailyAdd {
                answered_without_model: 1,
                ..DailyAdd::default()
            },
        )
        .await?;
        Ok(())
    }

    /// I.3.
    pub async fn today(&self) -> Result<Today, UsageError> {
        let row = store::daily(&self.pool, &day_key()).await?;
        let turns = row.model_turns.max(0) as u64;
        let smaller_share_permille = (row.turns_on_smaller.max(0) as u64 * 1000)
            .checked_div(turns)
            .map_or(0, clamp_u32);
        Ok(Today {
            model_turns: to_u32(row.model_turns),
            smaller_share_permille,
            answered_without_model: to_u32(row.answered_without_model),
            typical_day_permille: clamp_u32(turns * 1000 / TYPICAL_DAY_TURNS),
        })
    }

    /// The Journal's pruning hook (Data Model).
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant> {
        Arc::new(UsagePruneParticipant)
    }

    // ---- journal

    /// One journal line; a failure is tried once more after a short wait (Error Table).
    async fn journal_entry(
        &self,
        kind: &str,
        run_id: Option<RunId>,
        crew_run_id: Option<CrewRunId>,
        summary: String,
        data: Value,
    ) -> Result<(), UsageError> {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: Self::MODULE.to_owned(),
            run_id,
            crew_run_id,
            workspace_id: None,
            summary,
            data,
        };
        if self.journal.append_entry(entry.clone()).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(JOURNAL_RETRY).await;
        self.journal
            .append_entry(entry)
            .await
            .map(|_| ())
            .map_err(UsageError::Journal)
    }

    /// A journal line from a synchronous call (`step_up`): written on a spawned task, because the
    /// call returns at once.
    // SPEC-GAP-8: `step_up` is synchronous in the Public Interface but D asks for a journal line,
    // which is asynchronous. The line is written by a spawned task; outside a Tokio runtime it is
    // not written and `usage_journal_failed` is logged.
    fn journal_in_background(&self, kind: &'static str, summary: String, data: Value) {
        let journal = self.journal.clone();
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: Self::MODULE.to_owned(),
            run_id: None,
            crew_run_id: None,
            workspace_id: None,
            summary,
            data,
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    if let Err(error) = journal.append_entry(entry).await {
                        log::journal_failed(CodedError::code(&error), kind);
                    }
                });
            }
            Err(_) => log::journal_failed("USAGE_JOURNAL_UNAVAILABLE", kind),
        }
    }
}

fn input_map(inputs: &[InputUsed]) -> BTreeMap<(String, String), String> {
    inputs
        .iter()
        .map(|input| {
            (
                (input.name.clone(), input.location.clone()),
                input.version_read.clone(),
            )
        })
        .collect()
}

fn reused_answer(row: ReusedRow) -> Result<ReusedAnswer, UsageError> {
    Ok(ReusedAnswer {
        id: ReusedAnswerId::parse(&row.id).map_err(|_| UsageError::corrupt("reused answer id"))?,
        request: row.request,
        agent: serde_json::from_str(&row.agent).map_err(|_| UsageError::corrupt("agent"))?,
        workspace_id: WorkspaceId::parse(&row.workspace_id)
            .map_err(|_| UsageError::corrupt("workspace id"))?,
        input_versions: serde_json::from_str(&row.input_versions)
            .map_err(|_| UsageError::corrupt("input versions"))?,
        source_run_id: RunId::parse(&row.source_run_id)
            .map_err(|_| UsageError::corrupt("run id"))?,
        text: row.text,
        answered_at: store::parse_ts(&row.answered_at)?,
        shown_again_at: store::parse_ts(&row.shown_again_at)?,
        expires_at: store::parse_ts(&row.expires_at)?,
    })
}

/// The Journal's pruning of history and reused answers.
struct UsagePruneParticipant;

#[async_trait]
impl PruneParticipant for UsagePruneParticipant {
    fn module(&self) -> &'static str {
        UsagePlanner::MODULE
    }

    async fn prune(
        &self,
        datastore: &Datastore,
        request: &PruneRequest,
    ) -> Result<u64, PruneFailure> {
        let failed = || PruneFailure {
            code: "USAGE_STORE_UNAVAILABLE",
        };
        let Datastore::Sqlite(pool) = datastore else {
            return Err(failed());
        };
        let runs: Vec<String> = request
            .runs_in_progress
            .iter()
            .map(|run| run.to_string())
            .collect();
        let runs = serde_json::to_string(&runs).map_err(|_| failed())?;
        store::prune(pool, &request.cutoff.to_wire(), &runs)
            .await
            .map_err(|_| failed())
    }
}
