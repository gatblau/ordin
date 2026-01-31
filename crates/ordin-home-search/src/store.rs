//! The module's own tables (HomeAndSearch.md *Data Model*), reached through the Journal's SQLite
//! datastore (SA-RECON-01). Every statement binds its values; nothing is built from input.

use std::collections::{HashMap, HashSet};

use ordin_types::domain::places::ChangedMark;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{ChangedMarkId, PlaceId, WorkspaceId};
use sqlx::{Row, SqlitePool};

use crate::error::HomeError;

fn time(raw: &str) -> Result<Timestamp, HomeError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| HomeError::DatastoreUnavailable)
}

fn id<T: std::str::FromStr>(raw: &str) -> Result<T, HomeError> {
    raw.parse().map_err(|_| HomeError::DatastoreUnavailable)
}

pub(crate) async fn home_last_opened(pool: &SqlitePool) -> Result<Option<Timestamp>, HomeError> {
    let row = sqlx::query("SELECT home_last_opened_at FROM home_marks WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    match row.and_then(|r| r.get::<Option<String>, _>(0)) {
        Some(raw) => Ok(Some(time(&raw)?)),
        None => Ok(None),
    }
}

pub(crate) async fn set_home_opened(pool: &SqlitePool, at: Timestamp) -> Result<(), HomeError> {
    sqlx::query(
        "INSERT INTO home_marks (id, home_last_opened_at) VALUES (1, ?1) \
         ON CONFLICT (id) DO UPDATE SET home_last_opened_at = excluded.home_last_opened_at",
    )
    .bind(at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_last_seen(
    pool: &SqlitePool,
    workspace: WorkspaceId,
    at: Timestamp,
) -> Result<(), HomeError> {
    sqlx::query(
        "INSERT INTO workspace_last_seen (workspace_id, last_seen_at) VALUES (?1, ?2) \
         ON CONFLICT (workspace_id) DO UPDATE SET last_seen_at = excluded.last_seen_at",
    )
    .bind(workspace.to_string())
    .bind(at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn last_seen_all(
    pool: &SqlitePool,
) -> Result<HashMap<WorkspaceId, Timestamp>, HomeError> {
    let rows = sqlx::query("SELECT workspace_id, last_seen_at FROM workspace_last_seen")
        .fetch_all(pool)
        .await?;
    let mut out = HashMap::new();
    for row in rows {
        let ws: String = row.get(0);
        let at: String = row.get(1);
        out.insert(id(&ws)?, time(&at)?);
    }
    Ok(out)
}

/// Upserts the mark of (workspace, place, path): a second change absorbs into the first row
/// (`HOME_MARK_DUPLICATE` is absorbed by upsert, Error Table). True when a row was added.
pub(crate) async fn upsert_mark(
    pool: &SqlitePool,
    workspace: WorkspaceId,
    place: PlaceId,
    path: &str,
    changed_at: Timestamp,
) -> Result<bool, HomeError> {
    let existed = sqlx::query(
        "SELECT 1 FROM changed_marks WHERE workspace_id = ?1 AND place_id = ?2 AND path = ?3",
    )
    .bind(workspace.to_string())
    .bind(place.to_string())
    .bind(path)
    .fetch_optional(pool)
    .await?
    .is_some();
    sqlx::query(
        "INSERT INTO changed_marks (id, workspace_id, place_id, path, changed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (workspace_id, place_id, path) DO UPDATE SET changed_at = excluded.changed_at",
    )
    .bind(ChangedMarkId::new().to_string())
    .bind(workspace.to_string())
    .bind(place.to_string())
    .bind(path)
    .bind(changed_at.to_wire())
    .execute(pool)
    .await?;
    Ok(!existed)
}

/// Deletes the mark of (place, path) in one workspace, or in every workspace when `workspace` is
/// `None`. Returns the number of rows removed.
pub(crate) async fn delete_mark(
    pool: &SqlitePool,
    workspace: Option<WorkspaceId>,
    place: PlaceId,
    path: &str,
) -> Result<u64, HomeError> {
    let done =
        match workspace {
            Some(ws) => sqlx::query(
                "DELETE FROM changed_marks WHERE workspace_id = ?1 AND place_id = ?2 AND path = ?3",
            )
            .bind(ws.to_string())
            .bind(place.to_string())
            .bind(path)
            .execute(pool)
            .await?,
            None => {
                sqlx::query("DELETE FROM changed_marks WHERE place_id = ?1 AND path = ?2")
                    .bind(place.to_string())
                    .bind(path)
                    .execute(pool)
                    .await?
            }
        };
    Ok(done.rows_affected())
}

fn mark_row(row: &sqlx::sqlite::SqliteRow) -> Result<ChangedMark, HomeError> {
    let mark_id: String = row.get("id");
    let ws: String = row.get("workspace_id");
    let place: String = row.get("place_id");
    let at: String = row.get("changed_at");
    Ok(ChangedMark {
        id: id(&mark_id)?,
        workspace_id: id(&ws)?,
        place_id: id(&place)?,
        path: row.get("path"),
        changed_at: time(&at)?,
    })
}

/// The marks of one workspace, newest first.
pub(crate) async fn marks_of(
    pool: &SqlitePool,
    workspace: WorkspaceId,
) -> Result<Vec<ChangedMark>, HomeError> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, place_id, path, changed_at FROM changed_marks \
         WHERE workspace_id = ?1 ORDER BY changed_at DESC, path",
    )
    .bind(workspace.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter().map(mark_row).collect()
}

/// Every mark changed after `since`, newest first.
pub(crate) async fn marks_since(
    pool: &SqlitePool,
    since: Timestamp,
) -> Result<Vec<ChangedMark>, HomeError> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, place_id, path, changed_at FROM changed_marks \
         WHERE changed_at > ?1 ORDER BY changed_at DESC, path",
    )
    .bind(since.to_wire())
    .fetch_all(pool)
    .await?;
    rows.iter().map(mark_row).collect()
}

/// Deletes the marks and the last-seen row of every workspace not in `listed` (Internal Logic 3.5).
/// Returns the number of marks removed.
pub(crate) async fn forget_unlisted(
    pool: &SqlitePool,
    listed: &HashSet<WorkspaceId>,
) -> Result<u64, HomeError> {
    let rows = sqlx::query(
        "SELECT DISTINCT workspace_id FROM changed_marks \
         UNION SELECT workspace_id FROM workspace_last_seen",
    )
    .fetch_all(pool)
    .await?;
    let mut removed = 0;
    for row in rows {
        let raw: String = row.get(0);
        let known = raw
            .parse::<WorkspaceId>()
            .map(|ws| listed.contains(&ws))
            .unwrap_or(false);
        if known {
            continue;
        }
        removed += sqlx::query("DELETE FROM changed_marks WHERE workspace_id = ?1")
            .bind(&raw)
            .execute(pool)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM workspace_last_seen WHERE workspace_id = ?1")
            .bind(&raw)
            .execute(pool)
            .await?;
    }
    Ok(removed)
}

pub(crate) async fn last_poll(pool: &SqlitePool) -> Result<Option<Timestamp>, HomeError> {
    let row = sqlx::query("SELECT last_poll_at FROM remote_mark_poll WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    match row {
        Some(row) => Ok(Some(time(&row.get::<String, _>(0))?)),
        None => Ok(None),
    }
}

pub(crate) async fn set_last_poll(pool: &SqlitePool, at: Timestamp) -> Result<(), HomeError> {
    sqlx::query(
        "INSERT INTO remote_mark_poll (id, last_poll_at) VALUES (1, ?1) \
         ON CONFLICT (id) DO UPDATE SET last_poll_at = excluded.last_poll_at",
    )
    .bind(at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}
