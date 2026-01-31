//! One query layer over both engines (SA-DB-01), the same device the Journal uses: every statement
//! is written once, with `$1, $2, …` placeholders, and a few markers where the engines differ.
//! Values travel as text, integers or bytes; PostgreSQL casts them to its column types on the way
//! in and back to text on the way out, so one decoder reads both. Every value is bound;
//! `AssertSqlSafe` wraps only text built from this crate's fixed templates and generated
//! placeholder lists, never from a caller's value.
//!
//! | Marker       | SQLite                                      | PostgreSQL                          |
//! |--------------|---------------------------------------------|-------------------------------------|
//! | `{uuid}`     | -                                           | `::uuid`                            |
//! | `{ts}`       | -                                           | `::timestamptz`                     |
//! | `{json}`     | -                                           | `::jsonb`                           |
//! | `{t:EXPR}`   | `EXPR`                                      | `EXPR` as `YYYY-MM-DDTHH:MM:SS.mmmZ` |
//! | `{s:EXPR}`   | `EXPR`                                      | `(EXPR)::text`                      |
//! | `{now}`      | `strftime('%Y-%m-%dT%H:%M:%fZ','now')`      | `now()`                             |
//! | `{lease}`    | now plus `LEASE_SECONDS`                    | `now() + make_interval(secs => 30)` |
//! | `{lock}`     | -                                           | ` FOR UPDATE`                       |
//! | `{skip}`     | -                                           | `FOR UPDATE SKIP LOCKED`            |
//! | `{seq}`      | `max(seq)+1` of `run_records`               | `DEFAULT` (identity column)         |
//! | `{sum:N}`    | append `$N` to `stretch_summaries`, or keep | the same, as `jsonb`                |

use sqlx::postgres::{PgRow, Postgres};
use sqlx::sqlite::{Sqlite, SqliteRow};
use sqlx::{AssertSqlSafe, Row, Transaction};

use ordin_journal::Datastore;

use crate::types::LEASE_SECONDS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Engine {
    Sqlite,
    Postgres,
}

pub(crate) fn engine_of(store: &Datastore) -> Engine {
    match store {
        Datastore::Sqlite(_) => Engine::Sqlite,
        Datastore::Postgres(_) => Engine::Postgres,
    }
}

/// One bound value.
#[derive(Debug, Clone)]
pub(crate) enum Arg {
    Text(String),
    OptText(Option<String>),
    Int(i64),
    Bytes(Option<Vec<u8>>),
}

const SQLITE_NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

