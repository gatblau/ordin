//! Shared helpers: a temporary data root, a migrated SQLite journal and a manager on it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalOptions, MigrationRegistry,
    SystemClock, MIGRATIONS,
};
use ordin_types::domain::places::{FolderAccess, FolderGrant, Grantee, PlaceKind};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, CrewCharterId, FolderGrantId, PlaceId, WorkspaceId};

use crate::{AgentFolderManager, WorkspaceGrant};

/// A folder removed when dropped.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-afm-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(
            std::fs::canonicalize(dir)
                .map(ordin_types::paths::plain_form)
                .expect("canonical temporary folder"),
        )
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// Makes a directory under the temporary folder and returns its canonical path.
    pub(crate) fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).expect("directory");
        std::fs::canonicalize(path)
            .map(ordin_types::paths::plain_form)
            .expect("canonical directory")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        restore_modes(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Gives back write permission to every directory so the clean-up can delete it.
#[cfg(unix)]
fn restore_modes(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.is_dir() {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    restore_modes(&entry.path());
                }
            }
        }
    }
}

#[cfg(not(unix))]
fn restore_modes(_: &Path) {}

pub(crate) struct Fixture {
    pub(crate) dir: TempDir,
    pub(crate) journal: Journal,
    pub(crate) afm: AgentFolderManager,
    pub(crate) root: PathBuf,
}

/// A migrated, started journal on a fresh SQLite file, with this module's migrations applied.
pub(crate) async fn journal_in(dir: &TempDir) -> Journal {
    let data = dir.path().join("journal");
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data.join("journal.db"),
        backup_dir: data.join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .register(crate::migrations())
        .expect("register agent_folders");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply migrations");
    Journal::start(
        store,
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(24 * 3600),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("start the journal")
}

pub(crate) async fn fixture() -> Fixture {
    fixture_waiting(crate::WRITE_LOCK_WAIT).await
}

/// A fixture whose `Write` and `SessionUpdate` requests give up after `wait`.
pub(crate) async fn fixture_waiting(wait: Duration) -> Fixture {
    let dir = TempDir::new();
    let journal = journal_in(&dir).await;
    let root = dir.dir("data");
    let afm = AgentFolderManager::new(journal.clone(), root.clone())
        .await
        .expect("start the manager")
        .with_lock_wait(wait);
    Fixture {
        dir,
        journal,
        afm,
        root,
    }
}

impl Fixture {
    /// The kinds of the journal entries written so far, newest first.
    pub(crate) async fn kinds(&self) -> Vec<String> {
        self.journal
            .list_entries(EntryFilter::default(), None)
            .await
            .expect("list entries")
            .items
            .into_iter()
            .map(|entry| entry.kind)
            .collect()
    }

    /// Runs one statement straight on the SQLite pool.
    pub(crate) async fn sql(&self, statement: &'static str) {
        let pool = self.journal.datastore().sqlite().expect("SQLite");
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("statement");
    }

    /// A count from one query on the SQLite pool.
    pub(crate) async fn count(&self, statement: &'static str) -> i64 {
        let pool = self.journal.datastore().sqlite().expect("SQLite");
        sqlx::query_scalar::<_, i64>(statement)
            .fetch_one(pool)
            .await
            .expect("count")
    }
}

pub(crate) fn instance(n: u8) -> AgentInstanceId {
    AgentInstanceId(uuid::Uuid::from_u128(
        0x0192_a0f9_aaaa_7bbb_8ccc_dddd_eeee_ff00 + u128::from(n),
    ))
}

pub(crate) fn grant_for(
    workspace_id: WorkspaceId,
    grantee: Grantee,
    place_id: PlaceId,
    access: FolderAccess,
    path: &Path,
) -> WorkspaceGrant {
    WorkspaceGrant {
        grant: FolderGrant {
            id: FolderGrantId::new(),
            workspace_id,
            grantee,
            place_id,
            access,
            granted_at: Timestamp::now(),
        },
        place_path: path.to_path_buf(),
        place_kind: PlaceKind::PersonsFolder,
    }
}

pub(crate) fn to_instance(id: AgentInstanceId) -> Grantee {
    Grantee::AgentInstance {
        agent_instance_id: id,
    }
}

pub(crate) fn to_crew(id: CrewCharterId) -> Grantee {
    Grantee::Crew {
        crew_charter_id: id,
    }
}
