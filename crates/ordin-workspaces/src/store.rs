//! Row mapping and the statements shared by several modules (WorkspaceService.md *Data Model*).
//! Every statement runs on the Journal's SQLite pool (SA-RECON-03); a failure is
//! `WS_DATASTORE_UNAVAILABLE`.

use ordin_types::domain::places::{
    SourceLink, SourceLinkKind, SourceLinkState, WorkingCopyItem, WorkingCopyState, Workspace,
    WorkspaceState,
};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{PlaceId, SourceLinkId, WorkingCopyItemId, WorkspaceId};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::error::WorkspaceError;

/// The snake_case text a value has on the wire and in the tables.
pub(crate) fn to_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

/// The value a stored snake_case text names; an unknown text is a damaged row.
pub(crate) fn from_text<T: DeserializeOwned>(text: &str) -> Result<T, WorkspaceError> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|_| WorkspaceError::DatastoreUnavailable)
}

pub(crate) fn parse_ts(text: &str) -> Result<Timestamp, WorkspaceError> {
    parse_rfc3339_ms_utc(text)
        .map(Timestamp)
        .map_err(|_| WorkspaceError::DatastoreUnavailable)
}

pub(crate) fn ts_opt(row: &SqliteRow, column: &str) -> Result<Option<Timestamp>, WorkspaceError> {
    let text: Option<String> = row.try_get(column)?;
    text.as_deref().map(parse_ts).transpose()
}

pub(crate) fn ts_req(row: &SqliteRow, column: &str) -> Result<Timestamp, WorkspaceError> {
    let text: String = row.try_get(column)?;
    parse_ts(&text)
}

pub(crate) fn id_of<T: std::str::FromStr>(
    row: &SqliteRow,
    column: &str,
) -> Result<T, WorkspaceError> {
    let text: String = row.try_get(column)?;
    text.parse::<T>()
        .map_err(|_| WorkspaceError::DatastoreUnavailable)
}

pub(crate) fn id_opt<T: std::str::FromStr>(
    row: &SqliteRow,
    column: &str,
) -> Result<Option<T>, WorkspaceError> {
    let text: Option<String> = row.try_get(column)?;
    text.map(|t| {
        t.parse::<T>()
            .map_err(|_| WorkspaceError::DatastoreUnavailable)
    })
    .transpose()
}

/// A workspace row; `WS_BIN_MISMATCH` when `moved_to_bin_at` and the state disagree (2C).
pub(crate) fn workspace_row(row: &SqliteRow) -> Result<Workspace, WorkspaceError> {
    let state: WorkspaceState = from_text(&row.try_get::<String, _>("state")?)?;
    let moved_to_bin_at = ts_opt(row, "moved_to_bin_at")?;
    if (state == WorkspaceState::InBin) != moved_to_bin_at.is_some() {
        return Err(WorkspaceError::Invalid("WS_BIN_MISMATCH"));
    }
    Ok(Workspace {
        id: id_of(row, "id")?,
        name: row.try_get("name")?,
        own_folder_place_id: id_of(row, "own_folder_place_id")?,
        state,
        // B7 contract D-9: Home and Search fills it.
        last_seen_at: None,
        moved_to_bin_at,
        created_at: ts_req(row, "created_at")?,
    })
}

pub(crate) fn link_row(row: &SqliteRow) -> Result<SourceLink, WorkspaceError> {
    Ok(SourceLink {
        id: id_of(row, "id")?,
        workspace_id: id_of(row, "workspace_id")?,
        kind: from_text::<SourceLinkKind>(&row.try_get::<String, _>("kind")?)?,
        origin_location: row.try_get("origin_location")?,
        display_name: row.try_get("display_name")?,
        place_id: id_opt::<PlaceId>(row, "place_id")?,
        state: from_text::<SourceLinkState>(&row.try_get::<String, _>("state")?)?,
        added_at: ts_req(row, "added_at")?,
        last_read_at: ts_opt(row, "last_read_at")?,
    })
}

pub(crate) fn item_row(row: &SqliteRow) -> Result<WorkingCopyItem, WorkspaceError> {
    let version: i64 = row.try_get("local_version")?;
    Ok(WorkingCopyItem {
        id: id_of(row, "id")?,
        source_link_id: id_of(row, "source_link_id")?,
        origin_path: row.try_get("origin_path")?,
        origin_version_read: row.try_get("origin_version_read")?,
        local_version: u32::try_from(version).unwrap_or(u32::MAX),
        state: from_text::<WorkingCopyState>(&row.try_get::<String, _>("state")?)?,
        read_at: ts_req(row, "read_at")?,
        origin_newer_at: ts_opt(row, "origin_newer_at")?,
    })
}

