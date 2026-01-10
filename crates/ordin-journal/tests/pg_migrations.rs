//! Migrations against a real PostgreSQL 18 container (plan 01 Phase 1, integration
//! `pg_migrations.rs`): DatabaseMigrations.md steps 1–9 from an empty database, and the
//! scaled-shape scenarios. Needs a reachable container runtime (`DOCKER_HOST`).

mod support;

use std::time::Instant;

use ordin_journal::{
    ApplyMode, Datastore, JournalError, MigrationRegistry, MigrationStep, MIGRATIONS,
};
use support::{applied, count, pg};

const JOURNAL_1_2: &[MigrationStep] = &[
    (1, "create journal tables", MIGRATIONS.postgres[0].2),
    (2, "two", "CREATE TABLE journal_two (x integer)"),
];

fn journal_1_2() -> MigrationRegistry {
    let mut registry = MigrationRegistry::new();
    registry
        .register(ordin_journal::ModuleMigrations {
            module: "journal",
            sqlite: &[],
            postgres: JOURNAL_1_2,
        })
        .unwrap();
    registry
}

/// The helper's own check (plan 01 Phase 0 spike): the template clones, and how long a clone takes.
#[tokio::test]
async fn the_template_database_clones_quickly() {
    pg::server().await;
    for _ in 0..2 {
        let started = Instant::now();
        let url = pg::migrated_database().await;
        eprintln!("template clone: {} ms", started.elapsed().as_millis());
        let store = pg::open_url(url).await;
        assert_eq!(applied(&store).await, vec![("journal".to_owned(), 1)]);
        store.close().await;
    }
}

#[tokio::test]
async fn migration_steps_1_to_9_on_postgres() {
    let fresh = || {
        Box::pin(async { pg::open_url(pg::empty_database().await).await })
            as std::pin::Pin<Box<dyn std::future::Future<Output = Datastore>>>
    };
    support::steps::migration_steps_1_to_9(&fresh).await;
}

#[tokio::test]
async fn the_journal_tables_are_created_on_postgres() {
    let store = pg::open_url(pg::empty_database().await).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_eq!(report.applied, vec![("journal".to_owned(), 1)]);
    assert_eq!(report.backup_file, None, "no copy is made on PostgreSQL");
    let tables = count(
        &store,
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name IN \
         ('journal_meta','journal_events','journal_entries','journal_runs','artefacts','tool_requests', \
          'external_actions','startup_reports','reports','dashboards','run_steps','schema_migrations')",
    )
    .await;
    assert_eq!(tables, 12);
    store.close().await;
}

// Scenario: A scaled-shape instance refuses a schema behind its program
#[tokio::test]
async fn a_scaled_shape_instance_refuses_a_schema_behind_its_program() {
    let store = pg::open_url(pg::migrated_database().await).await;
    match journal_1_2().apply(&store, ApplyMode::CheckOnly).await {
        Err(JournalError::SchemaBehind { pending }) => {
            assert_eq!(pending, vec![("journal".to_owned(), 2)])
        }
        other => panic!("{other:?}"),
    }
    store.close().await;
}

// DatabaseMigrations.md — Scenario: Edge case — the scaled shape never migrates from an instance
#[tokio::test]
async fn edge_case_the_scaled_shape_never_migrates_from_an_instance() {
    let store = pg::open_url(pg::migrated_database().await).await;
    let before = applied(&store).await;
    let error = journal_1_2()
        .apply(&store, ApplyMode::CheckOnly)
        .await
        .unwrap_err();
    assert_eq!(ordin_types::CodedError::code(&error), "JRNL_SCHEMA_BEHIND");
    assert_eq!(
        applied(&store).await,
        before,
        "schema_migrations is unchanged"
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = 'journal_two'"
        )
        .await,
        0
    );
    // The schema command then applies it.
    let report = journal_1_2()
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_eq!(report.applied, vec![("journal".to_owned(), 2)]);
    store.close().await;
}
