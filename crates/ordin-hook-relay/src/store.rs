//! The `hook_late_decisions` table (HookRelay.md *Data Model*). Reads and writes only: the table
//! holds no secret and no payload, and the Journal's datastore carries it on either engine.

use ordin_journal::Datastore;
use ordin_types::time::Timestamp;
use ordin_types::{ApprovalId, RunId};
use sqlx::Row;

/// What kind of wait the late row stands for (G.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LateKind {
    ToolCall,
    StopGate,
}

impl LateKind {
    pub(crate) fn text(self) -> &'static str {
        match self {
            LateKind::ToolCall => "tool_call",
            LateKind::StopGate => "stop_gate",
        }
    }

    fn parse(text: &str) -> Result<LateKind, sqlx::Error> {
        match text {
            "tool_call" => Ok(LateKind::ToolCall),
            "stop_gate" => Ok(LateKind::StopGate),
            other => Err(sqlx::Error::Decode(
                format!("unknown late-decision kind {other}").into(),
            )),
        }
    }
}

/// One row: an approval whose hook gave up waiting.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LateRow {
    pub approval_id: ApprovalId,
    pub run_id: RunId,
    pub kind: LateKind,
}

fn decode<T>(result: Result<T, ordin_types::TypesError>) -> Result<T, sqlx::Error> {
    result.map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn row_of_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<LateRow, sqlx::Error> {
    let approval: String = row.try_get("approval_id")?;
    let run: String = row.try_get("run_id")?;
    let kind: String = row.try_get("kind")?;
    Ok(LateRow {
        approval_id: decode(ApprovalId::parse(&approval))?,
        run_id: decode(RunId::parse(&run))?,
        kind: LateKind::parse(&kind)?,
    })
}

fn row_of_pg(row: &sqlx::postgres::PgRow) -> Result<LateRow, sqlx::Error> {
    let approval: uuid::Uuid = row.try_get("approval_id")?;
    let run: uuid::Uuid = row.try_get("run_id")?;
    let kind: String = row.try_get("kind")?;
    Ok(LateRow {
        approval_id: ApprovalId(approval),
        run_id: RunId(run),
        kind: LateKind::parse(&kind)?,
    })
}

const COLUMNS: &str = "approval_id, run_id, kind";

/// Records a late decision (D.5.2, E.2). A second insert for one approval changes nothing.
pub(crate) async fn insert(
    store: &Datastore,
    row: &LateRow,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    if let Some(pool) = store.sqlite() {
        sqlx::query(
            "INSERT INTO hook_late_decisions (approval_id, run_id, kind, created_at) \
             VALUES (?1, ?2, ?3, ?4) ON CONFLICT (approval_id) DO NOTHING",
        )
        .bind(row.approval_id.to_string())
        .bind(row.run_id.to_string())
        .bind(row.kind.text())
        .bind(now.to_wire())
        .execute(pool)
        .await?;
    } else if let Some(pool) = store.postgres() {
        sqlx::query(
            "INSERT INTO hook_late_decisions (approval_id, run_id, kind, created_at) \
             VALUES ($1, $2, $3, $4) ON CONFLICT (approval_id) DO NOTHING",
        )
        .bind(*row.approval_id.as_uuid())
        .bind(*row.run_id.as_uuid())
        .bind(row.kind.text())
        .bind(now.0)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// The row of one approval, if it is still waiting.
pub(crate) async fn get(
    store: &Datastore,
    approval_id: ApprovalId,
) -> Result<Option<LateRow>, sqlx::Error> {
    if let Some(pool) = store.sqlite() {
        let sql = format!("SELECT {COLUMNS} FROM hook_late_decisions WHERE approval_id = ?1");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(approval_id.to_string())
            .fetch_optional(pool)
            .await?;
        return row.as_ref().map(row_of_sqlite).transpose();
    }
    if let Some(pool) = store.postgres() {
        let sql = format!("SELECT {COLUMNS} FROM hook_late_decisions WHERE approval_id = $1");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(*approval_id.as_uuid())
            .fetch_optional(pool)
            .await?;
        return row.as_ref().map(row_of_pg).transpose();
    }
    Ok(None)
}

/// Every row, oldest first (A.3).
pub(crate) async fn list(store: &Datastore) -> Result<Vec<LateRow>, sqlx::Error> {
    if let Some(pool) = store.sqlite() {
        let sql = format!("SELECT {COLUMNS} FROM hook_late_decisions ORDER BY created_at");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await?;
        return rows.iter().map(row_of_sqlite).collect();
    }
    if let Some(pool) = store.postgres() {
        let sql = format!("SELECT {COLUMNS} FROM hook_late_decisions ORDER BY created_at");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(pool)
            .await?;
        return rows.iter().map(row_of_pg).collect();
    }
    Ok(Vec::new())
}

/// Deletes one row (G.1, G.2).
pub(crate) async fn delete(store: &Datastore, approval_id: ApprovalId) -> Result<(), sqlx::Error> {
    if let Some(pool) = store.sqlite() {
        sqlx::query("DELETE FROM hook_late_decisions WHERE approval_id = ?1")
            .bind(approval_id.to_string())
            .execute(pool)
            .await?;
    } else if let Some(pool) = store.postgres() {
        sqlx::query("DELETE FROM hook_late_decisions WHERE approval_id = $1")
            .bind(*approval_id.as_uuid())
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Deletes rows created before `cutoff` (rows older than 30 days at start); returns how many.
pub(crate) async fn delete_older_than(
    store: &Datastore,
    cutoff: Timestamp,
) -> Result<u64, sqlx::Error> {
    if let Some(pool) = store.sqlite() {
        let done = sqlx::query("DELETE FROM hook_late_decisions WHERE created_at < ?1")
            .bind(cutoff.to_wire())
            .execute(pool)
            .await?;
        return Ok(done.rows_affected());
    }
    if let Some(pool) = store.postgres() {
        let done = sqlx::query("DELETE FROM hook_late_decisions WHERE created_at < $1")
            .bind(cutoff.0)
            .execute(pool)
            .await?;
        return Ok(done.rows_affected());
    }
    Ok(0)
}