pub(crate) async fn load_workspace(
    pool: &SqlitePool,
    id: WorkspaceId,
) -> Result<Workspace, WorkspaceError> {
    let row = sqlx::query("SELECT * FROM workspaces WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(WorkspaceError::NotFound)?;
    workspace_row(&row)
}

pub(crate) async fn load_link(
    pool: &SqlitePool,
    id: SourceLinkId,
) -> Result<SourceLink, WorkspaceError> {
    let row = sqlx::query("SELECT * FROM source_links WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(WorkspaceError::LinkNotFound)?;
    link_row(&row)
}

pub(crate) async fn load_item(
    pool: &SqlitePool,
    id: WorkingCopyItemId,
) -> Result<WorkingCopyItem, WorkspaceError> {
    let row = sqlx::query("SELECT * FROM working_copy_items WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?
        .ok_or(WorkspaceError::WorkingCopyItemNotFound)?;
    item_row(&row)
}

pub(crate) async fn item_at(
    pool: &SqlitePool,
    link: SourceLinkId,
    origin_path: &str,
) -> Result<Option<WorkingCopyItem>, WorkspaceError> {
    let row = sqlx::query(
        "SELECT * FROM working_copy_items WHERE source_link_id = ?1 AND origin_path = ?2",
    )
    .bind(link.to_string())
    .bind(origin_path)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(item_row).transpose()
}

/// Records the size in bytes of an item read into the working copy; the budget-aware retry sums these.
pub(crate) async fn set_item_size(
    pool: &SqlitePool,
    link: SourceLinkId,
    origin_path: &str,
    size: u64,
) {
    let _ = sqlx::query(
        "UPDATE working_copy_items SET size_bytes = ?3 WHERE source_link_id = ?1 AND origin_path = ?2",
    )
    .bind(link.to_string())
    .bind(origin_path)
    .bind(i64::try_from(size).unwrap_or(i64::MAX))
    .execute(pool)
    .await;
}

/// Total of the recorded sizes of the items of a link; items read before sizes were recorded count as 0.
pub(crate) async fn read_bytes(pool: &SqlitePool, link: SourceLinkId) -> u64 {
    let total: Option<i64> = sqlx::query_scalar(
        "SELECT sum(size_bytes) FROM working_copy_items WHERE source_link_id = ?1",
    )
    .bind(link.to_string())
    .fetch_one(pool)
    .await
    .ok()
    .flatten();
    u64::try_from(total.unwrap_or(0)).unwrap_or(0)
}

/// Validates the 2C rules of a working-copy item (`WCI_PATH_INVALID`, `WCI_VERSION_INVALID`).
pub(crate) fn check_item(origin_path: &str, version: &str) -> Result<(), WorkspaceError> {
    if origin_path.is_empty() || origin_path.len() > 2048 {
        return Err(WorkspaceError::Invalid("WCI_PATH_INVALID"));
    }
    let chars = version.chars().count();
    if chars == 0 || chars > 256 {
        return Err(WorkspaceError::Invalid("WCI_VERSION_INVALID"));
    }
    Ok(())
}

/// Inserts a working-copy item in state `read` (or `state`); `WCI_DUPLICATE` when the
/// `(source_link_id, origin_path)` pair exists.
pub(crate) async fn insert_item(
    pool: &SqlitePool,
    link: SourceLinkId,
    origin_path: &str,
    version: &str,
    local_version: u32,
    state: WorkingCopyState,
    git_branch: Option<&str>,
) -> Result<WorkingCopyItem, WorkspaceError> {
    check_item(origin_path, version)?;
    let id = WorkingCopyItemId::new();
    let now = Timestamp::now();
    let done = sqlx::query(
        "INSERT INTO working_copy_items (id, source_link_id, origin_path, origin_version_read, \
         local_version, state, read_at, git_branch) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(id.to_string())
    .bind(link.to_string())
    .bind(origin_path)
    .bind(version)
    .bind(i64::from(local_version))
    .bind(to_text(&state))
    .bind(now.to_wire())
    .bind(git_branch)
    .execute(pool)
    .await;
    match done {
        Ok(_) => Ok(WorkingCopyItem {
            id,
            source_link_id: link,
            origin_path: origin_path.to_owned(),
            origin_version_read: version.to_owned(),
            local_version,
            state,
            read_at: now,
            origin_newer_at: None,
        }),
        Err(error) if crate::error::is_unique_violation(&error) => {
            Err(WorkspaceError::Invalid("WCI_DUPLICATE"))
        }
        Err(error) => Err(error.into()),
    }
}
