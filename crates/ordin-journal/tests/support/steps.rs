//! DatabaseMigrations.md *Internal Logic* steps 1–9, run on a datastore of either engine. Each
//! phase starts from an empty database the caller supplies.

use std::future::Future;
use std::pin::Pin;

use ordin_journal::{
    ApplyMode, Datastore, Engine, JournalError, MigrationRegistry, MigrationStep, MIGRATIONS,
};

use super::{applied, count, module};

const APPROVAL: &[MigrationStep] = &[
    (
        1,
        "approval table",
        "CREATE TABLE approval_t (id INTEGER PRIMARY KEY)",
    ),
    (
        2,
        "approval note",
        "ALTER TABLE approval_t ADD COLUMN note TEXT",
    ),
];
const APPROVAL_FAILING: &[MigrationStep] = &[
    (
        1,
        "approval table",
        "CREATE TABLE approval_t (id INTEGER PRIMARY KEY)",
    ),
    (
        2,
        "approval note",
        "ALTER TABLE approval_t ADD COLUMN note TEXT",
    ),
    (
        3,
        "approval extra",
        "CREATE TABLE approval_extra (id INTEGER PRIMARY KEY)",
    ),
    (4, "broken", "CREATE TABLE approval_broken ("),
];
const APPROVAL_ONE: &[MigrationStep] = &[(
    1,
    "approval table",
    "CREATE TABLE approval_t (id INTEGER PRIMARY KEY)",
)];
const APPROVAL_CHANGED: &[MigrationStep] = &[
    (
        1,
        "approval table",
        "CREATE TABLE approval_t (id BIGINT PRIMARY KEY)",
    ),
    (
        2,
        "approval note",
        "ALTER TABLE approval_t ADD COLUMN note TEXT",
    ),
];

fn registry(approval: &'static [MigrationStep]) -> MigrationRegistry {
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(module("approval", approval)).unwrap();
    registry
}

pub type Fresh = dyn Fn() -> Pin<Box<dyn Future<Output = Datastore>>>;

pub async fn migration_steps_1_to_9(fresh: &Fresh) {
    let store = fresh().await;
    let engine = store.engine();

    // Step 1: a second registration of a module is refused.
    let mut twice = registry(APPROVAL);
    assert!(matches!(
        twice.register(module("approval", APPROVAL)),
        Err(JournalError::MigrationDuplicate { .. })
    ));

    // Steps 2, 4 and 6: CheckOnly creates the bookkeeping table, lists the pending steps in
    // registration order and changes nothing else.
    match registry(APPROVAL).apply(&store, ApplyMode::CheckOnly).await {
        Err(JournalError::SchemaBehind { pending }) => assert_eq!(
            pending,
            vec![
                ("journal".to_owned(), 1),
                ("approval".to_owned(), 1),
                ("approval".to_owned(), 2)
            ]
        ),
        other => panic!("CheckOnly gave {other:?}"),
    }
    assert!(
        applied(&store).await.is_empty(),
        "schema_migrations exists and is empty"
    );

    // Steps 7, 8 and 9: applied in one transaction, after a backup on SQLite only.
    let report = registry(APPROVAL)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_eq!(
        report.applied,
        vec![
            ("journal".to_owned(), 1),
            ("approval".to_owned(), 1),
            ("approval".to_owned(), 2)
        ]
    );
    // An empty database holds no saved work: no copy on either engine.
    assert_eq!(report.backup_file, None);
    assert_eq!(applied(&store).await.len(), 3);
    assert_eq!(count(&store, "SELECT COUNT(*) FROM journal_meta").await, 0);

    // Step 5: nothing pending is an empty report with no backup.
    let again = registry(APPROVAL)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(again.applied.is_empty());
    assert_eq!(again.backup_file, None);

    // Step 8: a failing step rolls back every step of the call.
    match registry(APPROVAL_FAILING)
        .apply(&store, ApplyMode::ApplyPending)
        .await
    {
        Err(JournalError::MigrationFailed { module, version }) => {
            assert_eq!((module.as_str(), version), ("approval", 4));
        }
        other => panic!("a failing step gave {other:?}"),
    }
    let rows = applied(&store).await;
    if engine == Engine::Sqlite {
        let backup = store_backups(&store);
        assert!(
            backup,
            "the backup copy written before the failed call is kept"
        );
    }
    assert!(
        !rows.contains(&("approval".to_owned(), 3)),
        "step 3 rolled back"
    );
    assert_eq!(rows.len(), 3);

    // Step 3: an unknown module, a newer version and a changed step are refused.
    let mut journal_only = MigrationRegistry::new();
    journal_only.register(MIGRATIONS).unwrap();
    match journal_only.apply(&store, ApplyMode::ApplyPending).await {
        Err(JournalError::SchemaNewer {
            module,
            found,
            known,
        }) => {
            assert_eq!((module.as_str(), found, known), ("approval", 1, 0));
        }
        other => panic!("an unknown module gave {other:?}"),
    }
    match registry(APPROVAL_ONE)
        .apply(&store, ApplyMode::ApplyPending)
        .await
    {
        Err(JournalError::SchemaNewer {
            module,
            found,
            known,
        }) => {
            assert_eq!((module.as_str(), found, known), ("approval", 2, 1));
        }
        other => panic!("a newer version gave {other:?}"),
    }
    match registry(APPROVAL_CHANGED)
        .apply(&store, ApplyMode::ApplyPending)
        .await
    {
        Err(JournalError::MigrationChanged { module, version }) => {
            assert_eq!((module.as_str(), version), ("approval", 1));
        }
        other => panic!("a changed step gave {other:?}"),
    }
    store.close().await;
}

/// True when the SQLite datastore's backup folder holds a copy.
fn store_backups(store: &Datastore) -> bool {
    let file = store
        .sqlite()
        .unwrap()
        .connect_options()
        .get_filename()
        .to_path_buf();
    let dir = file.parent().unwrap().join("backups");
    std::fs::read_dir(dir)
        .map(|mut e| e.next().is_some())
        .unwrap_or(false)
}
