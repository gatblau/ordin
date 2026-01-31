//! One query layer over both engines (SA-DB-01).
//!
//! Every statement is written once, with `$1, $2, …` placeholders in order of first appearance
//! (SQLite numbers `$N` parameters the same way), and with markers for the few places the engines
//! differ. Values travel as text or integers on both engines; PostgreSQL casts them to its column
//! types on the way in and back to text on the way out, so one decoder reads both.
//!
//! | Marker      | SQLite | PostgreSQL |
//! |-------------|--------|------------|
//! | `{uuid}`    | —      | `::uuid` (after a placeholder) |
//! | `{ts}`      | —      | `::timestamptz` |
//! | `{json}`    | —      | `::jsonb` |
//! | `{t:EXPR}`  | `EXPR` | `EXPR` as `YYYY-MM-DDTHH:MM:SS.mmmZ` text |
//! | `{s:EXPR}`  | `EXPR` | `(EXPR)::text` |
//!
//! Timestamps are stored on SQLite as fixed-width RFC 3339 text, so comparing and ordering the text
//! orders the instants. Every value is bound; `AssertSqlSafe` wraps only text built from these
//! fixed templates and generated placeholders, never from a caller's value.

use sqlx::postgres::{PgRow, Postgres};
use sqlx::sqlite::{Sqlite, SqliteRow};
use sqlx::{AssertSqlSafe, Row, Transaction};

use crate::datastore::{Datastore, Engine};
use crate::error::JournalError;

/// One bound value.
#[derive(Debug, Clone)]
pub(crate) enum Arg {
    Text(String),
    OptText(Option<String>),
    Int(i64),
    OptInt(Option<i64>),
}

