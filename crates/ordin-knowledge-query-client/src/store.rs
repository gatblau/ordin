//! The SQLite statements of the crate (KnowledgeQueryClient.md *Data Model*). PostgreSQL holds the
//! same tables in the scaled shape but no rows (the agent profile has no workspaces), so every
//! statement here runs on SQLite only.

use ordin_types::common::SetBy;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::wire::knowledge_query::{KnowledgeContents, SourcesRecordPolicy};
use ordin_types::{KnowledgeConnectionId, WorkspaceId};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::error::KqError;
use crate::types::{AccountLinkState, ConnectionState, SourceSystem};

/// One `kq_connections` row.
#[derive(Debug, Clone)]
pub(crate) struct ConnRow {
    pub id: KnowledgeConnectionId,
    pub workspace_id: WorkspaceId,
    pub url: String,
    pub name: String,
    pub issuer: String,
    pub token_audience: String,
    pub added_by: SetBy,
    pub company_entry_key: Option<String>,
    pub removed_by_person: bool,
    pub state: ConnectionState,
    pub sources_record: Option<SourcesRecordPolicy>,
    pub last_problem: Option<String>,
    pub added_at: Timestamp,
    pub last_answered_at: Option<Timestamp>,
}

macro_rules! select_rows {
    ($tail:literal) => {
        concat!(
            "SELECT id, workspace_id, url, name, issuer, token_audience, added_by, \
             company_entry_key, removed_by_person, state, sources_record_enabled, \
             sources_record_days, last_problem, added_at, last_answered_at FROM kq_connections ",
            $tail
        )
    };
}

fn corrupt() -> KqError {
    KqError::StoreUnavailable
}

fn parse_ts(raw: &str) -> Result<Timestamp, KqError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| corrupt())
}

pub(crate) fn added_by_text(by: SetBy) -> &'static str {
    match by {
        SetBy::Person => "person",
        SetBy::Company => "company",
    }
}

fn row_of(row: &SqliteRow) -> Result<ConnRow, KqError> {
    let text = |name: &str| row.try_get::<String, _>(name).map_err(|_| corrupt());
    let opt = |name: &str| {
        row.try_get::<Option<String>, _>(name)
            .map_err(|_| corrupt())
    };
    let added_by = match text("added_by")?.as_str() {
        "person" => SetBy::Person,
        "company" => SetBy::Company,
        _ => return Err(corrupt()),
    };
    let enabled: i64 = row
        .try_get("sources_record_enabled")
        .map_err(|_| corrupt())?;
    let days: Option<i64> = row.try_get("sources_record_days").map_err(|_| corrupt())?;
    let removed: i64 = row.try_get("removed_by_person").map_err(|_| corrupt())?;
    Ok(ConnRow {
        id: KnowledgeConnectionId::parse(&text("id")?).map_err(|_| corrupt())?,
        workspace_id: WorkspaceId::parse(&text("workspace_id")?).map_err(|_| corrupt())?,
        url: text("url")?,
        name: text("name")?,
        issuer: text("issuer")?,
        token_audience: text("token_audience")?,
        added_by,
        company_entry_key: opt("company_entry_key")?,
        removed_by_person: removed != 0,
        state: ConnectionState::parse(&text("state")?).ok_or_else(corrupt)?,
        sources_record: match (enabled != 0, days) {
            (false, _) => None,
            (true, days) => Some(SourcesRecordPolicy {
                enabled: true,
                retention_days: days.and_then(|d| u32::try_from(d).ok()).unwrap_or(0),
            }),
        },
        last_problem: opt("last_problem")?,
        added_at: parse_ts(&text("added_at")?)?,
        last_answered_at: opt("last_answered_at")?
            .map(|raw| parse_ts(&raw))
            .transpose()?,
    })
}

/// A policy with `enabled = false` is stored as "no record"; the column pair is `(enabled, days)`.
fn policy_columns(policy: Option<SourcesRecordPolicy>) -> (i64, Option<i64>) {
    match policy {
        Some(p) if p.enabled => (1, Some(i64::from(p.retention_days))),
        _ => (0, None),
    }
}

