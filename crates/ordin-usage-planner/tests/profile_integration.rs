//! The planner on the real Organisation Profile service (agent profile: defaults, no profile read)
//! and the real Journal on a temporary SQLite file.

use std::sync::Arc;
use std::time::Duration;

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::{AdapterKind, ModelLevel};
use ordin_usage_planner::{UsagePlanner, WorkKind};

struct Now;

impl ordin_organisation_profile::Clock for Now {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

#[tokio::test]
async fn the_planner_runs_on_the_default_profile_and_the_journal() {
    let dir = std::env::temp_dir().join(format!("ordin-usage-it-{}", uuid::Uuid::now_v7()));
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.join("data").join("journal.db"),
        backup_dir: dir.join("data").join("backups"),
    })
    .await
    .expect("open the datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(UsagePlanner::migrations()).unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let journal = Arc::new(
        Journal::start(
            store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .unwrap(),
    );
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(Now),
            },
        )
        .await
        .unwrap(),
    );
    let planner = UsagePlanner::new(store.clone(), journal, profile, vec![])
        .await
        .unwrap();

    // No harness has been bound yet: the level is chosen and the model is left to the harness.
    let choice = planner.choose(WorkKind::Routine, AdapterKind::ClaudeLocal);
    assert_eq!(choice.level, ModelLevel::Smaller);
    assert_eq!(choice.model, None);
    assert_eq!(planner.agents_at_once(), 2);
    assert!(planner.today().await.unwrap().model_turns == 0);
    planner.may_start_ordin_work().await.unwrap();

    store.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_owned_code_is_in_the_registry() {
    for code in ordin_usage_planner::ALL_CODES {
        let entry = ordin_types::error_registry::lookup(code).expect("registered");
        assert_eq!(entry.owner_crate, "ordin-usage-planner");
    }
}
