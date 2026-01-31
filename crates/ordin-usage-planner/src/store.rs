//! The SQLite statements of the Usage Planner (UsagePlanner.md *Internal Logic* and *Data Model*).

use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::error::UsageError;

/// What one counting call adds to today's row.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DailyAdd {
    pub model_turns: i64,
    pub tool_calls: i64,
    pub turns_on_smaller: i64,
    pub answered_without_model: i64,
    /// `None` when the harness reported no figure: the column stays as it was (never zero).
    pub tokens: Option<i64>,
    pub tokens_from_cache: Option<i64>,
}

/// Today's counters as stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DailyRow {
    pub model_turns: i64,
    pub tool_calls: i64,
    pub turns_on_smaller: i64,
    pub answered_without_model: i64,
    pub tokens: Option<i64>,
    pub tokens_from_cache: Option<i64>,
}

/// One `reused_answers` row as stored.
pub(crate) struct ReusedRow {
    pub id: String,
    pub request: String,
    pub agent: String,
    pub workspace_id: String,
    pub input_versions: String,
    pub source_run_id: String,
    pub text: String,
    pub answered_at: String,
    pub shown_again_at: String,
    pub expires_at: String,
}

pub(crate) fn parse_ts(raw: &str) -> Result<Timestamp, UsageError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| UsageError::corrupt("usage timestamp"))
}

pub(crate) async fn load_settings(
    pool: &SqlitePool,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let row = sqlx::query("SELECT settings, updated_at FROM usage_settings WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    row.map(|row| Ok((row.try_get("settings")?, row.try_get("updated_at")?)))
        .transpose()
}

/// B.6: the single row of `usage_settings`.
pub(crate) async fn save_settings(
    pool: &SqlitePool,
    settings: &str,
    updated_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO usage_settings (id, settings, updated_at) VALUES (1, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET settings = excluded.settings, updated_at = excluded.updated_at",
    )
    .bind(settings)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// G.5: rows past `expires_at`.
pub(crate) async fn delete_expired(pool: &SqlitePool, now: &str) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM reused_answers WHERE expires_at <= ?")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Data Model: `usage_daily` rows older than 400 days.
pub(crate) async fn delete_old_days(
    pool: &SqlitePool,
    before_day: &str,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM usage_daily WHERE day < ?")
        .bind(before_day)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// I.1 and G.4: add to today's row, creating it when absent.
pub(crate) async fn add_daily(
    pool: &SqlitePool,
    day: &str,
    add: DailyAdd,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO usage_daily (day, model_turns, tool_calls, turns_on_smaller, \
         answered_without_model, tokens, tokens_from_cache) VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(day) DO UPDATE SET \
         model_turns = model_turns + excluded.model_turns, \
         tool_calls = tool_calls + excluded.tool_calls, \
         turns_on_smaller = turns_on_smaller + excluded.turns_on_smaller, \
         answered_without_model = answered_without_model + excluded.answered_without_model, \
         tokens = CASE WHEN excluded.tokens IS NULL THEN tokens \
                       WHEN tokens IS NULL THEN excluded.tokens \
                       ELSE tokens + excluded.tokens END, \
         tokens_from_cache = CASE WHEN excluded.tokens_from_cache IS NULL THEN tokens_from_cache \
                       WHEN tokens_from_cache IS NULL THEN excluded.tokens_from_cache \
                       ELSE tokens_from_cache + excluded.tokens_from_cache END",
    )
    .bind(day)
    .bind(add.model_turns)
    .bind(add.tool_calls)
    .bind(add.turns_on_smaller)
    .bind(add.answered_without_model)
    .bind(add.tokens)
    .bind(add.tokens_from_cache)
    .execute(pool)
    .await?;
    Ok(())
}

/// I.3: today's row; zeros when absent.
pub(crate) async fn daily(pool: &SqlitePool, day: &str) -> Result<DailyRow, sqlx::Error> {
    let row = sqlx::query(
        "SELECT model_turns, tool_calls, turns_on_smaller, answered_without_model, tokens, \
         tokens_from_cache FROM usage_daily WHERE day = ?",
    )
    .bind(day)
    .fetch_optional(pool)
    .await?;
    match row {
        None => Ok(DailyRow::default()),
        Some(row) => Ok(DailyRow {
            model_turns: row.try_get("model_turns")?,
            tool_calls: row.try_get("tool_calls")?,
            turns_on_smaller: row.try_get("turns_on_smaller")?,
            answered_without_model: row.try_get("answered_without_model")?,
            tokens: row.try_get("tokens")?,
            tokens_from_cache: row.try_get("tokens_from_cache")?,
        }),
    }
}

/// I.2: one finished run, then only the newest 20 rows of the key stay.
pub(crate) async fn insert_history(
    pool: &SqlitePool,
    work_key: &str,
    run_id: &str,
    model_turns: i64,
    finished_at: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT OR REPLACE INTO usage_work_history (work_key, run_id, model_turns, finished_at) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(work_key)
    .bind(run_id)
    .bind(model_turns)
    .bind(finished_at)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM usage_work_history WHERE work_key = ? AND rowid NOT IN \
         (SELECT rowid FROM usage_work_history WHERE work_key = ? \
          ORDER BY finished_at DESC, rowid DESC LIMIT 20)",
    )
    .bind(work_key)
    .bind(work_key)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// E.1: the model turns of the last 5 runs of the key.
pub(crate) async fn recent_history(
    pool: &SqlitePool,
    work_key: &str,
) -> Result<Vec<i64>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT model_turns FROM usage_work_history WHERE work_key = ? \
         ORDER BY finished_at DESC, rowid DESC LIMIT 5",
    )
    .bind(work_key)
    .fetch_all(pool)
    .await?;
    rows.iter().map(|row| row.try_get("model_turns")).collect()
}

