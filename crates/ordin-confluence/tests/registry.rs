//! X4: every code in `ALL_CODES` is in the error registry and `CONF` is owned by this crate; and the
//! module's migrations (DatabaseMigrations.md): one SQLite step, no PostgreSQL step, tables as the Data
//! Model states them.

mod support;

use ordin_confluence::{ConfluenceAdapter, ALL_CODES};
use ordin_types::error_registry::{lookup, owns_prefix};
use sqlx::Row as _;
use support::*;

#[test]
fn every_code_is_registered_and_owned_by_this_crate() {
    // 23 after CONF_SITE_INVALID and CONF_TOKEN_REFUSED (CONF-TOKEN-1).
    assert_eq!(ALL_CODES.len(), 23, "the 23 rows of the Error Table");
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-confluence", "{code}");
        assert!(code.starts_with("CONF_"), "{code}");
    }
    assert!(owns_prefix("ordin-confluence", "CONF"));
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
fn the_registry_flags_agree_with_the_error_table() {
    let retryable: Vec<&str> = ALL_CODES
        .iter()
        .copied()
        .filter(|c| lookup(c).is_some_and(|e| e.retryable))
        .collect();
    assert_eq!(
        retryable,
        [
            "CONF_APPROVAL_FAILED",
            "CONF_DATASTORE_UNAVAILABLE",
            "CONF_FILES_FAILED",
            "CONF_THROTTLED",
            "CONF_UNAVAILABLE"
        ]
    );
}

#[test]
fn the_module_declares_two_sqlite_steps_and_no_postgresql_step() {
    let m = ConfluenceAdapter::migrations();
    assert_eq!(m.module, "confluence");
    // Step 2 is ConfluenceAdapter.md amendment 2026-01-24 (Data Model, migration step 2).
    assert_eq!(m.sqlite.len(), 2);
    assert_eq!((m.sqlite[0].0, m.sqlite[0].1), (1, "confluence tables"));
    assert_eq!(
        (m.sqlite[1].0, m.sqlite[1].1),
        (2, "confluence ready items changed")
    );
    assert!(m.postgres.is_empty(), "the adapter runs on no server");
}

#[tokio::test]
async fn the_migration_creates_the_six_tables_and_the_index() {
    let env = Env::new().await;
    let rows =
        sqlx::query("SELECT name FROM sqlite_master WHERE name LIKE 'confluence%' ORDER BY name")
            .fetch_all(&env.pool)
            .await
            .unwrap();
    let names: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    for wanted in [
        "confluence_folder_pages",
        "confluence_items",
        "confluence_link_state",
        "confluence_origin_changes",
        "confluence_origin_changes_found",
        "confluence_publish_requests",
        "confluence_ready_items",
    ] {
        assert!(names.iter().any(|n| n == wanted), "{wanted}: {names:?}");
    }
}
