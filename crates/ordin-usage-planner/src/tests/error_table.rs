//! One test per row of UsagePlanner.md *Error Table* (X1), and the registry check (X4).

use std::collections::BTreeMap;

use ordin_runtime_adapter::UsageDelta;
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::ModelLevel;
use ordin_types::{CodedError, RepairAction, RunId, WorkspaceId};

use super::support::*;
use crate::error::{UsageError, ALL_CODES};
use crate::types::{RunUsage, UsageSettingsChange};

fn change() -> UsageSettingsChange {
    UsageSettingsChange::default()
}

#[tokio::test]
async fn usage_fixed_by_company() {
    let snapshot = profile(|usage| usage.person_may_change = Some(vec![]));
    let f = fixture_with(snapshot, vec![]).await;
    let error = f
        .planner
        .update_settings(UsageSettingsChange {
            reuse_answers: Some(false),
            ..change()
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        UsageError::FixedByCompany {
            setting: "reuse_answers"
        }
    ));
    assert_eq!(error.code(), "USAGE_FIXED_BY_COMPANY");
    assert!(!error.retryable());
}

#[tokio::test]
async fn usage_above_profile_limit() {
    let snapshot = profile(|usage| {
        usage.agents_at_once_max = 2;
        usage.levels_allowed.insert(
            ordin_organisation_profile::WorkKind::Code,
            vec![ModelLevel::Smaller],
        );
    });
    let f = fixture_with(snapshot, vec![]).await;
    // B.3: above the company's maximum.
    let agents = f
        .planner
        .update_settings(UsageSettingsChange {
            agents_at_same_time: Some(3),
            ..change()
        })
        .await
        .unwrap_err();
    assert!(matches!(
        agents,
        UsageError::AboveProfileLimit {
            setting: "agents_at_same_time"
        }
    ));
    // B.2: a level the company does not allow for the kind.
    let mut levels = BTreeMap::new();
    levels.insert("code".to_owned(), "most_capable".to_owned());
    let level = f
        .planner
        .update_settings(UsageSettingsChange {
            model_level_per_kind: Some(levels),
            ..change()
        })
        .await
        .unwrap_err();
    assert_eq!(level.code(), "USAGE_ABOVE_PROFILE_LIMIT");
    // B.4: a daily pause that is none of None, 100, 250 or 500.
    let pause = f
        .planner
        .update_settings(UsageSettingsChange {
            daily_pause_requests: Some(Some(7)),
            ..change()
        })
        .await
        .unwrap_err();
    assert_eq!(pause.code(), "USAGE_ABOVE_PROFILE_LIMIT");
    assert!(!pause.retryable());
}

#[tokio::test]
async fn usage_level_unknown() {
    let f = fixture().await;
    for (kind, level) in [("sleeping", "smaller"), ("talking", "gigantic")] {
        let mut levels = BTreeMap::new();
        levels.insert(kind.to_owned(), level.to_owned());
        let error = f
            .planner
            .update_settings(UsageSettingsChange {
                model_level_per_kind: Some(levels),
                ..change()
            })
            .await
            .unwrap_err();
        assert!(matches!(error, UsageError::LevelUnknown));
        assert_eq!(error.code(), "USAGE_LEVEL_UNKNOWN");
        assert!(!error.retryable());
    }
}

#[tokio::test]
async fn usage_agents_out_of_range() {
    let f = fixture().await;
    for agents in [0, 4, 16] {
        let error = f
            .planner
            .update_settings(UsageSettingsChange {
                agents_at_same_time: Some(agents),
                ..change()
            })
            .await
            .unwrap_err();
        assert!(matches!(error, UsageError::AgentsOutOfRange), "{agents}");
        assert_eq!(error.code(), "USAGE_AGENTS_OUT_OF_RANGE");
    }
}

#[tokio::test]
async fn usage_daily_pause_reached() {
    let f = fixture().await;
    f.planner
        .update_settings(UsageSettingsChange {
            daily_pause_requests: Some(Some(100)),
            ..change()
        })
        .await
        .unwrap();
    f.planner
        .may_start_ordin_work()
        .await
        .expect("below the pause");
    f.planner
        .record_usage(RunUsage {
            run_id: RunId::new(),
            crew_run_id: None,
            round: None,
            level: ModelLevel::Standard,
            delta: UsageDelta {
                model_turns: 101,
                ..UsageDelta::default()
            },
        })
        .await
        .unwrap();
    let error = f.planner.may_start_ordin_work().await.unwrap_err();
    assert!(matches!(
        error,
        UsageError::DailyPauseReached {
            turns_today: 101,
            limit: 100
        }
    ));
    assert_eq!(error.code(), "USAGE_DAILY_PAUSE_REACHED");
    assert!(!error.retryable());
}

