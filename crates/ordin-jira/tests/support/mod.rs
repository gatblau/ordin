//! Shared test support: a temporary SQLite journal and the Jira Cloud replies of the spec's own
//! examples (JiraAdapter.md, Gherkin and *Example*). Included by the unit tests and by the
//! integration tests, so it names no type of this crate.
#![allow(dead_code)]

pub mod fixtures;
pub mod identity;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-jira-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A migrated, started Journal on a fresh SQLite file in `dir`.
pub async fn open_journal(dir: &TempDir) -> Arc<Journal> {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.path().join("data").join("journal.db"),
        backup_dir: dir.path().join("data").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the journal's migrations");
    let options = JournalOptions {
        automatic_pruning: false,
        prune_interval: Duration::from_secs(24 * 3600),
        clock: Arc::new(SystemClock),
    };
    Arc::new(
        Journal::start(store, options)
            .await
            .expect("start the journal"),
    )
}
