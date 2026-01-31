//! This crate's three tables in the Journal's SQLite datastore (Data Model). Every statement is
//! parameterised; no SQL text is built from a value (SA-DB-01).

use std::collections::HashMap;

use ordin_types::time::Timestamp;
use ordin_types::SourceLinkId;
use sqlx::{Row as _, SqliteConnection, SqlitePool};

use crate::error::SharePointError;
use crate::types::{OriginChange, SharePointVersion};
use crate::wire::parse_time;

/// Origin changes older than this are deleted at each `check_changes` (Internal Logic 4.6).
pub(crate) const CHANGE_RETENTION_DAYS: i64 = 90;
/// `changes_since` returns at most this many rows (Internal Logic 4.5).
const CHANGES_SINCE_LIMIT: i64 = 1000;

#[derive(Debug, Clone)]
pub(crate) struct LinkState {
    pub drive_id: String,
    pub root_item_id: String,
    pub path_prefix: String,
    pub delta_link: String,
}

/// One row of `sharepoint_items`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ItemRow {
    pub item_id: String,
    pub origin_path: String,
    pub ctag: String,
    pub etag: String,
    pub version_label: Option<String>,
    pub changed_at: Timestamp,
    pub changed_by: Option<String>,
    pub removed: bool,
}

pub(crate) async fn load_state(
    pool: &SqlitePool,
    link: SourceLinkId,
) -> Result<Option<LinkState>, SharePointError> {
    let row = sqlx::query(
        "SELECT drive_id, root_item_id, path_prefix, delta_link \
         FROM sharepoint_link_state WHERE source_link_id = ?1",
    )
    .bind(link.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| LinkState {
        drive_id: r.get(0),
        root_item_id: r.get(1),
        path_prefix: r.get(2),
        delta_link: r.get(3),
    }))
}

pub(crate) async fn load_items(
    pool: &SqlitePool,
    link: SourceLinkId,
) -> Result<HashMap<String, ItemRow>, SharePointError> {
    let rows = sqlx::query(
        "SELECT item_id, origin_path, ctag, etag, version_label, changed_at, changed_by, removed \
         FROM sharepoint_items WHERE source_link_id = ?1",
    )
    .bind(link.to_string())
    .fetch_all(pool)
    .await?;
    let mut items = HashMap::with_capacity(rows.len());
    for row in rows {
        let changed_at: String = row.get(5);
        let item = ItemRow {
            item_id: row.get(0),
            origin_path: row.get(1),
            ctag: row.get(2),
            etag: row.get(3),
            version_label: row.get(4),
            changed_at: parse_time(&changed_at)?,
            changed_by: row.get(6),
            removed: row.get::<i64, _>(7) != 0,
        };
        items.insert(item.item_id.clone(), item);
    }
    Ok(items)
}

