//! The module's PostgreSQL slice against a real PostgreSQL 18 container (plan 01, "PostgreSQL 18 in
//! tests"): module `hook_relay` applies from an empty database, a second apply changes nothing, and
//! the table exists empty with its key and checks. One container for this binary (the helper is
//! `tests/support/pg.rs`). A test fails, never skips, when there is no container runtime.

mod support;
use support::pg;

use ordin_hook_relay::HookRelay;
use ordin_journal::{ApplyMode, Datastore, MigrationRegistry, MIGRATIONS};

async fn exec(store: &Datastore, sql: &str) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(store.postgres().expect("PostgreSQL"))
        .await
        .map(|_| ())
}

#[tokio::test]
async fn the_hook_relay_module_applies_on_postgresql_18() {
    let url = pg::empty_database().await;
    let store = pg::open_url(url, 4).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(HookRelay::migrations()).unwrap();
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("the migrations apply");
    assert!(report.applied.contains(&(HookRelay::MODULE.to_owned(), 1)));
    let again = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(again.applied.is_empty(), "applying again changes nothing");

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hook_late_decisions")
        .fetch_one(store.postgres().unwrap())
        .await
        .expect("the table exists");
    assert_eq!(n, 0);

    // The scaled shape's key and check (HookRelay.md *Data Model*).
    let (a, b) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
    let insert = |approval: uuid::Uuid, kind: &str| {
        format!(
            "INSERT INTO hook_late_decisions (approval_id, run_id, kind, created_at) \
             VALUES ('{approval}', '{b}', '{kind}', now())"
        )
    };
    assert!(exec(&store, &insert(a, "tool_call")).await.is_ok());
    assert!(
        exec(&store, &insert(a, "stop_gate")).await.is_err(),
        "approval_id is the key"
    );
    assert!(
        exec(&store, &insert(uuid::Uuid::now_v7(), "other"))
            .await
            .is_err(),
        "kind is one of two"
    );
    assert!(exec(&store, &insert(uuid::Uuid::now_v7(), "stop_gate"))
        .await
        .is_ok());
    store.close().await;
}

#[tokio::test]
async fn a_copy_of_the_template_has_the_table() {
    let store = pg::open_url(pg::migrated_database().await, 4).await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hook_late_decisions")
        .fetch_one(store.postgres().unwrap())
        .await
        .expect("the template holds the table");
    assert_eq!(n, 0);
    store.close().await;
}
