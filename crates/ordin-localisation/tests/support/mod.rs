//! Shared test helpers: a temporary journal database with Localisation's migrations applied, a
//! second connection for tests that change the database behind the service, and the snapshot of a
//! profile that offers given locales.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_organisation_profile::ProfileSnapshot;

/// A folder removed when dropped.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Dir {
        let dir = std::env::temp_dir().join(format!("ordin-localisation-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        Dir(dir)
    }

    pub fn db_file(&self) -> PathBuf {
        self.0.join("journal.db")
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Opens the journal database in `dir` and applies the journal's and Localisation's migrations.
pub async fn open_store(dir: &Dir) -> Datastore {
    let store = open_plain(dir).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal registers");
    registry
        .register(ordin_localisation::migrations())
        .expect("localisation registers");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply migrations");
    store
}

/// Opens the same database again, without migrating: a second connection for tests.
pub async fn open_plain(dir: &Dir) -> Datastore {
    Datastore::open(DatastoreConfig::Sqlite {
        path: dir.db_file(),
        backup_dir: dir.path().join("backups"),
    })
    .await
    .expect("open SQLite")
}

/// A snapshot that offers `offered` (empty: every bundled locale) with the fallback `fallback`.
pub fn snapshot(offered: &[&str], fallback: &str) -> Arc<ProfileSnapshot> {
    let mut snapshot = ProfileSnapshot::defaults();
    snapshot.document.locales.offered = offered.iter().map(|tag| (*tag).to_string()).collect();
    snapshot.document.locales.fallback = fallback.to_string();
    Arc::new(snapshot)
}

/// Removes the isolating marks Fluent puts around placeables.
pub fn plain(text: &str) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}

pub mod capture;
