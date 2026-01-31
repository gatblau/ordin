//! What the person can do when the saved data does not match this program (OrdinDesktop.md
//! amendment 2026-01-30, DSK-DATA-1 to DSK-DATA-3). The window shell shows the choice; this module
//! holds the words and the two actions, so they are tested without a window.

use std::path::{Path, PathBuf};

use ordin_journal::{Datastore, DatastoreConfig, JournalError, MigrationRegistry};

use crate::error::StartError;
use crate::plan::Row;

/// The choice a failed start offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataOffer {
    /// `JRNL_MIGRATION_CHANGED`: keep the data, start with empty data, or quit.
    Mismatch,
    /// `JRNL_SCHEMA_NEWER`: start with empty data, or quit.
    Newer,
}

pub const KEEP_DATA: &str = "Keep my data";
pub const EMPTY_DATA: &str = "Start with empty data";
pub const QUIT: &str = "Quit";

const MISMATCH_TEXT: &str = "ordin's saved data was set up by a different version of ordin, and part of it doesn't match this one. You can keep your data and carry on (ordin saves a copy first), or start with empty data (your current data is moved to a folder, not deleted).";
const NEWER_TEXT: &str = "ordin's saved data comes from a newer version of ordin. Install the newer version to open it, or start with empty data (your current data is moved to a folder, not deleted).";

/// The entries of the data root that stay where they are when the data is set aside.
const KEPT_IN_PLACE: &[&str] = &["logs", "backups"];
const SET_ASIDE_PREFIX: &str = "set-aside-";

impl DataOffer {
    /// DSK-DATA-1: the failures that ask the person instead of step 9's message box.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-DATA-1
    pub fn for_error(error: &StartError) -> Option<DataOffer> {
        match error {
            StartError::ModuleFailed { module, code } if *module == Row::Journal.module() => {
                match code.as_str() {
                    "JRNL_MIGRATION_CHANGED" => Some(DataOffer::Mismatch),
                    "JRNL_SCHEMA_NEWER" => Some(DataOffer::Newer),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            DataOffer::Mismatch => MISMATCH_TEXT,
            DataOffer::Newer => NEWER_TEXT,
        }
    }
}

/// DSK-DATA-2: accepts the applied steps this build does not recognise, after a backup copy.
/// Returns the number of rows accepted.
// spec: ordin-desktop/components/OrdinDesktop#DSK-DATA-2
pub async fn keep_data(data_root: &Path) -> Result<usize, JournalError> {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data_root.join("journal.db"),
        backup_dir: data_root.join("backups"),
    })
    .await?;
    let mut registry = MigrationRegistry::new();
    crate::migrations::register_all(&mut registry)?;
    let report = registry.accept_changed(&store).await;
    store.close().await;
    let accepted = report?.accepted.len();
    tracing::info!(component = "desktop", accepted, "data_kept");
    Ok(accepted)
}

/// DSK-DATA-3: moves every entry of the data root but `logs`, `backups` and earlier set-aside
/// folders into a new `set-aside-<time>` folder. Returns that folder.
// spec: ordin-desktop/components/OrdinDesktop#DSK-DATA-3
pub fn set_aside(data_root: &Path) -> Result<PathBuf, StartError> {
    let unwritable = |path: &Path, source: std::io::Error| StartError::DataRootUnwritable {
        path: path.to_path_buf(),
        source,
    };
    let stamp = ordin_types::time::Timestamp::now()
        .0
        .format("%Y%m%dT%H%M%SZ")
        .to_string();
    let folder = data_root.join(format!("{SET_ASIDE_PREFIX}{stamp}"));
    create_private_dir(&folder).map_err(|e| unwritable(&folder, e))?;
    let entries = std::fs::read_dir(data_root).map_err(|e| unwritable(data_root, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| unwritable(data_root, e))?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if KEPT_IN_PLACE.contains(&text.as_ref()) || text.starts_with(SET_ASIDE_PREFIX) {
            continue;
        }
        std::fs::rename(entry.path(), folder.join(&name))
            .map_err(|e| unwritable(&entry.path(), e))?;
    }
    let folder_name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    tracing::info!(
        component = "desktop",
        folder = folder_name.as_str(),
        "data_set_aside"
    );
    Ok(folder)
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use ordin_journal::ApplyMode;

    fn journal_failed(code: &str) -> StartError {
        StartError::ModuleFailed {
            module: Row::Journal.module(),
            code: code.to_owned(),
        }
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-DATA-1
    #[test]
    fn only_the_journals_two_data_failures_ask_the_person() {
        assert_eq!(
            DataOffer::for_error(&journal_failed("JRNL_MIGRATION_CHANGED")),
            Some(DataOffer::Mismatch)
        );
        assert_eq!(
            DataOffer::for_error(&journal_failed("JRNL_SCHEMA_NEWER")),
            Some(DataOffer::Newer)
        );
        assert_eq!(
            DataOffer::for_error(&journal_failed("JRNL_BACKUP_FAILED")),
            None
        );
        let other = StartError::ModuleFailed {
            module: "ordin-hook-relay",
            code: "JRNL_MIGRATION_CHANGED".to_owned(),
        };
        assert_eq!(DataOffer::for_error(&other), None);
        assert!(!DataOffer::Mismatch.text().contains("IT"));
        assert!(!DataOffer::Newer.text().contains("IT"));
    }

    async fn migrated(root: &Path) -> Datastore {
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: root.join("journal.db"),
            backup_dir: root.join("backups"),
        })
        .await
        .unwrap();
        let mut registry = MigrationRegistry::new();
        crate::migrations::register_all(&mut registry).unwrap();
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .unwrap();
        store
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-DATA-2
    #[tokio::test]
    async fn keeping_the_data_lets_the_next_start_open_it() {
        let dir = TempDir::new("keep-data");
        let store = migrated(dir.path()).await;
        sqlx::query("UPDATE schema_migrations SET checksum = ?1 WHERE module = 'journal'")
            .bind("4".repeat(64))
            .execute(store.sqlite().unwrap())
            .await
            .unwrap();
        let mut registry = MigrationRegistry::new();
        crate::migrations::register_all(&mut registry).unwrap();
        assert!(matches!(
            registry.apply(&store, ApplyMode::ApplyPending).await,
            Err(JournalError::MigrationChanged { .. })
        ));
        store.close().await;

        // One row per journal migration step had its fingerprint changed above.
        assert_eq!(
            keep_data(dir.path()).await.unwrap(),
            ordin_journal::MIGRATIONS.sqlite.len()
        );
        let store = migrated(dir.path()).await;
        store.close().await;
        let backups = std::fs::read_dir(dir.path().join("backups"))
            .unwrap()
            .count();
        assert!(backups >= 1, "a copy was saved first");
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-DATA-3
    #[test]
    fn starting_with_empty_data_moves_everything_but_logs_and_backups() {
        let dir = TempDir::new("set-aside");
        let root = dir.path();
        for folder in ["logs", "backups", "agents", "set-aside-20260101T000000Z"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join("journal.db"), b"data").unwrap();
        std::fs::write(root.join("agents").join("a.md"), b"agent").unwrap();

        let folder = set_aside(root).unwrap();
        assert!(folder.starts_with(root));
        assert!(folder.join("journal.db").is_file());
        assert!(folder.join("agents").join("a.md").is_file());
        assert!(!root.join("journal.db").exists());
        assert!(root.join("logs").is_dir() && root.join("backups").is_dir());
        assert!(root.join("set-aside-20260101T000000Z").is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&folder).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }
}
