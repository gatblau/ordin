//! One query layer over both engines (SA-RECON-03: the tables live in the Journal's datastore, which
//! is SQLite everywhere and PostgreSQL in the scaled shape). Every statement is written once, with
//! `$1, $2, …` placeholders, and a few markers where the engines differ; values travel as text or
//! integers, PostgreSQL casts them to its column types on the way in and back to text on the way
//! out, so one decoder reads both. Every value is bound; `AssertSqlSafe` wraps only text built
//! from this crate's fixed templates and generated placeholder lists, never a caller's value.
//!
//! | Marker       | SQLite   | PostgreSQL                             |
//! |--------------|----------|----------------------------------------|
//! | `{uuid}`     | -        | `::uuid`                               |
//! | `{ts}`       | -        | `::timestamptz`                        |
//! | `{json}`     | -        | `::jsonb`                              |
//! | `{t:EXPR}`   | `EXPR`   | `EXPR` as `YYYY-MM-DDTHH:MM:SS.mmmZ`   |
//! | `{s:EXPR}`   | `EXPR`   | `(EXPR)::text`                         |

use ordin_journal::Datastore;
use sqlx::postgres::{PgRow, Postgres};
use sqlx::sqlite::{Sqlite, SqliteRow};
use sqlx::{AssertSqlSafe, Row};

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
    OptInt(Option<i64>),
}

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
        match (marker.split_once(':'), pg) {
            (Some(("t", expr)), true) => out.push_str(&format!(
                "to_char(({expr}) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')"
            )),
            (Some(("s", expr)), true) => out.push_str(&format!("({expr})::text")),
            (Some(("t" | "s", expr)), false) => out.push_str(expr),
            (Some(_), _) => {
                out.push('{');
                out.push_str(marker);
                out.push('}');
            }
            (None, _) => match (marker, pg) {
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
    pub(crate) fn opt_int(&self, i: usize) -> Result<Option<i64>, sqlx::Error> {
        match self {
            DbRow::Sqlite(row) => row.try_get::<Option<i64>, _>(i),
            DbRow::Pg(row) => match row.try_get::<Option<i64>, _>(i) {
                Ok(value) => Ok(value),
                Err(_) => row.try_get::<Option<i32>, _>(i).map(|v| v.map(i64::from)),
            },
        }
    }

    pub(crate) fn int(&self, i: usize) -> Result<i64, sqlx::Error> {
        self.opt_int(i)?
            .ok_or(sqlx::Error::ColumnNotFound(format!("#{i} is null")))
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

/// One statement that changes rows; returns how many.
pub(crate) async fn execute(
    store: &Datastore,
    template: &str,
    args: &[Arg],
) -> Result<u64, sqlx::Error> {
    let sql = render(engine_of(store), template);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_render_per_engine() {
        let template =
            "SELECT {s:id}, {t:at} FROM t WHERE id = $1{uuid} AND d = $2{json} AND w = $3{ts}";
        assert_eq!(
            render(Engine::Sqlite, template),
            "SELECT id, at FROM t WHERE id = $1 AND d = $2 AND w = $3"
        );
        assert_eq!(
            render(Engine::Postgres, template),
            "SELECT (id)::text, to_char((at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
             FROM t WHERE id = $1::uuid AND d = $2::jsonb AND w = $3::timestamptz"
        );
        assert_eq!(render(Engine::Postgres, "a {x} b {"), "a {x} b {");
        assert_eq!(render(Engine::Sqlite, "a {q:x} b"), "a {q:x} b");
        assert_eq!(placeholders(3, 2, "{uuid}"), "$3{uuid}, $4{uuid}");
    }
}
