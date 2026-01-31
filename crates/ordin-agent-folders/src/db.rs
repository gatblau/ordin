//! One query layer over both engines (Journal's `Datastore`, SA-RECON-03).
//!
//! Every statement is written once with `$1, $2, …` placeholders in order of first appearance
//! (SQLite numbers `$N` parameters the same way) and markers for the few places the engines differ.
//! Values travel as text; PostgreSQL casts them to its column types on the way in and back to text
//! on the way out, so one decoder reads both.
//!
//! | Marker      | SQLite | PostgreSQL |
//! |-------------|--------|------------|
//! | `{uuid}`    | —      | `::uuid` (after a placeholder) |
//! | `{ts}`      | —      | `::timestamptz` |
//! | `{t:EXPR}`  | `EXPR` | `EXPR` as `YYYY-MM-DDTHH:MM:SS.mmmZ` text |
//! | `{s:EXPR}`  | `EXPR` | `(EXPR)::text` |
//!
//! Every value is bound; `AssertSqlSafe` wraps only text built from these fixed templates.

use ordin_journal::Datastore;
use sqlx::postgres::{PgRow, Postgres};
use sqlx::sqlite::{Sqlite, SqliteRow};
use sqlx::{AssertSqlSafe, Row, Transaction};

use crate::error::AfmError;

/// Renders a template for one engine (see the module table).
pub(crate) fn render(postgres: bool, template: &str) -> String {
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
        match marker.split_once(':') {
            Some(("t", expr)) if postgres => out.push_str(&format!(
                "to_char(({expr}) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')"
            )),
            Some(("s", expr)) if postgres => out.push_str(&format!("({expr})::text")),
            Some((_, expr)) => out.push_str(expr),
            None => match (marker, postgres) {
                ("uuid", true) => out.push_str("::uuid"),
                ("ts", true) => out.push_str("::timestamptz"),
                ("uuid" | "ts", false) => {}
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

/// One bound value.
#[derive(Debug, Clone)]
pub(crate) enum Arg {
    Text(String),
}

impl Arg {
    pub(crate) fn text(value: impl ToString) -> Arg {
        Arg::Text(value.to_string())
    }
}

/// One result row of either engine; every column is read as text.
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
        self.opt_text(i)?.ok_or(sqlx::Error::ColumnNotFound(
            "unexpected NULL in a NOT NULL column".to_owned(),
        ))
    }
}

macro_rules! bind_all {
    ($query:expr, $args:expr) => {{
        let mut query = $query;
        for arg in $args {
            query = match arg {
                Arg::Text(value) => query.bind(value.clone()),
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
    /// upgrade. PostgreSQL: `BEGIN`.
    pub(crate) async fn begin(store: &Datastore) -> Result<Tx, sqlx::Error> {
        Ok(match store {
            Datastore::Sqlite(pool) => Tx::Sqlite(pool.begin_with("BEGIN IMMEDIATE").await?),
            Datastore::Postgres(pool) => Tx::Pg(pool.begin().await?),
        })
    }

    fn postgres(&self) -> bool {
        matches!(self, Tx::Pg(_))
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
        let sql = render(self.postgres(), template);
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
        let sql = render(self.postgres(), template);
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

/// Reads outside a transaction.
pub(crate) async fn fetch_all(
    store: &Datastore,
    template: &str,
    args: &[Arg],
) -> Result<Vec<DbRow>, sqlx::Error> {
    Ok(match store {
        Datastore::Sqlite(pool) => {
            let sql = render(false, template);
            bind_all!(sqlx::query::<Sqlite>(AssertSqlSafe(sql)), args)
                .fetch_all(pool)
                .await?
                .into_iter()
                .map(DbRow::Sqlite)
                .collect()
        }
        Datastore::Postgres(pool) => {
            let sql = render(true, template);
            bind_all!(sqlx::query::<Postgres>(AssertSqlSafe(sql)), args)
                .fetch_all(pool)
                .await?
                .into_iter()
                .map(DbRow::Pg)
                .collect()
        }
    })
}

pub(crate) async fn fetch_optional(
    store: &Datastore,
    template: &str,
    args: &[Arg],
) -> Result<Option<DbRow>, sqlx::Error> {
    Ok(fetch_all(store, template, args).await?.into_iter().next())
}

/// True when the failure is a busy or locked database, which a repeat may clear (step 10).
fn is_busy(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::PoolTimedOut => true,
        sqlx::Error::Database(db) => match db.code() {
            Some(code) => {
                // SQLite primary codes 5 (busy) and 6 (locked), with their extended forms in the
                // low byte; PostgreSQL serialisation failure, deadlock and lock not available.
                let sqlite = code
                    .parse::<i64>()
                    .map(|n| matches!(n & 0xff, 5 | 6))
                    .unwrap_or(false);
                sqlite || matches!(code.as_ref(), "40001" | "40P01" | "55P03")
            }
            None => false,
        },
        _ => false,
    }
}

/// Step 10: any datastore error becomes `AFM_DATASTORE_FAILED`, logged with the statement name and
/// never a parameter value. The sqlx error is dropped: its text can hold a value.
pub(crate) fn failed(statement: &'static str) -> impl Fn(sqlx::Error) -> AfmError {
    move |error| {
        let retryable = is_busy(&error);
        crate::log::datastore_failed(statement, retryable);
        AfmError::DatastoreFailed { retryable }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_render_per_engine() {
        let template = "SELECT {s:id}, {t:at} FROM t WHERE id = $1{uuid} AND at < $2{ts}";
        assert_eq!(
            render(false, template),
            "SELECT id, at FROM t WHERE id = $1 AND at < $2"
        );
        assert_eq!(
            render(true, template),
            "SELECT (id)::text, to_char((at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') FROM t \
             WHERE id = $1::uuid AND at < $2::timestamptz"
        );
        assert_eq!(render(true, "a {x} b {"), "a {x} b {");
    }
}
