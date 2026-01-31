//! One test per Gherkin scenario of UsagePlanner.md *Acceptance Criteria*, named after the scenario.

use ordin_organisation_profile::{PlanMode, UsageSettingName, WorkKind};
use ordin_runtime_adapter::UsageDelta;
use ordin_types::domain::identity::{AskBefore, SummariseWhen, UsageChoice};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::{AdapterKind, ModelLevel, ThinkingLevel};
use ordin_types::{RunId, WorkspaceId};

use super::support::*;
use crate::error::UsageError;
use crate::types::{
    EstimateBand, EstimateBasis, EstimateRequest, RunUsage, UsageSettingsChange, WorkShape,
};
use ordin_organisation_profile::HarnessName;
use std::collections::BTreeMap;

fn counted(turns: u32, level: ModelLevel) -> RunUsage {
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

#[tokio::test]
async fn happy_path_balanced_chooses_smaller_for_routine_work_on_claude_code() {
    let f = fixture().await;
    let choice = f
        .planner
        .choose(WorkKind::Routine, AdapterKind::ClaudeLocal);
    assert_eq!(choice.level, ModelLevel::Smaller);
    assert_eq!(choice.model.as_deref(), Some("haiku"));
    assert_eq!(choice.thinking, ThinkingLevel::Automatic);
    assert!(!choice.stepped_up);
}

#[tokio::test]
async fn the_harness_default_map_applies_when_the_profile_names_no_model() {
    let f = fixture().await;
    let choice = f
        .planner
        .choose(WorkKind::Documents, AdapterKind::ClaudeLocal);
    assert_eq!(choice.model.as_deref(), Some("sonnet"));
    let wire = choice.model_choice();
    assert_eq!(wire.level, ModelLevel::Standard);
    assert_eq!(wire.model.as_deref(), Some("sonnet"));
}

#[tokio::test]
async fn a_profile_model_the_harness_does_not_accept() {
    let snapshot = profile(|usage| {
        let mut by_level = BTreeMap::new();
        by_level.insert(ModelLevel::Standard, "sonnet-9".to_owned());
        usage.level_models.insert(HarnessName::ClaudeCode, by_level);
    });
    let f = fixture_with(snapshot, vec![FakeHarness::claude()]).await;
    let choice = f
        .planner
        .choose(WorkKind::Documents, AdapterKind::ClaudeLocal);
    assert_eq!(choice.model.as_deref(), Some("sonnet"));
}

#[tokio::test]
async fn step_up_once_never_twice() {
    let f = fixture().await;
    let first = f
        .planner
        .choose(WorkKind::Routine, AdapterKind::ClaudeLocal);
    assert_eq!(first.level, ModelLevel::Smaller);
    let up = f
        .planner
        .step_up(&first, WorkKind::Routine, AdapterKind::ClaudeLocal)
        .expect("one step up");
    assert_eq!(up.level, ModelLevel::Standard);
    assert_eq!(up.model.as_deref(), Some("sonnet"));
    assert!(up.stepped_up);
    assert!(f
        .planner
        .step_up(&up, WorkKind::Routine, AdapterKind::ClaudeLocal)
        .is_none());
}

#[tokio::test]
async fn company_limits_the_levels_for_code() {
    let snapshot = profile(|usage| {
        usage.levels_allowed.insert(
            WorkKind::Code,
            vec![ModelLevel::Smaller, ModelLevel::Standard],
        );
    });
    let f = fixture_with(snapshot, vec![FakeHarness::claude()]).await;
    f.planner
        .update_settings(UsageSettingsChange {
            choice: Some(UsageChoice::BestResults),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap();
    let choice = f.planner.choose(WorkKind::Code, AdapterKind::ClaudeLocal);
    assert_eq!(choice.level, ModelLevel::Standard);
}

#[tokio::test]
async fn a_setting_the_company_fixed() {
    let snapshot = profile(|usage| {
        usage.person_may_change = Some(vec![UsageSettingName::AgentsAtOnce]);
    });
    let f = fixture_with(snapshot, vec![]).await;
    let before = f.planner.settings();
    let error = f
        .planner
        .update_settings(UsageSettingsChange {
            reuse_answers: Some(false),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "USAGE_FIXED_BY_COMPANY");
    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM usage_settings")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(stored, 0, "nothing is saved");
    assert_eq!(f.planner.settings(), before);
}

#[tokio::test]
async fn agents_at_once_above_the_company_maximum() {
    let snapshot = profile(|usage| usage.agents_at_once_max = 2);
    let f = fixture_with(snapshot, vec![]).await;
    let error = f
        .planner
        .update_settings(UsageSettingsChange {
            agents_at_same_time: Some(3),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "USAGE_ABOVE_PROFILE_LIMIT");
}

#[tokio::test]
async fn changing_a_level_makes_the_choice_custom() {
    let f = fixture().await;
    let mut levels = BTreeMap::new();
    levels.insert("talking".to_owned(), "most_capable".to_owned());
    let settings = f
        .planner
        .update_settings(UsageSettingsChange {
            model_level_per_kind: Some(levels),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap();
    assert_eq!(settings.choice.value, UsageChoice::Custom);
    let choice = f
        .planner
        .choose(WorkKind::Talking, AdapterKind::ClaudeLocal);
    assert_eq!(choice.level, ModelLevel::MostCapable);
}

#[tokio::test]
async fn estimate_from_history() {
    let f = fixture().await;
    for turns in [18, 22, 30] {
        f.planner
            .record_run_finished("workflow:W1:3", RunId::new(), turns)
            .await
            .unwrap();
    }
    let estimate = f
        .planner
        .estimate(EstimateRequest {
            work_key: "workflow:W1:3".to_owned(),
            shape: WorkShape::Workflow {
                agent_steps: 4,
                ordin_turns: 2,
            },
        })
        .await
        .unwrap();
    assert_eq!(estimate.min_turns, 18);
    assert_eq!(estimate.max_turns, 30);
    assert_eq!(estimate.band, EstimateBand::Medium);
    assert_eq!(
        estimate.based_on,
        vec![EstimateBasis::EarlierRuns { count: 3 }]
    );
}

#[tokio::test]
async fn heavy_estimate_asks_first_under_balanced() {
    let f = fixture().await;
    let estimate = f
        .planner
        .estimate(EstimateRequest {
            work_key: "crew:0192a1d0".to_owned(),
            shape: WorkShape::Crew {
                members: 3,
                max_rounds: 10,
            },
        })
        .await
        .unwrap();
    assert_eq!(estimate.band, EstimateBand::Heavy);
    assert!(estimate.ask_first);
    // The example of the Public Interface: min 24, max 250, a whole typical day.
    assert_eq!(estimate.min_turns, 24);
    assert_eq!(estimate.max_turns, 250);
    assert_eq!(estimate.typical_day_permille, 1000);
}

#[tokio::test]
async fn a_small_task_is_not_estimated() {
    let f = fixture().await;
    let estimate = f
        .planner
        .estimate(EstimateRequest {
            work_key: "task:a:free_text".to_owned(),
            shape: WorkShape::Task {
                documents: 12,
                total_bytes: 4_000,
            },
        })
        .await
        .unwrap();
    assert_eq!(estimate.band, EstimateBand::Light);
    assert!(!estimate.ask_first);
}

#[tokio::test]
async fn reuse_an_unchanged_answer_within_7_days() {
    let f = fixture().await;
    let analyst = agent();
    let sales = WorkspaceId::new();
    let id = f
        .planner
        .index_answer(answer(
            query("Compare the Q3 quotes", analyst, sales, vec![quote("3")]),
            days_ago(2),
            "Kestrel is cheapest.",
        ))
        .await
        .unwrap();
    let found = f
        .planner
        .find_reusable(query(
            "  COMPARE the   q3 quotes ",
            analyst,
            sales,
            vec![quote("3")],
        ))
        .await
        .unwrap()
        .expect("the earlier answer");
    assert_eq!(found.text, "Kestrel is cheapest.");
    assert_eq!(found.id, id);
    f.planner.record_reuse(id, None).await.unwrap();
    assert_eq!(f.planner.today().await.unwrap().answered_without_model, 1);
    let lines = f.entries_of("usage_answer_reused").await;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].data["reused_answer_id"], id.to_string());
}

#[tokio::test]
async fn summarise_after_40_turns_when_context_use_is_unknown() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            summarise_long_conversations: Some(SummariseWhen::Early),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap();
    assert!(f.planner.should_summarise(None, 40));
    assert!(!f.planner.should_summarise(None, 39));
}

#[tokio::test]
async fn context_use_decides_once_c_29_is_confirmed() {
    let f = fixture().await;
    assert_eq!(
        f.planner.settings().summarise_long_conversations.value,
        SummariseWhen::WhenNearlyFull
    );
    assert!(f.planner.should_summarise(Some(820), 3));
}

#[tokio::test]
async fn only_when_asked_never_summarises_by_itself() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            summarise_long_conversations: Some(SummariseWhen::OnlyWhenAsked),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap();
    assert!(!f.planner.should_summarise(None, 100));
}

#[tokio::test]
async fn no_reuse_when_an_input_changed() {
    let f = fixture().await;
    let analyst = agent();
    let sales = WorkspaceId::new();
    f.planner
        .index_answer(answer(
            query("Compare the Q3 quotes", analyst, sales, vec![quote("3")]),
            days_ago(2),
            "Kestrel is cheapest.",
        ))
        .await
        .unwrap();
    let found = f
        .planner
        .find_reusable(query(
            "Compare the Q3 quotes",
            analyst,
            sales,
            vec![quote("4")],
        ))
        .await
        .unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn never_index_an_action() {
    let f = fixture().await;
    let mut acted = answer(
        query("Send the quote", agent(), WorkspaceId::new(), vec![]),
        Timestamp::now(),
        "Sent.",
    );
    acted.external_actions = 1;
    let error = f.planner.index_answer(acted).await.unwrap_err();
    assert_eq!(error.code(), "USAGE_NOT_ANSWER_ONLY");
}

#[tokio::test]
async fn daily_pause_stops_ordins_work_not_the_persons() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            daily_pause_requests: Some(Some(100)),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap();
    f.planner
        .record_usage(counted(100, ModelLevel::Standard))
        .await
        .unwrap();
    let error = f.planner.may_start_ordin_work().await.unwrap_err();
    assert_eq!(error.code(), "USAGE_DAILY_PAUSE_REACHED");
    assert!(matches!(
        error,
        UsageError::DailyPauseReached {
            turns_today: 100,
            limit: 100
        }
    ));
}

#[tokio::test]
async fn counting_when_the_harness_reports_no_tokens() {
    let f = fixture().await;
    f.planner
        .record_usage(counted(4, ModelLevel::Standard))
        .await
        .unwrap();
    assert_eq!(f.planner.today().await.unwrap().model_turns, 4);
    let tokens: Option<i64> = sqlx::query_scalar("SELECT tokens FROM usage_daily")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(tokens, None, "tokens stay absent, never zero");
    let cached: Option<i64> = sqlx::query_scalar("SELECT tokens_from_cache FROM usage_daily")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(cached, None);
}

#[tokio::test]
async fn continue_from_a_summary_after_a_break() {
    let f = fixture().await;
    assert!(f.planner.settings().continue_from_summary_after_break.value);
    let now = Timestamp::now();
    let last = Timestamp(now.0 - chrono::TimeDelta::minutes(61));
    assert!(f.planner.continue_from_summary(last, now));
}

#[tokio::test]
async fn harnesses_bound_after_start() {
    let f = fixture_with(
        ordin_organisation_profile::ProfileSnapshot::defaults(),
        vec![],
    )
    .await;
    let before = f
        .planner
        .choose(WorkKind::Documents, AdapterKind::ClaudeLocal);
    assert_eq!(before.model, None);
    f.planner.set_harnesses(vec![FakeHarness::claude()]);
    let after = f
        .planner
        .choose(WorkKind::Documents, AdapterKind::ClaudeLocal);
    assert_eq!(after.model.as_deref(), Some("sonnet"));
}

#[tokio::test]
async fn ask_before_medium_or_heavy_work() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            ask_before_estimated_requests: Some(AskBefore::MediumOrHeavy),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap();
    f.planner
        .record_run_finished("workflow:M:1", RunId::new(), 60)
        .await
        .unwrap();
    let estimate = f
        .planner
        .estimate(EstimateRequest {
            work_key: "workflow:M:1".to_owned(),
            shape: WorkShape::Workflow {
                agent_steps: 1,
                ordin_turns: 1,
            },
        })
        .await
        .unwrap();
    assert_eq!(estimate.band, EstimateBand::Medium);
    assert!(estimate.ask_first);
}

#[tokio::test]
async fn store_unavailable_while_saving_a_setting() {
    let f = fixture().await;
    f.store.close().await;
    let error = f
        .planner
        .update_settings(UsageSettingsChange {
            reuse_answers: Some(false),
            ..UsageSettingsChange::default()
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "USAGE_STORE_UNAVAILABLE");
    assert!(error.retryable());
}

#[test]
fn plan_mode_names_stay_in_step_with_the_choices() {
    // The company's plan modes map onto the three named choices (A.1).
    for (mode, choice) in [
        (PlanMode::Save, UsageChoice::SaveMyPlan),
        (PlanMode::Balanced, UsageChoice::Balanced),
        (PlanMode::Best, UsageChoice::BestResults),
    ] {
        let snapshot = profile(|usage| usage.plan_mode = mode);
        let policy = crate::policy::Policy::build(
            std::sync::Arc::new(snapshot),
            &UsageSettingsChange::default(),
            Timestamp::now(),
        );
        assert_eq!(policy.settings.choice.value, choice);
    }
}
