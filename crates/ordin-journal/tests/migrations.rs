//! Migrations on SQLite: DatabaseMigrations.md steps 1–9, the migration scenarios of
//! JournalAndArtefactStore.md and the Gherkin of DatabaseMigrations.md (X2).

mod support;

use ordin_journal::{
    ApplyMode, Datastore, Journal, JournalError, MigrationRegistry, MigrationStep, MIGRATIONS,
};
use support::{applied, count, exec, migrate, module, open_sqlite, options, TempDir, TestClock};

const APPROVAL_1_2: &[MigrationStep] = &[
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
    (2, "broken", "CREATE TABLE approval_broken ("),
];

const APPROVAL_FAILING_LATER: &[MigrationStep] = &[
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
        "approval more",
        "ALTER TABLE approval_t ADD COLUMN more TEXT",
    ),
];

fn with_approval(steps: &'static [MigrationStep]) -> MigrationRegistry {
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(module("approval", steps)).unwrap();
    registry
}

#[tokio::test]
async fn migration_steps_1_to_9_on_sqlite() {
    let dir = TempDir::new();
    let counter = std::sync::atomic::AtomicUsize::new(0);
    let base = dir.path().to_path_buf();
    let fresh = move || {
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let base = base.clone();
        Box::pin(async move {
            Datastore::open(ordin_journal::DatastoreConfig::Sqlite {
                path: base.join(format!("db{n}")).join("journal.db"),
                backup_dir: base.join(format!("db{n}")).join("backups"),
            })
            .await
            .unwrap()
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = Datastore>>>
    };
    support::steps::migration_steps_1_to_9(&fresh).await;
}

// Scenario: First start creates the database and the backend identifier
#[tokio::test]
async fn first_start_creates_the_database_and_the_backend_identifier() {
    let dir = TempDir::new();
    assert!(!dir.db_file().exists());
    let store = open_sqlite(&dir).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let journal = Journal::start(store.clone(), options(TestClock::now_real()))
        .await
        .unwrap();
    assert!(dir.db_file().exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.db_file())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let folder = std::fs::metadata(dir.db_file().parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(folder & 0o777, 0o700);
    }
    assert_eq!(applied(&store).await, vec![("journal".to_owned(), 1)]);
    let pool = store.sqlite().unwrap();
    let (backend, last_seq, days): (String, i64, i64) =
        sqlx::query_as("SELECT backend_id, last_seq, retention_days FROM journal_meta")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(count(&store, "SELECT COUNT(*) FROM journal_meta").await, 1);
    let backend = uuid::Uuid::parse_str(&backend).unwrap();
    assert_eq!(backend.get_version_num(), 7);
    assert_eq!(backend, journal.backend_uuid());
    assert_eq!((last_seq, days), (0, 90));
    assert_eq!(report.backup_file, None);
    assert!(dir.backups().is_empty(), "no backup file was written");
    journal.shutdown().await;
    store.close().await;
}

// Scenario: Pending migrations are applied after a backup, in one transaction
#[tokio::test]
async fn pending_migrations_are_applied_after_a_backup_in_one_transaction() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    let report = with_approval(APPROVAL_1_2)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let backup = report.backup_file.expect("a backup copy");
    let name = backup.file_name().unwrap().to_str().unwrap().to_owned();
    assert!(backup.starts_with(dir.backup_dir()));
    assert!(
        name.starts_with("journal-") && name.ends_with("Z.db") && name.len() == 27,
        "{name}"
    );
    assert!(backup.exists());
    let rows = applied(&store).await;
    assert!(rows.contains(&("approval".to_owned(), 1)));
    assert!(rows.contains(&("approval".to_owned(), 2)));
    assert_eq!(
        report.applied,
        vec![("approval".to_owned(), 1), ("approval".to_owned(), 2)]
    );
    store.close().await;
}

// Scenario: A failing step leaves the database unchanged
#[tokio::test]
async fn a_failing_step_leaves_the_database_unchanged() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    match with_approval(APPROVAL_FAILING)
        .apply(&store, ApplyMode::ApplyPending)
        .await
    {
        Err(JournalError::MigrationFailed { module, version }) => {
            assert_eq!((module.as_str(), version), ("approval", 2))
        }
        other => panic!("{other:?}"),
    }
    assert!(!applied(&store).await.contains(&("approval".to_owned(), 1)));
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'approval_t'"
        )
        .await,
        0
    );
    assert_eq!(dir.backups().len(), 1, "the backup file is kept");
    store.close().await;
}

