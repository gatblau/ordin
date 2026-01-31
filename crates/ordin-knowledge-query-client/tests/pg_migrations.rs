//! The module's PostgreSQL slice against a real PostgreSQL 18 container (plan 01 Phase 3, `[pg]`):
//! the tables exist at version 2 and hold the same checks; in the scaled shape they hold no rows.
//! Needs a reachable container runtime (`DOCKER_HOST`).

mod support;

use ordin_journal::{ApplyMode, MigrationRegistry, MIGRATIONS};
use ordin_knowledge_query_client::{migrations, MODULE_NAME};
use sqlx::Row;
use support::pg;

const ID_A: &str = "0192c1d2-0a1b-7c3d-9e4f-5a6b7c8d9e0f";
const ID_B: &str = "0192c1d2-0a1b-7c3d-9e4f-5a6b7c8d9e10";
const WS: &str = "0192b7a0-5c1d-7e2f-8a3b-4c5d6e7f8091";

async fn run(store: &ordin_journal::Datastore, sql: String) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(store.postgres().expect("PostgreSQL"))
        .await
        .map(|_| ())
}

fn insert(id: &str, url: &str, added_by: &str, key: Option<&str>, removed: bool) -> String {
    let key = key.map_or("NULL".to_owned(), |k| format!("'{k}'"));
    format!(
        "INSERT INTO kq_connections (id, workspace_id, url, name, issuer, token_audience, added_by, \
         company_entry_key, removed_by_person, state, sources_record_enabled, sources_record_days, \
         added_at) VALUES ('{id}', '{WS}', '{url}', 'agent', 'https://id.example.org', 'aud', \
         '{added_by}', {key}, {removed}, 'ready', TRUE, 365, '2026-01-10T09:00:00.000Z')"
    )
}

#[tokio::test]
async fn the_module_applies_both_versions_from_an_empty_database() {
    let store = pg::open_url(pg::empty_database().await).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(migrations()).unwrap();
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(report.applied.contains(&(MODULE_NAME.to_owned(), 1)));
    assert!(report.applied.contains(&(MODULE_NAME.to_owned(), 2)));
    // Applying again changes nothing.
    let again = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(again.applied.is_empty());
    store.close().await;
}

#[tokio::test]
async fn the_tables_exist_empty_and_hold_the_same_checks() {
    let store = pg::open_url(pg::migrated_database().await).await;
    let pool = store.postgres().unwrap();
    for table in [
        "kq_connections",
        "kq_connection_contents",
        "kq_connection_accounts",
    ] {
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) AS n FROM {table}"
        )))
        .fetch_one(pool)
        .await
        .unwrap()
        .get("n");
        assert_eq!(n, 0, "{table} holds no rows");
    }

    run(
        &store,
        insert(ID_A, "https://a.example.org", "person", None, false),
    )
    .await
    .unwrap();
    let bad: Vec<(&str, String)> = vec![
        (
            "an http address",
            insert(ID_B, "http://b.example.org", "person", None, false),
        ),
        (
            "a company row with no key",
            insert(ID_B, "https://b.example.org", "company", None, false),
        ),
        (
            "a person row with a key",
            insert(ID_B, "https://b.example.org", "person", Some("k"), false),
        ),
        (
            "a person row removed by the person",
            insert(ID_B, "https://b.example.org", "person", None, true),
        ),
        (
            "the same address twice in a workspace",
            insert(ID_B, "https://a.example.org", "person", None, false),
        ),
    ];
    for (what, sql) in bad {
        assert!(run(&store, sql).await.is_err(), "{what} must be refused");
    }
    run(
        &store,
        insert(ID_B, "https://b.example.org", "company", Some("hq"), true),
    )
    .await
    .unwrap();

    run(&store, format!("INSERT INTO kq_connection_contents VALUES ('{ID_A}', '{{}}', '2026-01-10T09:00:00.000Z')")).await.unwrap();
    run(&store, format!("INSERT INTO kq_connection_accounts VALUES ('{ID_A}', 'github', 'linked', '2026-01-10T09:00:00.000Z')")).await.unwrap();
    assert!(run(&store, format!("INSERT INTO kq_connection_accounts VALUES ('{ID_A}', 'sharepoint', 'linked', '2026-01-10T09:00:00.000Z')")).await.is_err());
    assert!(run(&store, format!("INSERT INTO kq_connection_accounts VALUES ('{ID_A}', 'github', 'linked', '2026-01-10T09:00:00.000Z')")).await.is_err());
    run(
        &store,
        format!("DELETE FROM kq_connections WHERE id = '{ID_A}'"),
    )
    .await
    .unwrap();
    for table in ["kq_connection_contents", "kq_connection_accounts"] {
        let n: i64 = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) AS n FROM {table}"
        )))
        .fetch_one(pool)
        .await
        .unwrap()
        .get("n");
        assert_eq!(n, 0, "{table} cascades");
    }
    store.close().await;
}

#[tokio::test]
async fn an_agent_profile_client_over_postgres_reads_nothing() {
    use ordin_knowledge_query_client::{
        KnowledgeClientDeps, KnowledgeClientMode, KnowledgeQueryClient, KqError,
    };
    use std::sync::Arc;
    // The scaled shape's journal is PostgreSQL; an agent-profile client asks for no workspace data.
    let store = pg::open_url(pg::migrated_database().await).await;
    let journal = Arc::new(
        ordin_journal::Journal::start(
            store,
            ordin_journal::JournalOptions {
                automatic_pruning: false,
                prune_interval: std::time::Duration::from_secs(3600),
                clock: Arc::new(ordin_journal::SystemClock),
            },
        )
        .await
        .unwrap(),
    );
    let agent = KnowledgeQueryClient::new(KnowledgeClientDeps {
        journal: Arc::clone(&journal),
        identity: None,
        http: reqwest::Client::new(),
        mode: KnowledgeClientMode::Agent {
            proxies: vec![],
            service_token: None,
        },
    })
    .unwrap();
    let ws = ordin_types::WorkspaceId::new();
    assert!(agent.connections_of(ws).await.is_empty());
    assert!(agent.contents_names(ws).await.is_empty());
    assert_eq!(
        agent.list_connections(ws).await.unwrap_err(),
        KqError::NotSupported
    );
    journal.shutdown().await;
}
