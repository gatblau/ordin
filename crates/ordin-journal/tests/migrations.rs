//! Migrations on SQLite: DatabaseMigrations.md steps 1–9, the migration scenarios of
//! JournalAndArtefactStore.md and the Gherkin of DatabaseMigrations.md (X2).

mod support;

use ordin_journal::{
    ApplyMode, Datastore, Engine, Journal, JournalError, MigrationRegistry, MigrationStep,
    MIGRATIONS,
};
use support::{
    applied, count, exec, journal_last, journal_rows, journal_then, migrate, module, open_sqlite,
    options, TempDir, TestClock,
};

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

/// A journal row a newer program wrote, numbered `version`.
async fn insert_later_journal_row(store: &Datastore, version: i64) {
    sqlx::query(
        "INSERT INTO schema_migrations VALUES ('journal', ?1, 'later', \
         '0000000000000000000000000000000000000000000000000000000000000000', '2026-01-10T09:15:00.412Z')",
    )
    .bind(version)
    .execute(store.sqlite().unwrap())
    .await
    .unwrap();
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
    assert_eq!(applied(&store).await, journal_rows(Engine::Sqlite));
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
    // This program knows two steps beyond the real ones; the database holds one further still.
    let known = journal_last(Engine::Sqlite) + 2;
    let found = known + 3;
    insert_later_journal_row(&store, found).await;
    let later = journal_then(
        Engine::Sqlite,
        &[
            ("two", "CREATE TABLE two (x INTEGER)"),
            ("three", "CREATE TABLE three (x INTEGER)"),
        ],
    );
    let mut registry = MigrationRegistry::new();
    registry
        .register(ordin_journal::ModuleMigrations {
            module: "journal",
            sqlite: later,
            postgres: &[],
        })
        .unwrap();
    let tables_before = count(&store, "SELECT COUNT(*) FROM sqlite_master").await;
    match registry.apply(&store, ApplyMode::ApplyPending).await {
        Err(JournalError::SchemaNewer {
            module,
            found: found_now,
            known: known_now,
        }) => {
            assert_eq!(
                (module.as_str(), found_now, known_now),
                ("journal", found, known)
            )
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
    let later = journal_then(Engine::Sqlite, &[("two", "CREATE TABLE two (x INTEGER)")]);
    let mut registry = MigrationRegistry::new();
    registry
        .register(ordin_journal::ModuleMigrations {
            module: "journal",
            sqlite: later,
            postgres: &[],
        })
        .unwrap();
    match registry.apply(&store, ApplyMode::CheckOnly).await {
        Err(JournalError::SchemaBehind { pending }) => assert_eq!(
            pending,
            vec![("journal".to_owned(), journal_last(Engine::Sqlite) + 1)]
        ),
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
    let newer = journal_last(Engine::Sqlite) + 5;
    insert_later_journal_row(&store, newer).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    let error = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap_err();
    assert!(matches!(error, JournalError::SchemaNewer { found, .. } if found == newer));
    assert_eq!(ordin_types::CodedError::repair(&error), None);
    assert_eq!(
        applied(&store).await.len(),
        MIGRATIONS.sqlite.len() + 1,
        "nothing is downgraded"
    );
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

// Amendment 2026-01-30 (JRNL-FP-1 to JRNL-FP-3): updates survive harmless edits to a step.

const PLAIN: &[MigrationStep] = &[(
    1,
    "approval table",
    "CREATE TABLE approval_t (id INTEGER PRIMARY KEY)",
)];
const PLAIN_RECOMMENTED: &[MigrationStep] = &[(
    1,
    "approval table",
    "-- approval step 1 (amended 2026-01-24)\nCREATE TABLE approval_t (\n    id INTEGER PRIMARY KEY  -- the key\n);\n",
)];
const PLAIN_CHANGED: &[MigrationStep] = &[(
    1,
    "approval table",
    "CREATE TABLE approval_t (id INTEGER PRIMARY KEY, note TEXT)",
)];
const PLAIN_REPLACING: &[MigrationStep] = &[(
    1,
    "approval table",
    "-- ordin-replaces: 2222222222222222222222222222222222222222222222222222222222222222\n\
     CREATE TABLE approval_t (id INTEGER PRIMARY KEY)",
)];
const OTHER_FINGERPRINT: &str = "2222222222222222222222222222222222222222222222222222222222222222";

async fn stored(store: &Datastore, module: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT checksum FROM schema_migrations WHERE module = ?1")
        .bind(module)
        .fetch_one(store.sqlite().unwrap())
        .await
        .unwrap()
}

async fn stored_at(store: &Datastore, module: &str, version: i64) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT checksum FROM schema_migrations WHERE module = ?1 AND version = ?2",
    )
    .bind(module)
    .bind(version)
    .fetch_one(store.sqlite().unwrap())
    .await
    .unwrap()
}

async fn set_stored_at(store: &Datastore, module: &str, version: i64, checksum: &str) {
    sqlx::query("UPDATE schema_migrations SET checksum = ?1 WHERE module = ?2 AND version = ?3")
        .bind(checksum)
        .bind(module)
        .bind(version)
        .execute(store.sqlite().unwrap())
        .await
        .unwrap();
}

async fn set_stored(store: &Datastore, module: &str, checksum: &str) {
    sqlx::query("UPDATE schema_migrations SET checksum = ?1 WHERE module = ?2")
        .bind(checksum)
        .bind(module)
        .execute(store.sqlite().unwrap())
        .await
        .unwrap();
}

fn sha256_hex(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// Scenario: Edge case — only a comment of an applied step changed
#[tokio::test]
async fn a_step_whose_comments_and_spacing_changed_is_accepted() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    with_approval(PLAIN)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let before = stored(&store, "approval").await;
    with_approval(PLAIN_RECOMMENTED)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_eq!(stored(&store, "approval").await, before);
    store.close().await;
}

// Scenario: Edge case — a row fingerprinted by the earlier rule
#[tokio::test]
async fn a_row_fingerprinted_by_the_earlier_rule_is_accepted_and_rewritten() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    // Every journal row: its current fingerprint, and the earlier rule's, of its own step's text.
    let mut current = Vec::new();
    let mut earlier = Vec::new();
    for step in MIGRATIONS.sqlite {
        current.push(stored_at(&store, "journal", step.0).await);
        earlier.push(sha256_hex(step.2));
    }
    for (step, (now, old)) in MIGRATIONS.sqlite.iter().zip(current.iter().zip(&earlier)) {
        assert_ne!(now, old, "the fingerprint is not of the full text");
        set_stored_at(&store, "journal", step.0, old).await;
    }

    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.apply(&store, ApplyMode::CheckOnly).await.unwrap();
    for (step, old) in MIGRATIONS.sqlite.iter().zip(&earlier) {
        assert_eq!(
            &stored_at(&store, "journal", step.0).await,
            old,
            "CheckOnly writes nothing"
        );
    }
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    for (step, now) in MIGRATIONS.sqlite.iter().zip(&current) {
        assert_eq!(&stored_at(&store, "journal", step.0).await, now);
    }
    store.close().await;
}

// Scenario: Edge case — a step lists the fingerprint it replaces
#[tokio::test]
async fn a_step_that_lists_the_fingerprint_it_replaces_is_accepted() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    with_approval(PLAIN)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let current = stored(&store, "approval").await;
    set_stored(&store, "approval", OTHER_FINGERPRINT).await;
    with_approval(PLAIN_REPLACING)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_eq!(stored(&store, "approval").await, current);
    store.close().await;
}

