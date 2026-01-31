//! Statement building and execution over either dialect.
//!
//! Every statement the SQL storage runs is built by a [`Stmt`]: SQL text assembled from constants
//! only, with each caller value bound as a numbered parameter (`$1`, `$2`, …, which both SQLite and
//! PostgreSQL accept) — never interpolated (sql-safety layer rule). The few places the dialects
//! disagree — how a JSON value or a list of strings is bound, how a timestamp is stored — are answered
//! here once, so the storage logic above is written once for both.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value as Json;
use sqlx::postgres::PgRow;
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Postgres, Row, Sqlite};

use super::{SqlDialect, SqlPool};
use crate::types::ports::{QueueError, StoreError};

/// One bound value, held in its dialect-neutral form until the statement is run.
enum Arg {
    Text(String),
    OptText(Option<String>),
    Int(i64),
    Float(f64),
    Bool(bool),
    Bytes(Option<Vec<u8>>),
    Ts(DateTime<Utc>),
    OptTs(Option<DateTime<Utc>>),
    /// A list of strings, matched with [`Stmt::in_list`].
    List(Vec<String>),
}

/// A statement under construction: constant SQL text plus the values bound to its placeholders.
pub(super) struct Stmt {
    dialect: SqlDialect,
    sql: String,
    args: Vec<Arg>,
}

impl Stmt {
    pub(super) fn new(dialect: SqlDialect) -> Self {
        Self {
            dialect,
            sql: String::new(),
            args: Vec::new(),
        }
    }

    /// Append constant SQL text. Never pass a caller value here — bind it instead.
    pub(super) fn sql(&mut self, text: &str) -> &mut Self {
        self.sql.push_str(text);
        self
    }

    fn bind(&mut self, arg: Arg) -> String {
        self.args.push(arg);
        format!("${}", self.args.len())
    }

    /// Bind a string; returns its placeholder.
    pub(super) fn text(&mut self, v: impl Into<String>) -> String {
        self.bind(Arg::Text(v.into()))
    }

    pub(super) fn opt_text(&mut self, v: Option<String>) -> String {
        self.bind(Arg::OptText(v))
    }

    pub(super) fn int(&mut self, v: i64) -> String {
        self.bind(Arg::Int(v))
    }

    pub(super) fn float(&mut self, v: f64) -> String {
        self.bind(Arg::Float(v))
    }

    pub(super) fn boolean(&mut self, v: bool) -> String {
        self.bind(Arg::Bool(v))
    }

    pub(super) fn bytes(&mut self, v: Option<Vec<u8>>) -> String {
        self.bind(Arg::Bytes(v))
    }

    /// Bind a timestamp: RFC 3339 ms `Z` text in SQLite, `TIMESTAMPTZ` in PostgreSQL (§B).
    pub(super) fn ts(&mut self, v: DateTime<Utc>) -> String {
        self.bind(Arg::Ts(v))
    }

    pub(super) fn opt_ts(&mut self, v: Option<DateTime<Utc>>) -> String {
        self.bind(Arg::OptTs(v))
    }

    /// Bind a JSON value for a JSON column: text in SQLite, cast to `JSONB` in PostgreSQL.
    pub(super) fn json(&mut self, v: &Json) -> String {
        let p = self.bind(Arg::Text(v.to_string()));
        match self.dialect {
            SqlDialect::Sqlite => p,
            SqlDialect::Postgres => format!("{p}::jsonb"),
        }
    }

    /// The predicate `<col> ∈ items`, with the list bound as one parameter: a JSON array read through
    /// `json_each` in SQLite, a `TEXT[]` in PostgreSQL. An empty list matches nothing in both.
    pub(super) fn in_list(&mut self, col: &str, items: &[String]) -> String {
        let p = self.bind(Arg::List(items.to_vec()));
        match self.dialect {
            SqlDialect::Sqlite => format!("{col} IN (SELECT value FROM json_each({p}))"),
            SqlDialect::Postgres => format!("{col} = ANY({p})"),
        }
    }

    /// The placeholder of `items` as a JSON array, for a `json_each` the statement names itself.
    pub(super) fn json_each(&mut self, items: &[String]) -> String {
        self.bind(Arg::List(items.to_vec()))
    }

