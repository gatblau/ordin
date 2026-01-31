//! One query layer over both engines, as the Journal's: every statement is written once with
//! `$1, $2, …` placeholders in order of first appearance. Values are bound in their column's type:
//! on PostgreSQL identifiers are `uuid`, booleans `boolean` and times `timestamptz`; on SQLite they
//! are text (lowercase hyphenated, RFC 3339 ms `Z`) and `0`/`1`. All SQL is built from constants and
//! generated placeholders; every value is bound (C10, sql-safety).

use chrono::{DateTime, Utc};
use ordin_types::time::Timestamp;
use sqlx::postgres::{PgRow, Postgres};
use sqlx::sqlite::{Sqlite, SqliteRow};
use sqlx::{AssertSqlSafe, Row, Transaction};
use uuid::Uuid;

use crate::error::MemoryError;

/// One bound value.
#[derive(Debug, Clone)]
pub(crate) enum Val {
    Text(Option<String>),
    Int(i64),
    Bool(bool),
    Uuid(Option<Uuid>),
    Ts(Option<Timestamp>),
}

/// A statement and its values, in placeholder order.
#[derive(Debug, Clone)]
pub(crate) struct Q {
    sql: String,
    args: Vec<Val>,
}

pub(crate) fn q(sql: impl Into<String>) -> Q {
    Q {
        sql: sql.into(),
        args: Vec::new(),
    }
}

impl Q {
    pub(crate) fn text(mut self, value: impl Into<String>) -> Q {
        self.args.push(Val::Text(Some(value.into())));
        self
    }

    pub(crate) fn opt_text(mut self, value: Option<&str>) -> Q {
        self.args.push(Val::Text(value.map(str::to_owned)));
        self
    }

    pub(crate) fn int(mut self, value: i64) -> Q {
        self.args.push(Val::Int(value));
        self
    }

    pub(crate) fn boolean(mut self, value: bool) -> Q {
        self.args.push(Val::Bool(value));
        self
    }

    pub(crate) fn uuid(mut self, value: Uuid) -> Q {
        self.args.push(Val::Uuid(Some(value)));
        self
    }

    pub(crate) fn opt_uuid(mut self, value: Option<Uuid>) -> Q {
        self.args.push(Val::Uuid(value));
        self
    }

    pub(crate) fn ts(mut self, value: Timestamp) -> Q {
        self.args.push(Val::Ts(Some(value)));
        self
    }

    pub(crate) fn opt_ts(mut self, value: Option<Timestamp>) -> Q {
        self.args.push(Val::Ts(value));
        self
    }
}