#[tokio::test]
async fn usage_not_answer_only() {
    let f = fixture().await;
    for which in 0..3 {
        let mut acted = answer(
            query("Do something", agent(), WorkspaceId::new(), vec![]),
            Timestamp::now(),
            "Done.",
        );
        match which {
            0 => acted.external_actions = 1,
            1 => acted.files_changed = 2,
            _ => acted.approvals = 1,
        }
        let error = f.planner.index_answer(acted).await.unwrap_err();
        assert!(matches!(error, UsageError::NotAnswerOnly));
        assert_eq!(error.code(), "USAGE_NOT_ANSWER_ONLY");
        assert!(!error.retryable());
    }
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reused_answers")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0, "nothing was indexed");
}

#[tokio::test]
async fn reuse_request_invalid() {
    let f = fixture().await;
    let long = "x".repeat(8_001);
    for request in ["", long.as_str()] {
        let found = f
            .planner
            .find_reusable(query(request, agent(), WorkspaceId::new(), vec![]))
            .await
            .unwrap_err();
        assert!(matches!(found, UsageError::RequestInvalid));
        assert_eq!(found.code(), "REUSE_REQUEST_INVALID");
        assert!(!found.retryable());
        let indexed = f
            .planner
            .index_answer(answer(
                query(request, agent(), WorkspaceId::new(), vec![]),
                Timestamp::now(),
                "x",
            ))
            .await
            .unwrap_err();
        assert_eq!(indexed.code(), "REUSE_REQUEST_INVALID");
    }
    // The limits themselves are valid: 1 and 8,000 characters.
    let longest = "x".repeat(8_000);
    for request in ["x", longest.as_str()] {
        f.planner
            .find_reusable(query(request, agent(), WorkspaceId::new(), vec![]))
            .await
            .expect("a request within the limits");
    }
}

#[tokio::test]
async fn usage_store_unavailable() {
    let f = fixture().await;
    f.store.close().await;
    let counted = f
        .planner
        .record_usage(RunUsage {
            run_id: RunId::new(),
            crew_run_id: None,
            round: None,
            level: ModelLevel::Smaller,
            delta: UsageDelta::default(),
        })
        .await
        .unwrap_err();
    assert!(matches!(counted, UsageError::Store(_)));
    assert_eq!(counted.code(), "USAGE_STORE_UNAVAILABLE");
    assert!(counted.retryable());
    assert_eq!(CodedError::repair(&counted), Some(RepairAction::TryAgain));
    assert_eq!(
        f.planner.today().await.unwrap_err().code(),
        "USAGE_STORE_UNAVAILABLE"
    );
}

#[tokio::test]
async fn usage_journal_unavailable() {
    // The planner's own store works; its journal does not.
    let dir = TempDir::new();
    let (store, _healthy) = open_store(&dir).await;
    let other = TempDir::new();
    let (broken_store, broken) = open_store(&other).await;
    broken_store.close().await;
    let planner =
        crate::UsagePlanner::build(store, broken, TestSource::new(profile(|_| {})), vec![])
            .await
            .unwrap();
    let error = planner
        .update_settings(UsageSettingsChange {
            reuse_answers: Some(false),
            ..change()
        })
        .await
        .unwrap_err();
    assert!(matches!(error, UsageError::Journal(_)));
    assert_eq!(error.code(), "USAGE_JOURNAL_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(CodedError::repair(&error), Some(RepairAction::TryAgain));
    // "Counters are kept": the setting itself was saved.
    assert!(!planner.settings().reuse_answers.value);
}

// X4: ErrorHandling.md Internal Logic 11.
#[test]
fn every_owned_code_is_registered_and_the_prefix_is_owned() {
    assert!(!ALL_CODES.is_empty());
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-usage-planner", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(owns_prefix("ordin-usage-planner", prefix), "{prefix}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, ALL_CODES,
        "ALL_CODES is sorted and has no duplicates"
    );
}

#[test]
fn every_returned_code_is_in_all_codes() {
    let errors = [
        UsageError::FixedByCompany { setting: "x" },
        UsageError::AboveProfileLimit { setting: "x" },
        UsageError::LevelUnknown,
        UsageError::AgentsOutOfRange,
        UsageError::DailyPauseReached {
            turns_today: 1,
            limit: 1,
        },
        UsageError::NotAnswerOnly,
        UsageError::RequestInvalid,
        UsageError::Store(sqlx::Error::PoolClosed),
        UsageError::Journal(ordin_journal::JournalError::Unavailable),
    ];
    for error in errors {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
        assert_eq!(CodedError::code(&error), error.code());
        assert_eq!(CodedError::retryable(&error), error.retryable());
        // Display carries no value from the caller or the datastore.
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn only_the_store_and_journal_rows_are_retryable_and_repairable() {
    assert!(!UsageError::LevelUnknown.retryable());
    assert_eq!(CodedError::repair(&UsageError::LevelUnknown), None);
}
