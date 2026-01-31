//! The phase-1 table owners registered together, as a program builds its registry
//! (DatabaseMigrations.md *Modules that register migrations*; plan 01 Phase 1 behavioural check):
//! the journal, Organisation Profile and Identity migrations applied to SQLite and to PostgreSQL 18,
//! with `schema_migrations` read back. Needs a reachable container runtime (`DOCKER_HOST`).

#[allow(dead_code)]
#[path = "../../ordin-journal/tests/support/pg.rs"]
mod pg;

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MigrationStep, ModuleMigrations,
    MIGRATIONS,
};

/// The registry the desktop builds for these three modules: the journal first, then the others.
fn registry() -> MigrationRegistry {
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal registers");
    registry
        .register(ModuleMigrations {
            module: ordin_organisation_profile::MODULE_NAME,
            sqlite: ordin_organisation_profile::SQLITE_MIGRATIONS,
            // DatabaseMigrations.md: Organisation Profile holds no tables in the scaled shape.
            postgres: &[],
        })
        .expect("organisation profile registers");
    registry
        .register(ModuleMigrations {
            module: ordin_identity::MODULE_NAME,
            sqlite: ordin_identity::SQLITE_MIGRATIONS,
            // IdentityAndCredentials.md *Data Model*: no PostgreSQL variant.
            postgres: &[],
        })
        .expect("identity registers");
    registry
}

/// Every (module, version) the registry should leave behind on one engine, sorted.
fn expected(modules: &[(&str, &[MigrationStep])]) -> Vec<(String, i64)> {
    let mut rows: Vec<(String, i64)> = modules
        .iter()
        .flat_map(|(module, steps)| steps.iter().map(|s| (module.to_string(), s.0)))
        .collect();
    rows.sort();
    rows
}

#[tokio::test]
async fn phase_one_migrations_register_and_apply_on_sqlite() {
    let dir = std::env::temp_dir().join(format!("ordin-registry-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("temporary folder");
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.join("journal.db"),
        backup_dir: dir.join("backups"),
    })
    .await
    .expect("open SQLite");

    let report = registry()
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply");
    let want = expected(&[
        ("journal", MIGRATIONS.sqlite),
        (
            ordin_organisation_profile::MODULE_NAME,
            ordin_organisation_profile::SQLITE_MIGRATIONS,
        ),
        (
            ordin_identity::MODULE_NAME,
            ordin_identity::SQLITE_MIGRATIONS,
        ),
    ]);
    let mut applied = report.applied.clone();
    applied.sort();
    assert_eq!(applied, want);

    let pool = store.sqlite().expect("SQLite pool");
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT module, version FROM schema_migrations ORDER BY module, version")
            .fetch_all(pool)
            .await
            .expect("read schema_migrations");
    assert_eq!(rows, want);

    // A second run finds nothing pending.
    let again = registry()
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply again");
    assert!(again.applied.is_empty());

    store.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn phase_one_migrations_register_and_apply_on_postgres() {
    let url = pg::empty_database().await;
    let store = pg::open_url(url).await;

    let report = registry()
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply");
    // Only the journal has PostgreSQL steps among these three modules (engine rule, Internal Logic 11).
    let want = expected(&[("journal", MIGRATIONS.postgres)]);
    let mut applied = report.applied.clone();
    applied.sort();
    assert_eq!(applied, want);

    let pool = store.postgres().expect("PostgreSQL pool");
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT module, version FROM schema_migrations ORDER BY module, version")
            .fetch_all(pool)
            .await
            .expect("read schema_migrations");
    assert_eq!(rows, want);

    // The scaled shape's instance check: nothing pending.
    registry()
        .apply(&store, ApplyMode::CheckOnly)
        .await
        .expect("nothing pending");

    store.close().await;
}
