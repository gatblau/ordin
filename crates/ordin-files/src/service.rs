//! Construction, registration, shutdown, and the places of the File Service (FileService.md
//! *Construction and registration*, *Places*; Internal Logic 1 and 2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ordin_agent_folders::{AgentFolderManager, WorkspaceGrant};
use ordin_journal::{Journal, ModuleMigrations, NewJournalEntry};
use ordin_rules::RulesEngine;
use ordin_types::domain::places::{Place, PlaceKind};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{PlaceId, RunId, WorkspaceId};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tokio::sync::{broadcast, Semaphore};

use crate::error::FileError;
use crate::log;
use crate::types::{AddPlace, FileServiceDeps, PlaceChange};
use crate::{CHANGE_CHANNEL, RECENT_WINDOW};

/// The module name under which the program registers [`FileService::migrations`].
pub const MODULE: &str = "files";

#[cfg(test)]
pub(crate) type AfterRead = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

const SQLITE_MIGRATIONS: &[ordin_journal::MigrationStep] = &[
    (1, "create file tables", include_str!("sql/sqlite_v1.sql")),
    (
        2,
        "create work folder table",
        include_str!("sql/sqlite_v2.sql"),
    ),
    (
        3,
        "create agent write claims",
        include_str!("sql/sqlite_v3.sql"),
    ),
];
const POSTGRES_MIGRATIONS: &[ordin_journal::MigrationStep] = &[(
    1,
    "create work folder table",
    include_str!("sql/postgres_v1.sql"),
)];

/// The system bin. The platform call is behind this trait so that tests can stand in for a
/// computer with no bin (Internal Logic 7.2).
pub(crate) trait Bin: Send + Sync {
    fn delete(&self, path: &Path) -> Result<(), ()>;
}

/// `trash::delete`, through the faster call that needs no extra permission on macOS.
pub(crate) struct SystemBin;

impl Bin for SystemBin {
    fn delete(&self, path: &Path) -> Result<(), ()> {
        #[allow(unused_mut)]
        let mut context = trash::TrashContext::new();
        #[cfg(target_os = "macos")]
        {
            use trash::macos::{DeleteMethod, TrashContextExtMacos};
            context.set_delete_method(DeleteMethod::NsFileManager);
        }
        context.delete(path).map_err(|_| ())
    }
}

/// A write this service made, remembered for `RECENT_WINDOW` (Internal Logic 5.6).
pub(crate) struct Recent {
    pub(crate) hash: String,
    pub(crate) at: Instant,
}

/// The place row as the service reads it.
#[derive(Debug, Clone)]
pub(crate) struct PlaceRow {
    pub(crate) id: PlaceId,
    pub(crate) friendly_name: String,
    pub(crate) root: PathBuf,
    pub(crate) kind: PlaceKind,
    pub(crate) added_at: Timestamp,
    pub(crate) index_state: String,
}

impl PlaceRow {
    pub(crate) fn to_place(&self) -> Place {
        Place {
            id: self.id,
            friendly_name: self.friendly_name.clone(),
            path: self.root.to_string_lossy().into_owned(),
            kind: self.kind,
            added_at: self.added_at,
        }
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn kind_str(kind: PlaceKind) -> &'static str {
    match kind {
        PlaceKind::PersonsFolder => "persons_folder",
        PlaceKind::WorkspaceFolder => "workspace_folder",
        PlaceKind::AgentFolder => "agent_folder",
        PlaceKind::WorkingCopy => "working_copy",
        PlaceKind::CodingTaskWorktree => "coding_task_worktree",
    }
}

fn kind_from(raw: &str) -> Result<PlaceKind, FileError> {
    Ok(match raw {
        "persons_folder" => PlaceKind::PersonsFolder,
        "workspace_folder" => PlaceKind::WorkspaceFolder,
        "agent_folder" => PlaceKind::AgentFolder,
        "working_copy" => PlaceKind::WorkingCopy,
        "coding_task_worktree" => PlaceKind::CodingTaskWorktree,
        _ => return Err(FileError::DatastoreUnavailable),
    })
}

pub(crate) fn ts(raw: &str) -> Result<Timestamp, FileError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| FileError::DatastoreUnavailable)
}

