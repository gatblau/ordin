//! The program's one datastore (JournalAndArtefactStore.md *Public Interface*, *Datastore*;
//! Internal Logic 1 and 15.3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::error::JournalError;

/// At most 8 SQLite connections per program (Internal Logic 1.1).
const SQLITE_MAX_CONNECTIONS: u32 = 8;
/// A writer waits at most this long for the SQLite file's lock (Internal Logic 1.1).
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// A PostgreSQL connection is waited for at most this long (Internal Logic 1.2).
const PG_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// The program's one datastore. Cloning is cheap (pools are reference-counted).
#[derive(Clone)]
pub enum Datastore {
    Sqlite(sqlx::SqlitePool),
    Postgres(sqlx::PgPool),
}

pub enum DatastoreConfig {
    /// Desktop, personal server, agent single shape.
    Sqlite {
        path: std::path::PathBuf,
        backup_dir: std::path::PathBuf,
    },
    /// Agent scaled shape.
    Postgres {
        url: DatabaseUrl,
        max_connections: u32,
    },
}

/// A PostgreSQL connection string. `Debug` and `Display` print `DatabaseUrl(<redacted>)`.
pub struct DatabaseUrl(String);

impl DatabaseUrl {
    pub fn new(s: String) -> Self {
        DatabaseUrl(s)
    }

    /// The connection string, for the one call that connects; never logged or put in an error.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseUrl(<redacted>)")
    }
}

impl std::fmt::Display for DatabaseUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DatabaseUrl(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Sqlite,
    Postgres,
}

impl Engine {
    /// The value of the `engine` log field.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Engine::Sqlite => "sqlite",
            Engine::Postgres => "postgres",
        }
    }
}

// SPEC-GAP-1: `Datastore::Sqlite` holds only the pool, yet `MigrationRegistry::apply(&Datastore, …)`
// must write its backup copy into `DatastoreConfig::Sqlite::backup_dir`. The folder given to
// `Datastore::open` is kept here, keyed by the database file, and read back by `apply`; a datastore
// built without `open` falls back to `<folder of the file>/backups`, the spec's constant
// (*Configuration and constants*: `<application data root>/backups/`).
fn backup_dirs() -> &'static Mutex<HashMap<PathBuf, PathBuf>> {
    static DIRS: OnceLock<Mutex<HashMap<PathBuf, PathBuf>>> = OnceLock::new();
    DIRS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Datastore {
    pub async fn open(config: DatastoreConfig) -> Result<Datastore, JournalError> {
        let store = match config {
            DatastoreConfig::Sqlite { path, backup_dir } => {
                let store = open_sqlite(&path).await?;
                if let Ok(mut dirs) = backup_dirs().lock() {
                    dirs.insert(path, backup_dir);
                }
                store
            }
            DatastoreConfig::Postgres {
                url,
                max_connections,
            } => open_postgres(&url, max_connections).await?,
        };
        tracing::info!(
            component = "journal",
            engine = store.engine().as_str(),
            "journal.open.done"
        );
        Ok(store)
    }

    pub fn engine(&self) -> Engine {
        match self {
            Datastore::Sqlite(_) => Engine::Sqlite,
            Datastore::Postgres(_) => Engine::Postgres,
        }
    }

    /// The pool for modules that own SQLite tables (2B-1 item 5).
    pub fn sqlite(&self) -> Option<&sqlx::SqlitePool> {
        match self {
            Datastore::Sqlite(pool) => Some(pool),
            Datastore::Postgres(_) => None,
        }
    }

    pub fn postgres(&self) -> Option<&sqlx::PgPool> {
        match self {
            Datastore::Postgres(pool) => Some(pool),
            Datastore::Sqlite(_) => None,
        }
    }

    /// Closes every connection; called last at shutdown (SA-XC-13). SQLite folds the write-ahead
    /// log back into the file first (Internal Logic 15.3).
    pub async fn close(&self) {
        match self {
            Datastore::Sqlite(pool) => {
                // A failed checkpoint leaves the log beside the file; SQLite replays it at the next open.
                let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                    .execute(pool)
                    .await;
                pool.close().await;
            }
            Datastore::Postgres(pool) => pool.close().await,
        }
    }

    /// The folder the backup copy before migrating goes to (SQLite only).
    pub(crate) fn backup_dir(&self) -> Option<PathBuf> {
        let pool = self.sqlite()?;
        let file = pool.connect_options().get_filename().to_path_buf();
        let kept = backup_dirs()
            .lock()
            .ok()
            .and_then(|dirs| dirs.get(&file).cloned());
        kept.or_else(|| file.parent().map(|parent| parent.join("backups")))
    }
}

/// Internal Logic 1.1.
async fn open_sqlite(path: &Path) -> Result<Datastore, JournalError> {
    let failed = |reason: &'static str| {
        tracing::error!(
            component = "journal",
            engine = "sqlite",
            reason,
            "journal.open.failed"
        );
        JournalError::OpenFailed
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if !parent.exists() {
            create_private_dir(parent).map_err(|_| failed("folder"))?;
        }
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(SQLITE_BUSY_TIMEOUT);
    let pool = SqlitePoolOptions::new()
        .max_connections(SQLITE_MAX_CONNECTIONS)
        .connect_with(options)
        .await
        .map_err(|_| failed("open"))?;
    set_private_file(path).map_err(|_| failed("permissions"))?;
    let check: Result<String, sqlx::Error> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_one(&pool)
        .await;
    match check {
        Ok(answer) if answer == "ok" => Ok(Datastore::Sqlite(pool)),
        _ => {
            pool.close().await;
            Err(failed("integrity"))
        }
    }
}

/// Internal Logic 1.2. A refused connection is retryable; the program retries with backoff.
async fn open_postgres(url: &DatabaseUrl, max_connections: u32) -> Result<Datastore, JournalError> {
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(PG_ACQUIRE_TIMEOUT)
        .connect(url.expose())
        .await;
    match pool {
        Ok(pool) => Ok(Datastore::Postgres(pool)),
        Err(sqlx::Error::Configuration(_)) => {
            tracing::error!(
                component = "journal",
                engine = "postgres",
                reason = "configuration",
                "journal.open.failed"
            );
            Err(JournalError::OpenFailed)
        }
        Err(_) => {
            tracing::warn!(
                component = "journal",
                engine = "postgres",
                reason = "unreachable",
                "journal.open.failed"
            );
            Err(JournalError::Unavailable)
        }
    }
}

/// A folder only the running user can enter: mode `0700` on macOS and Linux.
pub(crate) fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// A file only the running user can read: mode `0600` on macOS and Linux.
pub(crate) fn set_private_file(file: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = file;
    Ok(())
}
