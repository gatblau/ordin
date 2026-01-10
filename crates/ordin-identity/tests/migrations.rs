//! The migration steps this crate exports as data, applied directly to a SQLite file as the Journal's
//! registry applies them (each step inside one transaction; DatabaseMigrations.md step 8).
//!
//! `migration_step_2_keeps_every_credential` is the rule-12a migration test the ADR-106 amendment names
//! (scenario *Edge case — the migration keeps every credential*).

use ordin_identity::{MODULE_NAME, SQLITE_MIGRATIONS};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row as _, SqlitePool};

async fn empty_database() -> SqlitePool {
    let dir = std::env::temp_dir().join(format!(
        "ordin-identity-migrations-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(dir.join("journal.db"))
        .create_if_missing(true)
        .foreign_keys(true);
    SqlitePool::connect_with(options).await.unwrap()
}

/// One step in one transaction, as the registry runs it.
async fn apply(pool: &SqlitePool, version: i64) {
    let (_, _, sql) = SQLITE_MIGRATIONS
        .iter()
        .find(|(v, _, _)| *v == version)
        .expect("step exists");
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(*sql).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

async fn rows(pool: &SqlitePool, sql: &'static str) -> Vec<Vec<Option<String>>> {
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            (0..r.len())
                .map(|i| r.try_get::<Option<String>, _>(i).ok().flatten())
                .collect()
        })
        .collect()
}

#[test]
fn the_module_registers_as_identity_with_versions_one_and_two() {
    assert_eq!(MODULE_NAME, "identity");
    let versions: Vec<i64> = SQLITE_MIGRATIONS.iter().map(|(v, _, _)| *v).collect();
    assert_eq!(versions, vec![1, 2]);
    assert_eq!(
        SQLITE_MIGRATIONS[0].1,
        "create credentials and credential_meta"
    );
    assert_eq!(SQLITE_MIGRATIONS[1].1, "jira credential kind");
}

#[tokio::test]
async fn migration_step_2_keeps_every_credential() {
    let pool = empty_database().await;
    apply(&pool, 1).await;
    // Step 1 refuses the kind step 2 adds.
    let refused = sqlx::query(
        "INSERT INTO credentials VALUES ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f79','jira','acme.atlassian.net','credential/jira/x',NULL,'active','2026-01-10T09:00:00.000Z',NULL)",
    )
    .execute(&pool)
    .await;
    assert!(refused.is_err());
    sqlx::raw_sql(
        "INSERT INTO credentials VALUES
           ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f70','github','asharao','credential/github/0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f70','2026-01-10T17:00:00.000Z','active','2026-01-10T09:00:00.000Z',NULL),
           ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f71','confluence','acme.atlassian.net','credential/confluence/0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f71',NULL,'revoked','2026-01-10T09:00:00.000Z','2026-01-10T10:00:00.000Z');
         INSERT INTO credential_meta VALUES
           ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f70','github.com','[\"repo\"]',0,NULL,NULL,'Asha Rao'),
           ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f71','acme.atlassian.net','[]',1,'2026-01-10T11:00:00.000Z','CRED_REFRESH_REFUSED',NULL);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let credentials_before = rows(&pool, "SELECT * FROM credentials ORDER BY id").await;
    let meta_before = rows(
        &pool,
        "SELECT * FROM credential_meta ORDER BY credential_id",
    )
    .await;

    apply(&pool, 2).await;

    assert_eq!(
        rows(&pool, "SELECT * FROM credentials ORDER BY id").await,
        credentials_before
    );
    assert_eq!(
        rows(
            &pool,
            "SELECT * FROM credential_meta ORDER BY credential_id"
        )
        .await,
        meta_before
    );
    assert!(sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap()
        .is_empty());
    // A row of kind jira can now be inserted, with its meta row.
    sqlx::raw_sql(
        "INSERT INTO credentials VALUES ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f72','jira','acme.atlassian.net','credential/jira/0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f72',NULL,'active','2026-01-10T09:00:00.000Z',NULL);
         INSERT INTO credential_meta (credential_id, provider_host) VALUES ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f72','acme.atlassian.net');",
    )
    .execute(&pool)
    .await
    .unwrap();
    // The partial unique index and the cascade survive the rebuild.
    let duplicate = sqlx::query(
        "INSERT INTO credentials VALUES ('0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f73','github','asharao','credential/github/y',NULL,'active','2026-01-10T09:00:00.000Z',NULL)",
    )
    .execute(&pool)
    .await;
    assert!(
        duplicate.is_err(),
        "credentials_active_kind_label is recreated"
    );
    sqlx::query("DELETE FROM credentials WHERE id = '0192a3f0-7c2e-7d10-9a11-2b3c4d5e6f72'")
        .execute(&pool)
        .await
        .unwrap();
    let meta_left: i64 = sqlx::query_scalar("SELECT count(*) FROM credential_meta")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        meta_left, 2,
        "credential_meta still refers to credentials with ON DELETE CASCADE"
    );
}