/// The File Service. Consumers hold `Arc<FileService>`.
pub struct FileService {
    pub(crate) pool: SqlitePool,
    pub(crate) journal: Arc<Journal>,
    pub(crate) rules: Arc<RulesEngine>,
    pub(crate) agent_folders: Arc<AgentFolderManager>,
    pub(crate) versions_dir: PathBuf,
    pub(crate) bin: Arc<dyn Bin>,
    pub(crate) changes: broadcast::Sender<PlaceChange>,
    pub(crate) recent: Mutex<HashMap<(PlaceId, String), Recent>>,
    #[cfg(test)]
    pub(crate) after_read: Mutex<Option<AfterRead>>,
    /// Paths whose watcher events are dropped: fetched writes and moves made by this service.
    pub(crate) quiet: Mutex<HashMap<(PlaceId, String), Instant>>,
    pub(crate) granted: Mutex<HashMap<WorkspaceId, Vec<WorkspaceGrant>>>,
    pub(crate) watching: Mutex<crate::watch::WatchState>,
    /// Index text extraction runs on at most 2 blocking threads (Performance).
    pub(crate) extraction: Arc<Semaphore>,
    /// Set by `shutdown`; long scans check it between files.
    pub(crate) stopping: Arc<std::sync::atomic::AtomicBool>,
    /// A handle to this service for the tasks it spawns (`watch` takes `&self`).
    pub(crate) me: std::sync::Weak<FileService>,
}

