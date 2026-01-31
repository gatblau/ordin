//! The module's PostgreSQL slice against a real PostgreSQL 18 container (plan 01, "PostgreSQL 18 in
//! tests"): module `rules` applies from an empty database, a second apply changes nothing, and the six
//! tables exist empty with their checks. One container for this binary; its data folder is on tmpfs
//! and it runs with `fsync=off`, safe only because the data is thrown away. The image is pinned.
//! Needs a reachable container runtime (`DOCKER_HOST`); a test fails, never skips, when there is none.

use ordin_journal::{
    ApplyMode, DatabaseUrl, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS,
};
use ordin_rules::RulesEngine;
use testcontainers::core::{IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const IMAGE: &str = "postgres";
const TAG: &str = "18.6";
const PASSWORD: &str = "ordin-test";
const READY: &str = "database system is ready to accept connections";

const TABLES: [&str; 6] = [
    "rules_control_starts",
    "rules_control_messages",
    "rules_control_locks",
    "rules_intake_places",
    "rules_run_facts",
    "rules_approved_calls",
];

async fn exec(store: &Datastore, sql: &str) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(store.postgres().expect("PostgreSQL"))
        .await
        .map(|_| ())
}

#[tokio::test]
async fn the_rules_module_applies_on_postgresql_18() {
    let container = GenericImage::new(IMAGE, TAG)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(READY))
        .with_wait_for(WaitFor::message_on_stdout(READY))
        .with_env_var("POSTGRES_PASSWORD", PASSWORD)
        .with_mount(Mount::tmpfs_mount("/var/lib/postgresql"))
        .with_cmd([
            "postgres",
            "-c",
            "fsync=off",
            "-c",
            "synchronous_commit=off",
        ])
        .start()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "cannot start the PostgreSQL test container ({IMAGE}:{TAG}): {error}. Is the \
                 container runtime running, and does DOCKER_HOST point at its socket? DOCKER_HOST \
                 is currently {:?}.",
                std::env::var("DOCKER_HOST").ok()
            )
        });
    let host = container.get_host().await.expect("host").to_string();
    let port = container
        .get_host_port_ipv4(5432.tcp())
        .await
        .expect("mapped port");
    let store = Datastore::open(DatastoreConfig::Postgres {
        url: DatabaseUrl::new(format!(
            "postgres://postgres:{PASSWORD}@{host}:{port}/postgres"
        )),
        max_connections: 4,
    })
    .await
    .expect("open the PostgreSQL datastore");

    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(RulesEngine::migrations()).unwrap();
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("the migrations apply");
    assert!(report
        .applied
        .contains(&(RulesEngine::MODULE.to_owned(), 1)));
    let again = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(again.applied.is_empty(), "applying again changes nothing");

    for table in TABLES {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
                .fetch_one(store.postgres().unwrap())
                .await
                .unwrap_or_else(|e| panic!("{table}: {e}"));
        assert_eq!(n, 0, "{table} holds no rows");
    }
    // The keys and checks hold: a second holder of one lock, a digest that is not SHA-256 and a
    // start kind outside the three are refused.
    assert!(exec(
        &store,
        "INSERT INTO rules_control_locks (key, holder, expires_at) VALUES ('duty:x', 'r', now())"
    )
    .await
    .is_ok());
    assert!(exec(
        &store,
        "INSERT INTO rules_control_locks (key, holder, expires_at) VALUES ('duty:x', 'r2', now())"
    )
    .await
    .is_err());
    let digest = "a".repeat(64);
    let message = |digest: &str| {
        format!("INSERT INTO rules_control_messages (scope, asker, digest, seen_at) VALUES ('s', 'a', '{digest}', now())")
    };
    assert!(exec(&store, &message(&digest)).await.is_ok());
    assert!(exec(&store, &message("nothex")).await.is_err());
    let start = |kind: &str| {
        format!("INSERT INTO rules_control_starts (kind, key, period_seconds, window_start, starts) VALUES ('{kind}', 'k', 60, now(), 1)")
    };
    assert!(exec(&store, &start("scope")).await.is_ok());
    assert!(exec(&store, &start("person")).await.is_err());
    store.close().await;
}
