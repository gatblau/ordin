//! This module's tables in the Journal's SQLite datastore (Data Model; module `team_knowledge`).
//! Owned and written by this service only.

use std::collections::HashMap;

use ordin_types::domain::{PullRequestRef, TeamAccess};
use ordin_types::time::Timestamp;
use ordin_types::{ApprovalId, SourceLinkId, TeamShareId, WorkspaceId};
use serde::{de::DeserializeOwned, Serialize};
use sqlx::{Row, SqlitePool};

use crate::error::TeamError;
use crate::model::{GetReport, ShareProposed, TeamAvailable, TeamCounts, TeamProblem};

pub(crate) fn now() -> String {
    Timestamp::now().to_wire()
}

pub(crate) fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn from_json<T: DeserializeOwned>(raw: Option<String>) -> Option<T> {
    raw.and_then(|r| serde_json::from_str(&r).ok())
}

pub(crate) fn ts(raw: Option<String>) -> Option<Timestamp> {
    raw.and_then(|r| serde_json::from_value(serde_json::Value::String(r)).ok())
}

/// A code read back from a table as the registry's own `&'static str`.
pub(crate) fn static_code(raw: &str) -> &'static str {
    ordin_types::error_registry::lookup(raw)
        .map(|e| e.code)
        .unwrap_or("TEAM_FILE_UNREADABLE")
}

/// One row of `team_knowledge_bindings`.
#[derive(Debug, Clone)]
pub(crate) struct Binding {
    pub workspace_id: WorkspaceId,
    pub source_link_id: SourceLinkId,
    pub host: String,
    pub repository: String,
    pub branch: String,
    pub folder: String,
    pub access: TeamAccess,
    pub enabled_at: Timestamp,
    pub last_checked_at: Option<Timestamp>,
    pub last_checked_commit: Option<String>,
    pub access_checked_at: Option<Timestamp>,
    pub available: Option<TeamAvailable>,
    pub to_share: Option<TeamCounts>,
    pub held_for_check: u32,
    pub folder_large: bool,
    pub last_error_code: Option<String>,
    pub last_get: Option<GetReport>,
    pub last_share: Option<ShareProposed>,
}

impl Binding {
    /// "acme/supplier-team › knowledge" (the folder left out at the repository root).
    pub fn header_line(&self) -> String {
        if self.folder.is_empty() {
            self.repository.clone()
        } else {
            format!("{} › {}", self.repository, self.folder)
        }
    }
}

pub(crate) fn access_text(access: TeamAccess) -> &'static str {
    match access {
        TeamAccess::ReadWrite => "read_write",
        TeamAccess::ReadOnly => "read_only",
    }
}

fn binding_of(row: &sqlx::sqlite::SqliteRow) -> Result<Binding, TeamError> {
    let ws: String = row.try_get("workspace_id")?;
    let link: String = row.try_get("source_link_id")?;
    let access: String = row.try_get("access")?;
    Ok(Binding {
        workspace_id: WorkspaceId::parse(&ws).map_err(|_| TeamError::datastore())?,
        source_link_id: SourceLinkId::parse(&link).map_err(|_| TeamError::datastore())?,
        host: row.try_get("host")?,
        repository: row.try_get("repository")?,
        branch: row.try_get("branch")?,
        folder: row.try_get("folder")?,
        access: if access == "read_write" {
            TeamAccess::ReadWrite
        } else {
            TeamAccess::ReadOnly
        },
        enabled_at: ts(row.try_get("enabled_at")?).ok_or_else(TeamError::datastore)?,
        last_checked_at: ts(row.try_get("last_checked_at")?),
        last_checked_commit: row.try_get("last_checked_commit")?,
        access_checked_at: ts(row.try_get("access_checked_at")?),
        available: from_json(row.try_get("available")?),
        to_share: from_json(row.try_get("to_share")?),
        held_for_check: u32::try_from(row.try_get::<i64, _>("held_for_check")?).unwrap_or(0),
        folder_large: row.try_get::<i64, _>("folder_large")? == 1,
        last_error_code: row.try_get("last_error_code")?,
        last_get: from_json(row.try_get("last_get")?),
        last_share: from_json(row.try_get("last_share")?),
    })
}

