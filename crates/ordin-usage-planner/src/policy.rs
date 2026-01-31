//! The effective policy (UsagePlanner.md *Internal Logic* A): the company frame from the profile
//! snapshot, the person's stored values where the profile lets them, and the clamps.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ordin_organisation_profile::{
    DailyPause, PlanMode, ProfileSnapshot, UsageSettingName, WorkKind,
};
use ordin_types::common::{SetBy, Setting};
use ordin_types::domain::identity::{UsageChoice, UsageSettings};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::{ModelLevel, ThinkingLevel};

use crate::types::UsageSettingsChange;

/// The five kinds of work, in the order the profile names them.
pub(crate) const KINDS: [WorkKind; 5] = [
    WorkKind::Talking,
    WorkKind::Coordinating,
    WorkKind::Routine,
    WorkKind::Documents,
    WorkKind::Code,
];

/// All four levels: what a kind allows when the profile names none (2D-2).
pub(crate) const ALL_LEVELS: [ModelLevel; 4] = [
    ModelLevel::Automatic,
    ModelLevel::Smaller,
    ModelLevel::Standard,
    ModelLevel::MostCapable,
];

/// The largest number of agents at the same time (ADR-062: 1, 2 or 3).
pub(crate) const AGENTS_MAX: u32 = 3;

pub(crate) fn kind_text(kind: WorkKind) -> &'static str {
    match kind {
        WorkKind::Talking => "talking",
        WorkKind::Coordinating => "coordinating",
        WorkKind::Routine => "routine",
        WorkKind::Documents => "documents",
        WorkKind::Code => "code",
    }
}

pub(crate) fn parse_kind(raw: &str) -> Option<WorkKind> {
    KINDS.into_iter().find(|kind| kind_text(*kind) == raw)
}

pub(crate) fn level_text(level: ModelLevel) -> &'static str {
    match level {
        ModelLevel::Automatic => "automatic",
        ModelLevel::Smaller => "smaller",
        ModelLevel::Standard => "standard",
        ModelLevel::MostCapable => "most_capable",
    }
}

pub(crate) fn parse_level(raw: &str) -> Option<ModelLevel> {
    ALL_LEVELS
        .into_iter()
        .find(|level| level_text(*level) == raw)
}

/// C.1: the level table of ADR-062 for Save, Balanced and Best. `Custom` has no table of its own and
/// reads the Balanced one.
pub(crate) fn table_level(choice: UsageChoice, kind: WorkKind) -> ModelLevel {
    use ModelLevel::{MostCapable, Smaller, Standard};
    let (save, balanced, best) = match kind {
        WorkKind::Talking => (Smaller, Standard, MostCapable),
        WorkKind::Coordinating => (Standard, Standard, MostCapable),
        WorkKind::Routine => (Smaller, Smaller, Standard),
        WorkKind::Documents | WorkKind::Code => (Standard, Standard, MostCapable),
    };
    match choice {
        UsageChoice::SaveMyPlan => save,
        UsageChoice::Balanced | UsageChoice::Custom => balanced,
        UsageChoice::BestResults => best,
    }
}

/// A.4: the thinking level of each named choice; `Custom` is Automatic.
pub(crate) fn table_thinking(choice: UsageChoice) -> ThinkingLevel {
    match choice {
        UsageChoice::SaveMyPlan => ThinkingLevel::Low,
        UsageChoice::Balanced | UsageChoice::Custom => ThinkingLevel::Automatic,
        UsageChoice::BestResults => ThinkingLevel::High,
    }
}

/// The map of one named choice, by 2C key and value.
pub(crate) fn table_map(choice: UsageChoice) -> BTreeMap<String, String> {
    KINDS
        .into_iter()
        .map(|kind| {
            (
                kind_text(kind).to_owned(),
                level_text(table_level(choice, kind)).to_owned(),
            )
        })
        .collect()
}

/// C.3: `level` when allowed; else the highest allowed level below it; else the lowest allowed above.
/// `None` for an empty list.
pub(crate) fn clamp_level(allowed: &[ModelLevel], level: ModelLevel) -> Option<ModelLevel> {
    if allowed.contains(&level) {
        return Some(level);
    }
    allowed
        .iter()
        .copied()
        .filter(|candidate| *candidate < level)
        .max()
        .or_else(|| allowed.iter().copied().filter(|c| *c > level).min())
}

