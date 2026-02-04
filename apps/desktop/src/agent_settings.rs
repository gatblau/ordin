//! Settings › Agents and models (OrdinDesktop.md DSK-AGM-1; amended January 2026; user decision;
//! Approved by the user January 2026): the person's own harness, model per level and effort per kind
//! of work, kept in `preferences.json` beside the memory settings.
//!
//! Precedence: the person's choice, then the organisation profile, then the built-in default. The
//! models and efforts reach every run through the Usage Planner (`set_personal_choices`), from the
//! next step; the harness is read when the program starts, because the Session Supervisor, the Ordin
//! Host and the Package Catalogue are built on it.

use std::collections::BTreeMap;

use ordin_claude_adapter::ClaudeEffort;
use ordin_organisation_profile::{HarnessName, ProfileSnapshot, WorkKind};
use ordin_service_api::commands::profile as wire;
use ordin_service_api::commands::{
    AgentHarnessView, AgentSettingsView, ChangeAgentSetting, HarnessReadiness, KindEffortView,
    LevelModelView,
};
use ordin_types::domain::{HarnessAttachment, HarnessAttachmentState, HarnessKind};
use ordin_types::wire::run::ModelLevel;
use ordin_usage_planner::PersonalModelChoices;
use serde::{Deserialize, Serialize};

use crate::prefs::PreferenceError;

/// The harnesses, in the order the section lists them.
pub const HARNESSES: [HarnessName; 2] = [HarnessName::ClaudeCode, HarnessName::Copilot];
/// The three levels a person names a model for; `Automatic` has none.
pub const LEVELS: [ModelLevel; 3] = [
    ModelLevel::Smaller,
    ModelLevel::Standard,
    ModelLevel::MostCapable,
];
/// The kinds of work the Usage Planner chooses for, in the profile's order.
pub const KINDS: [WorkKind; 5] = [
    WorkKind::Talking,
    WorkKind::Coordinating,
    WorkKind::Routine,
    WorkKind::Documents,
    WorkKind::Code,
];

/// The person's own choices; every absent entry is "use the default".
// spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentPreferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<HarnessName>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<HarnessName, BTreeMap<ModelLevel, String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub effort: BTreeMap<HarnessName, BTreeMap<WorkKind, String>>,
}

impl AgentPreferences {
    /// What the Usage Planner is given.
    pub fn choices(&self) -> PersonalModelChoices {
        PersonalModelChoices {
            models: self.models.clone(),
            effort: self.effort.clone(),
        }
    }