/// `$first, $first+1, …` for an `IN` list of `count` values.
pub(crate) fn placeholders(first: usize, count: usize) -> String {
    (first..first + count)
        .map(|n| format!("${n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `%` and `_` of `value` escaped for `LIKE … ESCAPE '\'`.
pub(crate) fn like_prefix(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 1);
    for c in value.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// One result row of either engine.
pub(crate) enum R {
    Sqlite(SqliteRow),
    Pg(PgRow),
}

impl R {
    pub(crate) fn opt_text(&self, column: &str) -> Result<Option<String>, MemoryError> {
        Ok(match self {
            R::Sqlite(row) => row.try_get::<Option<String>, _>(column)?,
            R::Pg(row) => row.try_get::<Option<String>, _>(column)?,
        })
    }

    pub(crate) fn text(&self, column: &str) -> Result<String, MemoryError> {
        self.opt_text(column)?.ok_or_else(MemoryError::internal)
    }

    /// `integer`, `smallint`, `bigint` and `count(*)`.
    pub(crate) fn int(&self, column: &str) -> Result<i64, MemoryError> {
        Ok(match self {
            R::Sqlite(row) => row.try_get::<i64, _>(column)?,
            R::Pg(row) => match row.try_get::<i64, _>(column) {
                Ok(value) => value,
                Err(_) => match row.try_get::<i32, _>(column) {
                    Ok(value) => i64::from(value),
                    Err(_) => i64::from(row.try_get::<i16, _>(column)?),
                },
            },
        })
    }

    pub(crate) fn boolean(&self, column: &str) -> Result<bool, MemoryError> {
        Ok(match self {
            R::Sqlite(row) => row.try_get::<i64, _>(column)? != 0,
            R::Pg(row) => row.try_get::<bool, _>(column)?,
        })
    }

    pub(crate) fn opt_uuid(&self, column: &str) -> Result<Option<Uuid>, MemoryError> {
        match self {
            R::Sqlite(row) => row
                .try_get::<Option<String>, _>(column)?
                .map(|raw| Uuid::parse_str(&raw).map_err(|_| MemoryError::internal()))
                .transpose(),
            R::Pg(row) => Ok(row.try_get::<Option<Uuid>, _>(column)?),
        }
    }

    pub(crate) fn uuid(&self, column: &str) -> Result<Uuid, MemoryError> {
        self.opt_uuid(column)?.ok_or_else(MemoryError::internal)
    }

    pub(crate) fn opt_ts(&self, column: &str) -> Result<Option<Timestamp>, MemoryError> {
        match self {
            R::Sqlite(row) => row
                .try_get::<Option<String>, _>(column)?
                .map(|raw| {
                    ordin_types::time::parse_rfc3339_ms_utc(&raw)
                        .map(Timestamp)
                        .map_err(|_| MemoryError::internal())
                })
                .transpose(),
            R::Pg(row) => Ok(row
                .try_get::<Option<DateTime<Utc>>, _>(column)?
                .map(Timestamp)),
        }
    }

    pub(crate) fn ts(&self, column: &str) -> Result<Timestamp, MemoryError> {
        self.opt_ts(column)?.ok_or_else(MemoryError::internal)
    }
}

macro_rules! bind_sqlite {
    ($query:expr, $args:expr) => {{
        let mut query = $query;
        for arg in $args {
            query = match arg {
                Val::Text(value) => query.bind(value.clone()),
                Val::Int(value) => query.bind(*value),
                Val::Bool(value) => query.bind(i64::from(*value)),
                Val::Uuid(value) => query.bind(value.map(|v| v.to_string())),
                Val::Ts(value) => query.bind(value.map(|v| v.to_wire())),
            };
        }
        query
    }};
}

macro_rules! bind_pg {
    ($query:expr, $args:expr) => {{
        let mut query = $query;
        for arg in $args {
            query = match arg {
                Val::Text(value) => query.bind(value.clone()),
                Val::Int(value) => query.bind(*value),
                Val::Bool(value) => query.bind(*value),
                Val::Uuid(value) => query.bind(*value),
                Val::Ts(value) => query.bind(value.map(|v| v.0)),
            };
        }
        query
    }};
}

/// The datastore of this module's own tables.
#[derive(Clone)]
pub(crate) enum Db {
    Sqlite(sqlx::SqlitePool),
    Postgres(sqlx::PgPool),
}

impl Db {
    pub(crate) fn is_postgres(&self) -> bool {
        matches!(self, Db::Postgres(_))
    }

    pub(crate) async fn exec(&self, st: Q) -> Result<u64, MemoryError> {
        Ok(match self {
            Db::Sqlite(pool) => {
                bind_sqlite!(sqlx::query::<Sqlite>(AssertSqlSafe(st.sql)), &st.args)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
            Db::Postgres(pool) => {
                bind_pg!(sqlx::query::<Postgres>(AssertSqlSafe(st.sql)), &st.args)
                    .execute(pool)
                    .await?
                    .rows_affected()
            }
        })
    }

    pub(crate) async fn all(&self, st: Q) -> Result<Vec<R>, MemoryError> {
        Ok(match self {
            Db::Sqlite(pool) => {
                bind_sqlite!(sqlx::query::<Sqlite>(AssertSqlSafe(st.sql)), &st.args)
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(R::Sqlite)
                    .collect()
            }
            Db::Postgres(pool) => {
                bind_pg!(sqlx::query::<Postgres>(AssertSqlSafe(st.sql)), &st.args)
                    .fetch_all(pool)
                    .await?
                    .into_iter()
                    .map(R::Pg)
                    .collect()
            }
        })
    }

    pub(crate) async fn first(&self, st: Q) -> Result<Option<R>, MemoryError> {
        Ok(self.all(st).await?.into_iter().next())
    }

    pub(crate) async fn begin(&self) -> Result<Tx, MemoryError> {
        Ok(match self {
            Db::Sqlite(pool) => Tx::Sqlite(pool.begin_with("BEGIN IMMEDIATE").await?),
            Db::Postgres(pool) => Tx::Pg(pool.begin().await?),
        })
    }
}

/// One transaction on either engine. Dropping it without `commit` rolls it back.
pub(crate) enum Tx {
    Sqlite(Transaction<'static, Sqlite>),
    Pg(Transaction<'static, Postgres>),
}

impl Tx {
    pub(crate) async fn commit(self) -> Result<(), MemoryError> {
        match self {
            Tx::Sqlite(tx) => tx.commit().await?,
            Tx::Pg(tx) => tx.commit().await?,
        }
        Ok(())
    }

    pub(crate) async fn exec(&mut self, st: Q) -> Result<u64, MemoryError> {
        Ok(match self {
            Tx::Sqlite(tx) => bind_sqlite!(sqlx::query::<Sqlite>(AssertSqlSafe(st.sql)), &st.args)
                .execute(&mut **tx)
                .await?
                .rows_affected(),
            Tx::Pg(tx) => bind_pg!(sqlx::query::<Postgres>(AssertSqlSafe(st.sql)), &st.args)
                .execute(&mut **tx)
                .await?
                .rows_affected(),
        })
    }
}