impl FileService {
    /// This module's migrations: SQLite steps 1 (places, file index, versions, published copies,
    /// held edits) and 2 (work folder), PostgreSQL step 1 (work folder).
    // spec: ordin-desktop/components/FileService#SA-RECON-01
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: MODULE,
            sqlite: SQLITE_MIGRATIONS,
            postgres: POSTGRES_MIGRATIONS,
        }
    }

    /// Checks `<data_root>/versions/` exists, creating it with mode `0700` on macOS and Linux
    /// (Internal Logic 1.2). The file tables are SQLite only: another datastore is
    /// `FILE_DATASTORE_UNAVAILABLE`.
    // spec: ordin-desktop/components/FileService#SA-RECON-01
    pub async fn new(deps: FileServiceDeps) -> Result<Arc<FileService>, FileError> {
        Self::new_with_bin(deps, Arc::new(SystemBin)).await
    }

    pub(crate) async fn new_with_bin(
        deps: FileServiceDeps,
        bin: Arc<dyn Bin>,
    ) -> Result<Arc<FileService>, FileError> {
        let pool = deps
            .journal
            .datastore()
            .sqlite()
            .cloned()
            .ok_or(FileError::DatastoreUnavailable)?;
        let versions_dir = deps.data_root.join("versions");
        let dir = versions_dir.clone();
        crate::versions::blocking(move || make_private_dir(&dir)).await??;
        let (changes, _) = broadcast::channel(CHANGE_CHANNEL);
        Ok(Arc::new_cyclic(|me| FileService {
            me: me.clone(),
            pool,
            journal: deps.journal,
            rules: deps.rules,
            agent_folders: deps.agent_folders,
            versions_dir,
            bin,
            changes,
            recent: Mutex::new(HashMap::new()),
            #[cfg(test)]
            after_read: Mutex::new(None),
            quiet: Mutex::new(HashMap::new()),
            granted: Mutex::new(HashMap::new()),
            watching: Mutex::new(crate::watch::WatchState::default()),
            extraction: Arc::new(Semaphore::new(2)),
            stopping: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }))
    }

    /// Every "Journal `<kind>`" of the spec. A `JournalError` is logged and does not fail the
    /// operation that already completed (Internal Logic 1.1).
    pub(crate) async fn record(
        &self,
        kind: &str,
        workspace_id: Option<WorkspaceId>,
        run_id: Option<RunId>,
        summary: &str,
        data: serde_json::Value,
    ) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: log::COMPONENT.to_owned(),
            run_id,
            crew_run_id: None,
            workspace_id,
            summary: summary.to_owned(),
            data,
        };
        if self.journal.append_entry(entry).await.is_err() {
            log::journal_failed(kind);
        }
    }

    pub(crate) fn emit(&self, change: PlaceChange) {
        // No subscriber is not an error.
        let _ = self.changes.send(change);
    }

    /// A transaction that holds the write lock from its first statement. These transactions read and
    /// then write, and a deferred one upgrading after another writer has committed fails at once,
    /// whatever the busy timeout (`SQLITE_BUSY_SNAPSHOT`), where this one waits its turn.
    // spec: ordin-desktop/components/FileService#SA-RECON-15
    pub(crate) async fn begin_write(&self) -> Result<Transaction<'_, Sqlite>, sqlx::Error> {
        self.pool.begin_with("BEGIN IMMEDIATE").await
    }

    /// Runs what a test set to happen between reading a file and recording it.
    #[cfg(test)]
    pub(crate) async fn after_read_for_test(&self) {
        let step = lock(&self.after_read).take();
        if let Some(step) = step {
            step.await;
        }
    }

    /// Remembers a write made here so the watcher drops its event.
    pub(crate) fn note_recent(&self, place_id: PlaceId, path: &str, hash: &str) {
        let mut map = lock(&self.recent);
        map.retain(|_, r| r.at.elapsed() < RECENT_WINDOW);
        map.insert(
            (place_id, path.to_owned()),
            Recent {
                hash: hash.to_owned(),
                at: Instant::now(),
            },
        );
    }

    pub(crate) fn recent_matches(&self, place_id: PlaceId, path: &str, hash: &str) -> bool {
        lock(&self.recent)
            .get(&(place_id, path.to_owned()))
            .is_some_and(|r| r.at.elapsed() < RECENT_WINDOW && r.hash == hash)
    }

    pub(crate) fn recent_age_under(
        &self,
        place_id: PlaceId,
        path: &str,
        hash: &str,
        window: std::time::Duration,
    ) -> bool {
        lock(&self.recent)
            .get(&(place_id, path.to_owned()))
            .is_some_and(|r| r.at.elapsed() < window && r.hash == hash)
    }

    pub(crate) fn mark_quiet(&self, place_id: PlaceId, path: &str) {
        let mut map = lock(&self.quiet);
        map.retain(|_, at| at.elapsed() < RECENT_WINDOW);
        map.insert((place_id, path.to_owned()), Instant::now());
    }

    pub(crate) fn is_quiet(&self, place_id: PlaceId, path: &str) -> bool {
        lock(&self.quiet)
            .get(&(place_id, path.to_owned()))
            .is_some_and(|at| at.elapsed() < RECENT_WINDOW)
    }

    // ---- place rows

    fn row_to_place(row: &SqliteRow) -> Result<PlaceRow, FileError> {
        let id: String = row.try_get("id")?;
        let kind: String = row.try_get("kind")?;
        let added_at: String = row.try_get("added_at")?;
        let path: String = row.try_get("path")?;
        Ok(PlaceRow {
            id: PlaceId::parse(&id).map_err(|_| FileError::DatastoreUnavailable)?,
            friendly_name: row.try_get("friendly_name")?,
            root: PathBuf::from(path),
            kind: kind_from(&kind)?,
            added_at: ts(&added_at)?,
            index_state: row.try_get("index_state")?,
        })
    }

    pub(crate) async fn place_row(&self, place_id: PlaceId) -> Result<PlaceRow, FileError> {
        let row = sqlx::query(
            "SELECT id, friendly_name, path, kind, added_at, watched, index_state \
             FROM places WHERE id = ?1",
        )
        .bind(place_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Self::row_to_place(&row),
            None => Err(FileError::PlaceNotFound),
        }
    }

    pub(crate) async fn all_place_rows(&self) -> Result<Vec<PlaceRow>, FileError> {
        let rows = sqlx::query(
            "SELECT id, friendly_name, path, kind, added_at, watched, index_state FROM places \
             ORDER BY added_at, id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(Self::row_to_place).collect()
    }

    // ---- places (Internal Logic 2)

    /// Canonicalises, refuses a protected or unreadable folder, then inserts the row.
    pub async fn add_place(&self, req: AddPlace) -> Result<Place, FileError> {
        // 2.1: absolute, at most 4,096 bytes.
        if !req.path.is_absolute() || req.path.as_os_str().len() > crate::PATH_MAX_BYTES {
            return Err(FileError::Invalid("PLACE_PATH_INVALID"));
        }
        let given = req.path.clone();
        let canonical =
            crate::versions::blocking(move || crate::paths::canonicalise_lenient(&given)).await?;
        let text = canonical
            .to_str()
            .filter(|t| t.len() <= crate::PATH_MAX_BYTES)
            .ok_or(FileError::Invalid("PLACE_PATH_INVALID"))?
            .to_owned();
        // 2.2: the system's own locations.
        // A coding task's worktree lies under the application data root (ProjectService.md
        // `WORKTREE_ROOT`), which K.6 protects; the Project Service is the only caller of this
        // kind, so the check does not apply to it (Internal Logic 2.2). A working copy lies under
        // the same root (WorkspaceService.md `WORKING_COPY_ROOT`) and the Workspace Service is
        // the only caller that adds that kind, so it is exempt on the same terms.
        if !matches!(
            req.kind,
            PlaceKind::CodingTaskWorktree | PlaceKind::WorkingCopy
        ) && (self.rules.is_protected_location(&canonical)
            || self.rules.is_protected_location(&req.path))
        {
            return Err(FileError::ProtectedLocation);
        }
        // 2.3: the folder exists and the person's account can list it.
        let probe = canonical.clone();
        let readable = crate::versions::blocking(move || {
            std::fs::metadata(&probe).is_ok_and(|m| m.is_dir()) && std::fs::read_dir(&probe).is_ok()
        })
        .await?;
        if !readable {
            return Err(FileError::FolderUnreadable);
        }
        // 2.4: one place per canonical path.
        if let Some(existing) = self.place_id_for_path(&text).await? {
            return Err(FileError::PlaceExists { existing });
        }
        // 2.5: the friendly name.
        let name = match req.friendly_name {
            Some(name) => name,
            None => canonical
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned(),
        };
        let name = validate_name(&name)?;
        // 2.6: insert; the path is stored in the row only.
        let id = PlaceId::new();
        let added_at = Timestamp::now();
        sqlx::query(
            "INSERT INTO places (id, friendly_name, path, kind, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(id.to_string())
        .bind(&name)
        .bind(&text)
        .bind(kind_str(req.kind))
        .bind(added_at.to_wire())
        .execute(&self.pool)
        .await?;
        self.record(
            "place_added",
            None,
            None,
            "A folder was added to ordin.",
            serde_json::json!({ "place_id": id.to_string(), "kind": kind_str(req.kind) }),
        )
        .await;
        log::place_added(id, kind_str(req.kind));
        Ok(Place {
            id,
            friendly_name: name,
            path: text,
            kind: req.kind,
            added_at,
        })
    }

    async fn place_id_for_path(&self, path: &str) -> Result<Option<PlaceId>, FileError> {
        let row = sqlx::query("SELECT id FROM places WHERE path = ?1")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => {
                let id: String = row.try_get("id")?;
                Ok(Some(
                    PlaceId::parse(&id).map_err(|_| FileError::DatastoreUnavailable)?,
                ))
            }
            None => Ok(None),
        }
    }

    pub async fn rename_place(
        &self,
        place_id: PlaceId,
        friendly_name: String,
    ) -> Result<Place, FileError> {
        let name = validate_name(&friendly_name)?;
        let done = sqlx::query("UPDATE places SET friendly_name = ?2 WHERE id = ?1")
            .bind(place_id.to_string())
            .bind(&name)
            .execute(&self.pool)
            .await?;
        if done.rows_affected() == 0 {
            return Err(FileError::PlaceNotFound);
        }
        self.record(
            "place_renamed",
            None,
            None,
            "A folder was renamed in ordin.",
            serde_json::json!({ "place_id": place_id.to_string() }),
        )
        .await;
        Ok(self.place_row(place_id).await?.to_place())
    }

    /// Deletes the row and its index; version bookkeeping stays; never touches the folder.
    pub async fn remove_place(&self, place_id: PlaceId) -> Result<(), FileError> {
        self.place_row(place_id).await?;
        self.unwatch(&[place_id]).await;
        let id = place_id.to_string();
        let mut tx = self.begin_write().await?;
        sqlx::query("DELETE FROM file_index_fts WHERE place_id = ?1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM held_edits WHERE place_id = ?1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM file_index WHERE place_id = ?1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM places WHERE id = ?1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.record(
            "place_removed",
            None,
            None,
            "A folder was removed from ordin.",
            serde_json::json!({ "place_id": id }),
        )
        .await;
        log::place_removed(place_id);
        Ok(())
    }

    pub async fn place(&self, place_id: PlaceId) -> Result<Place, FileError> {
        Ok(self.place_row(place_id).await?.to_place())
    }

    /// Places in `ids`, in the given order; an unknown id is skipped. Agent folders (and coding
    /// task worktrees) are never returned unless `include_hidden` is true.
    pub async fn places(
        &self,
        ids: &[PlaceId],
        include_hidden: bool,
    ) -> Result<Vec<Place>, FileError> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            match self.place_row(*id).await {
                Ok(row) => {
                    let hidden = matches!(
                        row.kind,
                        PlaceKind::AgentFolder | PlaceKind::CodingTaskWorktree
                    );
                    if include_hidden || !hidden {
                        out.push(row.to_place());
                    }
                }
                Err(FileError::PlaceNotFound) => {}
                Err(other) => return Err(other),
            }
        }
        Ok(out)
    }

    /// The place holding an absolute path, and the path relative to it (deepest place root wins).
    pub async fn locate(&self, absolute: &Path) -> Result<Option<(PlaceId, String)>, FileError> {
        let given = absolute.to_path_buf();
        let canonical =
            crate::versions::blocking(move || crate::paths::canonicalise_lenient(&given)).await?;
        let mut best: Option<(PlaceId, String, usize)> = None;
        for row in self.all_place_rows().await? {
            if let Some(rel) = crate::paths::relative_to(&row.root, &canonical) {
                let depth = row.root.components().count();
                if best.as_ref().is_none_or(|(_, _, d)| depth > *d) {
                    best = Some((row.id, rel, depth));
                }
            }
        }
        Ok(best.map(|(id, rel, _)| (id, rel)))
    }
}

/// Trimmed, 1–120 characters, no control characters (Internal Logic 2.5).
fn validate_name(raw: &str) -> Result<String, FileError> {
    let name = raw.trim();
    let count = name.chars().count();
    if !(1..=120).contains(&count) || name.chars().any(char::is_control) {
        return Err(FileError::Invalid("PLACE_NAME_INVALID"));
    }
    Ok(name.to_owned())
}

/// Creates the folder (and parents), mode `0700` on macOS and Linux.
fn make_private_dir(dir: &Path) -> Result<(), FileError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|_| FileError::WriteFailed)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| FileError::WriteFailed)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir).map_err(|_| FileError::WriteFailed)
    }
}
