//! The SQLite statements of the control-state tables (RulesEngine.md *Data Model*, *Key
//! statements*; Internal Logic G). Every statement failure is `RULE_STATE_UNAVAILABLE` (G.7).

use sqlx::{Row, SqlitePool};

use crate::error::RulesError;

pub(crate) type StoreResult<T> = Result<T, RulesError>;

/// The fixed window `floor(now / period) * period`, in Unix seconds (G.2).
pub(crate) fn window_start(now_ms: i64, period_seconds: u32) -> i64 {
    let now = now_ms.div_euclid(1_000);
    let period = i64::from(period_seconds.max(1));
    now.div_euclid(period) * period
}

/// G.2: adds one start to the window and returns the window's count; the first start of a window
/// deletes the earlier windows of the same key.
pub(crate) async fn count_start(
    pool: &SqlitePool,
    kind: &str,
    key: &str,
    period_seconds: u32,
    now_ms: i64,
) -> StoreResult<i64> {
    let window = window_start(now_ms, period_seconds);
    let row = sqlx::query(
        "INSERT INTO rules_control_starts (kind, key, period_seconds, window_start, starts) \
         VALUES (?1, ?2, ?3, ?4, 1) \
         ON CONFLICT (kind, key, period_seconds, window_start) DO UPDATE SET starts = starts + 1 \
         RETURNING starts",
    )
    .bind(kind)
    .bind(key)
    .bind(i64::from(period_seconds))
    .bind(window)
    .fetch_one(pool)
    .await?;
    let starts: i64 = row.try_get("starts")?;
    if starts == 1 {
        sqlx::query(
            "DELETE FROM rules_control_starts \
             WHERE kind = ?1 AND key = ?2 AND period_seconds = ?3 AND window_start < ?4",
        )
        .bind(kind)
        .bind(key)
        .bind(i64::from(period_seconds))
        .bind(window)
        .execute(pool)
        .await?;
    }
    Ok(starts)
}

/// C `repeated-message`: true when the same digest was seen within the period.
pub(crate) async fn seen_message(
    pool: &SqlitePool,
    scope: &str,
    asker: &str,
    digest: &str,
    now_ms: i64,
    period_ms: i64,
) -> StoreResult<bool> {
    let row = sqlx::query(
        "INSERT INTO rules_control_messages (scope, asker, digest, seen_at, repeated) \
         VALUES (?1, ?2, ?3, ?4, 0) \
         ON CONFLICT (scope, asker, digest) DO UPDATE SET repeated = (seen_at > ?4 - ?5), \
         seen_at = excluded.seen_at RETURNING repeated",
    )
    .bind(scope)
    .bind(asker)
    .bind(digest)
    .bind(now_ms)
    .bind(period_ms)
    .fetch_one(pool)
    .await?;
    let repeated: i64 = row.try_get("repeated")?;
    Ok(repeated != 0)
}