/// The profile setting that governs a field of the person's settings (B.1).
///
/// SPEC-GAP-1: the spec does not say which `usage.person_may_change` entry governs `choice`,
/// `thinking` and `model_level_per_kind` (the profile has no key of the same name). They are read
/// as `plan_mode` (the choice and its thinking level) and `levels_allowed` (the per-kind levels).
pub(crate) const FIELD_NAMES: [(&str, UsageSettingName); 12] = [
    ("choice", UsageSettingName::PlanMode),
    ("model_level_per_kind", UsageSettingName::LevelsAllowed),
    ("thinking", UsageSettingName::PlanMode),
    (
        "answer_simple_requests_without_model",
        UsageSettingName::AnswerWithoutModel,
    ),
    ("reuse_answers", UsageSettingName::ReuseAnswers),
    ("give_agents_only_needed_parts", UsageSettingName::PartsOnly),
    ("summarise_long_conversations", UsageSettingName::Summarise),
    (
        "continue_from_summary_after_break",
        UsageSettingName::ContinueFromSummary,
    ),
    ("ask_before_estimated_requests", UsageSettingName::AskBefore),
    ("agents_at_same_time", UsageSettingName::AgentsAtOnce),
    ("show_usage_on_each_run", UsageSettingName::ShowUsage),
    ("daily_pause_requests", UsageSettingName::DailyPause),
];

fn name_of(field: &str) -> UsageSettingName {
    FIELD_NAMES
        .iter()
        .find(|(name, _)| *name == field)
        .map_or(UsageSettingName::PlanMode, |(_, setting)| *setting)
}

/// The built policy: the 2C settings the screens show, plus what `choose` and `update_settings` read.
pub(crate) struct Policy {
    pub settings: UsageSettings,
    pub snapshot: Arc<ProfileSnapshot>,
    /// `None`: the person may change every usage setting.
    may_change: Option<BTreeSet<UsageSettingName>>,
}

/// A changeable setting is the person's (set_by person) even when they stored nothing; a fixed one
/// takes the company value (A.2).
fn pick<T>(changeable: bool, stored: Option<T>, company: T) -> Setting<T> {
    if changeable {
        Setting {
            value: stored.unwrap_or(company),
            set_by: SetBy::Person,
        }
    } else {
        Setting {
            value: company,
            set_by: SetBy::Company,
        }
    }
}