// Scenario: Error path — a statement of an applied step changed
#[tokio::test]
async fn a_step_whose_statement_changed_is_refused() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    with_approval(PLAIN)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    match with_approval(PLAIN_CHANGED)
        .apply(&store, ApplyMode::ApplyPending)
        .await
    {
        Err(JournalError::MigrationChanged { module, version }) => {
            assert_eq!((module.as_str(), version), ("approval", 1))
        }
        other => panic!("{other:?}"),
    }
    store.close().await;
}

// Scenario: Happy path — the person keeps their data
#[tokio::test]
async fn keeping_the_data_backs_it_up_and_accepts_the_steps() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    with_approval(PLAIN)
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    exec(&store, "INSERT INTO approval_t (id) VALUES (7)").await;
    let registry = with_approval(PLAIN_CHANGED);
    assert!(registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .is_err());
    let report = registry.accept_changed(&store).await.unwrap();
    assert_eq!(report.accepted, vec![("approval".to_owned(), 1)]);
    assert!(report.backup_file.as_ref().is_some_and(|f| f.is_file()));
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_eq!(count(&store, "SELECT COUNT(*) FROM approval_t").await, 1);
    let again = registry.accept_changed(&store).await.unwrap();
    assert!(again.accepted.is_empty() && again.backup_file.is_none());
    store.close().await;
}

// Scenario: Error path — keeping data written by a newer program
#[tokio::test]
async fn keeping_data_written_by_a_newer_program_is_refused() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    set_stored(&store, "journal", OTHER_FINGERPRINT).await;
    exec(
        &store,
        "INSERT INTO schema_migrations (module, version, description, checksum, applied_at) \
         VALUES ('later_module', 1, 'later', \
         '3333333333333333333333333333333333333333333333333333333333333333', '2026-01-30T00:00:00Z')",
    )
    .await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    match registry.accept_changed(&store).await {
        Err(JournalError::SchemaNewer { module, .. }) => assert_eq!(module, "later_module"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        stored(&store, "journal").await,
        OTHER_FINGERPRINT,
        "no row changed"
    );
    store.close().await;
}