/// G.3 `take_lock`: succeeds when no row exists, the row expired, or the holder already holds it.
pub(crate) async fn take_lock(
    pool: &SqlitePool,
    key: &str,
    holder: &str,
    now_ms: i64,
    lease_ms: i64,
) -> StoreResult<bool> {
    let row = sqlx::query(
        "INSERT INTO rules_control_locks (key, holder, expires_at) VALUES (?1, ?2, ?3 + ?4) \
         ON CONFLICT (key) DO UPDATE SET holder = excluded.holder, expires_at = excluded.expires_at \
         WHERE rules_control_locks.expires_at <= ?3 OR rules_control_locks.holder = excluded.holder \
         RETURNING holder",
    )
    .bind(key)
    .bind(holder)
    .bind(now_ms)
    .bind(lease_ms)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// G.3 `renew_lock`.
pub(crate) async fn renew_lock(
    pool: &SqlitePool,
    key: &str,
    holder: &str,
    now_ms: i64,
    lease_ms: i64,
) -> StoreResult<bool> {
    let row = sqlx::query(
        "UPDATE rules_control_locks SET expires_at = ?3 + ?4 \
         WHERE key = ?1 AND holder = ?2 AND expires_at > ?3 RETURNING key",
    )
    .bind(key)
    .bind(holder)
    .bind(now_ms)
    .bind(lease_ms)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// G.3 `release_lock`.
pub(crate) async fn release_lock(pool: &SqlitePool, key: &str, holder: &str) -> StoreResult<()> {
    sqlx::query("DELETE FROM rules_control_locks WHERE key = ?1 AND holder = ?2")
        .bind(key)
        .bind(holder)
        .execute(pool)
        .await?;
    Ok(())
}

/// K.8.
pub(crate) async fn lock_held(pool: &SqlitePool, key: &str, now_ms: i64) -> StoreResult<bool> {
    let row = sqlx::query("SELECT 1 FROM rules_control_locks WHERE key = ?1 AND expires_at > ?2")
        .bind(key)
        .bind(now_ms)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

// ---- intake places (G.5)

pub(crate) struct Slot {
    pub slot: i64,
    pub run_id: Option<String>,
    pub taken_at: Option<i64>,
}

pub(crate) async fn intake_slots(
    pool: &SqlitePool,
    intake: &str,
    limit: u32,
) -> StoreResult<Vec<Slot>> {
    for slot in 1..=i64::from(limit) {
        sqlx::query("INSERT OR IGNORE INTO rules_intake_places (intake, slot) VALUES (?1, ?2)")
            .bind(intake)
            .bind(slot)
            .execute(pool)
            .await?;
    }
    let rows = sqlx::query(
        "SELECT slot, run_id, taken_at FROM rules_intake_places \
         WHERE intake = ?1 AND slot <= ?2 ORDER BY slot",
    )
    .bind(intake)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(Slot {
                slot: row.try_get("slot")?,
                run_id: row.try_get("run_id")?,
                taken_at: row.try_get("taken_at")?,
            })
        })
        .collect()
}

/// The compare-and-set take of one slot; false when another instance changed it first.
pub(crate) async fn take_slot(
    pool: &SqlitePool,
    intake: &str,
    seen: &Slot,
    run_id: &str,
    now_ms: i64,
) -> StoreResult<bool> {
    let done = sqlx::query(
        "UPDATE rules_intake_places SET run_id = ?1, taken_at = ?2 \
         WHERE intake = ?3 AND slot = ?4 AND run_id IS ?5 AND taken_at IS ?6",
    )
    .bind(run_id)
    .bind(now_ms)
    .bind(intake)
    .bind(seen.slot)
    .bind(seen.run_id.as_deref())
    .bind(seen.taken_at)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

// ---- run facts (G.1)

pub(crate) async fn save_facts(
    pool: &SqlitePool,
    run_id: &str,
    agent_instance_id: &str,
    facts: &str,
    now_ms: i64,
) -> StoreResult<()> {
    sqlx::query(
        "INSERT INTO rules_run_facts (run_id, agent_instance_id, facts, updated_at) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT (run_id) DO UPDATE SET facts = excluded.facts, updated_at = excluded.updated_at",
    )
    .bind(run_id)
    .bind(agent_instance_id)
    .bind(facts)
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn load_facts(pool: &SqlitePool, run_id: &str) -> StoreResult<Option<String>> {
    let row = sqlx::query("SELECT facts FROM rules_run_facts WHERE run_id = ?1")
        .bind(run_id)
        .fetch_optional(pool)
        .await?;
    row.map(|r| r.try_get::<String, _>("facts"))
        .transpose()
        .map_err(RulesError::from)
}

/// `end_run`: the run's facts and approved-call rows.
pub(crate) async fn delete_run(pool: &SqlitePool, run_id: &str) -> StoreResult<()> {
    sqlx::query("DELETE FROM rules_run_facts WHERE run_id = ?1")
        .bind(run_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rules_approved_calls WHERE run_id = ?1")
        .bind(run_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Rows older than 30 days are deleted at start (*Data Model*).
pub(crate) async fn delete_stale(pool: &SqlitePool, before_ms: i64) -> StoreResult<()> {
    sqlx::query("DELETE FROM rules_run_facts WHERE updated_at < ?1")
        .bind(before_ms)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM rules_approved_calls WHERE created_at < ?1")
        .bind(before_ms)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- approved calls (D.6, D.8, K.2)

pub(crate) async fn open_approved_call(
    pool: &SqlitePool,
    run_id: &str,
    tool: &str,
    digest: &str,
) -> StoreResult<Option<String>> {
    let row = sqlx::query(
        "SELECT approval_id FROM rules_approved_calls \
         WHERE run_id = ?1 AND tool = ?2 AND arguments_digest = ?3 AND used = 0 \
         ORDER BY created_at LIMIT 1",
    )
    .bind(run_id)
    .bind(tool)
    .bind(digest)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.try_get::<String, _>("approval_id"))
        .transpose()
        .map_err(RulesError::from)
}

pub(crate) async fn insert_approved_call(
    pool: &SqlitePool,
    run_id: &str,
    tool: &str,
    digest: &str,
    approval_id: &str,
    now_ms: i64,
) -> StoreResult<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO rules_approved_calls \
         (run_id, tool, arguments_digest, approval_id, used, created_at) \
         VALUES (?1, ?2, ?3, ?4, 0, ?5)",
    )
    .bind(run_id)
    .bind(tool)
    .bind(digest)
    .bind(approval_id)
    .bind(now_ms)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_used(pool: &SqlitePool, approval_id: &str) -> StoreResult<()> {
    sqlx::query("UPDATE rules_approved_calls SET used = 1 WHERE approval_id = ?1")
        .bind(approval_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn delete_approved_call(pool: &SqlitePool, approval_id: &str) -> StoreResult<()> {
    sqlx::query("DELETE FROM rules_approved_calls WHERE approval_id = ?1")
        .bind(approval_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Whether an approved-call row of this approval is still unused.
#[cfg(test)]
pub(crate) async fn approved_call_unused(
    pool: &SqlitePool,
    approval_id: &str,
) -> StoreResult<Option<bool>> {
    let row = sqlx::query("SELECT used FROM rules_approved_calls WHERE approval_id = ?1")
        .bind(approval_id)
        .fetch_optional(pool)
        .await?;
    row.map(|r| r.try_get::<i64, _>("used").map(|used| used == 0))
        .transpose()
        .map_err(RulesError::from)
}

// ---- retained content (G.6, K.9)

pub(crate) async fn count_for_asker(
    pool: &SqlitePool,
    connection: &str,
    asker: &str,
) -> StoreResult<u64> {
    let key = format!("{connection}/{asker}");
    let scopes = format!("{}:%", like_escape(connection));
    let row = sqlx::query(
        "SELECT (SELECT COUNT(*) FROM rules_control_starts WHERE kind = 'asker' AND key = ?1) \
         + (SELECT COUNT(*) FROM rules_control_messages WHERE asker = ?2 AND scope LIKE ?3 ESCAPE '\\') \
         AS n",
    )
    .bind(&key)
    .bind(asker)
    .bind(&scopes)
    .fetch_one(pool)
    .await?;
    count_of(&row)
}

pub(crate) async fn forget_asker(
    pool: &SqlitePool,
    connection: &str,
    asker: &str,
) -> StoreResult<u64> {
    let key = format!("{connection}/{asker}");
    let scopes = format!("{}:%", like_escape(connection));
    let starts = sqlx::query("DELETE FROM rules_control_starts WHERE kind = 'asker' AND key = ?1")
        .bind(&key)
        .execute(pool)
        .await?
        .rows_affected();
    let messages = sqlx::query(
        "DELETE FROM rules_control_messages WHERE asker = ?1 AND scope LIKE ?2 ESCAPE '\\'",
    )
    .bind(asker)
    .bind(&scopes)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(starts + messages)
}

pub(crate) async fn count_scope(pool: &SqlitePool, scope: &str) -> StoreResult<u64> {
    let row = sqlx::query(
        "SELECT (SELECT COUNT(*) FROM rules_control_starts WHERE kind = 'scope' AND key = ?1) \
         + (SELECT COUNT(*) FROM rules_control_messages WHERE scope = ?1) AS n",
    )
    .bind(scope)
    .fetch_one(pool)
    .await?;
    count_of(&row)
}

pub(crate) async fn forget_scope(pool: &SqlitePool, scope: &str) -> StoreResult<u64> {
    let starts = sqlx::query("DELETE FROM rules_control_starts WHERE kind = 'scope' AND key = ?1")
        .bind(scope)
        .execute(pool)
        .await?
        .rows_affected();
    let messages = sqlx::query("DELETE FROM rules_control_messages WHERE scope = ?1")
        .bind(scope)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(starts + messages)
}

pub(crate) async fn prune_before(pool: &SqlitePool, cutoff_ms: i64) -> StoreResult<u64> {
    let messages = sqlx::query("DELETE FROM rules_control_messages WHERE seen_at < ?1")
        .bind(cutoff_ms)
        .execute(pool)
        .await?
        .rows_affected();
    let starts =
        sqlx::query("DELETE FROM rules_control_starts WHERE window_start + period_seconds < ?1")
            .bind(cutoff_ms.div_euclid(1_000))
            .execute(pool)
            .await?
            .rows_affected();
    Ok(messages + starts)
}

fn count_of(row: &sqlx::sqlite::SqliteRow) -> StoreResult<u64> {
    let n: i64 = row.try_get("n")?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// Escapes `%`, `_` and `\` for a `LIKE … ESCAPE '\'` pattern.
fn like_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_fixed() {
        assert_eq!(window_start(86_400_500, 86_400), 86_400);
        assert_eq!(window_start(86_399_999, 86_400), 0);
        assert_eq!(window_start(10_000, 0), 10);
    }

    #[test]
    fn like_patterns_are_escaped() {
        assert_eq!(like_escape("a_b%c\\"), "a\\_b\\%c\\\\");
    }
}