/// Renders a template for one engine (see the module table).
pub(crate) fn render(engine: Engine, template: &str) -> String {
    let pg = engine == Engine::Postgres;
    let mut out = String::with_capacity(template.len() + 64);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let marker = &after[..close];
        match marker.split_once(':') {
            Some(("t", expr)) if pg => out.push_str(&format!(
                "to_char(({expr}) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')"
            )),
            Some(("s", expr)) if pg => out.push_str(&format!("({expr})::text")),
            Some(("t" | "s", expr)) => out.push_str(expr),
            Some(("sum", n)) => {
                if pg {
                    out.push_str(&format!(
                        "CASE WHEN ${n}::text IS NULL THEN stretch_summaries \
                         ELSE stretch_summaries || to_jsonb(${n}::text) END"
                    ));
                } else {
                    out.push_str(&format!(
                        "CASE WHEN ${n} IS NULL THEN stretch_summaries \
                         ELSE json_insert(stretch_summaries, '$[#]', ${n}) END"
                    ));
                }
            }
            Some(_) => {
                out.push('{');
                out.push_str(marker);
                out.push('}');
            }
            None => match (marker, pg) {
                ("uuid", true) => out.push_str("::uuid"),
                ("ts", true) => out.push_str("::timestamptz"),
                ("json", true) => out.push_str("::jsonb"),
                ("uuid" | "ts" | "json", false) => {}
                ("now", true) => out.push_str("now()"),
                ("now", false) => out.push_str(SQLITE_NOW),
                ("lease", true) => {
                    out.push_str(&format!("now() + make_interval(secs => {LEASE_SECONDS})"))
                }
                ("lease", false) => out.push_str(&format!(
                    "strftime('%Y-%m-%dT%H:%M:%fZ','now','+{LEASE_SECONDS} seconds')"
                )),
                ("lock", true) => out.push_str(" FOR UPDATE"),
                ("skip", true) => out.push_str("FOR UPDATE SKIP LOCKED"),
                ("lock" | "skip", false) => {}
                ("seq", true) => out.push_str("DEFAULT"),
                ("seq", false) => {
                    out.push_str("(SELECT coalesce(max(seq), 0) + 1 FROM run_records)")
                }
                (other, _) => {
                    out.push('{');
                    out.push_str(other);
                    out.push('}');
                }
            },
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// `$first{suffix}, $first+1{suffix}, …` for `count` values.
pub(crate) fn placeholders(first: usize, count: usize, suffix: &str) -> String {
    (first..first + count)
        .map(|n| format!("${n}{suffix}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One result row of either engine.
pub(crate) enum DbRow {
    Sqlite(SqliteRow),
    Pg(PgRow),
}

impl DbRow {
    pub(crate) fn opt_text(&self, i: usize) -> Result<Option<String>, sqlx::Error> {
        match self {
            DbRow::Sqlite(row) => row.try_get::<Option<String>, _>(i),
            DbRow::Pg(row) => row.try_get::<Option<String>, _>(i),
        }
    }

    pub(crate) fn text(&self, i: usize) -> Result<String, sqlx::Error> {
        self.opt_text(i)?
            .ok_or(sqlx::Error::ColumnNotFound(format!("#{i} is null")))
    }

    /// PostgreSQL `integer` columns read as `i32`, `bigint` and `count(*)` as `i64`.
    pub(crate) fn int(&self, i: usize) -> Result<i64, sqlx::Error> {
        match self {
            DbRow::Sqlite(row) => row.try_get::<i64, _>(i),
            DbRow::Pg(row) => match row.try_get::<i64, _>(i) {
                Ok(value) => Ok(value),
                Err(_) => row.try_get::<i32, _>(i).map(i64::from),
            },
        }
    }

    pub(crate) fn opt_bytes(&self, i: usize) -> Result<Option<Vec<u8>>, sqlx::Error> {
        match self {
            DbRow::Sqlite(row) => row.try_get::<Option<Vec<u8>>, _>(i),
            DbRow::Pg(row) => row.try_get::<Option<Vec<u8>>, _>(i),
        }
    }
}

macro_rules! bind_all {
    ($query:expr, $args:expr) => {{
        let mut query = $query;
        for arg in $args {
            query = match arg {
                Arg::Text(value) => query.bind(value.clone()),
                Arg::OptText(value) => query.bind(value.clone()),
                Arg::Int(value) => query.bind(*value),
                Arg::Bytes(value) => query.bind(value.clone()),
            };
        }
        query
    }};
}

/// One transaction on either engine. Dropping it without `commit` rolls it back.
pub(crate) enum Tx {
    Sqlite(Transaction<'static, Sqlite>),
    Pg(Transaction<'static, Postgres>),
}

impl Tx {
    /// SQLite: `BEGIN IMMEDIATE`, so the single writer is taken at once and a reader never has to
    /// upgrade. PostgreSQL: `BEGIN`.
    pub(crate) async fn begin(store: &Datastore) -> Result<Tx, sqlx::Error> {
        Ok(match store {
            Datastore::Sqlite(pool) => Tx::Sqlite(pool.begin_with("BEGIN IMMEDIATE").await?),
            Datastore::Postgres(pool) => Tx::Pg(pool.begin().await?),
        })
    }

    pub(crate) fn engine(&self) -> Engine {
        match self {
            Tx::Sqlite(_) => Engine::Sqlite,
            Tx::Pg(_) => Engine::Postgres,
        }
    }

    pub(crate) async fn commit(self) -> Result<(), sqlx::Error> {
        match self {
            Tx::Sqlite(tx) => tx.commit().await,
            Tx::Pg(tx) => tx.commit().await,
        }
    }

    pub(crate) async fn execute(
        &mut self,
        template: &str,
        args: &[Arg],
    ) -> Result<u64, sqlx::Error> {
        let sql = render(self.engine(), template);
        Ok(match self {
            Tx::Sqlite(tx) => bind_all!(sqlx::query::<Sqlite>(AssertSqlSafe(sql)), args)
                .execute(&mut **tx)
                .await?
                .rows_affected(),
            Tx::Pg(tx) => bind_all!(sqlx::query::<Postgres>(AssertSqlSafe(sql)), args)
                .execute(&mut **tx)
                .await?
                .rows_affected(),
        })
    }

    pub(crate) async fn fetch_all(
        &mut self,
        template: &str,
        args: &[Arg],
    ) -> Result<Vec<DbRow>, sqlx::Error> {
        let sql = render(self.engine(), template);
        Ok(match self {
            Tx::Sqlite(tx) => bind_all!(sqlx::query::<Sqlite>(AssertSqlSafe(sql)), args)
                .fetch_all(&mut **tx)
                .await?
                .into_iter()
                .map(DbRow::Sqlite)
                .collect(),
            Tx::Pg(tx) => bind_all!(sqlx::query::<Postgres>(AssertSqlSafe(sql)), args)
                .fetch_all(&mut **tx)
                .await?
                .into_iter()
                .map(DbRow::Pg)
                .collect(),
        })
    }

    pub(crate) async fn fetch_optional(
        &mut self,
        template: &str,
        args: &[Arg],
    ) -> Result<Option<DbRow>, sqlx::Error> {
        Ok(self.fetch_all(template, args).await?.into_iter().next())
    }
}

/// A statement outside a transaction (a read, or a single-statement write).
pub(crate) async fn fetch_all(
    store: &Datastore,
    template: &str,
    args: &[Arg],
) -> Result<Vec<DbRow>, sqlx::Error> {
    let sql = render(engine_of(store), template);
    Ok(match store {
        Datastore::Sqlite(pool) => bind_all!(sqlx::query::<Sqlite>(AssertSqlSafe(sql)), args)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(DbRow::Sqlite)
            .collect(),
        Datastore::Postgres(pool) => bind_all!(sqlx::query::<Postgres>(AssertSqlSafe(sql)), args)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(DbRow::Pg)
            .collect(),
    })
}

pub(crate) async fn fetch_optional(
    store: &Datastore,
    template: &str,
    args: &[Arg],
) -> Result<Option<DbRow>, sqlx::Error> {
    Ok(fetch_all(store, template, args).await?.into_iter().next())
}

/// Which unique constraint a failed insert broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unique {
    RunId,
    OpenPerThread,
    Other,
}

/// `None` when the error is not a unique violation.
pub(crate) fn unique_violation(error: &sqlx::Error) -> Option<Unique> {
    let sqlx::Error::Database(db) = error else {
        return None;
    };
    if !db.is_unique_violation() {
        return None;
    }
    let constraint = db.constraint().unwrap_or_default();
    let message = db.message();
    Some(
        if constraint == "run_records_pkey" || message.contains("run_records.id") {
            Unique::RunId
        } else if constraint == "run_records_one_open_per_thread"
            || message.contains("run_records_one_open_per_thread")
            || message.contains("run_records.thread_id")
        {
            Unique::OpenPerThread
        } else {
            Unique::Other
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_render_per_engine() {
        let template = "SELECT {s:id}, {t:at} FROM t WHERE id = $1{uuid} AND d = $2{json} AND x < {now} {lock}";
        assert_eq!(
            render(Engine::Sqlite, template),
            "SELECT id, at FROM t WHERE id = $1 AND d = $2 AND x < strftime('%Y-%m-%dT%H:%M:%fZ','now') "
        );
        assert_eq!(
            render(Engine::Postgres, template),
            "SELECT (id)::text, to_char((at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
             FROM t WHERE id = $1::uuid AND d = $2::jsonb AND x < now()  FOR UPDATE"
        );
        assert_eq!(render(Engine::Postgres, "a {x} b {"), "a {x} b {");
        assert_eq!(placeholders(3, 2, "{uuid}"), "$3{uuid}, $4{uuid}");
    }

    #[test]
    fn lease_and_summary_markers_render() {
        assert!(render(Engine::Postgres, "{lease}").contains("make_interval(secs => 30)"));
        assert!(render(Engine::Sqlite, "{lease}").contains("'+30 seconds'"));
        assert!(render(Engine::Sqlite, "{sum:4}").contains("json_insert"));
        assert!(render(Engine::Postgres, "{sum:4}").contains("to_jsonb($4::text)"));
        assert_eq!(render(Engine::Postgres, "{seq}"), "DEFAULT");
    }
}