pub(crate) async fn upsert_item(
    conn: &mut SqliteConnection,
    link: SourceLinkId,
    item: &ItemRow,
) -> Result<(), SharePointError> {
    sqlx::query(
        "INSERT INTO sharepoint_items \
         (source_link_id, item_id, origin_path, ctag, etag, version_label, changed_at, changed_by, removed) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT (source_link_id, item_id) DO UPDATE SET \
         origin_path = excluded.origin_path, ctag = excluded.ctag, etag = excluded.etag, \
         version_label = excluded.version_label, changed_at = excluded.changed_at, \
         changed_by = excluded.changed_by, removed = excluded.removed",
    )
    .bind(link.to_string())
    .bind(&item.item_id)
    .bind(&item.origin_path)
    .bind(&item.ctag)
    .bind(&item.etag)
    .bind(&item.version_label)
    .bind(item.changed_at.to_wire())
    .bind(&item.changed_by)
    .bind(i64::from(item.removed))
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn insert_change(
    conn: &mut SqliteConnection,
    change: &OriginChange,
    found_at: Timestamp,
) -> Result<(), SharePointError> {
    sqlx::query(
        "INSERT INTO sharepoint_origin_changes \
         (id, source_link_id, origin_path, new_version, changed_by, changed_at, removed, found_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(uuid::Uuid::now_v7().hyphenated().to_string())
    .bind(change.source_link_id.to_string())
    .bind(&change.origin_path)
    .bind(&change.new_version)
    .bind(&change.changed_by)
    .bind(change.changed_at.to_wire())
    .bind(i64::from(change.removed))
    .bind(found_at.to_wire())
    .execute(conn)
    .await?;
    Ok(())
}

/// Stores the link's state, replaces its item rows with `rows` and inserts `changes`, in one transaction
/// (Internal Logic 4.1 and 4.3).
pub(crate) async fn replace_tracking(
    pool: &SqlitePool,
    link: SourceLinkId,
    state: &LinkState,
    rows: &[ItemRow],
    changes: &[OriginChange],
    now: Timestamp,
    checked: bool,
) -> Result<(), SharePointError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO sharepoint_link_state \
         (source_link_id, drive_id, root_item_id, path_prefix, delta_link, tracked_at, last_checked_at, last_error) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL) \
         ON CONFLICT (source_link_id) DO UPDATE SET \
         drive_id = excluded.drive_id, root_item_id = excluded.root_item_id, \
         path_prefix = excluded.path_prefix, delta_link = excluded.delta_link, \
         tracked_at = excluded.tracked_at, last_checked_at = excluded.last_checked_at, last_error = NULL",
    )
    .bind(link.to_string())
    .bind(&state.drive_id)
    .bind(&state.root_item_id)
    .bind(&state.path_prefix)
    .bind(&state.delta_link)
    .bind(now.to_wire())
    .bind(checked.then(|| now.to_wire()))
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM sharepoint_items WHERE source_link_id = ?1")
        .bind(link.to_string())
        .execute(&mut *tx)
        .await?;
    for row in rows {
        upsert_item(&mut tx, link, row).await?;
    }
    for change in changes {
        insert_change(&mut tx, change, now).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Internal Logic 4.2: the new delta link, the item rows that changed and the change rows, together.
pub(crate) async fn commit_check(
    pool: &SqlitePool,
    link: SourceLinkId,
    delta_link: &str,
    upserts: &[ItemRow],
    changes: &[OriginChange],
    now: Timestamp,
) -> Result<(), SharePointError> {
    let mut tx = pool.begin().await?;
    for row in upserts {
        upsert_item(&mut tx, link, row).await?;
    }
    for change in changes {
        insert_change(&mut tx, change, now).await?;
    }
    sqlx::query(
        "UPDATE sharepoint_link_state SET delta_link = ?2, last_checked_at = ?3, last_error = NULL \
         WHERE source_link_id = ?1",
    )
    .bind(link.to_string())
    .bind(delta_link)
    .bind(now.to_wire())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The error code only, never a message (Data Model).
pub(crate) async fn set_error(pool: &SqlitePool, link: SourceLinkId, code: &str) {
    let _ =
        sqlx::query("UPDATE sharepoint_link_state SET last_error = ?2 WHERE source_link_id = ?1")
            .bind(link.to_string())
            .bind(code)
            .execute(pool)
            .await;
}

pub(crate) async fn delete_link(
    pool: &SqlitePool,
    link: SourceLinkId,
) -> Result<(), SharePointError> {
    sqlx::query("DELETE FROM sharepoint_link_state WHERE source_link_id = ?1")
        .bind(link.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn purge_old_changes(
    pool: &SqlitePool,
    now: Timestamp,
) -> Result<(), SharePointError> {
    let cutoff = Timestamp(now.0 - chrono::Duration::days(CHANGE_RETENTION_DAYS));
    sqlx::query("DELETE FROM sharepoint_origin_changes WHERE found_at < ?1")
        .bind(cutoff.to_wire())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn changes_since(
    pool: &SqlitePool,
    links: &[SourceLinkId],
    since: Timestamp,
) -> Result<Vec<OriginChange>, SharePointError> {
    if links.is_empty() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for link in links {
        rows.extend(
            sqlx::query(
                "SELECT source_link_id, origin_path, new_version, changed_by, changed_at, removed, found_at \
                 FROM sharepoint_origin_changes WHERE source_link_id = ?1 AND found_at > ?2 \
                 ORDER BY found_at DESC, id DESC LIMIT ?3",
            )
            .bind(link.to_string())
            .bind(since.to_wire())
            .bind(CHANGES_SINCE_LIMIT)
            .fetch_all(pool)
            .await?,
        );
    }
    // Newest first across the links, at most 1,000 in all.
    rows.sort_by_key(|row| std::cmp::Reverse(row.get::<String, _>(6)));
    rows.truncate(CHANGES_SINCE_LIMIT as usize);
    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        let link: String = row.get(0);
        let changed_at: String = row.get(4);
        changes.push(OriginChange {
            source_link_id: SourceLinkId::parse(&link)
                .map_err(|_| SharePointError::DatastoreUnavailable)?,
            origin_path: row.get(1),
            new_version: row.get(2),
            changed_by: row.get(3),
            changed_at: parse_time(&changed_at)?,
            removed: row.get::<i64, _>(5) != 0,
        });
    }
    Ok(changes)
}

/// Step 6.5: ordin's own push is recorded so the next `check_changes` does not report it.
pub(crate) async fn update_item_tags(
    pool: &SqlitePool,
    item_id: &str,
    version: &SharePointVersion,
) -> Result<(), SharePointError> {
    sqlx::query(
        "UPDATE sharepoint_items SET ctag = ?2, etag = ?3, version_label = ?4, changed_at = ?5, \
         changed_by = ?6, removed = 0 WHERE item_id = ?1",
    )
    .bind(item_id)
    .bind(&version.tag)
    .bind(&version.etag)
    .bind(&version.label)
    .bind(version.changed_at.to_wire())
    .bind(&version.changed_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Step 8.3: ordin's own removal is not reported as a change.
pub(crate) async fn delete_item_rows(
    pool: &SqlitePool,
    item_id: &str,
) -> Result<(), SharePointError> {
    sqlx::query("DELETE FROM sharepoint_items WHERE item_id = ?1")
        .bind(item_id)
        .execute(pool)
        .await?;
    Ok(())
}