/// Renders a template for one engine (see the module table).
pub(crate) fn render(engine: Engine, template: &str) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let marker = &after[..close];
        let pg = engine == Engine::Postgres;
        match marker.split_once(':') {
            Some(("t", expr)) if pg => out.push_str(&format!(
                "to_char(({expr}) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')"
            )),
            Some(("s", expr)) if pg => out.push_str(&format!("({expr})::text")),
            Some((_, expr)) => out.push_str(expr),
            None => match (marker, pg) {
                ("uuid", true) => out.push_str("::uuid"),
                ("ts", true) => out.push_str("::timestamptz"),
                ("json", true) => out.push_str("::jsonb"),
                ("uuid" | "ts" | "json", false) => {}
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

/// `$first, $first+1, …` for `count` values, each followed by `suffix` (a marker such as `{uuid}`).
pub(crate) fn placeholders(first: usize, count: usize, suffix: &str) -> String {
    (first..first + count)
        .map(|n| format!("${n}{suffix}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A `WHERE` clause built from conditions whose placeholders are written `#1`, `#2`, … relative
/// to the condition's own values; `add` numbers them after the values already held.
#[derive(Default)]
pub(crate) struct Conditions {
    clauses: Vec<String>,
    pub(crate) args: Vec<Arg>,
}

impl Conditions {
    pub(crate) fn add(&mut self, clause: &str, values: Vec<Arg>) {
        let base = self.args.len();
        let mut out = String::with_capacity(clause.len() + 8);
        let mut chars = clause.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek().and_then(|d| d.to_digit(10))) {
                ('#', Some(k)) => {
                    chars.next();
                    out.push_str(&format!("${}", base + k as usize));
                }
                _ => out.push(c),
            }
        }
        self.clauses.push(out);
        self.args.extend(values);
    }

    /// ` WHERE a AND b`, or nothing.
    pub(crate) fn sql(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.clauses.join(" AND "))
        }
    }

    /// The number the next placeholder after these values takes.
    pub(crate) fn next(&self) -> usize {
        self.args.len() + 1
    }
}

/// One result row of either engine.
pub(crate) enum DbRow {
    Sqlite(SqliteRow),
    Pg(PgRow),
}

impl DbRow {
    pub(crate) fn text(&self, i: usize) -> Result<String, JournalError> {
        self.opt_text(i)?.ok_or(JournalError::Unavailable)
    }

    pub(crate) fn opt_text(&self, i: usize) -> Result<Option<String>, JournalError> {
        Ok(match self {
            DbRow::Sqlite(row) => row.try_get::<Option<String>, _>(i)?,
            DbRow::Pg(row) => row.try_get::<Option<String>, _>(i)?,
        })
    }

    pub(crate) fn int(&self, i: usize) -> Result<i64, JournalError> {
        self.opt_int(i)?.ok_or(JournalError::Unavailable)
    }

    /// PostgreSQL `integer` columns read as `i32`, `bigint` and `count(*)` as `i64`.
    pub(crate) fn opt_int(&self, i: usize) -> Result<Option<i64>, JournalError> {
        Ok(match self {
            DbRow::Sqlite(row) => row.try_get::<Option<i64>, _>(i)?,
            DbRow::Pg(row) => match row.try_get::<Option<i64>, _>(i) {
                Ok(value) => value,
                Err(_) => row.try_get::<Option<i32>, _>(i)?.map(i64::from),
            },
        })
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
                Arg::OptInt(value) => query.bind(*value),
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
    /// SQLite: `BEGIN IMMEDIATE`, so the write lock is taken at once and a reader never has to
    /// upgrade (Internal Logic 2.8). PostgreSQL: `BEGIN`.
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

/// Statements outside a transaction (reads, and single-statement writes).
pub(crate) async fn execute(
    store: &Datastore,
    template: &str,
    args: &[Arg],
) -> Result<u64, sqlx::Error> {
    let sql = render(store.engine(), template);
    Ok(match store {
        Datastore::Sqlite(pool) => bind_all!(sqlx::query::<Sqlite>(AssertSqlSafe(sql)), args)
            .execute(pool)
            .await?
            .rows_affected(),
        Datastore::Postgres(pool) => bind_all!(sqlx::query::<Postgres>(AssertSqlSafe(sql)), args)
            .execute(pool)
            .await?
            .rows_affected(),
    })
}

pub(crate) async fn fetch_all(
    store: &Datastore,
    template: &str,
    args: &[Arg],
) -> Result<Vec<DbRow>, sqlx::Error> {
    let sql = render(store.engine(), template);
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

/// True when the statement failed on a unique or primary-key constraint.
pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_unique_violation())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_render_per_engine() {
        let template =
            "SELECT {s:id}, {t:at} FROM t WHERE id = $1{uuid} AND at < $2{ts} AND d = $3{json}";
        assert_eq!(
            render(Engine::Sqlite, template),
            "SELECT id, at FROM t WHERE id = $1 AND at < $2 AND d = $3"
        );
        assert_eq!(
            render(Engine::Postgres, template),
            "SELECT (id)::text, to_char((at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') FROM t \
             WHERE id = $1::uuid AND at < $2::timestamptz AND d = $3::jsonb"
        );
        // An unknown marker and an unclosed brace pass through unchanged.
        assert_eq!(render(Engine::Postgres, "a {x} b {"), "a {x} b {");
        assert_eq!(placeholders(3, 2, "{uuid}"), "$3{uuid}, $4{uuid}");
    }

    #[test]
    fn conditions_number_their_placeholders_in_order() {
        let mut conditions = Conditions::default();
        assert_eq!(conditions.sql(), "");
        conditions.add("run_id = #1{uuid}", vec![Arg::Text("r".into())]);
        conditions.add(
            "(at < #1{ts} OR (at = #1{ts} AND id < #2{uuid}))",
            vec![Arg::Text("t".into()), Arg::Text("i".into())],
        );
        assert_eq!(
            conditions.sql(),
            " WHERE run_id = $1{uuid} AND (at < $2{ts} OR (at = $2{ts} AND id < $3{uuid}))"
        );
        assert_eq!(conditions.next(), 4);
    }
}