pub(crate) async fn live_of(pool: &SqlitePool, ws: WorkspaceId) -> Result<Vec<ConnRow>, KqError> {
    let rows = sqlx::query(select_rows!(
        "WHERE workspace_id = ?1 AND removed_by_person = 0 ORDER BY added_at, id"
    ))
    .bind(ws.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_of).collect()
}

pub(crate) async fn all_live(pool: &SqlitePool) -> Result<Vec<ConnRow>, KqError> {
    let rows = sqlx::query(select_rows!(
        "WHERE removed_by_person = 0 ORDER BY added_at, id"
    ))
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_of).collect()
}

pub(crate) async fn get(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
) -> Result<Option<ConnRow>, KqError> {
    let row = sqlx::query(select_rows!("WHERE id = ?1"))
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(row_of).transpose()
}

pub(crate) async fn company_rows(
    pool: &SqlitePool,
    ws: WorkspaceId,
) -> Result<Vec<ConnRow>, KqError> {
    let rows = sqlx::query(select_rows!(
        "WHERE workspace_id = ?1 AND added_by = 'company' ORDER BY added_at, id"
    ))
    .bind(ws.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_of).collect()
}

pub(crate) async fn url_taken(
    pool: &SqlitePool,
    ws: WorkspaceId,
    url: &str,
) -> Result<bool, KqError> {
    let row = sqlx::query("SELECT 1 FROM kq_connections WHERE workspace_id = ?1 AND url = ?2")
        .bind(ws.to_string())
        .bind(url)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Inserts a row, and its contents row when the contents are known, in one transaction (B.8).
pub(crate) async fn insert(
    pool: &SqlitePool,
    row: &ConnRow,
    contents: Option<&str>,
) -> Result<(), KqError> {
    let (enabled, days) = policy_columns(row.sources_record);
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO kq_connections (id, workspace_id, url, name, issuer, token_audience, \
         added_by, company_entry_key, removed_by_person, state, sources_record_enabled, \
         sources_record_days, last_problem, added_at, last_answered_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?11, ?12, ?13, NULL)",
    )
    .bind(row.id.to_string())
    .bind(row.workspace_id.to_string())
    .bind(&row.url)
    .bind(&row.name)
    .bind(&row.issuer)
    .bind(&row.token_audience)
    .bind(added_by_text(row.added_by))
    .bind(row.company_entry_key.as_deref())
    .bind(row.state.as_str())
    .bind(enabled)
    .bind(days)
    .bind(row.last_problem.as_deref())
    .bind(row.added_at.to_string())
    .execute(&mut *tx)
    .await?;
    if let Some(json) = contents {
        sqlx::query(
            "INSERT INTO kq_connection_contents (connection_id, contents_json, fetched_at) \
             VALUES (?1, ?2, ?3)",
        )
        .bind(row.id.to_string())
        .bind(json)
        .bind(Timestamp::now().to_string())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn delete(pool: &SqlitePool, id: KnowledgeConnectionId) -> Result<(), KqError> {
    sqlx::query("DELETE FROM kq_connections WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn mark_removed(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
) -> Result<(), KqError> {
    sqlx::query("UPDATE kq_connections SET removed_by_person = 1 WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_state(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
    state: ConnectionState,
    problem: Option<&str>,
) -> Result<(), KqError> {
    sqlx::query("UPDATE kq_connections SET state = ?2, last_problem = ?3 WHERE id = ?1")
        .bind(id.to_string())
        .bind(state.as_str())
        .bind(problem)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_problem(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
    problem: Option<&str>,
) -> Result<(), KqError> {
    sqlx::query("UPDATE kq_connections SET last_problem = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(problem)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_answered(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
    at: Timestamp,
) -> Result<(), KqError> {
    sqlx::query("UPDATE kq_connections SET last_answered_at = ?2, state = 'ready' WHERE id = ?1")
        .bind(id.to_string())
        .bind(at.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// Replaces what the front door says about itself (steps B.4 and E) after a successful read.
pub(crate) async fn set_about(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
    issuer: &str,
    audience: &str,
    policy: Option<SourcesRecordPolicy>,
) -> Result<(), KqError> {
    let (enabled, days) = policy_columns(policy);
    sqlx::query(
        "UPDATE kq_connections SET issuer = ?2, token_audience = ?3, \
         sources_record_enabled = ?4, sources_record_days = ?5 WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(issuer)
    .bind(audience)
    .bind(enabled)
    .bind(days)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn upsert_contents(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
    json: &str,
) -> Result<(), KqError> {
    sqlx::query(
        "INSERT INTO kq_connection_contents (connection_id, contents_json, fetched_at) \
         VALUES (?1, ?2, ?3) ON CONFLICT(connection_id) DO UPDATE SET \
         contents_json = excluded.contents_json, fetched_at = excluded.fetched_at",
    )
    .bind(id.to_string())
    .bind(json)
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// The cached contents of one connection and when they were fetched.
pub(crate) async fn contents_of(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
) -> Result<Option<(KnowledgeContents, Timestamp)>, KqError> {
    let row = sqlx::query(
        "SELECT contents_json, fetched_at FROM kq_connection_contents WHERE connection_id = ?1",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let json: String = row.try_get("contents_json").map_err(|_| corrupt())?;
    let fetched: String = row.try_get("fetched_at").map_err(|_| corrupt())?;
    let contents = serde_json::from_str(&json).map_err(|_| corrupt())?;
    Ok(Some((contents, parse_ts(&fetched)?)))
}

/// The cached contents of every live connection of a workspace.
pub(crate) async fn live_contents(
    pool: &SqlitePool,
    ws: WorkspaceId,
) -> Result<Vec<KnowledgeContents>, KqError> {
    let rows = sqlx::query(
        "SELECT c.contents_json FROM kq_connection_contents c \
         JOIN kq_connections k ON k.id = c.connection_id \
         WHERE k.workspace_id = ?1 AND k.removed_by_person = 0 ORDER BY k.added_at, k.id",
    )
    .bind(ws.to_string())
    .fetch_all(pool)
    .await?;
    let mut all = Vec::with_capacity(rows.len());
    for row in rows {
        let json: String = row.try_get("contents_json").map_err(|_| corrupt())?;
        all.push(serde_json::from_str(&json).map_err(|_| corrupt())?);
    }
    Ok(all)
}

pub(crate) async fn upsert_account(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
    system: SourceSystem,
    state: AccountLinkState,
) -> Result<(), KqError> {
    sqlx::query(
        "INSERT INTO kq_connection_accounts (connection_id, system, state, changed_at) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT(connection_id, system) DO UPDATE SET \
         state = excluded.state, changed_at = excluded.changed_at",
    )
    .bind(id.to_string())
    .bind(system.as_str())
    .bind(state.as_str())
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_account(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
    system: SourceSystem,
) -> Result<(), KqError> {
    sqlx::query("DELETE FROM kq_connection_accounts WHERE connection_id = ?1 AND system = ?2")
        .bind(id.to_string())
        .bind(system.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn accounts_of(
    pool: &SqlitePool,
    id: KnowledgeConnectionId,
) -> Result<Vec<(SourceSystem, AccountLinkState)>, KqError> {
    let rows =
        sqlx::query("SELECT system, state FROM kq_connection_accounts WHERE connection_id = ?1")
            .bind(id.to_string())
            .fetch_all(pool)
            .await?;
    let mut all = Vec::with_capacity(rows.len());
    for row in rows {
        let system: String = row.try_get("system").map_err(|_| corrupt())?;
        let state: String = row.try_get("state").map_err(|_| corrupt())?;
        let system = match system.as_str() {
            "github" => SourceSystem::Github,
            "gitlab" => SourceSystem::Gitlab,
            "confluence" => SourceSystem::Confluence,
            _ => return Err(corrupt()),
        };
        all.push((system, AccountLinkState::parse(&state).ok_or_else(corrupt)?));
    }
    Ok(all)
}