pub(crate) async fn binding(
    pool: &SqlitePool,
    ws: WorkspaceId,
) -> Result<Option<Binding>, TeamError> {
    let row = sqlx::query("SELECT * FROM team_knowledge_bindings WHERE workspace_id = ?1")
        .bind(ws.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(binding_of).transpose()
}

pub(crate) async fn bindings(pool: &SqlitePool) -> Result<Vec<Binding>, TeamError> {
    let rows = sqlx::query("SELECT * FROM team_knowledge_bindings ORDER BY enabled_at")
        .fetch_all(pool)
        .await?;
    rows.iter().map(binding_of).collect()
}

pub(crate) async fn insert_binding(pool: &SqlitePool, b: &Binding) -> Result<(), TeamError> {
    sqlx::query(
        "INSERT INTO team_knowledge_bindings (workspace_id, source_link_id, host, repository, branch, \
         folder, access, enabled_at, access_checked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
    )
    .bind(b.workspace_id.to_string())
    .bind(b.source_link_id.to_string())
    .bind(&b.host)
    .bind(&b.repository)
    .bind(&b.branch)
    .bind(&b.folder)
    .bind(access_text(b.access))
    .bind(b.enabled_at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_access(
    pool: &SqlitePool,
    ws: WorkspaceId,
    access: TeamAccess,
) -> Result<(), TeamError> {
    sqlx::query(
        "UPDATE team_knowledge_bindings SET access = ?2, access_checked_at = ?3 WHERE workspace_id = ?1",
    )
    .bind(ws.to_string())
    .bind(access_text(access))
    .bind(now())
    .execute(pool)
    .await?;
    Ok(())
}

/// §D step 6.
pub(crate) async fn store_check(
    pool: &SqlitePool,
    ws: WorkspaceId,
    available: Option<&TeamAvailable>,
    to_share: Option<&TeamCounts>,
    held: u32,
    commit: &str,
    folder_large: bool,
) -> Result<(), TeamError> {
    sqlx::query(
        "UPDATE team_knowledge_bindings SET available = ?2, to_share = ?3, held_for_check = ?4, \
         last_checked_commit = ?5, last_checked_at = ?6, folder_large = ?7 WHERE workspace_id = ?1",
    )
    .bind(ws.to_string())
    .bind(available.map(json))
    .bind(to_share.map(json))
    .bind(i64::from(held))
    .bind(commit)
    .bind(now())
    .bind(i64::from(folder_large))
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn touch_checked(pool: &SqlitePool, ws: WorkspaceId) -> Result<(), TeamError> {
    sqlx::query("UPDATE team_knowledge_bindings SET last_checked_at = ?2 WHERE workspace_id = ?1")
        .bind(ws.to_string())
        .bind(now())
        .execute(pool)
        .await?;
    Ok(())
}

/// §F step 7.
pub(crate) async fn store_got(
    pool: &SqlitePool,
    ws: WorkspaceId,
    commit: &str,
) -> Result<(), TeamError> {
    sqlx::query(
        "UPDATE team_knowledge_bindings SET last_got_commit = ?2, last_checked_commit = ?2, \
         available = NULL, last_checked_at = ?3 WHERE workspace_id = ?1",
    )
    .bind(ws.to_string())
    .bind(commit)
    .bind(now())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_last_get(
    pool: &SqlitePool,
    ws: WorkspaceId,
    report: &GetReport,
) -> Result<(), TeamError> {
    sqlx::query("UPDATE team_knowledge_bindings SET last_get = ?2 WHERE workspace_id = ?1")
        .bind(ws.to_string())
        .bind(json(report))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_last_share(
    pool: &SqlitePool,
    ws: WorkspaceId,
    proposed: &ShareProposed,
) -> Result<(), TeamError> {
    sqlx::query("UPDATE team_knowledge_bindings SET last_share = ?2 WHERE workspace_id = ?1")
        .bind(ws.to_string())
        .bind(json(proposed))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_last_error(
    pool: &SqlitePool,
    ws: WorkspaceId,
    code: Option<&str>,
) -> Result<(), TeamError> {
    sqlx::query("UPDATE team_knowledge_bindings SET last_error_code = ?2 WHERE workspace_id = ?1")
        .bind(ws.to_string())
        .bind(code)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_shared_at(pool: &SqlitePool, ws: WorkspaceId) -> Result<(), TeamError> {
    sqlx::query("UPDATE team_knowledge_bindings SET last_shared_at = ?2 WHERE workspace_id = ?1")
        .bind(ws.to_string())
        .bind(now())
        .execute(pool)
        .await?;
    Ok(())
}

/// §C step 2: the binding's item records, problems and row (the shares keep their history).
pub(crate) async fn delete_binding(pool: &SqlitePool, ws: WorkspaceId) -> Result<(), TeamError> {
    let mut tx = pool.begin().await?;
    for table in [
        "team_knowledge_items",
        "team_knowledge_problems",
        "team_knowledge_bindings",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE workspace_id = ?1"
        )))
        .bind(ws.to_string())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// §A step 3 and TW-I step 3: every row of the workspace in every table of the module.
pub(crate) async fn delete_workspace(pool: &SqlitePool, ws: WorkspaceId) -> Result<(), TeamError> {
    let mut tx = pool.begin().await?;
    for (table, column) in [
        ("team_activity", "workspace_id"),
        ("team_workspace_files", "workspace_id"),
        ("team_workspace_places", "workspace_id"),
        ("team_workspace_agents", "workspace_id"),
        ("team_workspaces", "workspace_id"),
        ("team_knowledge_items", "workspace_id"),
        ("team_knowledge_problems", "workspace_id"),
        ("team_knowledge_shares", "workspace_id"),
        ("team_knowledge_bindings", "workspace_id"),
        ("team_knowledge_bundle_files", "folder_id"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE {column} = ?1"
        )))
        .bind(ws.to_string())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ---- item records ----

#[derive(Debug, Clone)]
pub(crate) struct Record {
    pub kind: String,
    pub item_id: String,
    pub blob: String,
    pub local_sha256: String,
}

pub(crate) async fn records(
    pool: &SqlitePool,
    ws: WorkspaceId,
) -> Result<HashMap<String, Record>, TeamError> {
    let rows = sqlx::query(
        "SELECT path, kind, item_id, blob, local_sha256 FROM team_knowledge_items WHERE workspace_id = ?1",
    )
    .bind(ws.to_string())
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::with_capacity(rows.len());
    for row in rows {
        out.insert(
            row.try_get("path")?,
            Record {
                kind: row.try_get("kind")?,
                item_id: row.try_get("item_id")?,
                blob: row.try_get("blob")?,
                local_sha256: row.try_get("local_sha256")?,
            },
        );
    }
    Ok(out)
}

pub(crate) async fn put_record(
    pool: &SqlitePool,
    ws: WorkspaceId,
    path: &str,
    record: &Record,
) -> Result<(), TeamError> {
    sqlx::query(
        "INSERT INTO team_knowledge_items (workspace_id, path, kind, item_id, blob, local_sha256, synced_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT (workspace_id, path) DO UPDATE SET \
         kind = excluded.kind, item_id = excluded.item_id, blob = excluded.blob, \
         local_sha256 = excluded.local_sha256, synced_at = excluded.synced_at",
    )
    .bind(ws.to_string())
    .bind(path)
    .bind(&record.kind)
    .bind(&record.item_id)
    .bind(&record.blob)
    .bind(&record.local_sha256)
    .bind(now())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn drop_record(
    pool: &SqlitePool,
    ws: WorkspaceId,
    path: &str,
) -> Result<(), TeamError> {
    sqlx::query("DELETE FROM team_knowledge_items WHERE workspace_id = ?1 AND path = ?2")
        .bind(ws.to_string())
        .bind(path)
        .execute(pool)
        .await?;
    Ok(())
}

// spec: ordin-desktop/components/TeamKnowledgeService#TK-FE-1
/// The memory items taken for `ws` are dropped, so the next get takes them again; pages stay.
pub(crate) async fn drop_memory_items(
    pool: &SqlitePool,
    ws: WorkspaceId,
) -> Result<u64, TeamError> {
    let done = sqlx::query(
        "DELETE FROM team_knowledge_items WHERE workspace_id = ?1 \
         AND kind IN ('fact','entity','source','relationship')",
    )
    .bind(ws.to_string())
    .execute(pool)
    .await?;
    Ok(done.rows_affected())
}

// ---- problems ----

pub(crate) async fn problems(
    pool: &SqlitePool,
    ws: WorkspaceId,
) -> Result<Vec<(TeamProblem, String)>, TeamError> {
    let rows = sqlx::query(
        "SELECT path, code, blob, first_seen_at FROM team_knowledge_problems WHERE workspace_id = ?1 ORDER BY path",
    )
    .bind(ws.to_string())
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let code: String = row.try_get("code")?;
        out.push((
            TeamProblem {
                path: row.try_get("path")?,
                code: static_code(&code),
                first_seen_at: ts(row.try_get("first_seen_at")?).unwrap_or_else(Timestamp::now),
            },
            row.try_get("blob")?,
        ));
    }
    Ok(out)
}

/// Recorded once per path: `first_seen_at` is kept while the blob and the code are the same.
pub(crate) async fn put_problem(
    pool: &SqlitePool,
    ws: WorkspaceId,
    path: &str,
    code: &str,
    blob: &str,
) -> Result<(), TeamError> {
    sqlx::query(
        "INSERT INTO team_knowledge_problems (workspace_id, path, code, blob, first_seen_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT (workspace_id, path) DO UPDATE SET \
         code = excluded.code, blob = excluded.blob, \
         first_seen_at = CASE WHEN team_knowledge_problems.blob = excluded.blob \
           AND team_knowledge_problems.code = excluded.code \
           THEN team_knowledge_problems.first_seen_at ELSE excluded.first_seen_at END",
    )
    .bind(ws.to_string())
    .bind(path)
    .bind(code)
    .bind(blob)
    .bind(now())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn clear_problem(
    pool: &SqlitePool,
    ws: WorkspaceId,
    path: &str,
) -> Result<(), TeamError> {
    sqlx::query("DELETE FROM team_knowledge_problems WHERE workspace_id = ?1 AND path = ?2")
        .bind(ws.to_string())
        .bind(path)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn clear_problems_with(
    pool: &SqlitePool,
    ws: WorkspaceId,
    code: &str,
) -> Result<(), TeamError> {
    sqlx::query("DELETE FROM team_knowledge_problems WHERE workspace_id = ?1 AND code = ?2")
        .bind(ws.to_string())
        .bind(code)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- shares ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShareScope {
    All,
    Results,
    Workspace,
}

impl ShareScope {
    pub fn as_str(self) -> &'static str {
        match self {
            ShareScope::All => "all",
            ShareScope::Results => "results",
            ShareScope::Workspace => "workspace",
        }
    }

    fn parse(raw: &str) -> ShareScope {
        match raw {
            "results" => ShareScope::Results,
            "workspace" => ShareScope::Workspace,
            _ => ShareScope::All,
        }
    }
}

/// One planned path of a share: `[{path, blob | null}]` (null = delete), the path relative to the
/// folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub(crate) struct Planned {
    pub path: String,
    pub blob: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Share {
    pub id: TeamShareId,
    pub workspace_id: WorkspaceId,
    pub state: String,
    pub base_commit: String,
    pub commit_id: String,
    pub pull_request: Option<PullRequestRef>,
    pub counts: TeamCounts,
    pub planned: Vec<Planned>,
    pub moves: u32,
    pub scope: ShareScope,
}

fn share_of(row: &sqlx::sqlite::SqliteRow) -> Result<Share, TeamError> {
    let id: String = row.try_get("id")?;
    let ws: String = row.try_get("workspace_id")?;
    let counts: String = row.try_get("counts")?;
    let planned: String = row.try_get("planned")?;
    let scope: String = row.try_get("scope")?;
    Ok(Share {
        id: TeamShareId::parse(&id).map_err(|_| TeamError::datastore())?,
        workspace_id: WorkspaceId::parse(&ws).map_err(|_| TeamError::datastore())?,
        state: row.try_get("state")?,
        base_commit: row.try_get("base_commit")?,
        commit_id: row.try_get("commit_id")?,
        pull_request: from_json(row.try_get("pull_request")?),
        counts: serde_json::from_str(&counts).unwrap_or_default(),
        planned: serde_json::from_str(&planned).unwrap_or_default(),
        moves: u32::try_from(row.try_get::<i64, _>("moves")?).unwrap_or(0),
        scope: ShareScope::parse(&scope),
    })
}

pub(crate) async fn insert_share(pool: &SqlitePool, s: &Share) -> Result<(), TeamError> {
    sqlx::query(
        "INSERT INTO team_knowledge_shares (id, workspace_id, state, approval_id, base_commit, commit_id, \
         counts, planned, moves, created_at, scope) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
    )
    .bind(s.id.to_string())
    .bind(s.workspace_id.to_string())
    .bind(&s.state)
    .bind(&s.base_commit)
    .bind(&s.commit_id)
    .bind(json(&s.counts))
    .bind(json(&s.planned))
    .bind(now())
    .bind(s.scope.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn share(pool: &SqlitePool, id: TeamShareId) -> Result<Option<Share>, TeamError> {
    let row = sqlx::query("SELECT * FROM team_knowledge_shares WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(share_of).transpose()
}

/// The workspace's share in state `proposed`, `approved`, `sending` or `waiting_review`.
pub(crate) async fn open_share(
    pool: &SqlitePool,
    ws: WorkspaceId,
) -> Result<Option<Share>, TeamError> {
    let row = sqlx::query(
        "SELECT * FROM team_knowledge_shares WHERE workspace_id = ?1 AND \
         state IN ('proposed','approved','sending','waiting_review') ORDER BY created_at DESC LIMIT 1",
    )
    .bind(ws.to_string())
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(share_of).transpose()
}

pub(crate) async fn shares_in(pool: &SqlitePool, states: &[&str]) -> Result<Vec<Share>, TeamError> {
    let rows = sqlx::query("SELECT * FROM team_knowledge_shares ORDER BY created_at")
        .fetch_all(pool)
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let share = share_of(row)?;
        if states.contains(&share.state.as_str()) {
            out.push(share);
        }
    }
    Ok(out)
}

pub(crate) async fn set_share_state(
    pool: &SqlitePool,
    id: TeamShareId,
    state: &str,
    result_code: Option<&str>,
) -> Result<(), TeamError> {
    let finished = matches!(
        state,
        "done" | "rejected" | "failed" | "abandoned" | "superseded"
    );
    sqlx::query(
        "UPDATE team_knowledge_shares SET state = ?2, result_code = COALESCE(?3, result_code), \
         finished_at = CASE WHEN ?4 THEN ?5 ELSE finished_at END WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(state)
    .bind(result_code)
    .bind(finished)
    .bind(now())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_share_approval(
    pool: &SqlitePool,
    id: TeamShareId,
    approval: ApprovalId,
) -> Result<(), TeamError> {
    sqlx::query("UPDATE team_knowledge_shares SET approval_id = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(approval.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_share_pull_request(
    pool: &SqlitePool,
    id: TeamShareId,
    pr: &PullRequestRef,
) -> Result<(), TeamError> {
    sqlx::query("UPDATE team_knowledge_shares SET pull_request = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(json(pr))
        .execute(pool)
        .await?;
    Ok(())
}

/// §G step 3: the rebuilt commit after a moved branch.
pub(crate) async fn set_share_rebuilt(
    pool: &SqlitePool,
    id: TeamShareId,
    base: &str,
    commit: &str,
    planned: &[Planned],
    moves: u32,
) -> Result<(), TeamError> {
    sqlx::query(
        "UPDATE team_knowledge_shares SET base_commit = ?2, commit_id = ?3, planned = ?4, moves = ?5 WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(base)
    .bind(commit)
    .bind(json(&planned))
    .bind(i64::from(moves))
    .execute(pool)
    .await?;
    Ok(())
}
