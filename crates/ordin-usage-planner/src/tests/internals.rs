//! Behaviour the Gherkin does not spell out: the level tables, the policy rules, estimates, reuse,
//! counting, pruning and the log events.

use std::collections::BTreeMap;

use ordin_journal::{PruneRequest, MIGRATIONS};
use ordin_organisation_profile::{
    DailyPause, HarnessName, ProfileSnapshot, UsageSettingName, WorkKind,
};
use ordin_runtime_adapter::UsageDelta;
use ordin_types::common::SetBy;
use ordin_types::domain::identity::{AskBefore, SummariseWhen, UsageChoice};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::{AdapterKind, ModelLevel, ThinkingLevel};
use ordin_types::{CrewRunId, ReusedAnswerId, RunId, WorkspaceId};

use super::support::*;
use crate::planner::request_digest;
use crate::types::{
    EstimateBand, EstimateBasis, EstimateRequest, RunUsage, UsageSettingsChange, WorkShape,
};
use crate::UsagePlanner;

fn change() -> UsageSettingsChange {
    UsageSettingsChange::default()
}

fn choose_level(f: &Fixture, kind: WorkKind) -> ModelLevel {
    f.planner.choose(kind, AdapterKind::ClaudeLocal).level
}

fn usage(turns: u32, level: ModelLevel) -> RunUsage {
    RunUsage {
        run_id: RunId::new(),
        crew_run_id: None,
        round: None,
        level,
        delta: UsageDelta {
            model_turns: turns,
            ..UsageDelta::default()
        },
    }
}

async fn estimate_of(f: &Fixture, key: &str, shape: WorkShape) -> crate::UsageEstimate {
    f.planner
        .estimate(EstimateRequest {
            work_key: key.to_owned(),
            shape,
        })
        .await
        .unwrap()
}

// ---- levels

#[tokio::test]
async fn the_level_table_of_each_named_choice() {
    use ModelLevel::{MostCapable, Smaller, Standard};
    let kinds = [
        WorkKind::Talking,
        WorkKind::Coordinating,
        WorkKind::Routine,
        WorkKind::Documents,
        WorkKind::Code,
    ];
    let cases = [
        (
            UsageChoice::SaveMyPlan,
            [Smaller, Standard, Smaller, Standard, Standard],
            ThinkingLevel::Low,
        ),
        (
            UsageChoice::Balanced,
            [Standard, Standard, Smaller, Standard, Standard],
            ThinkingLevel::Automatic,
        ),
        (
            UsageChoice::BestResults,
            [MostCapable, MostCapable, Standard, MostCapable, MostCapable],
            ThinkingLevel::High,
        ),
    ];
    for (choice, expected, thinking) in cases {
        let f = fixture().await;
        f.planner
            .update_settings(UsageSettingsChange {
                choice: Some(choice),
                ..change()
            })
            .await
            .unwrap();
        for (kind, level) in kinds.into_iter().zip(expected) {
            let chosen = f.planner.choose(kind, AdapterKind::ClaudeLocal);
            assert_eq!(chosen.level, level, "{choice:?} {kind:?}");
            assert_eq!(chosen.thinking, thinking);
        }
    }
}

#[tokio::test]
async fn the_company_plan_mode_is_the_default_choice() {
    use ordin_organisation_profile::PlanMode;
    let f = fixture_with(
        profile(|usage| usage.plan_mode = PlanMode::Best),
        vec![FakeHarness::claude()],
    )
    .await;
    assert_eq!(f.planner.settings().choice.value, UsageChoice::BestResults);
    assert_eq!(choose_level(&f, WorkKind::Talking), ModelLevel::MostCapable);
    assert_eq!(f.planner.settings().thinking.value, ThinkingLevel::High);
}