// Scenario: A database written by a newer ordin is refused
#[tokio::test]
async fn a_database_written_by_a_newer_ordin_is_refused() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    exec(
        &store,
        "INSERT INTO schema_migrations VALUES ('journal', 7, 'later', \
         '0000000000000000000000000000000000000000000000000000000000000000', '2026-01-10T09:15:00.412Z')",
    )
    .await;
    const THREE: &[MigrationStep] = &[
        (1, "create journal tables", MIGRATIONS.sqlite[0].2),
        (2, "two", "CREATE TABLE two (x INTEGER)"),
        (3, "three", "CREATE TABLE three (x INTEGER)"),
    ];
    let mut registry = MigrationRegistry::new();
    registry
        .register(ordin_journal::ModuleMigrations {
            module: "journal",
            sqlite: THREE,
            postgres: &[],
        })
        .unwrap();
    let tables_before = count(&store, "SELECT COUNT(*) FROM sqlite_master").await;
    match registry.apply(&store, ApplyMode::ApplyPending).await {
        Err(JournalError::SchemaNewer {
            module,
            found,
            known,
        }) => {
            assert_eq!((module.as_str(), found, known), ("journal", 7, 3))
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM sqlite_master").await,
        tables_before
    );
    store.close().await;
}

// Scenario: A changed migration is refused
#[tokio::test]
async fn a_changed_migration_is_refused() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    exec(
        &store,
        "UPDATE schema_migrations SET checksum = \
         '1111111111111111111111111111111111111111111111111111111111111111' WHERE module = 'journal'",
    )
    .await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    match registry.apply(&store, ApplyMode::ApplyPending).await {
        Err(JournalError::MigrationChanged { module, version }) => {
            assert_eq!((module.as_str(), version), ("journal", 1))
        }
        other => panic!("{other:?}"),
    }
    store.close().await;
}

// Scenario: A scaled-shape instance refuses a schema behind its program (SQLite form; the
// PostgreSQL form is in pg_migrations.rs)
#[tokio::test]
async fn a_scaled_shape_instance_refuses_a_schema_behind_its_program() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    const TWO: &[MigrationStep] = &[
        (1, "create journal tables", MIGRATIONS.sqlite[0].2),
        (2, "two", "CREATE TABLE two (x INTEGER)"),
    ];
    let mut registry = MigrationRegistry::new();
    registry
        .register(ordin_journal::ModuleMigrations {
            module: "journal",
            sqlite: TWO,
            postgres: &[],
        })
        .unwrap();
    match registry.apply(&store, ApplyMode::CheckOnly).await {
        Err(JournalError::SchemaBehind { pending }) => {
            assert_eq!(pending, vec![("journal".to_owned(), 2)])
        }
        other => panic!("{other:?}"),
    }
    store.close().await;
}

// DatabaseMigrations.md — Scenario: Happy path — pending steps are applied after a backup, in one transaction
#[tokio::test]
async fn happy_path_pending_steps_are_applied_after_a_backup_in_one_transaction() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    const APPROVAL_1: &[MigrationStep] = &[(
        1,
        "approval table",
        "CREATE TABLE approval_t (id INTEGER PRIMARY KEY)",
    )];
    with_approval(APPROVAL_1)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let report = with_approval(APPROVAL_1_2)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(report.backup_file.as_ref().is_some_and(|f| f.exists()));
    assert!(applied(&store).await.contains(&("approval".to_owned(), 2)));
    assert_eq!(report.applied, vec![("approval".to_owned(), 2)]);
    store.close().await;
}

// DatabaseMigrations.md — Scenario: Edge case — nothing pending writes no backup
#[tokio::test]
async fn edge_case_nothing_pending_writes_no_backup() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    with_approval(APPROVAL_1_2)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let before = dir.backups();
    let report = with_approval(APPROVAL_1_2)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_eq!(report.backup_file, None);
    assert!(report.applied.is_empty());
    assert_eq!(dir.backups(), before);
    store.close().await;
}