    fn sqlite(self) -> sqlx::query::Query<'static, Sqlite, sqlx::sqlite::SqliteArguments> {
        let mut q = sqlx::query::<Sqlite>(AssertSqlSafe(self.sql));
        for a in self.args {
            q = match a {
                Arg::Text(v) => q.bind(v),
                Arg::OptText(v) => q.bind(v),
                Arg::Int(v) => q.bind(v),
                Arg::Float(v) => q.bind(v),
                Arg::Bool(v) => q.bind(v),
                Arg::Bytes(v) => q.bind(v),
                Arg::Ts(v) => q.bind(ts_text(v)),
                Arg::OptTs(v) => q.bind(v.map(ts_text)),
                Arg::List(v) => q.bind(Json::from(v).to_string()),
            };
        }
        q
    }

    fn postgres(self) -> sqlx::query::Query<'static, Postgres, sqlx::postgres::PgArguments> {
        let mut q = sqlx::query::<Postgres>(AssertSqlSafe(self.sql));
        for a in self.args {
            q = match a {
                Arg::Text(v) => q.bind(v),
                Arg::OptText(v) => q.bind(v),
                Arg::Int(v) => q.bind(v),
                Arg::Float(v) => q.bind(v),
                Arg::Bool(v) => q.bind(v),
                Arg::Bytes(v) => q.bind(v),
                Arg::Ts(v) => q.bind(v),
                Arg::OptTs(v) => q.bind(v),
                Arg::List(v) => q.bind(v),
            };
        }
        q
    }
}

/// The text form §B stores a SQLite timestamp in: RFC 3339, milliseconds, `Z`. Fixed width, so the
/// text orders as the instant does.
pub(super) fn ts_text(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// A JSON column in a select list, under `alias`: read as text in both dialects.
pub(super) fn json_col(dialect: SqlDialect, expr: &str, alias: &str) -> String {
    match dialect {
        SqlDialect::Sqlite => format!("{expr} AS {alias}"),
        SqlDialect::Postgres => format!("{expr}::text AS {alias}"),
    }
}

impl SqlPool {
    pub(super) async fn fetch(&self, st: Stmt) -> Result<Vec<SqlRow>, sqlx::Error> {
        Ok(match self {
            SqlPool::Sqlite(p) => st
                .sqlite()
                .fetch_all(p)
                .await?
                .into_iter()
                .map(SqlRow::Sqlite)
                .collect(),
            SqlPool::Postgres(p) => st
                .postgres()
                .fetch_all(p)
                .await?
                .into_iter()
                .map(SqlRow::Postgres)
                .collect(),
        })
    }

    /// Run a statement; returns the rows it affected.
    pub(super) async fn execute(&self, st: Stmt) -> Result<u64, sqlx::Error> {
        Ok(match self {
            SqlPool::Sqlite(p) => st.sqlite().execute(p).await?.rows_affected(),
            SqlPool::Postgres(p) => st.postgres().execute(p).await?.rows_affected(),
        })
    }

    /// Begin a write transaction. SQLite takes the write lock up front (`BEGIN IMMEDIATE`): a deferred
    /// transaction that reads and then writes cannot wait on another writer and fails at once instead.
    pub(super) async fn begin(&self) -> Result<Tx, sqlx::Error> {
        Ok(match self {
            SqlPool::Sqlite(p) => Tx::Sqlite(p.begin_with("BEGIN IMMEDIATE").await?),
            SqlPool::Postgres(p) => Tx::Postgres(p.begin().await?),
        })
    }
}

/// An open transaction on either dialect. Dropped without `commit`, it rolls back.
pub(super) enum Tx {
    Sqlite(sqlx::Transaction<'static, Sqlite>),
    Postgres(sqlx::Transaction<'static, Postgres>),
}

impl Tx {
    pub(super) async fn fetch(&mut self, st: Stmt) -> Result<Vec<SqlRow>, sqlx::Error> {
        Ok(match self {
            Tx::Sqlite(tx) => st
                .sqlite()
                .fetch_all(&mut **tx)
                .await?
                .into_iter()
                .map(SqlRow::Sqlite)
                .collect(),
            Tx::Postgres(tx) => st
                .postgres()
                .fetch_all(&mut **tx)
                .await?
                .into_iter()
                .map(SqlRow::Postgres)
                .collect(),
        })
    }

    pub(super) async fn execute(&mut self, st: Stmt) -> Result<u64, sqlx::Error> {
        Ok(match self {
            Tx::Sqlite(tx) => st.sqlite().execute(&mut **tx).await?.rows_affected(),
            Tx::Postgres(tx) => st.postgres().execute(&mut **tx).await?.rows_affected(),
        })
    }

    pub(super) async fn commit(self) -> Result<(), sqlx::Error> {
        match self {
            Tx::Sqlite(tx) => tx.commit().await,
            Tx::Postgres(tx) => tx.commit().await,
        }
    }
}

/// A result row of either dialect, read by column name.
pub(super) enum SqlRow {
    Sqlite(SqliteRow),
    Postgres(PgRow),
}

fn decode(col: &str, e: sqlx::Error) -> StoreError {
    StoreError::Internal(format!("decode {col}: {e}"))
}

impl SqlRow {
    pub(super) fn text(&self, col: &str) -> Result<String, StoreError> {
        match self {
            SqlRow::Sqlite(r) => r.try_get(col),
            SqlRow::Postgres(r) => r.try_get(col),
        }
        .map_err(|e| decode(col, e))
    }

    pub(super) fn opt_text(&self, col: &str) -> Result<Option<String>, StoreError> {
        match self {
            SqlRow::Sqlite(r) => r.try_get(col),
            SqlRow::Postgres(r) => r.try_get(col),
        }
        .map_err(|e| decode(col, e))
    }

    /// An integer column: `INTEGER` is 64-bit in SQLite and 32-bit in PostgreSQL, where a `COUNT`
    /// is 64-bit; both read as `i64`.
    pub(super) fn int(&self, col: &str) -> Result<i64, StoreError> {
        match self {
            SqlRow::Sqlite(r) => r.try_get::<i64, _>(col),
            SqlRow::Postgres(r) => r
                .try_get::<i64, _>(col)
                .or_else(|_| r.try_get::<i32, _>(col).map(i64::from)),
        }
        .map_err(|e| decode(col, e))
    }

    pub(super) fn float(&self, col: &str) -> Result<f64, StoreError> {
        match self {
            SqlRow::Sqlite(r) => r.try_get(col),
            SqlRow::Postgres(r) => r.try_get(col),
        }
        .map_err(|e| decode(col, e))
    }

    pub(super) fn boolean(&self, col: &str) -> Result<bool, StoreError> {
        match self {
            SqlRow::Sqlite(r) => r.try_get(col),
            SqlRow::Postgres(r) => r.try_get(col),
        }
        .map_err(|e| decode(col, e))
    }

    pub(super) fn bytes(&self, col: &str) -> Result<Option<Vec<u8>>, StoreError> {
        match self {
            SqlRow::Sqlite(r) => r.try_get(col),
            SqlRow::Postgres(r) => r.try_get(col),
        }
        .map_err(|e| decode(col, e))
    }

    pub(super) fn ts(&self, col: &str) -> Result<DateTime<Utc>, StoreError> {
        self.opt_ts(col)?
            .ok_or_else(|| StoreError::Internal(format!("decode {col}: null")))
    }

    pub(super) fn opt_ts(&self, col: &str) -> Result<Option<DateTime<Utc>>, StoreError> {
        match self {
            SqlRow::Sqlite(r) => {
                let raw: Option<String> = r.try_get(col).map_err(|e| decode(col, e))?;
                raw.map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|d| d.with_timezone(&Utc))
                        .map_err(|e| StoreError::Internal(format!("decode {col}: {e}")))
                })
                .transpose()
            }
            SqlRow::Postgres(r) => r.try_get(col).map_err(|e| decode(col, e)),
        }
    }

    /// A JSON column selected through [`json_col`].
    pub(super) fn json(&self, col: &str) -> Result<Json, StoreError> {
        let raw = self.text(col)?;
        serde_json::from_str(&raw).map_err(|e| StoreError::Internal(format!("decode {col}: {e}")))
    }
}