impl Policy {
    /// A.1 to A.4.
    pub(crate) fn build(
        snapshot: Arc<ProfileSnapshot>,
        stored: &UsageSettingsChange,
        updated_at: Timestamp,
    ) -> Policy {
        let usage = &snapshot.document.usage;
        let defaults = &usage.defaults;
        let may_change: Option<BTreeSet<UsageSettingName>> = usage
            .person_may_change
            .as_ref()
            .map(|names| names.iter().copied().collect());
        let permits =
            |name: UsageSettingName| may_change.as_ref().is_none_or(|set| set.contains(&name));
        let company_choice = match usage.plan_mode {
            PlanMode::Save => UsageChoice::SaveMyPlan,
            PlanMode::Balanced => UsageChoice::Balanced,
            PlanMode::Best => UsageChoice::BestResults,
        };
        let choice = pick(
            permits(UsageSettingName::PlanMode),
            stored.choice,
            company_choice,
        );
        let named = choice.value;

        // Thinking (A.4): the stored value applies when the person may change it.
        let thinking = pick(
            permits(UsageSettingName::PlanMode),
            stored.thinking,
            table_thinking(named),
        );

        // Per-kind levels: Custom reads the person's map; a named choice reads its table (C.1).
        let levels_changeable = permits(UsageSettingName::LevelsAllowed);
        let mut map = table_map(named);
        if named == UsageChoice::Custom && levels_changeable {
            if let Some(own) = &stored.model_level_per_kind {
                for (kind, level) in own {
                    if parse_kind(kind).is_some() && parse_level(level).is_some() {
                        map.insert(kind.clone(), level.clone());
                    }
                }
            }
        }
        // A.3: every level within `levels_allowed` for its kind.
        for kind in KINDS {
            let allowed = allowed_levels(&snapshot, kind);
            let key = kind_text(kind);
            let level = map.get(key).and_then(|text| parse_level(text));
            if let (Some(level), Some(clamped)) =
                (level, level.and_then(|level| clamp_level(&allowed, level)))
            {
                if clamped != level {
                    map.insert(key.to_owned(), level_text(clamped).to_owned());
                }
            }
        }
        let model_level_per_kind = Setting {
            value: map,
            set_by: if levels_changeable {
                SetBy::Person
            } else {
                SetBy::Company
            },
        };

        let agents_cap = usage.agents_at_once_max.clamp(1, AGENTS_MAX);
        let mut agents = pick(
            permits(UsageSettingName::AgentsAtOnce),
            stored.agents_at_same_time,
            usage.agents_at_once,
        );
        agents.value = agents.value.clamp(1, agents_cap);

        let company_pause = match defaults.daily_pause {
            DailyPause::NoPause => None,
            DailyPause::After100 => Some(100),
            DailyPause::After250 => Some(250),
            DailyPause::After500 => Some(500),
        };

        let settings = UsageSettings {
            choice,
            model_level_per_kind,
            thinking,
            answer_simple_requests_without_model: pick(
                permits(UsageSettingName::AnswerWithoutModel),
                stored.answer_simple_requests_without_model,
                defaults.answer_without_model,
            ),
            reuse_answers: pick(
                permits(UsageSettingName::ReuseAnswers),
                stored.reuse_answers,
                defaults.reuse_answers,
            ),
            give_agents_only_needed_parts: pick(
                permits(UsageSettingName::PartsOnly),
                stored.give_agents_only_needed_parts,
                defaults.parts_only,
            ),
            summarise_long_conversations: pick(
                permits(UsageSettingName::Summarise),
                stored.summarise_long_conversations,
                defaults.summarise,
            ),
            continue_from_summary_after_break: pick(
                permits(UsageSettingName::ContinueFromSummary),
                stored.continue_from_summary_after_break,
                defaults.continue_from_summary,
            ),
            ask_before_estimated_requests: pick(
                permits(UsageSettingName::AskBefore),
                stored.ask_before_estimated_requests,
                defaults.ask_before,
            ),
            agents_at_same_time: agents,
            // SPEC-GAP-2: the profile default is "true in workspaces with code" when unset, and the
            // planner has no workspace; an unset default is read as true.
            show_usage_on_each_run: pick(
                permits(UsageSettingName::ShowUsage),
                stored.show_usage_on_each_run,
                defaults.show_usage.unwrap_or(true),
            ),
            daily_pause_requests: pick(
                permits(UsageSettingName::DailyPause),
                stored.daily_pause_requests,
                company_pause,
            ),
            updated_at,
        };
        Policy {
            settings,
            snapshot,
            may_change,
        }
    }

    /// B.1: may the person change the setting a field of `UsageSettingsChange` stands for?
    pub(crate) fn person_may_change(&self, field: &str) -> bool {
        let name = name_of(field);
        self.may_change
            .as_ref()
            .is_none_or(|set| set.contains(&name))
    }

    /// The level the person's settings give a kind of work, before C.2 and C.3.
    pub(crate) fn level_for(&self, kind: WorkKind) -> ModelLevel {
        self.settings
            .model_level_per_kind
            .value
            .get(kind_text(kind))
            .and_then(|text| parse_level(text))
            .unwrap_or_else(|| table_level(UsageChoice::Balanced, kind))
    }

    /// `usage.levels_allowed.<kind>`; all four levels when the profile names none.
    pub(crate) fn allowed(&self, kind: WorkKind) -> Vec<ModelLevel> {
        allowed_levels(&self.snapshot, kind)
    }
}

fn allowed_levels(snapshot: &ProfileSnapshot, kind: WorkKind) -> Vec<ModelLevel> {
    snapshot
        .document
        .usage
        .levels_allowed
        .get(&kind)
        .cloned()
        .unwrap_or_else(|| ALL_LEVELS.to_vec())
}