// DatabaseMigrations.md — Scenario: Error path — a newer database is refused, not downgraded
#[tokio::test]
async fn error_path_a_newer_database_is_refused_not_downgraded() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    exec(
        &store,
        "INSERT INTO schema_migrations VALUES ('journal', 7, 'later', \
         '0000000000000000000000000000000000000000000000000000000000000000', '2026-01-10T09:15:00.412Z')",
    )
    .await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    let error = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap_err();
    assert!(matches!(error, JournalError::SchemaNewer { found: 7, .. }));
    assert_eq!(
        ordin_types::CodedError::repair(&error),
        Some(ordin_types::RepairAction::SendToIt)
    );
    assert_eq!(applied(&store).await.len(), 2, "nothing is downgraded");
    store.close().await;
}

// DatabaseMigrations.md — Scenario: Error path — a failing step leaves the database unchanged
#[tokio::test]
async fn error_path_a_failing_step_leaves_the_database_unchanged() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    let error = with_approval(APPROVAL_FAILING)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        JournalError::MigrationFailed { version: 2, .. }
    ));
    assert!(!applied(&store).await.contains(&("approval".to_owned(), 2)));
    assert!(!dir.backups().is_empty(), "the backup file is kept");
    store.close().await;
}

// DatabaseMigrations.md — Scenario: Edge case — a widening rebuild keeps every row (rule 12a)
// The registry's part of rule 12a, with a module of this test's own: a parent and a child table,
// the parent rebuilt to widen its CHECK list inside the step's transaction.
#[tokio::test]
async fn edge_case_a_widening_rebuild_keeps_every_row() {
    const STEPS: &[MigrationStep] = &[
        (
            1,
            "links",
            "CREATE TABLE source_links (id INTEGER PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('page')));
             CREATE TABLE working_copy_items (id INTEGER PRIMARY KEY,
                 link_id INTEGER NOT NULL REFERENCES source_links (id) ON DELETE CASCADE);",
        ),
        (
            2,
            "widen source_links",
            "CREATE TEMPORARY TABLE items_keep AS SELECT * FROM working_copy_items;
             CREATE TABLE source_links_new (id INTEGER PRIMARY KEY,
                 kind TEXT NOT NULL CHECK (kind IN ('page', 'jira_issue')));
             INSERT INTO source_links_new SELECT * FROM source_links;
             DROP TABLE working_copy_items;
             DROP TABLE source_links;
             ALTER TABLE source_links_new RENAME TO source_links;
             CREATE TABLE working_copy_items (id INTEGER PRIMARY KEY,
                 link_id INTEGER NOT NULL REFERENCES source_links (id) ON DELETE CASCADE);
             INSERT INTO working_copy_items SELECT * FROM items_keep;
             DROP TABLE items_keep;",
        ),
    ];
    const STEP_1: &[MigrationStep] = &[STEPS[0]];
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    let mut first = MigrationRegistry::new();
    first.register(module("workspaces", STEP_1)).unwrap();
    first.apply(&store, ApplyMode::ApplyPending).await.unwrap();
    exec(
        &store,
        "INSERT INTO source_links VALUES (1, 'page'), (2, 'page'), (3, 'page');
         INSERT INTO working_copy_items VALUES (1, 1), (2, 1), (3, 2), (4, 3), (5, 3);",
    )
    .await;
    let mut second = MigrationRegistry::new();
    second.register(module("workspaces", STEPS)).unwrap();
    second.apply(&store, ApplyMode::ApplyPending).await.unwrap();
    assert_eq!(count(&store, "SELECT COUNT(*) FROM source_links").await, 3);
    assert_eq!(
        count(&store, "SELECT COUNT(*) FROM working_copy_items").await,
        5
    );
    let problems: Vec<(String,)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(store.sqlite().unwrap())
        .await
        .unwrap();
    assert!(problems.is_empty());
    exec(&store, "INSERT INTO source_links VALUES (4, 'jira_issue')").await;
    store.close().await;
}

#[tokio::test]
async fn a_backup_keeps_the_newest_two_copies() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    std::fs::create_dir_all(dir.backup_dir()).unwrap();
    for old in ["journal-20200101T000000Z.db", "journal-20200102T000000Z.db"] {
        std::fs::write(dir.backup_dir().join(old), b"old").unwrap();
    }
    with_approval(APPROVAL_1_2)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    with_approval(APPROVAL_FAILING_LATER)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let names = dir.backups();
    assert_eq!(names.len(), 2);
    assert_eq!(
        names[0], "journal-20200102T000000Z.db",
        "the oldest copy was removed"
    );
    store.close().await;
}