/// The PostgreSQL code for a statement cancelled by `statement_timeout`.
const PG_QUERY_CANCELED: &str = "57014";

/// Map a driver error to the store's error: a timed-out pool acquire or a cancelled statement is a
/// timeout; a failure to decode is an internal fault; anything else means the store did not answer
/// as it should (`StoreError::Unavailable`, as `map_db_err` does for SurrealDB).
pub(super) fn map_sql_err(e: sqlx::Error) -> StoreError {
    match &e {
        sqlx::Error::PoolTimedOut => StoreError::Timeout,
        sqlx::Error::Database(db) if db.code().as_deref() == Some(PG_QUERY_CANCELED) => {
            StoreError::Timeout
        }
        sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::TypeNotFound { .. }
        | sqlx::Error::Decode(_) => StoreError::Internal(e.to_string()),
        _ => StoreError::Unavailable(e.to_string()),
    }
}

/// Map a driver error on a queue path (X1: every queue failure is `503 QUEUE_UNAVAILABLE`).
pub(super) fn map_queue_err(e: sqlx::Error) -> QueueError {
    QueueError::BackendUnavailable(e.to_string())
}

/// Map a store-shaped error met on a queue path.
pub(super) fn store_to_queue(e: StoreError) -> QueueError {
    QueueError::BackendUnavailable(e.to_string())
}

/// Whether a driver error is a unique-constraint violation.
pub(super) fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}
