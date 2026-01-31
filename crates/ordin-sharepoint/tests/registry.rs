//! X4: every code in `ALL_CODES` is in the error registry and `SP_` is owned by this crate; and the
//! module's migrations (DatabaseMigrations.md): one SQLite step, no PostgreSQL step, tables as the Data
//! Model states them.

mod support;

use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry};
use ordin_sharepoint::{migrations, ALL_CODES};
use ordin_types::error_registry::{lookup, owns_prefix};
use sqlx::Row as _;
use support::*;

#[test]
fn every_code_is_registered_and_owned_by_this_crate() {
    assert_eq!(
        ALL_CODES.len(),
        16,
        "the sixteen SP_ rows of the Error Table"
    );
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-sharepoint", "{code}");
        assert!(code.starts_with("SP_"), "{code}");
    }
    assert!(owns_prefix("ordin-sharepoint", "SP"));
    // The cursor code is raised here and owned by ordin-types.
    assert_eq!(
        lookup("SRV_BAD_CURSOR").map(|e| e.owner_crate),
        Some("ordin-types")
    );
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, ALL_CODES, "kept in registry order");
}

#[test]
fn the_registry_flags_agree_with_the_error_type() {
    // Retryable rows: SP_THROTTLED, SP_UNAVAILABLE, SP_DATASTORE_UNAVAILABLE.
    let retryable: Vec<&str> = ALL_CODES
        .iter()
        .copied()
        .filter(|c| lookup(c).is_some_and(|e| e.retryable))
        .collect();
    assert_eq!(
        retryable,
        ["SP_DATASTORE_UNAVAILABLE", "SP_THROTTLED", "SP_UNAVAILABLE"]
    );
}

#[test]
fn the_module_declares_one_sqlite_step_and_no_postgresql_step() {
    let m = migrations();
    assert_eq!(m.module, "sharepoint");
    assert_eq!(m.sqlite.len(), 1);
    assert_eq!((m.sqlite[0].0, m.sqlite[0].1), (1, "sharepoint tables"));
    assert!(
        m.postgres.is_empty(),
        "the adapter runs on no agent-profile server"
    );
}

#[tokio::test]
async fn the_step_creates_the_three_tables_and_registers_once() {
    let dir = TempDir::new();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.path().join("journal.db"),
        backup_dir: dir.path().join("backups"),
    })
    .await
    .unwrap();
    let mut registry = MigrationRegistry::new();
    registry.register(ordin_journal::MIGRATIONS).unwrap();
    registry.register(migrations()).unwrap();
    assert!(
        registry.register(migrations()).is_err(),
        "JRNL_MIGRATION_DUPLICATE"
    );
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let pool = store.sqlite().unwrap();
    let tables: Vec<String> = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'sharepoint_%' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get::<String, _>(0))
    .collect();
    assert_eq!(
        tables,
        [
            "sharepoint_items",
            "sharepoint_link_state",
            "sharepoint_origin_changes"
        ]
    );
    let steps: i64 =
        sqlx::query("SELECT count(*) FROM schema_migrations WHERE module = 'sharepoint'")
            .fetch_one(pool)
            .await
            .unwrap()
            .get(0);
    assert_eq!(steps, 1);
    // A second apply changes nothing.
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(report.applied.is_empty());
    // The delta link column refuses more than 4,096 characters (Data Model).
    let refused = sqlx::query(
        "INSERT INTO sharepoint_link_state (source_link_id, drive_id, root_item_id, path_prefix, delta_link, tracked_at) \
         VALUES ('l', 'd', 'r', '', ?1, 't')",
    )
    .bind("x".repeat(4097))
    .execute(pool)
    .await;
    assert!(refused.is_err());
}