    /// One change, checked: an effort the harness does not offer is `DSK_PREFERENCE_VALUE_INVALID`.
    pub fn apply(&mut self, change: &ChangeAgentSetting) -> Result<(), PreferenceError> {
        match change {
            ChangeAgentSetting::Harness { harness } => {
                self.harness = harness.map(harness_name);
            }
            ChangeAgentSetting::Model {
                harness,
                level,
                model,
            } => {
                if !LEVELS.contains(level) {
                    return Err(PreferenceError::ValueInvalid);
                }
                let harness = harness_name(*harness);
                match model {
                    Some(name) if ordin_service_api::commands::is_model_name(name) => {
                        self.models
                            .entry(harness)
                            .or_default()
                            .insert(*level, name.clone());
                    }
                    Some(_) => return Err(PreferenceError::ValueInvalid),
                    None => {
                        if let Some(by_level) = self.models.get_mut(&harness) {
                            by_level.remove(level);
                            if by_level.is_empty() {
                                self.models.remove(&harness);
                            }
                        }
                    }
                }
            }
            ChangeAgentSetting::Effort {
                harness,
                kind,
                effort,
            } => {
                let harness = harness_name(*harness);
                let kind = work_kind(*kind);
                match effort {
                    Some(value) if effort_levels(harness).contains(&value.as_str()) => {
                        self.effort
                            .entry(harness)
                            .or_default()
                            .insert(kind, value.clone());
                    }
                    Some(_) => return Err(PreferenceError::ValueInvalid),
                    None => {
                        if let Some(by_kind) = self.effort.get_mut(&harness) {
                            by_kind.remove(&kind);
                            if by_kind.is_empty() {
                                self.effort.remove(&harness);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// The effort values a harness offers, lowest first; empty when it has no effort control. Claude
/// Code's are its `--effort` values; the Copilot command line has none in this version.
pub fn effort_levels(harness: HarnessName) -> Vec<&'static str> {
    match harness {
        HarnessName::ClaudeCode => ClaudeEffort::ALL.iter().map(|e| e.flag_value()).collect(),
        HarnessName::Copilot => Vec::new(),
    }
}

/// True when this version can run agents with the harness: Claude Code in every build, Copilot
/// exactly when the program is built with the `copilot` switch.
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-1
pub fn in_this_version(harness: HarnessName) -> bool {
    match harness {
        HarnessName::ClaudeCode => true,
        HarnessName::Copilot => cfg!(feature = "copilot"),
    }
}

/// The harness runs use: the person's, when this version can run it; else the organisation
/// profile's first harness.
// spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
pub fn harness_to_use(prefs: &AgentPreferences, snapshot: &ProfileSnapshot) -> HarnessName {
    match prefs.harness {
        Some(chosen) if in_this_version(chosen) => chosen,
        _ => snapshot.default_harness(),
    }
}

fn harness_kind(harness: HarnessName) -> HarnessKind {
    match harness {
        HarnessName::ClaudeCode => HarnessKind::ClaudeCode,
        HarnessName::Copilot => HarnessKind::CopilotCli,
    }
}

/// The Setup Assistant's `HarnessChoice` over `preferences.json` `agents.harness` (DSK-CHO-2):
/// `chosen` reads the stored harness, `choose` writes it as `app.change_agent_setting`
/// `harness { harness }` does.
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-2
pub struct PreferencesHarnessChoice {
    pub preferences: std::sync::Arc<crate::prefs::Preferences>,
}

#[async_trait::async_trait]
impl ordin_setup::HarnessChoice for PreferencesHarnessChoice {
    /// An absent entry, or an unreadable file (read as the defaults), gives `None`.
    fn chosen(&self) -> Option<HarnessKind> {
        self.preferences.read().agents.harness.map(harness_kind)
    }

    async fn choose(&self, harness: HarnessKind) -> Result<(), ordin_setup::ChoiceError> {
        let harness = match harness {
            HarnessKind::ClaudeCode => wire::HarnessName::ClaudeCode,
            HarnessKind::CopilotCli => wire::HarnessName::Copilot,
        };
        self.preferences
            .change_agents(&ChangeAgentSetting::Harness {
                harness: Some(harness),
            })
            .map(|_| ())
            .map_err(|_| ordin_setup::ChoiceError)
    }
}

/// From the Setup Assistant's attachments.
pub fn readiness(harness: HarnessName, attachments: &[HarnessAttachment]) -> HarnessReadiness {
    if !in_this_version(harness) {
        return HarnessReadiness::NotInThisVersion;
    }
    let kind = harness_kind(harness);
    let found: Vec<&HarnessAttachment> = attachments.iter().filter(|a| a.harness == kind).collect();
    if found.is_empty() {
        HarnessReadiness::NotFound
    } else if found
        .iter()
        .any(|a| a.state == HarnessAttachmentState::Ready)
    {
        HarnessReadiness::Ready
    } else {
        HarnessReadiness::NeedsSetUp
    }
}

/// The section's view: the person's choices, and each level's default from the profile (its
/// `usage.level_models`, else the built-in haiku, sonnet and opus for Claude Code).
pub fn view(
    prefs: &AgentPreferences,
    snapshot: &ProfileSnapshot,
    harness_in_use: HarnessName,
    attachments: &[HarnessAttachment],
) -> AgentSettingsView {
    let document = &snapshot.document;
    let harnesses = HARNESSES
        .into_iter()
        .map(|harness| {
            let models = LEVELS
                .into_iter()
                .map(|level| LevelModelView {
                    level,
                    chosen: prefs
                        .models
                        .get(&harness)
                        .and_then(|by_level| by_level.get(&level))
                        .cloned(),
                    default_model: document.level_model(harness, level),
                })
                .collect();
            let levels = effort_levels(harness);
            let effort = if levels.is_empty() {
                Vec::new()
            } else {
                KINDS
                    .into_iter()
                    .map(|kind| KindEffortView {
                        kind: wire_kind(kind),
                        chosen: prefs
                            .effort
                            .get(&harness)
                            .and_then(|by_kind| by_kind.get(&kind))
                            .cloned(),
                    })
                    .collect()
            };
            AgentHarnessView {
                harness: wire_harness(harness),
                readiness: readiness(harness, attachments),
                models,
                effort_levels: levels.into_iter().map(str::to_owned).collect(),
                effort,
            }
        })
        .collect();
    AgentSettingsView {
        chosen_harness: prefs.harness.map(wire_harness),
        organisation_harness: wire_harness(snapshot.default_harness()),
        harness_in_use: wire_harness(harness_in_use),
        harnesses,
    }
}

fn harness_name(harness: wire::HarnessName) -> HarnessName {
    match harness {
        wire::HarnessName::ClaudeCode => HarnessName::ClaudeCode,
        wire::HarnessName::Copilot => HarnessName::Copilot,
    }
}

fn wire_harness(harness: HarnessName) -> wire::HarnessName {
    match harness {
        HarnessName::ClaudeCode => wire::HarnessName::ClaudeCode,
        HarnessName::Copilot => wire::HarnessName::Copilot,
    }
}

fn work_kind(kind: wire::WorkKind) -> WorkKind {
    match kind {
        wire::WorkKind::Talking => WorkKind::Talking,
        wire::WorkKind::Coordinating => WorkKind::Coordinating,
        wire::WorkKind::Routine => WorkKind::Routine,
        wire::WorkKind::Documents => WorkKind::Documents,
        wire::WorkKind::Code => WorkKind::Code,
    }
}

fn wire_kind(kind: WorkKind) -> wire::WorkKind {
    match kind {
        WorkKind::Talking => wire::WorkKind::Talking,
        WorkKind::Coordinating => wire::WorkKind::Coordinating,
        WorkKind::Routine => wire::WorkKind::Routine,
        WorkKind::Documents => wire::WorkKind::Documents,
        WorkKind::Code => wire::WorkKind::Code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::time::Timestamp;
    use ordin_types::HarnessAttachmentId;

    fn model(
        harness: wire::HarnessName,
        level: ModelLevel,
        model: Option<&str>,
    ) -> ChangeAgentSetting {
        ChangeAgentSetting::Model {
            harness,
            level,
            model: model.map(str::to_owned),
        }
    }

    fn effort(kind: wire::WorkKind, effort: Option<&str>) -> ChangeAgentSetting {
        ChangeAgentSetting::Effort {
            harness: wire::HarnessName::ClaudeCode,
            kind,
            effort: effort.map(str::to_owned),
        }
    }

    fn attachment(harness: HarnessKind, state: HarnessAttachmentState) -> HarnessAttachment {
        HarnessAttachment {
            id: HarnessAttachmentId::new(),
            harness,
            location: "/usr/local/bin/claude".to_owned(),
            version: None,
            tested: true,
            signed_in: state == HarnessAttachmentState::Ready,
            sign_in_method: None,
            state,
            last_checked_at: Timestamp::now(),
        }
    }

    #[test]
    fn a_model_is_set_and_use_default_clears_it() {
        let mut p = AgentPreferences::default();
        p.apply(&model(
            wire::HarnessName::ClaudeCode,
            ModelLevel::Standard,
            Some("claude-sonnet-4.5"),
        ))
        .unwrap();
        assert_eq!(
            p.choices().models[&HarnessName::ClaudeCode][&ModelLevel::Standard],
            "claude-sonnet-4.5"
        );
        p.apply(&model(
            wire::HarnessName::ClaudeCode,
            ModelLevel::Standard,
            None,
        ))
        .unwrap();
        assert_eq!(p, AgentPreferences::default(), "nothing left behind");
    }

    #[test]
    fn a_bad_model_name_or_level_is_refused() {
        let mut p = AgentPreferences::default();
        for bad in [
            model(
                wire::HarnessName::ClaudeCode,
                ModelLevel::Standard,
                Some("two words"),
            ),
            model(
                wire::HarnessName::ClaudeCode,
                ModelLevel::Automatic,
                Some("opus"),
            ),
        ] {
            assert_eq!(p.apply(&bad), Err(PreferenceError::ValueInvalid));
        }
        assert_eq!(p, AgentPreferences::default());
    }

    #[test]
    fn an_effort_must_be_one_the_harness_offers() {
        let mut p = AgentPreferences::default();
        p.apply(&effort(wire::WorkKind::Code, Some("max"))).unwrap();
        assert_eq!(
            p.choices().effort[&HarnessName::ClaudeCode][&WorkKind::Code],
            "max"
        );
        assert_eq!(
            p.apply(&effort(wire::WorkKind::Code, Some("extreme"))),
            Err(PreferenceError::ValueInvalid)
        );
        // Copilot has no effort control.
        assert_eq!(
            p.apply(&ChangeAgentSetting::Effort {
                harness: wire::HarnessName::Copilot,
                kind: wire::WorkKind::Code,
                effort: Some("high".to_owned()),
            }),
            Err(PreferenceError::ValueInvalid)
        );
        p.apply(&effort(wire::WorkKind::Code, None)).unwrap();
        assert_eq!(p, AgentPreferences::default());
    }

    #[test]
    fn the_harness_is_the_persons_when_it_can_run_else_the_organisations() {
        let snapshot = ProfileSnapshot::defaults();
        let mut p = AgentPreferences::default();
        assert_eq!(harness_to_use(&p, &snapshot), snapshot.default_harness());
        p.apply(&ChangeAgentSetting::Harness {
            harness: Some(wire::HarnessName::ClaudeCode),
        })
        .unwrap();
        assert_eq!(harness_to_use(&p, &snapshot), HarnessName::ClaudeCode);
        // With `copilot` the choice is the harness in use; without it the choice is kept and the
        // organisation's harness is used (DSK-CHO-1).
        p.apply(&ChangeAgentSetting::Harness {
            harness: Some(wire::HarnessName::Copilot),
        })
        .unwrap();
        assert_eq!(p.harness, Some(HarnessName::Copilot), "the choice is kept");
        let expected = if cfg!(feature = "copilot") {
            HarnessName::Copilot
        } else {
            snapshot.default_harness()
        };
        assert_eq!(harness_to_use(&p, &snapshot), expected);
    }

    #[test]
    fn readiness_follows_the_setup_assistants_attachments() {
        assert_eq!(
            readiness(HarnessName::ClaudeCode, &[]),
            HarnessReadiness::NotFound
        );
        assert_eq!(
            readiness(
                HarnessName::ClaudeCode,
                &[attachment(
                    HarnessKind::ClaudeCode,
                    HarnessAttachmentState::NeedsSignIn
                )]
            ),
            HarnessReadiness::NeedsSetUp
        );
        assert_eq!(
            readiness(
                HarnessName::ClaudeCode,
                &[attachment(
                    HarnessKind::ClaudeCode,
                    HarnessAttachmentState::Ready
                )]
            ),
            HarnessReadiness::Ready
        );
        assert_eq!(
            readiness(
                HarnessName::Copilot,
                &[attachment(
                    HarnessKind::CopilotCli,
                    HarnessAttachmentState::Ready
                )]
            ),
            if cfg!(feature = "copilot") {
                HarnessReadiness::Ready
            } else {
                HarnessReadiness::NotInThisVersion
            }
        );
    }

    #[test]
    fn the_view_shows_each_levels_default_from_the_profile_then_the_built_in_one() {
        let mut snapshot = ProfileSnapshot::defaults();
        let mut by_level = BTreeMap::new();
        by_level.insert(ModelLevel::Smaller, "claude-haiku-4.5".to_owned());
        snapshot
            .document
            .usage
            .level_models
            .insert(HarnessName::ClaudeCode, by_level);
        let mut p = AgentPreferences::default();
        p.apply(&model(
            wire::HarnessName::ClaudeCode,
            ModelLevel::MostCapable,
            Some("claude-opus-4.5"),
        ))
        .unwrap();
        let v = view(&p, &snapshot, HarnessName::ClaudeCode, &[]);
        let claude = &v.harnesses[0];
        assert_eq!(claude.harness, wire::HarnessName::ClaudeCode);
        let defaults: Vec<Option<&str>> = claude
            .models
            .iter()
            .map(|m| m.default_model.as_deref())
            .collect();
        assert_eq!(
            defaults,
            [Some("claude-haiku-4.5"), Some("sonnet"), Some("opus")]
        );
        assert_eq!(claude.models[2].chosen.as_deref(), Some("claude-opus-4.5"));
        assert_eq!(
            claude.effort_levels,
            ["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(claude.effort.len(), 5);
        let copilot = &v.harnesses[1];
        assert!(copilot.effort_levels.is_empty() && copilot.effort.is_empty());
        let expected = if cfg!(feature = "copilot") {
            HarnessReadiness::NotFound
        } else {
            HarnessReadiness::NotInThisVersion
        };
        assert_eq!(copilot.readiness, expected);
        assert_eq!(v.chosen_harness, None);
    }

    #[test]
    fn the_stored_form_reads_back() {
        let mut p = AgentPreferences::default();
        p.apply(&ChangeAgentSetting::Harness {
            harness: Some(wire::HarnessName::Copilot),
        })
        .unwrap();
        p.apply(&model(
            wire::HarnessName::Copilot,
            ModelLevel::Smaller,
            Some("gpt-5-mini"),
        ))
        .unwrap();
        p.apply(&effort(wire::WorkKind::Documents, Some("low")))
            .unwrap();
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "harness": "copilot",
                "models": { "copilot": { "smaller": "gpt-5-mini" } },
                "effort": { "claude_code": { "documents": "low" } }
            })
        );
        let back: AgentPreferences = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }
}
