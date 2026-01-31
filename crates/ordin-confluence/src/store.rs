//! This crate's six tables in the Journal's SQLite datastore (Data Model). Every statement is
//! parameterised; no SQL text is built from a value (SA-DB-01).

use std::collections::HashMap;

use ordin_types::time::Timestamp;
use ordin_types::{ApprovalId, ConfluenceLinkId, PublishedCopyId, SourceLinkId};
use serde::{Deserialize, Serialize};
use sqlx::{Row as _, SqliteConnection, SqlitePool};

use crate::error::ConfluenceError;
use crate::types::{OriginChange, ReadyItem, ReadyKind, RemoteVersion};
use crate::wire::parse_time;

/// Origin changes older than this are deleted at each check (Internal Logic 3.3).
const CHANGE_RETENTION_DAYS: i64 = 90;
/// `changes_since` returns at most this many rows (Internal Logic 3.3).
const CHANGES_SINCE_LIMIT: i64 = 1000;

#[derive(Debug, Clone)]
pub(crate) struct LinkState {
    pub space_key: String,
    pub root_page_id: Option<String>,
    pub last_checked_at: Timestamp,
}

/// One row of `confluence_items`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ItemRow {
    pub item_key: String,
    pub origin_path: String,
    pub version_number: i64,
    pub changed_at: Timestamp,
    pub changed_by: Option<String>,
    pub removed: bool,
}

/// One item of a card, bound to the bytes the card was made from (Internal Logic 6.1). The hash is this
/// crate's own record, never taken from the caller's `ReadyItem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ApprovedItem {
    pub item: ReadyItem,
    /// SHA-256 hex of the file when the card was made; `None` for `Removed`, which sends no content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// The card said an agent may have written these bytes, so approving it covers them (step 6.1).
    #[serde(default)]
    pub agent_written: bool,
}

/// A row of `confluence_publish_requests`.
#[derive(Debug, Clone)]
pub(crate) struct PublishRequest {
    pub id: String,
    pub items: Vec<ApprovedItem>,
}

/// A stored time is written by this crate, so one that does not parse is a datastore fault.
fn stored_time(raw: &str) -> Result<Timestamp, ConfluenceError> {
    parse_time(raw).map_err(|_| ConfluenceError::DatastoreUnavailable)
}

pub(crate) async fn load_state(
    pool: &SqlitePool,
    link: SourceLinkId,
) -> Result<Option<LinkState>, ConfluenceError> {
    let row = sqlx::query(
        "SELECT space_key, root_page_id, last_checked_at \
         FROM confluence_link_state WHERE source_link_id = ?1",
    )
    .bind(link.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        let checked: String = r.get(2);
        Ok(LinkState {
            space_key: r.get(0),
            root_page_id: r.get(1),
            last_checked_at: stored_time(&checked)?,
        })
    })
    .transpose()
}

pub(crate) async fn load_items(
    pool: &SqlitePool,
    link: SourceLinkId,
) -> Result<HashMap<String, ItemRow>, ConfluenceError> {
    let rows = sqlx::query(
        "SELECT item_key, origin_path, version_number, changed_at, changed_by, removed \
         FROM confluence_items WHERE source_link_id = ?1",
    )
    .bind(link.to_string())
    .fetch_all(pool)
    .await?;
    let mut items = HashMap::with_capacity(rows.len());
    for row in rows {
        let changed_at: String = row.get(3);
        let item = ItemRow {
            item_key: row.get(0),
            origin_path: row.get(1),
            version_number: row.get(2),
            changed_at: stored_time(&changed_at)?,
            changed_by: row.get(4),
            removed: row.get::<i64, _>(5) != 0,
        };
        items.insert(item.item_key.clone(), item);
    }
    Ok(items)
}

