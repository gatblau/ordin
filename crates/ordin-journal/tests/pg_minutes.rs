//! Minutes kept by Ordin (JA-MIN-1 to JA-MIN-3) on PostgreSQL 18: migration step 4 on a version-3
//! database with dashboards, and the core paths on a copy of the migrated template. Needs a
//! reachable container runtime (`DOCKER_HOST`).

mod support;

use ordin_journal::{
    ApplyMode, Journal, MigrationRegistry, MinutesUpdate, ModuleMigrations, NewMinutes, MIGRATIONS,
};
use ordin_types::wire::minutes::MinutesSource;
use ordin_types::{RecordingId, RunId, WorkspaceId};
use support::{applied, assert_code, count, exec, options, pg, pg_journal, TestClock};

fn new_minutes(workspace: WorkspaceId, title: &str) -> NewMinutes {
    NewMinutes {
        workspace_id: workspace,
        title: title.to_owned(),
        source: MinutesSource::AudioFile,
        source_name: "meeting.m4a".to_owned(),
        body: "## Summary\n\nWe agreed the supplier terms.\n".to_owned(),
        recording: None,
        run_id: Some(RunId::new()),
    }
}

// Scenario: Edge — migration step 4 on an existing database
#[tokio::test]
async fn edge_migration_step_4_on_an_existing_database_on_postgres() {
    let store = pg::open_url(pg::empty_database().await).await;
    let mut registry = MigrationRegistry::new();
    registry
        .register(ModuleMigrations {
            module: "journal",
            sqlite: &MIGRATIONS.sqlite[..3],
            postgres: &MIGRATIONS.postgres[..3],
        })
        .unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    exec(
        &store,
        "INSERT INTO dashboards (id, workspace_id, name, made_by, inputs, refresh_runs, kept_at) \
         VALUES ('0192f0e0-0000-7000-8000-000000000001', '0192f0e0-0000-7000-8000-000000000002', \
         'Weekly sales', '{\"kind\":\"conversation\"}', '[]', \
         '[\"0192f0e0-0000-7000-8000-000000000003\"]', '2026-01-10T09:00:00.000Z')",
    )
    .await;

    support::migrate(&store).await;

    let versions: Vec<i64> = applied(&store).await.into_iter().map(|(_, v)| v).collect();
    assert_eq!(versions, [1, 2, 3, 4]);
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = 'minutes'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM pg_indexes WHERE indexname = 'minutes_workspace'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM dashboards WHERE name = 'Weekly sales' \
             AND kept_at = '2026-01-10T09:00:00.000Z' AND made_by = '{\"kind\":\"conversation\"}'"
        )
        .await,
        1,
        "the dashboards are unchanged"
    );
    let journal = Journal::start(store, options(TestClock::now_real()))
        .await
        .unwrap();
    let ws = WorkspaceId::new();
    journal
        .keep_minutes(new_minutes(ws, "After the upgrade"))
        .await
        .unwrap();
    assert_eq!(journal.list_minutes(ws, None).await.unwrap().items.len(), 1);
    journal.shutdown().await;
}

#[tokio::test]
async fn minutes_on_postgres() {
    let (j, clock) = pg_journal().await;
    let ws = WorkspaceId::new();
    let kept = j
        .keep_minutes(new_minutes(ws, "Supplier call"))
        .await
        .unwrap();
    assert_eq!(j.minutes(kept.id).await.unwrap(), Some(kept.clone()));
    assert_eq!(
        count(
            j.datastore(),
            "SELECT COUNT(*) FROM journal_events WHERE kind = 'workspace_changed'"
        )
        .await,
        1
    );
    clock.advance_ms(5);
    let MinutesUpdate::Updated(edited) = j
        .update_minutes_body(kept.id, "Changed".to_owned(), 1)
        .await
        .unwrap()
    else {
        panic!("not updated");
    };
    assert_eq!(edited.revision, 2);
    assert_eq!(
        j.update_minutes_body(kept.id, "Again".to_owned(), 1)
            .await
            .unwrap(),
        MinutesUpdate::Stale {
            current_revision: 2
        }
    );
    assert_eq!(j.minutes(kept.id).await.unwrap(), Some(edited));
    let recording = RecordingId::new();
    let with = j
        .set_minutes_recording(kept.id, Some(recording))
        .await
        .unwrap();
    assert_eq!(j.minutes(kept.id).await.unwrap(), Some(with));
    let error = j
        .keep_minutes(new_minutes(ws, &"t".repeat(121)))
        .await
        .unwrap_err();
    assert_code(&error, "MINUTES_TITLE_INVALID", false);
    let listed = j.list_minutes(ws, None).await.unwrap();
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].source, MinutesSource::AudioFile);
    j.keep_minutes(new_minutes(ws, "Second")).await.unwrap();
    assert_eq!(j.forget_workspace(ws).await.unwrap(), 2);
    assert_eq!(
        count(j.datastore(), "SELECT COUNT(*) FROM minutes").await,
        0
    );
    j.shutdown().await;
}