#[tokio::test]
async fn a_stored_thinking_value_applies_until_a_named_choice_removes_it() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            thinking: Some(ThinkingLevel::High),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(
        f.planner
            .choose(WorkKind::Code, AdapterKind::ClaudeLocal)
            .thinking,
        ThinkingLevel::High
    );
    f.planner
        .update_settings(UsageSettingsChange {
            choice: Some(UsageChoice::SaveMyPlan),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(f.planner.settings().thinking.value, ThinkingLevel::Low);
}

#[tokio::test]
async fn a_named_choice_resets_the_levels_and_custom_keeps_them() {
    let f = fixture().await;
    let mut levels = BTreeMap::new();
    levels.insert("routine".to_owned(), "most_capable".to_owned());
    f.planner
        .update_settings(UsageSettingsChange {
            model_level_per_kind: Some(levels),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(choose_level(&f, WorkKind::Routine), ModelLevel::MostCapable);
    // The unchanged kinds keep the Balanced table the custom choice started from.
    assert_eq!(choose_level(&f, WorkKind::Talking), ModelLevel::Standard);
    f.planner
        .update_settings(UsageSettingsChange {
            choice: Some(UsageChoice::Balanced),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(choose_level(&f, WorkKind::Routine), ModelLevel::Smaller);
    // Choosing Custom with no levels starts from the table in force.
    f.planner
        .update_settings(UsageSettingsChange {
            choice: Some(UsageChoice::BestResults),
            ..change()
        })
        .await
        .unwrap();
    let settings = f
        .planner
        .update_settings(UsageSettingsChange {
            choice: Some(UsageChoice::Custom),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(settings.choice.value, UsageChoice::Custom);
    assert_eq!(choose_level(&f, WorkKind::Talking), ModelLevel::MostCapable);
}

#[tokio::test]
async fn a_level_outside_the_allowed_list_moves_to_the_nearest_allowed() {
    // Below: the highest allowed level below; above: the lowest allowed above.
    let snapshot = profile(|usage| {
        usage
            .levels_allowed
            .insert(WorkKind::Documents, vec![ModelLevel::Smaller]);
        usage
            .levels_allowed
            .insert(WorkKind::Talking, vec![ModelLevel::MostCapable]);
    });
    let f = fixture_with(snapshot, vec![FakeHarness::claude()]).await;
    assert_eq!(choose_level(&f, WorkKind::Documents), ModelLevel::Smaller);
    assert_eq!(choose_level(&f, WorkKind::Talking), ModelLevel::MostCapable);
    // The settings the screens show are clamped too (A.3).
    assert_eq!(
        f.planner.settings().model_level_per_kind.value["documents"],
        "smaller"
    );
}

#[tokio::test]
async fn an_empty_allowed_list_gives_standard() {
    let snapshot = profile(|usage| {
        usage.levels_allowed.insert(WorkKind::Code, vec![]);
    });
    let f = fixture_with(snapshot, vec![FakeHarness::claude()]).await;
    assert_eq!(choose_level(&f, WorkKind::Code), ModelLevel::Standard);
}

#[tokio::test]
async fn automatic_is_standard_on_claude_and_the_harnesses_own_choice_on_copilot() {
    let f = fixture_with(
        ProfileSnapshot::defaults(),
        vec![FakeHarness::claude(), FakeHarness::copilot()],
    )
    .await;
    let mut levels = BTreeMap::new();
    levels.insert("talking".to_owned(), "automatic".to_owned());
    f.planner
        .update_settings(UsageSettingsChange {
            model_level_per_kind: Some(levels),
            ..change()
        })
        .await
        .unwrap();
    let claude = f
        .planner
        .choose(WorkKind::Talking, AdapterKind::ClaudeLocal);
    assert_eq!(
        (claude.level, claude.model.as_deref()),
        (ModelLevel::Standard, Some("sonnet"))
    );
    let copilot = f
        .planner
        .choose(WorkKind::Talking, AdapterKind::CopilotLocal);
    assert_eq!(
        (copilot.level, copilot.model),
        (ModelLevel::Automatic, None)
    );
    // A Copilot level with a default model takes it.
    let documents = f
        .planner
        .choose(WorkKind::Documents, AdapterKind::CopilotLocal);
    assert_eq!(documents.model.as_deref(), Some("gpt-standard"));
}

#[tokio::test]
async fn remote_harnesses_choose_the_remote_sides_model() {
    let f = fixture().await;
    for adapter in [AdapterKind::HostedAgent, AdapterKind::SlackThread] {
        let chosen = f.planner.choose(WorkKind::Documents, adapter);
        assert_eq!(chosen.model, None);
        assert_eq!(chosen.level, ModelLevel::Standard);
    }
}

#[tokio::test]
async fn an_accepted_profile_model_is_used() {
    let snapshot = profile(|usage| {
        let mut by_level = BTreeMap::new();
        by_level.insert(ModelLevel::Standard, "opus".to_owned());
        usage.level_models.insert(HarnessName::ClaudeCode, by_level);
    });
    let f = fixture_with(snapshot, vec![FakeHarness::claude()]).await;
    let chosen = f
        .planner
        .choose(WorkKind::Documents, AdapterKind::ClaudeLocal);
    assert_eq!(chosen.model.as_deref(), Some("opus"));
}

#[tokio::test]
async fn a_harness_for_another_cli_gives_no_model() {
    // Only Copilot is bound, so Claude Code has no HarnessModels.
    let f = fixture_with(ProfileSnapshot::defaults(), vec![FakeHarness::copilot()]).await;
    assert_eq!(
        f.planner
            .choose(WorkKind::Documents, AdapterKind::ClaudeLocal)
            .model,
        None
    );
}

#[tokio::test]
async fn step_up_from_automatic_and_the_limits() {
    let f = fixture_with(
        ProfileSnapshot::defaults(),
        vec![FakeHarness::claude(), FakeHarness::copilot()],
    )
    .await;
    let automatic = crate::LevelChoice {
        level: ModelLevel::Automatic,
        model: None,
        thinking: ThinkingLevel::High,
        effort: None,
        stepped_up: false,
    };
    let up = f
        .planner
        .step_up(&automatic, WorkKind::Documents, AdapterKind::CopilotLocal)
        .unwrap();
    assert_eq!(up.level, ModelLevel::Standard);
    assert_eq!(up.thinking, ThinkingLevel::High, "thinking is kept");
    let top = crate::LevelChoice {
        level: ModelLevel::MostCapable,
        ..automatic.clone()
    };
    assert!(f
        .planner
        .step_up(&top, WorkKind::Documents, AdapterKind::ClaudeLocal)
        .is_none());
    let standard = crate::LevelChoice {
        level: ModelLevel::Standard,
        ..automatic
    };
    assert_eq!(
        f.planner
            .step_up(&standard, WorkKind::Documents, AdapterKind::ClaudeLocal)
            .unwrap()
            .model
            .as_deref(),
        Some("opus")
    );
    // Not allowed for the kind: nothing.
    let narrow = fixture_with(
        profile(|usage| {
            usage.levels_allowed.insert(
                WorkKind::Code,
                vec![ModelLevel::Smaller, ModelLevel::Standard],
            );
        }),
        vec![FakeHarness::claude()],
    )
    .await;
    assert!(narrow
        .planner
        .step_up(&standard, WorkKind::Code, AdapterKind::ClaudeLocal)
        .is_none());
}

#[tokio::test]
async fn a_step_up_is_journalled() {
    let f = fixture().await;
    let first = f
        .planner
        .choose(WorkKind::Routine, AdapterKind::ClaudeLocal);
    f.planner
        .step_up(&first, WorkKind::Routine, AdapterKind::ClaudeLocal)
        .unwrap();
    let mut seen = Vec::new();
    for _ in 0..500 {
        seen = f.entries_of("usage_stepped_up").await;
        if !seen.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].component, "usage_planner");
    assert_eq!(seen[0].data["kind"], "routine");
    assert_eq!(seen[0].data["from"], "smaller");
    assert_eq!(seen[0].data["to"], "standard");
}

#[tokio::test]
async fn a_step_up_outside_a_runtime_still_returns_the_choice() {
    let f = fixture().await;
    let first = f
        .planner
        .choose(WorkKind::Routine, AdapterKind::ClaudeLocal);
    let planner = f.planner.clone();
    let up = std::thread::spawn(move || {
        planner.step_up(&first, WorkKind::Routine, AdapterKind::ClaudeLocal)
    })
    .join()
    .unwrap();
    assert_eq!(up.unwrap().level, ModelLevel::Standard);
}

// ---- policy

#[tokio::test]
async fn the_company_value_and_mark_for_a_setting_the_person_may_not_change() {
    let snapshot = profile(|usage| {
        usage.person_may_change = Some(vec![UsageSettingName::AgentsAtOnce]);
        usage.defaults.reuse_answers = false;
        usage.defaults.ask_before = AskBefore::Never;
        usage.defaults.summarise = SummariseWhen::Early;
        usage.defaults.daily_pause = DailyPause::After250;
        usage.defaults.show_usage = Some(false);
    });
    let f = fixture_with(snapshot, vec![]).await;
    let s = f.planner.settings();
    assert_eq!(
        (s.reuse_answers.value, s.reuse_answers.set_by),
        (false, SetBy::Company)
    );
    assert_eq!(s.ask_before_estimated_requests.value, AskBefore::Never);
    assert_eq!(s.summarise_long_conversations.value, SummariseWhen::Early);
    assert_eq!(s.daily_pause_requests.value, Some(250));
    assert!(!s.show_usage_on_each_run.value);
    assert!(!f.planner.show_usage_on_each_run());
    assert_eq!(s.agents_at_same_time.set_by, SetBy::Person);
    assert_eq!(s.choice.set_by, SetBy::Company);
    assert_eq!(s.model_level_per_kind.set_by, SetBy::Company);
    assert_eq!(s.thinking.set_by, SetBy::Company);
}

#[tokio::test]
async fn every_company_pause_choice_maps_to_its_number() {
    for (pause, expected) in [
        (DailyPause::NoPause, None),
        (DailyPause::After100, Some(100)),
        (DailyPause::After250, Some(250)),
        (DailyPause::After500, Some(500)),
    ] {
        let f = fixture_with(profile(|usage| usage.defaults.daily_pause = pause), vec![]).await;
        assert_eq!(f.planner.settings().daily_pause_requests.value, expected);
    }
}

#[tokio::test]
async fn the_default_settings_are_the_company_defaults() {
    let f = fixture().await;
    let s = f.planner.settings();
    assert_eq!(s.choice.value, UsageChoice::Balanced);
    assert!(f.planner.parts_only());
    assert!(f.planner.answer_without_model());
    assert!(f.planner.show_usage_on_each_run());
    assert_eq!(f.planner.agents_at_once(), 2);
    assert_eq!(
        s.summarise_long_conversations.value,
        SummariseWhen::WhenNearlyFull
    );
    assert_eq!(s.ask_before_estimated_requests.value, AskBefore::Heavy);
    assert_eq!(s.daily_pause_requests.value, None);
    assert_eq!(s.choice.set_by, SetBy::Person);
}

#[tokio::test]
async fn a_profile_change_is_picked_up_and_lowers_a_stored_value() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            agents_at_same_time: Some(3),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(f.planner.agents_at_once(), 3);
    f.source.set(profile(|usage| usage.agents_at_once_max = 1));
    assert_eq!(f.planner.agents_at_once(), 1, "lowered at use (A.3)");
    f.source.set(profile(|usage| {
        usage.person_may_change = Some(vec![]);
        usage.agents_at_once = 2;
    }));
    assert_eq!(
        f.planner.agents_at_once(),
        2,
        "the stored value no longer applies (A.2)"
    );
}

#[tokio::test]
async fn the_persons_settings_survive_a_restart() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            reuse_answers: Some(false),
            agents_at_same_time: Some(1),
            daily_pause_requests: Some(Some(250)),
            ..change()
        })
        .await
        .unwrap();
    let again = f.restart().await;
    let s = again.settings();
    assert!(!s.reuse_answers.value);
    assert_eq!(s.agents_at_same_time.value, 1);
    assert_eq!(s.daily_pause_requests.value, Some(250));
    // The pause can be taken off again: Some(None) is "no pause", not "unchanged".
    again
        .update_settings(UsageSettingsChange {
            daily_pause_requests: Some(None),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(
        f.restart().await.settings().daily_pause_requests.value,
        None
    );
}

#[tokio::test]
async fn every_simple_setting_can_be_changed() {
    let f = fixture().await;
    let s = f
        .planner
        .update_settings(UsageSettingsChange {
            answer_simple_requests_without_model: Some(false),
            give_agents_only_needed_parts: Some(false),
            continue_from_summary_after_break: Some(false),
            ask_before_estimated_requests: Some(AskBefore::Never),
            show_usage_on_each_run: Some(false),
            summarise_long_conversations: Some(SummariseWhen::Early),
            ..change()
        })
        .await
        .unwrap();
    assert!(!s.answer_simple_requests_without_model.value);
    assert!(!f.planner.answer_without_model());
    assert!(!f.planner.parts_only());
    assert!(!s.continue_from_summary_after_break.value);
    assert_eq!(s.ask_before_estimated_requests.value, AskBefore::Never);
    assert!(!f.planner.show_usage_on_each_run());
    assert_eq!(s.summarise_long_conversations.value, SummariseWhen::Early);
    let lines = f.entries_of("usage_settings_changed").await;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].data["fields"].as_array().unwrap().len(), 6);
}

#[test]
fn the_settings_change_reads_null_as_no_pause_and_refuses_unknown_fields() {
    let none: UsageSettingsChange =
        serde_json::from_str(r#"{"daily_pause_requests":null}"#).unwrap();
    assert_eq!(none.daily_pause_requests, Some(None));
    let some: UsageSettingsChange =
        serde_json::from_str(r#"{"daily_pause_requests":100}"#).unwrap();
    assert_eq!(some.daily_pause_requests, Some(Some(100)));
    let absent: UsageSettingsChange = serde_json::from_str("{}").unwrap();
    assert_eq!(absent.daily_pause_requests, None);
    assert!(serde_json::from_str::<UsageSettingsChange>(r#"{"unknown":1}"#).is_err());
    let round = serde_json::to_string(&none).unwrap();
    assert_eq!(
        serde_json::from_str::<UsageSettingsChange>(&round)
            .unwrap()
            .daily_pause_requests,
        Some(None)
    );
}

// ---- estimates

#[tokio::test]
async fn estimates_by_shape_without_history() {
    let f = fixture().await;
    let workflow = estimate_of(
        &f,
        "workflow:A:1",
        WorkShape::Workflow {
            agent_steps: 3,
            ordin_turns: 2,
        },
    )
    .await;
    assert_eq!((workflow.min_turns, workflow.max_turns), (26, 50));
    assert_eq!(workflow.band, EstimateBand::Medium);
    assert_eq!(workflow.typical_day_permille, 200);
    assert_eq!(workflow.based_on, vec![EstimateBasis::Steps { count: 3 }]);
    assert!(!workflow.ask_first);

    let task = estimate_of(
        &f,
        "task:a:b",
        WorkShape::Task {
            documents: 30,
            total_bytes: 9,
        },
    )
    .await;
    assert_eq!((task.min_turns, task.max_turns), (19, 64));
    assert_eq!(
        task.based_on,
        vec![EstimateBasis::Documents {
            count: 30,
            total_bytes: 9
        }]
    );

    let light = estimate_of(
        &f,
        "task:a:c",
        WorkShape::Task {
            documents: 21,
            total_bytes: 0,
        },
    )
    .await;
    assert_eq!(
        light.band,
        EstimateBand::Medium,
        "21 documents are estimated: max 46"
    );

    let crew = estimate_of(
        &f,
        "crew:x",
        WorkShape::Crew {
            members: 1,
            max_rounds: 1,
        },
    )
    .await;
    assert_eq!(
        (crew.min_turns, crew.max_turns, crew.band),
        (8, 9, EstimateBand::Light)
    );
    assert_eq!(
        crew.based_on,
        vec![
            EstimateBasis::Members { count: 1 },
            EstimateBasis::Limits { max_rounds: 1 }
        ]
    );
}

#[tokio::test]
async fn the_crew_maximum_is_capped_and_the_day_share_is_capped() {
    let f = fixture().await;
    let crew = estimate_of(
        &f,
        "crew:big",
        WorkShape::Crew {
            members: 9,
            max_rounds: 30,
        },
    )
    .await;
    assert_eq!(crew.max_turns, 2_160);
    let huge = estimate_of(
        &f,
        "workflow:huge:1",
        WorkShape::Workflow {
            agent_steps: 100_000,
            ordin_turns: 0,
        },
    )
    .await;
    assert_eq!(huge.typical_day_permille, 10_000);
    assert_eq!(huge.band, EstimateBand::Heavy);
}

#[tokio::test]
async fn the_bands_change_at_25_and_100_turns() {
    let f = fixture().await;
    for (turns, band) in [
        (25, EstimateBand::Light),
        (26, EstimateBand::Medium),
        (100, EstimateBand::Medium),
        (101, EstimateBand::Heavy),
    ] {
        let key = format!("workflow:band:{turns}");
        f.planner
            .record_run_finished(&key, RunId::new(), turns)
            .await
            .unwrap();
        let estimate = estimate_of(
            &f,
            &key,
            WorkShape::Workflow {
                agent_steps: 1,
                ordin_turns: 0,
            },
        )
        .await;
        assert_eq!(estimate.band, band, "{turns}");
    }
}

#[tokio::test]
async fn ask_first_follows_the_choice() {
    let f = fixture().await;
    let heavy = WorkShape::Crew {
        members: 3,
        max_rounds: 10,
    };
    for (ask, expected) in [
        (AskBefore::Heavy, true),
        (AskBefore::MediumOrHeavy, true),
        (AskBefore::Never, false),
    ] {
        f.planner
            .update_settings(UsageSettingsChange {
                ask_before_estimated_requests: Some(ask),
                ..change()
            })
            .await
            .unwrap();
        let estimate = estimate_of(&f, "crew:ask", heavy_shape(&heavy)).await;
        assert_eq!(estimate.ask_first, expected, "{ask:?}");
    }
    // Medium work does not ask under Heavy.
    f.planner
        .update_settings(UsageSettingsChange {
            ask_before_estimated_requests: Some(AskBefore::Heavy),
            ..change()
        })
        .await
        .unwrap();
    let medium = estimate_of(
        &f,
        "workflow:ask",
        WorkShape::Workflow {
            agent_steps: 3,
            ordin_turns: 0,
        },
    )
    .await;
    assert_eq!(medium.band, EstimateBand::Medium);
    assert!(!medium.ask_first);
}

fn heavy_shape(shape: &WorkShape) -> WorkShape {
    shape.clone()
}

#[tokio::test]
async fn history_keeps_the_newest_twenty_and_estimates_from_the_last_five() {
    let f = fixture().await;
    for turns in 1..=25u32 {
        f.planner
            .record_run_finished("workflow:long:1", RunId::new(), turns)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let kept: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM usage_work_history WHERE work_key = 'workflow:long:1'",
    )
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(kept, 20);
    let estimate = estimate_of(
        &f,
        "workflow:long:1",
        WorkShape::Workflow {
            agent_steps: 1,
            ordin_turns: 0,
        },
    )
    .await;
    assert_eq!((estimate.min_turns, estimate.max_turns), (21, 25));
    assert_eq!(
        estimate.based_on,
        vec![EstimateBasis::EarlierRuns { count: 5 }]
    );
}

#[tokio::test]
async fn no_pause_means_ordin_may_always_start() {
    let f = fixture().await;
    f.planner
        .record_usage(usage(10_000, ModelLevel::Standard))
        .await
        .unwrap();
    f.planner.may_start_ordin_work().await.unwrap();
}

// ---- reuse

#[tokio::test]
async fn reuse_needs_the_same_request_agent_workspace_and_inputs() {
    let f = fixture().await;
    let (analyst, sales) = (agent(), WorkspaceId::new());
    f.planner
        .index_answer(answer(
            query("Q?", analyst, sales, vec![quote("1")]),
            Timestamp::now(),
            "A",
        ))
        .await
        .unwrap();
    let find = |request: &str, who, ws, inputs| {
        let planner = f.planner.clone();
        let q = query(request, who, ws, inputs);
        async move { planner.find_reusable(q).await.unwrap() }
    };
    assert!(find("q?", analyst, sales, vec![quote("1")]).await.is_some());
    assert!(find("Other?", analyst, sales, vec![quote("1")])
        .await
        .is_none());
    assert!(find("Q?", agent(), sales, vec![quote("1")]).await.is_none());
    assert!(find("Q?", analyst, WorkspaceId::new(), vec![quote("1")])
        .await
        .is_none());
    assert!(
        find("Q?", analyst, sales, vec![]).await.is_none(),
        "a different set of inputs"
    );
    let mut other = quote("1");
    other.location = "workspace:Sales/Other.pdf".to_owned();
    assert!(find("Q?", analyst, sales, vec![other]).await.is_none());
}

#[tokio::test]
async fn the_most_recent_matching_answer_is_returned_first() {
    let f = fixture().await;
    let (analyst, sales) = (agent(), WorkspaceId::new());
    for (age, text) in [(3, "older"), (1, "newer")] {
        f.planner
            .index_answer(answer(
                query("Q?", analyst, sales, vec![]),
                days_ago(age),
                text,
            ))
            .await
            .unwrap();
    }
    let found = f
        .planner
        .find_reusable(query("Q?", analyst, sales, vec![]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.text, "newer");
    assert_eq!(
        (found.expires_at.unix_millis() - found.answered_at.unix_millis()),
        7 * 24 * 3600 * 1000
    );
}

#[tokio::test]
async fn an_answer_older_than_seven_days_is_not_reused_and_is_swept() {
    let f = fixture().await;
    let (analyst, sales) = (agent(), WorkspaceId::new());
    f.planner
        .index_answer(answer(
            query("Q?", analyst, sales, vec![]),
            days_ago(8),
            "stale",
        ))
        .await
        .unwrap();
    assert!(f
        .planner
        .find_reusable(query("Q?", analyst, sales, vec![]))
        .await
        .unwrap()
        .is_none());
    // The daily sweep deletes it once the day changes (G.5).
    *f.planner.swept_on.lock().unwrap() = "2000-01-01".to_owned();
    f.planner
        .find_reusable(query("Q?", analyst, sales, vec![]))
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reused_answers")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn expired_answers_are_deleted_at_start() {
    let f = fixture().await;
    f.planner
        .index_answer(answer(
            query("Q?", agent(), WorkspaceId::new(), vec![]),
            days_ago(9),
            "stale",
        ))
        .await
        .unwrap();
    let _again = f.restart().await;
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reused_answers")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn reuse_switched_off_finds_nothing() {
    let f = fixture().await;
    let (analyst, sales) = (agent(), WorkspaceId::new());
    f.planner
        .index_answer(answer(
            query("Q?", analyst, sales, vec![]),
            Timestamp::now(),
            "A",
        ))
        .await
        .unwrap();
    f.planner
        .update_settings(UsageSettingsChange {
            reuse_answers: Some(false),
            ..change()
        })
        .await
        .unwrap();
    assert!(f
        .planner
        .find_reusable(query("Q?", analyst, sales, vec![]))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn reusing_an_unknown_answer_changes_nothing() {
    let f = fixture().await;
    f.planner
        .record_reuse(ReusedAnswerId::new(), None)
        .await
        .unwrap();
    assert_eq!(f.planner.today().await.unwrap().answered_without_model, 0);
    assert!(f.entries_of("usage_answer_reused").await.is_empty());
}

#[tokio::test]
async fn reuse_stamps_the_time_shown_again_and_links_the_run() {
    let f = fixture().await;
    let (analyst, sales) = (agent(), WorkspaceId::new());
    let source = answer(query("Q?", analyst, sales, vec![]), days_ago(1), "A");
    let source_run = source.source_run_id;
    let id = f.planner.index_answer(source).await.unwrap();
    let run = RunId::new();
    f.planner.record_reuse(id, Some(run)).await.unwrap();
    let again = f
        .planner
        .find_reusable(query("Q?", analyst, sales, vec![]))
        .await
        .unwrap()
        .unwrap();
    assert!(again.shown_again_at > again.answered_at);
    let lines = f.entries_of("usage_answer_reused").await;
    assert_eq!(lines[0].run_id, Some(run));
    assert_eq!(lines[0].data["source_run_id"], source_run.to_string());
}

#[test]
fn the_request_digest_ignores_case_and_spacing() {
    assert_eq!(
        request_digest("  Compare   THE Quotes "),
        request_digest("compare the quotes")
    );
    assert_ne!(request_digest("a"), request_digest("b"));
    assert_eq!(request_digest("a").len(), 64);
}

#[tokio::test]
async fn a_remote_agent_is_a_separate_key() {
    use ordin_types::common::AgentRef;
    use ordin_types::RemoteAgentId;
    let f = fixture().await;
    let remote = AgentRef::RemoteAgent {
        remote_agent_id: RemoteAgentId::new(),
    };
    let ws = WorkspaceId::new();
    f.planner
        .index_answer(answer(
            query("Q?", remote, ws, vec![]),
            Timestamp::now(),
            "remote",
        ))
        .await
        .unwrap();
    let found = f
        .planner
        .find_reusable(query("Q?", remote, ws, vec![]))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.agent, remote);
}

// ---- summaries

#[tokio::test]
async fn the_summary_thresholds() {
    let f = fixture().await;
    // WhenNearlyFull is the company default: 800, and the turns are ignored.
    assert!(!f.planner.should_summarise(Some(799), 1_000));
    assert!(f.planner.should_summarise(Some(800), 0));
    assert!(
        f.planner.should_summarise(Some(5_000), 0),
        "above 1,000 counts as 1,000"
    );
    f.planner
        .update_settings(UsageSettingsChange {
            summarise_long_conversations: Some(SummariseWhen::Early),
            ..change()
        })
        .await
        .unwrap();
    assert!(!f.planner.should_summarise(Some(499), 1_000));
    assert!(f.planner.should_summarise(Some(500), 0));
}

#[tokio::test]
async fn the_break_rule_is_strictly_longer_than_an_hour_and_can_be_switched_off() {
    let f = fixture().await;
    let now = Timestamp::now();
    let at = |minutes| Timestamp(now.0 - chrono::TimeDelta::minutes(minutes));
    assert!(!f.planner.continue_from_summary(at(60), now));
    assert!(!f.planner.continue_from_summary(at(5), now));
    f.planner
        .update_settings(UsageSettingsChange {
            continue_from_summary_after_break: Some(false),
            ..change()
        })
        .await
        .unwrap();
    assert!(!f.planner.continue_from_summary(at(600), now));
}

// ---- counting

#[tokio::test]
async fn today_counts_turns_tokens_and_the_smaller_share() {
    let f = fixture().await;
    f.planner
        .record_usage(usage(3, ModelLevel::Smaller))
        .await
        .unwrap();
    f.planner
        .record_usage(RunUsage {
            run_id: RunId::new(),
            crew_run_id: Some(CrewRunId::new()),
            round: Some(2),
            level: ModelLevel::Standard,
            delta: UsageDelta {
                model_turns: 1,
                tool_calls: 5,
                input_tokens: Some(100),
                output_tokens: Some(50),
                cached_input_tokens: Some(40),
            },
        })
        .await
        .unwrap();
    f.planner
        .record_usage(RunUsage {
            run_id: RunId::new(),
            crew_run_id: None,
            round: None,
            level: ModelLevel::Standard,
            delta: UsageDelta {
                model_turns: 0,
                tool_calls: 0,
                input_tokens: Some(10),
                output_tokens: None,
                cached_input_tokens: None,
            },
        })
        .await
        .unwrap();
    f.planner.record_answered_without_model().await.unwrap();
    f.planner.record_answered_without_model().await.unwrap();
    let today = f.planner.today().await.unwrap();
    assert_eq!(today.model_turns, 4);
    assert_eq!(today.smaller_share_permille, 750);
    assert_eq!(today.answered_without_model, 2);
    assert_eq!(today.typical_day_permille, 16);
    let tokens: i64 = sqlx::query_scalar("SELECT tokens FROM usage_daily")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(tokens, 160);
    let cached: i64 = sqlx::query_scalar("SELECT tokens_from_cache FROM usage_daily")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(cached, 40);
    let lines = f.entries_of("usage_run_counted").await;
    assert_eq!(lines.len(), 3);
    let full = lines
        .iter()
        .find(|l| l.data.get("round").is_some())
        .unwrap();
    assert_eq!(full.data["level"], "standard");
    assert_eq!(full.data["tokens"], 150);
    assert_eq!(full.data["cached"], 40);
    assert!(full.crew_run_id.is_some());
    let plain = lines.iter().find(|l| l.data["level"] == "smaller").unwrap();
    assert!(plain.data.get("tokens").is_none(), "absent stays absent");
    assert!(plain.data.get("cached").is_none());
}

#[tokio::test]
async fn today_is_zero_before_anything_is_counted() {
    let f = fixture().await;
    let today = f.planner.today().await.unwrap();
    assert_eq!(
        (
            today.model_turns,
            today.smaller_share_permille,
            today.answered_without_model,
            today.typical_day_permille
        ),
        (0, 0, 0, 0)
    );
}

#[tokio::test]
async fn daily_rows_older_than_400_days_are_deleted_at_start() {
    let f = fixture().await;
    let old = (chrono::Local::now() - chrono::TimeDelta::days(401))
        .format("%Y-%m-%d")
        .to_string();
    let recent = (chrono::Local::now() - chrono::TimeDelta::days(399))
        .format("%Y-%m-%d")
        .to_string();
    for day in [&old, &recent] {
        sqlx::query("INSERT INTO usage_daily (day, model_turns) VALUES (?, 1)")
            .bind(day)
            .execute(f.pool())
            .await
            .unwrap();
    }
    let _again = f.restart().await;
    let days: Vec<String> = sqlx::query_scalar("SELECT day FROM usage_daily")
        .fetch_all(f.pool())
        .await
        .unwrap();
    assert_eq!(days, vec![recent]);
}

// ---- pruning and migrations

#[tokio::test]
async fn pruning_removes_history_and_answers_before_the_cutoff_except_runs_in_progress() {
    let f = fixture().await;
    let running = RunId::new();
    let finished = RunId::new();
    f.planner
        .record_run_finished("k", running, 1)
        .await
        .unwrap();
    f.planner
        .record_run_finished("k", finished, 1)
        .await
        .unwrap();
    let mut source = answer(
        query("Q?", agent(), WorkspaceId::new(), vec![]),
        days_ago(1),
        "A",
    );
    source.source_run_id = running;
    f.planner.index_answer(source).await.unwrap();
    f.planner
        .index_answer(answer(
            query("R?", agent(), WorkspaceId::new(), vec![]),
            days_ago(1),
            "B",
        ))
        .await
        .unwrap();
    f.planner
        .record_usage(usage(5, ModelLevel::Standard))
        .await
        .unwrap();

    let participant = f.planner.prune_participant();
    assert_eq!(participant.module(), "usage_planner");
    let cutoff = Timestamp(Timestamp::now().0 + chrono::TimeDelta::days(1));
    let deleted = participant
        .prune(
            &f.store,
            &PruneRequest {
                cutoff,
                runs_in_progress: vec![running],
            },
        )
        .await
        .unwrap();
    assert_eq!(deleted, 2, "one history row and one answer");
    let history: Vec<String> = sqlx::query_scalar("SELECT run_id FROM usage_work_history")
        .fetch_all(f.pool())
        .await
        .unwrap();
    assert_eq!(history, vec![running.to_string()]);
    let answers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reused_answers")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(answers, 1);
    assert_eq!(
        f.planner.today().await.unwrap().model_turns,
        5,
        "the daily counters keep their own rule"
    );
}

#[tokio::test]
async fn pruning_a_closed_store_reports_the_store_code() {
    let f = fixture().await;
    f.store.close().await;
    let participant = f.planner.prune_participant();
    let failure = participant
        .prune(
            &f.store,
            &PruneRequest {
                cutoff: Timestamp::now(),
                runs_in_progress: vec![],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "USAGE_STORE_UNAVAILABLE");
}

#[test]
fn the_migrations_are_data_with_a_slice_per_engine() {
    let m = UsagePlanner::migrations();
    assert_eq!(m.module, "usage_planner");
    assert_eq!(m.sqlite.len(), 1);
    assert_eq!(m.sqlite[0].0, 1);
    for table in [
        "usage_settings",
        "reused_answers",
        "usage_daily",
        "usage_work_history",
    ] {
        assert!(
            m.sqlite[0].2.contains(&format!("CREATE TABLE {table}")),
            "{table}"
        );
    }
    assert_eq!(m.postgres.len(), 1);
    assert!(m.postgres[0].2.contains("CREATE TABLE usage_daily"));
    assert!(m.postgres[0].2.contains("CREATE TABLE usage_work_history"));
    assert!(!m.postgres[0].2.contains("reused_answers"));
    assert!(!m.postgres[0].2.contains("usage_settings"));
    let _ = MIGRATIONS;
}

// ---- log events

fn log_lines() -> Vec<serde_json::Value> {
    let text = String::from_utf8(LOG.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

/// A line of `event` whose fields include every `(key, value)` pair.
fn has_line(event: &str, fields: &[(&str, serde_json::Value)]) -> bool {
    log_lines().iter().any(|line| {
        line["event"] == event
            && line["logger"] == "ordin_usage_planner"
            && fields.iter().all(|(key, value)| line[*key] == *value)
    })
}

#[tokio::test]
async fn each_event_is_logged_with_its_fields_and_never_with_request_text() {
    use serde_json::json;
    let snapshot = profile(|usage| {
        let mut by_level = BTreeMap::new();
        by_level.insert(ModelLevel::Standard, "sonnet-9".to_owned());
        usage.level_models.insert(HarnessName::ClaudeCode, by_level);
        usage.levels_allowed.insert(WorkKind::Code, vec![]);
    });
    let f = fixture_with(snapshot, vec![FakeHarness::claude()]).await;
    f.planner
        .choose(WorkKind::Documents, AdapterKind::ClaudeLocal);
    f.planner.choose(WorkKind::Code, AdapterKind::ClaudeLocal);
    let first = f
        .planner
        .choose(WorkKind::Routine, AdapterKind::ClaudeLocal);
    f.planner
        .step_up(&first, WorkKind::Routine, AdapterKind::ClaudeLocal);
    f.planner
        .update_settings(UsageSettingsChange {
            daily_pause_requests: Some(Some(100)),
            ..change()
        })
        .await
        .unwrap();
    f.planner
        .record_usage(usage(100, ModelLevel::Standard))
        .await
        .unwrap();
    f.planner.may_start_ordin_work().await.unwrap_err();
    let marker = format!("SECRET-{}", uuid::Uuid::now_v7());
    let (analyst, sales) = (agent(), WorkspaceId::new());
    let id = f
        .planner
        .index_answer(answer(
            query(&marker, analyst, sales, vec![quote("3")]),
            Timestamp::now(),
            &format!("ANSWER-{marker}"),
        ))
        .await
        .unwrap();
    f.planner
        .find_reusable(query(&marker, analyst, sales, vec![quote("4")]))
        .await
        .unwrap();
    f.planner.record_reuse(id, None).await.unwrap();

    assert!(has_line(
        "usage_level_chosen",
        &[
            ("kind", json!("documents")),
            ("harness", json!("claude_local")),
            ("model_level", json!("standard"))
        ]
    ));
    assert!(has_line(
        "usage_model_not_accepted",
        &[
            ("harness", json!("claude_local")),
            ("model_level", json!("standard"))
        ]
    ));
    assert!(has_line("usage_levels_empty", &[("kind", json!("code"))]));
    assert!(has_line(
        "usage_stepped_up",
        &[
            ("kind", json!("routine")),
            ("from", json!("smaller")),
            ("to", json!("standard"))
        ]
    ));
    assert!(has_line(
        "usage_settings_changed",
        &[("fields", json!("daily_pause_requests"))]
    ));
    assert!(has_line(
        "usage_daily_pause_reached",
        &[("turns_today", json!(100)), ("limit", json!(100))]
    ));
    assert!(has_line(
        "usage_answer_reused",
        &[("reused_answer_id", json!(id.to_string()))]
    ));
    assert!(has_line(
        "usage_reuse_input_newer",
        &[("code", json!("REUSE_INPUT_NEWER"))]
    ));
    let text = String::from_utf8(LOG.lock().unwrap().clone()).unwrap();
    assert!(
        !text.contains(&marker),
        "request and answer text are never logged"
    );
    assert!(
        !text.contains("Quote.pdf"),
        "input names and locations are never logged"
    );
}

// ---- Settings › Agents and models (SA-AGM-01)

fn personal_model(level: ModelLevel, model: &str) -> crate::PersonalModelChoices {
    let mut choices = crate::PersonalModelChoices::default();
    choices
        .models
        .entry(HarnessName::ClaudeCode)
        .or_default()
        .insert(level, model.to_owned());
    choices
}

#[tokio::test]
async fn the_persons_model_comes_before_the_profile_and_the_default() {
    let snapshot = profile(|usage| {
        let mut by_level = BTreeMap::new();
        by_level.insert(ModelLevel::Standard, "haiku".to_owned());
        usage.level_models.insert(HarnessName::ClaudeCode, by_level);
    });
    let f = fixture_with(snapshot, vec![FakeHarness::claude()]).await;
    let standard = |f: &Fixture| {
        f.planner
            .choose(WorkKind::Documents, AdapterKind::ClaudeLocal)
            .model
    };
    // The profile's model, with no choice of the person's.
    assert_eq!(standard(&f).as_deref(), Some("haiku"));
    // The person's own model comes first.
    f.planner
        .set_personal_choices(personal_model(ModelLevel::Standard, "opus"));
    assert_eq!(standard(&f).as_deref(), Some("opus"));
    assert_eq!(
        f.planner.personal_choices(),
        personal_model(ModelLevel::Standard, "opus")
    );
    // Another level of the person's leaves this one to the profile.
    f.planner
        .set_personal_choices(personal_model(ModelLevel::Smaller, "sonnet"));
    assert_eq!(standard(&f).as_deref(), Some("haiku"));
    // "Use default" again: the profile's model.
    f.planner
        .set_personal_choices(crate::PersonalModelChoices::default());
    assert_eq!(standard(&f).as_deref(), Some("haiku"));
}

#[tokio::test]
async fn the_persons_model_over_the_built_in_default_and_an_unknown_one() {
    let f = fixture().await;
    let standard = |f: &Fixture| {
        f.planner
            .choose(WorkKind::Documents, AdapterKind::ClaudeLocal)
            .model
    };
    assert_eq!(standard(&f).as_deref(), Some("sonnet"), "built-in default");
    f.planner
        .set_personal_choices(personal_model(ModelLevel::Standard, "opus"));
    assert_eq!(standard(&f).as_deref(), Some("opus"));
    // A name the harness does not accept falls back to the harness's default.
    f.planner
        .set_personal_choices(personal_model(ModelLevel::Standard, "no-such-model"));
    assert_eq!(standard(&f).as_deref(), Some("sonnet"));
}

#[tokio::test]
async fn the_persons_effort_follows_the_kind_of_work_and_the_harness() {
    let f = fixture_with(
        ProfileSnapshot::defaults(),
        vec![FakeHarness::claude(), FakeHarness::copilot()],
    )
    .await;
    assert_eq!(
        f.planner
            .choose(WorkKind::Code, AdapterKind::ClaudeLocal)
            .effort,
        None,
        "no choice leaves it to the harness"
    );
    let mut choices = crate::PersonalModelChoices::default();
    choices
        .effort
        .entry(HarnessName::ClaudeCode)
        .or_default()
        .insert(WorkKind::Code, "high".to_owned());
    f.planner.set_personal_choices(choices);
    let code = f.planner.choose(WorkKind::Code, AdapterKind::ClaudeLocal);
    assert_eq!(code.effort.as_deref(), Some("high"));
    assert_eq!(
        f.planner
            .choose(WorkKind::Talking, AdapterKind::ClaudeLocal)
            .effort,
        None,
        "another kind of work"
    );
    assert_eq!(
        f.planner
            .choose(WorkKind::Code, AdapterKind::CopilotLocal)
            .effort,
        None,
        "another harness"
    );
    assert_eq!(
        f.planner
            .choose(WorkKind::Code, AdapterKind::HostedAgent)
            .effort,
        None,
        "a remote agent"
    );
    let up = f
        .planner
        .step_up(&code, WorkKind::Code, AdapterKind::ClaudeLocal)
        .expect("one step up");
    assert_eq!(up.effort.as_deref(), Some("high"), "a step up keeps it");
}

/// SA-AGM-01 precedence: the person's effort, then the thinking choice's effort, then none.
#[tokio::test]
async fn the_thinking_choice_gives_the_effort_when_the_person_named_none() {
    let f = fixture_with(
        ProfileSnapshot::defaults(),
        vec![FakeHarness::claude(), FakeHarness::copilot()],
    )
    .await;
    let effort = |kind| f.planner.choose(kind, AdapterKind::ClaudeLocal).effort;
    // 3. Thinking is automatic and no effort is named: none.
    assert_eq!(
        effort(WorkKind::Code),
        None,
        "automatic thinking gives none"
    );

    // 2. The thinking choice decides.
    for (thinking, expected) in [
        (ThinkingLevel::Low, Some("low")),
        (ThinkingLevel::High, Some("high")),
        (ThinkingLevel::Automatic, None),
    ] {
        f.planner
            .update_settings(UsageSettingsChange {
                thinking: Some(thinking),
                ..change()
            })
            .await
            .unwrap();
        assert_eq!(effort(WorkKind::Code).as_deref(), expected, "{thinking:?}");
    }
    f.planner
        .update_settings(UsageSettingsChange {
            thinking: Some(ThinkingLevel::High),
            ..change()
        })
        .await
        .unwrap();
    assert_eq!(
        f.planner
            .choose(WorkKind::Code, AdapterKind::CopilotLocal)
            .effort,
        None,
        "only Claude Code takes an effort"
    );
    assert_eq!(
        f.planner
            .choose(WorkKind::Code, AdapterKind::HostedAgent)
            .effort,
        None,
        "a remote agent"
    );

    // 1. The person's effort for the kind of work wins over the thinking choice.
    let mut choices = crate::PersonalModelChoices::default();
    choices
        .effort
        .entry(HarnessName::ClaudeCode)
        .or_default()
        .insert(WorkKind::Code, "max".to_owned());
    f.planner.set_personal_choices(choices);
    assert_eq!(effort(WorkKind::Code).as_deref(), Some("max"));
    assert_eq!(
        effort(WorkKind::Talking).as_deref(),
        Some("high"),
        "another kind of work falls to the thinking choice"
    );
}