async fn upsert_item(
    conn: &mut SqliteConnection,
    link: SourceLinkId,
    item: &ItemRow,
) -> Result<(), ConfluenceError> {
    sqlx::query(
        "INSERT INTO confluence_items \
         (source_link_id, item_key, origin_path, version_number, changed_at, changed_by, removed) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT (source_link_id, item_key) DO UPDATE SET \
         origin_path = excluded.origin_path, version_number = excluded.version_number, \
         changed_at = excluded.changed_at, changed_by = excluded.changed_by, \
         removed = excluded.removed",
    )
    .bind(link.to_string())
    .bind(&item.item_key)
    .bind(&item.origin_path)
    .bind(item.version_number)
    .bind(item.changed_at.to_wire())
    .bind(&item.changed_by)
    .bind(i64::from(item.removed))
    .execute(conn)
    .await?;
    Ok(())
}

/// Step 3.1: the link's state and its item rows, replacing any earlier ones, in one transaction.
pub(crate) async fn replace_tracking(
    pool: &SqlitePool,
    link: SourceLinkId,
    state: &LinkState,
    rows: &[ItemRow],
    now: Timestamp,
) -> Result<(), ConfluenceError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO confluence_link_state \
         (source_link_id, space_key, root_page_id, tracked_at, last_checked_at, last_error) \
         VALUES (?1, ?2, ?3, ?4, ?5, NULL) \
         ON CONFLICT (source_link_id) DO UPDATE SET \
         space_key = excluded.space_key, root_page_id = excluded.root_page_id, \
         tracked_at = excluded.tracked_at, last_checked_at = excluded.last_checked_at, \
         last_error = NULL",
    )
    .bind(link.to_string())
    .bind(&state.space_key)
    .bind(&state.root_page_id)
    .bind(now.to_wire())
    .bind(state.last_checked_at.to_wire())
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM confluence_items WHERE source_link_id = ?1")
        .bind(link.to_string())
        .execute(&mut *tx)
        .await?;
    for row in rows {
        upsert_item(&mut tx, link, row).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Step 3.2: the item rows that changed, the change rows and `last_checked_at`, together. `found_at` is
/// the time of this commit, so a reader asking "since t" cannot miss rows a running check adds after t.
pub(crate) async fn commit_check(
    pool: &SqlitePool,
    link: SourceLinkId,
    upserts: &[ItemRow],
    changes: &[OriginChange],
    now: Timestamp,
) -> Result<(), ConfluenceError> {
    let found_at = Timestamp::now();
    let mut tx = pool.begin().await?;
    for row in upserts {
        upsert_item(&mut tx, link, row).await?;
    }
    for change in changes {
        sqlx::query(
            "INSERT INTO confluence_origin_changes \
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
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE confluence_link_state SET last_checked_at = ?2, last_error = NULL \
         WHERE source_link_id = ?1",
    )
    .bind(link.to_string())
    .bind(now.to_wire())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The error code only, never a message.
pub(crate) async fn set_error(pool: &SqlitePool, link: SourceLinkId, code: &str) {
    let _ =
        sqlx::query("UPDATE confluence_link_state SET last_error = ?2 WHERE source_link_id = ?1")
            .bind(link.to_string())
            .bind(code)
            .execute(pool)
            .await;
}

pub(crate) async fn delete_link(
    pool: &SqlitePool,
    link: SourceLinkId,
) -> Result<(), ConfluenceError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM confluence_origin_changes WHERE source_link_id = ?1")
        .bind(link.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM confluence_items WHERE source_link_id = ?1")
        .bind(link.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM confluence_link_state WHERE source_link_id = ?1")
        .bind(link.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn purge_old_changes(
    pool: &SqlitePool,
    now: Timestamp,
) -> Result<(), ConfluenceError> {
    let cutoff = Timestamp(now.0 - chrono::Duration::days(CHANGE_RETENTION_DAYS));
    sqlx::query("DELETE FROM confluence_origin_changes WHERE found_at < ?1")
        .bind(cutoff.to_wire())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn changes_since(
    pool: &SqlitePool,
    links: &[SourceLinkId],
    since: Timestamp,
) -> Result<Vec<OriginChange>, ConfluenceError> {
    let mut rows = Vec::new();
    for link in links {
        rows.extend(
            sqlx::query(
                "SELECT source_link_id, origin_path, new_version, changed_by, changed_at, removed, found_at \
                 FROM confluence_origin_changes WHERE source_link_id = ?1 AND found_at > ?2 \
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
                .map_err(|_| ConfluenceError::DatastoreUnavailable)?,
            origin_path: row.get(1),
            new_version: row.get(2),
            changed_by: row.get(3),
            changed_at: stored_time(&changed_at)?,
            removed: row.get::<i64, _>(5) != 0,
        });
    }
    Ok(changes)
}

/// The tracked path of an item, in whichever link holds it.
pub(crate) async fn known_path(
    pool: &SqlitePool,
    item_key: &str,
) -> Result<Option<String>, ConfluenceError> {
    let row = sqlx::query(
        "SELECT origin_path FROM confluence_items WHERE item_key = ?1 AND removed = 0 LIMIT 1",
    )
    .bind(item_key)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.get(0)))
}

/// Step 3.4: the live row at `origin_path` of a link.
pub(crate) async fn item_at_path(
    pool: &SqlitePool,
    link: SourceLinkId,
    origin_path: &str,
) -> Result<Option<String>, ConfluenceError> {
    let row = sqlx::query(
        "SELECT item_key FROM confluence_items \
         WHERE source_link_id = ?1 AND origin_path = ?2 AND removed = 0",
    )
    .bind(link.to_string())
    .bind(origin_path)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.get(0)))
}

/// Step 7.6: ordin's own push is recorded so the next `check_changes` does not report it.
pub(crate) async fn update_item_version(
    pool: &SqlitePool,
    item_key: &str,
    number: i64,
    version: &RemoteVersion,
) -> Result<(), ConfluenceError> {
    sqlx::query(
        "UPDATE confluence_items SET version_number = ?2, changed_at = ?3, changed_by = ?4, \
         removed = 0 WHERE item_key = ?1",
    )
    .bind(item_key)
    .bind(number)
    .bind(version.changed_at.to_wire())
    .bind(&version.changed_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Step 7.7: ordin's own removal is not reported as a change.
pub(crate) async fn delete_item(pool: &SqlitePool, item_key: &str) -> Result<(), ConfluenceError> {
    sqlx::query("DELETE FROM confluence_items WHERE item_key = ?1")
        .bind(item_key)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn folder_page(
    pool: &SqlitePool,
    link: ConfluenceLinkId,
    folder_path: &str,
) -> Result<Option<String>, ConfluenceError> {
    let row = sqlx::query(
        "SELECT page_id FROM confluence_folder_pages \
         WHERE confluence_link_id = ?1 AND folder_path = ?2",
    )
    .bind(link.to_string())
    .bind(folder_path)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.get(0)))
}

pub(crate) async fn insert_folder_page(
    pool: &SqlitePool,
    link: ConfluenceLinkId,
    folder_path: &str,
    page_id: &str,
) -> Result<(), ConfluenceError> {
    sqlx::query(
        "INSERT INTO confluence_folder_pages (confluence_link_id, folder_path, page_id) \
         VALUES (?1, ?2, ?3) \
         ON CONFLICT (confluence_link_id, folder_path) DO UPDATE SET page_id = excluded.page_id",
    )
    .bind(link.to_string())
    .bind(folder_path)
    .bind(page_id)
    .execute(pool)
    .await?;
    Ok(())
}

fn kind_word(kind: ReadyKind) -> &'static str {
    match kind {
        ReadyKind::New => "new",
        ReadyKind::Renamed => "renamed",
        ReadyKind::Removed => "removed",
        ReadyKind::Changed => "changed",
    }
}

/// Step 5.4: replaces the link's Ready list.
pub(crate) async fn replace_ready(
    pool: &SqlitePool,
    link: ConfluenceLinkId,
    items: &[ReadyItem],
    now: Timestamp,
) -> Result<(), ConfluenceError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM confluence_ready_items WHERE confluence_link_id = ?1")
        .bind(link.to_string())
        .execute(&mut *tx)
        .await?;
    for item in items {
        sqlx::query(
            "INSERT INTO confluence_ready_items \
             (confluence_link_id, kind, path, from_path, published_copy_id, found_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(link.to_string())
        .bind(kind_word(item.kind))
        .bind(&item.path)
        .bind(&item.from_path)
        .bind(item.published_copy_id.map(|id| id.to_string()))
        .bind(now.to_wire())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn load_ready(
    pool: &SqlitePool,
    link: ConfluenceLinkId,
) -> Result<Vec<ReadyItem>, ConfluenceError> {
    let rows = sqlx::query(
        "SELECT kind, path, from_path, published_copy_id FROM confluence_ready_items \
         WHERE confluence_link_id = ?1 ORDER BY path",
    )
    .bind(link.to_string())
    .fetch_all(pool)
    .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let kind: String = row.get(0);
        let copy: Option<String> = row.get(3);
        items.push(ReadyItem {
            confluence_link_id: link,
            kind: match kind.as_str() {
                "new" => ReadyKind::New,
                "renamed" => ReadyKind::Renamed,
                "removed" => ReadyKind::Removed,
                "changed" => ReadyKind::Changed,
                _ => return Err(ConfluenceError::DatastoreUnavailable),
            },
            path: row.get(1),
            from_path: row.get(2),
            published_copy_id: copy
                .as_deref()
                .map(PublishedCopyId::parse)
                .transpose()
                .map_err(|_| ConfluenceError::DatastoreUnavailable)?,
        });
    }
    Ok(items)
}

pub(crate) async fn delete_ready_item(
    pool: &SqlitePool,
    link: ConfluenceLinkId,
    path: &str,
) -> Result<(), ConfluenceError> {
    sqlx::query("DELETE FROM confluence_ready_items WHERE confluence_link_id = ?1 AND path = ?2")
        .bind(link.to_string())
        .bind(path)
        .execute(pool)
        .await?;
    Ok(())
}

// spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-05
pub(crate) async fn insert_publish_request(
    pool: &SqlitePool,
    link: ConfluenceLinkId,
    approval: ApprovalId,
    items: &[ApprovedItem],
    now: Timestamp,
) -> Result<(), ConfluenceError> {
    let json = serde_json::to_string(items).map_err(|_| ConfluenceError::DatastoreUnavailable)?;
    sqlx::query(
        "INSERT INTO confluence_publish_requests (id, confluence_link_id, approval_id, items, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(uuid::Uuid::now_v7().hyphenated().to_string())
    .bind(link.to_string())
    .bind(approval.to_string())
    .bind(json)
    .bind(now.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn publish_request(
    pool: &SqlitePool,
    approval: ApprovalId,
) -> Result<Option<PublishRequest>, ConfluenceError> {
    let row =
        sqlx::query("SELECT id, items FROM confluence_publish_requests WHERE approval_id = ?1")
            .bind(approval.to_string())
            .fetch_optional(pool)
            .await?;
    row.map(|r| {
        let items: String = r.get(1);
        Ok(PublishRequest {
            id: r.get(0),
            items: serde_json::from_str(&items)
                .map_err(|_| ConfluenceError::DatastoreUnavailable)?,
        })
    })
    .transpose()
}

pub(crate) async fn delete_publish_request(
    pool: &SqlitePool,
    id: &str,
) -> Result<(), ConfluenceError> {
    sqlx::query("DELETE FROM confluence_publish_requests WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// `forget_link`: the link's rows of the three linked-folder tables.
pub(crate) async fn forget_link(
    pool: &SqlitePool,
    link: ConfluenceLinkId,
) -> Result<(), ConfluenceError> {
    let id = link.to_string();
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM confluence_folder_pages WHERE confluence_link_id = ?1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM confluence_ready_items WHERE confluence_link_id = ?1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM confluence_publish_requests WHERE confluence_link_id = ?1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
