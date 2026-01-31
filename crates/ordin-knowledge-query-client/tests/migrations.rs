//! The module's SQLite migrations against a real journal file: both versions apply, and the
//! constraints of the Data Model hold (KnowledgeQueryClient.md *Data Model*).

use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_knowledge_query_client::{migrations, MODULE_NAME};
use sqlx::Row;

async fn store() -> (Datastore, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("ordin-kq-mig-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.join("data").join("journal.db"),
        backup_dir: dir.join("data").join("backups"),
    })
    .await
    .unwrap();
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(migrations()).unwrap();
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(report.applied.contains(&(MODULE_NAME.to_owned(), 1)));
    assert!(report.applied.contains(&(MODULE_NAME.to_owned(), 2)));
    (store, dir)
}

const ID_A: &str = "0192c1d2-0a1b-7c3d-9e4f-5a6b7c8d9e0f";
const ID_B: &str = "0192c1d2-0a1b-7c3d-9e4f-5a6b7c8d9e10";
const WS: &str = "0192b7a0-5c1d-7e2f-8a3b-4c5d6e7f8091";

fn insert(
    id: &str,
    url: &str,
    added_by: &str,
    key: Option<&str>,
    removed: i64,
    state: &str,
) -> String {
    let key = key.map_or("NULL".to_owned(), |k| format!("'{k}'"));
    format!(
        "INSERT INTO kq_connections (id, workspace_id, url, name, issuer, token_audience, added_by, \
         company_entry_key, removed_by_person, state, sources_record_enabled, sources_record_days, \
         added_at) VALUES ('{id}', '{WS}', '{url}', 'agent', 'https://id.example.org', 'aud', \
         '{added_by}', {key}, {removed}, '{state}', 1, 365, '2026-01-10T09:00:00.000Z')"
    )
}

async fn run(store: &Datastore, sql: String) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(store.sqlite().unwrap())
        .await
        .map(|_| ())
}

#[test]
fn the_module_is_named_and_carries_both_engines() {
    let m = migrations();
    assert_eq!(m.module, "knowledge_query_client");
    assert_eq!(m.sqlite.iter().map(|s| s.0).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(m.postgres.iter().map(|s| s.0).collect::<Vec<_>>(), [1, 2]);
}

#[tokio::test]
async fn the_connection_table_enforces_its_checks() {
    let (store, dir) = store().await;
    run(
        &store,
        insert(ID_A, "https://a.example.org", "person", None, 0, "ready"),
    )
    .await
    .unwrap();
    let bad: Vec<(&str, String)> = vec![
        (
            "an http address",
            insert(ID_B, "http://b.example.org", "person", None, 0, "ready"),
        ),
        (
            "an unknown state",
            insert(ID_B, "https://b.example.org", "person", None, 0, "gone"),
        ),
        (
            "a company row with no key",
            insert(ID_B, "https://b.example.org", "company", None, 0, "ready"),
        ),
        (
            "a person row with a key",
            insert(
                ID_B,
                "https://b.example.org",
                "person",
                Some("k"),
                0,
                "ready",
            ),
        ),
        (
            "a person row removed by the person",
            insert(ID_B, "https://b.example.org", "person", None, 1, "ready"),
        ),
        (
            "the same address twice in a workspace",
            insert(ID_B, "https://a.example.org", "person", None, 0, "ready"),
        ),
    ];
    for (what, sql) in bad {
        assert!(run(&store, sql).await.is_err(), "{what} must be refused");
    }
    // A company row may be removed by the person, and a key is unique per workspace.
    run(
        &store,
        insert(
            ID_B,
            "https://b.example.org",
            "company",
            Some("hq"),
            1,
            "unreachable",
        ),
    )
    .await
    .unwrap();
    assert!(run(
        &store,
        insert(
            "0192c1d2-0a1b-7c3d-9e4f-5a6b7c8d9e11",
            "https://c.example.org",
            "company",
            Some("hq"),
            0,
            "ready"
        )
    )
    .await
    .is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn contents_and_accounts_follow_their_connection() {
    let (store, dir) = store().await;
    run(
        &store,
        insert(ID_A, "https://a.example.org", "person", None, 0, "ready"),
    )
    .await
    .unwrap();
    run(&store, format!("INSERT INTO kq_connection_contents VALUES ('{ID_A}', '{{}}', '2026-01-10T09:00:00.000Z')")).await.unwrap();
    for system in ["github", "gitlab", "confluence"] {
        run(&store, format!("INSERT INTO kq_connection_accounts VALUES ('{ID_A}', '{system}', 'linked', '2026-01-10T09:00:00.000Z')")).await.unwrap();
    }
    assert!(run(&store, format!("INSERT INTO kq_connection_accounts VALUES ('{ID_A}', 'sharepoint', 'linked', '2026-01-10T09:00:00.000Z')")).await.is_err());
    assert!(run(&store, format!("INSERT INTO kq_connection_accounts VALUES ('{ID_A}', 'github', 'maybe', '2026-01-10T09:00:00.000Z')")).await.is_err());
    assert!(run(&store, format!("INSERT INTO kq_connection_accounts VALUES ('{ID_A}', 'github', 'linked', '2026-01-10T09:00:00.000Z')")).await.is_err(), "one row per system");
    let big = "x".repeat(2_097_153);
    assert!(run(
        &store,
        format!("UPDATE kq_connection_contents SET contents_json = '{big}'")
    )
    .await
    .is_err());
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
        .fetch_one(store.sqlite().unwrap())
        .await
        .unwrap()
        .get("n");
        assert_eq!(n, 0, "{table} cascades");
    }
    let _ = std::fs::remove_dir_all(dir);
}