/// G.3: one indexed answer.
pub(crate) async fn insert_reused(
    pool: &SqlitePool,
    row: &ReusedRow,
    digest: &str,
    agent_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO reused_answers (id, request, request_digest, agent, agent_key, workspace_id, \
         input_versions, source_run_id, text, answered_at, shown_again_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.id)
    .bind(&row.request)
    .bind(digest)
    .bind(&row.agent)
    .bind(agent_key)
    .bind(&row.workspace_id)
    .bind(&row.input_versions)
    .bind(&row.source_run_id)
    .bind(&row.text)
    .bind(&row.answered_at)
    .bind(&row.shown_again_at)
    .bind(&row.expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

fn reused_row(row: &SqliteRow) -> Result<ReusedRow, sqlx::Error> {
    Ok(ReusedRow {
        id: row.try_get("id")?,
        request: row.try_get("request")?,
        agent: row.try_get("agent")?,
        workspace_id: row.try_get("workspace_id")?,
        input_versions: row.try_get("input_versions")?,
        source_run_id: row.try_get("source_run_id")?,
        text: row.try_get("text")?,
        answered_at: row.try_get("answered_at")?,
        shown_again_at: row.try_get("shown_again_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

/// G.2: unexpired rows of the same request, agent and workspace, most recent first.
pub(crate) async fn reuse_candidates(
    pool: &SqlitePool,
    digest: &str,
    agent_key: &str,
    workspace_id: &str,
    now: &str,
) -> Result<Vec<ReusedRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, request, agent, workspace_id, input_versions, source_run_id, text, answered_at, \
         shown_again_at, expires_at FROM reused_answers \
         WHERE request_digest = ? AND agent_key = ? AND workspace_id = ? AND expires_at > ? \
         ORDER BY answered_at DESC, rowid DESC",
    )
    .bind(digest)
    .bind(agent_key)
    .bind(workspace_id)
    .bind(now)
    .fetch_all(pool)
    .await?;
    rows.iter().map(reused_row).collect()
}

/// G.4: stamps `shown_again_at`; returns the source run of the row, or `None` when no row has the id.
pub(crate) async fn touch_reused(
    pool: &SqlitePool,
    id: &str,
    shown_again_at: &str,
) -> Result<Option<String>, sqlx::Error> {
    let result = sqlx::query("UPDATE reused_answers SET shown_again_at = ? WHERE id = ?")
        .bind(shown_again_at)
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }
    let row = sqlx::query("SELECT source_run_id FROM reused_answers WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(Some(row.try_get("source_run_id")?))
}

/// Data Model, pruning: history finished and answers given before the cutoff, never those of a run in
/// progress. Returns the number of rows deleted.
pub(crate) async fn prune(
    pool: &SqlitePool,
    cutoff: &str,
    runs_in_progress: &str,
) -> Result<u64, sqlx::Error> {
    let history = sqlx::query(
        "DELETE FROM usage_work_history WHERE finished_at < ? \
         AND run_id NOT IN (SELECT value FROM json_each(?))",
    )
    .bind(cutoff)
    .bind(runs_in_progress)
    .execute(pool)
    .await?;
    let answers = sqlx::query(
        "DELETE FROM reused_answers WHERE answered_at < ? \
         AND source_run_id NOT IN (SELECT value FROM json_each(?))",
    )
    .bind(cutoff)
    .bind(runs_in_progress)
    .execute(pool)
    .await?;
    Ok(history.rows_affected() + answers.rows_affected())
}
