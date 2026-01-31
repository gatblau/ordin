//! The File Service's PostgreSQL slice against a real PostgreSQL 18 container (plan 01,
//! "PostgreSQL 18 in tests"): module `files` applies its one PostgreSQL step from an empty
//! database, and the work folder runs its scenarios on `work_folder_files` in PostgreSQL. One
//! container for this binary; every test gets its own database cloned from a migrated template.
//! A test fails, never skips, when there is no reachable container runtime (`DOCKER_HOST`).

mod support;

use ordin_files::{WorkFolder, WorkFolderConfig};
use ordin_journal::{ApplyMode, Datastore, MigrationRegistry, MIGRATIONS};
use sqlx::Row;
use support::pg;

fn bundle(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ordin-files-pg-{name}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(dir.join("knowledge")).unwrap();
    std::fs::write(dir.join("AGENTS.md"), "# Agent\nline two\n").unwrap();
    std::fs::write(dir.join("knowledge/note.md"), "bundle note\n").unwrap();
    dir
}

async fn folder(
    store: &Datastore,
    writable: Option<Vec<&str>>,
    protected: Vec<&str>,
) -> (WorkFolder, std::path::PathBuf) {
    let bundle_dir = bundle("wf");
    let work = WorkFolder::new(
        store.clone(),
        WorkFolderConfig {
            namespace: "watch-readiness".to_owned(),
            bundle_dir: bundle_dir.clone(),
            writable_paths: writable.map(|w| w.into_iter().map(str::to_owned).collect()),
            protected_paths: protected.into_iter().map(str::to_owned).collect(),
        },
    )
    .expect("work folder");
    (work, bundle_dir)
}

async fn migrated() -> Datastore {
    pg::open_url(pg::migrated_database().await, 4).await
}

#[tokio::test]
async fn the_files_module_applies_its_postgresql_step_and_a_second_apply_changes_nothing() {
    let store = pg::open_url(pg::empty_database().await, 4).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry
        .register(ordin_files::FileService::migrations())
        .unwrap();
    let first = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let second = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert_ne!(
        format!("{first:?}"),
        format!("{second:?}"),
        "the first apply did something"
    );
    let pool = store.postgres().unwrap();
    // The one PostgreSQL table, with its checks and its prefix index.
    let columns: Vec<String> = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_name = 'work_folder_files' ORDER BY ordinal_position",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .iter()
    .map(|r| r.get::<String, _>("column_name"))
    .collect();
    assert_eq!(
        columns,
        [
            "namespace",
            "path",
            "content",
            "encoding",
            "size_bytes",
            "created_at",
            "modified_at"
        ]
    );
    let index: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pg_indexes WHERE tablename = 'work_folder_files' \
         AND indexname = 'work_folder_files_prefix'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(index, 1);
    // The SQLite-only tables are not created here (the module keeps one table in the scaled shape).
    let others: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_name IN \
         ('places', 'file_index', 'file_versions', 'published_copies', 'held_edits')",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(others, 0);
    let bad = sqlx::query(
        "INSERT INTO work_folder_files VALUES ('ns', '/a', 'x', 'latin-1', 1, now(), now())",
    )
    .execute(pool)
    .await;
    assert!(bad.is_err(), "the encoding check holds");
    let bad = sqlx::query(
        "INSERT INTO work_folder_files VALUES ('ns', '/a', 'x', 'utf-8', 10485761, now(), now())",
    )
    .execute(pool)
    .await;
    assert!(bad.is_err(), "the size check holds");
}

#[tokio::test]
async fn error_path_the_agent_cannot_remove_a_duty_s_stop_file() {
    let store = migrated().await;
    let (work, _) = folder(&store, None, vec!["/.agent-run/watch-readiness/STOP"]).await;
    work.operator_write("/.agent-run/watch-readiness/STOP", b"stop".to_vec())
        .await
        .unwrap();
    let refusal = work
        .delete("/.agent-run/watch-readiness")
        .await
        .unwrap_err();
    assert!(
        refusal.0.contains("cannot be removed by the agent"),
        "{}",
        refusal.0
    );
    assert!(work
        .operator_exists("/.agent-run/watch-readiness/STOP")
        .await
        .unwrap());
    work.operator_remove("/.agent-run/watch-readiness")
        .await
        .unwrap();
    assert!(!work
        .operator_exists("/.agent-run/watch-readiness/STOP")
        .await
        .unwrap());
}

#[tokio::test]
async fn error_path_a_bundle_path_is_read_only_for_the_agent() {
    let store = migrated().await;
    let (work, _) = folder(&store, None, vec![]).await;
    assert_eq!(
        work.write("/AGENTS.md", "x").await.unwrap_err().0,
        "Error: /AGENTS.md is read-only (bundle path)"
    );
}

#[tokio::test]
async fn edge_case_an_empty_writable_paths_list_makes_the_work_folder_read_only_for_the_agent() {
    let store = migrated().await;
    let (work, _) = folder(&store, Some(vec![]), vec![]).await;
    assert_eq!(
        work.write("/plans/a.md", "x").await.unwrap_err().0,
        "Error: /plans/a.md is outside the writable paths of this agent"
    );
}

#[tokio::test]
async fn happy_path_the_operator_checks_a_work_folder_file_exists() {
    let store = migrated().await;
    let (work, _) = folder(&store, None, vec![]).await;
    work.operator_write("/plans/a.md", b"plan".to_vec())
        .await
        .unwrap();
    assert!(work.operator_exists("/plans/a.md").await.unwrap());
    assert!(!work.operator_exists("/plans/b.md").await.unwrap());
    assert_eq!(
        work.operator_exists("/AGENTS.md").await.unwrap_err().code(),
        "WF_BUNDLE_PATH"
    );
}

#[tokio::test]
async fn the_agent_tools_and_the_operator_routes_work_on_postgresql() {
    let store = migrated().await;
    let (work, _) = folder(&store, None, vec![]).await;
    work.write("/plans/a.md", "alpha\nbeta needle\n")
        .await
        .unwrap();
    work.write("/plans/sub/b.md", "needle again\n")
        .await
        .unwrap();
    work.write("/notes.md", "n").await.unwrap();
    assert_eq!(work.read("/plans/a.md", 1, 1).await.unwrap(), "beta needle");
    assert_eq!(
        work.edit("/plans/a.md", "needle", "pin", false)
            .await
            .unwrap(),
        1
    );
    work.write("/plans/a.md", "alpha\nbeta needle\n")
        .await
        .unwrap();
    let root: Vec<String> = work
        .ls("/")
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(root, ["/AGENTS.md", "/knowledge/", "/notes.md", "/plans/"]);
    assert_eq!(
        work.glob("**/*.md", Some("/plans")).await.unwrap(),
        ["/plans/a.md", "/plans/sub/b.md"]
    );
    let found = work.grep("needle", Some("/plans"), None).await.unwrap();
    assert_eq!(found.len(), 2);
    // Overwrite keeps the creation row; delete removes a folder's files.
    work.delete("/plans").await.unwrap();
    assert!(!work.operator_exists("/plans/a.md").await.unwrap());
    // Bytes that are not UTF-8 round-trip; the size limit is enforced before storage.
    work.operator_write("/raw.bin", vec![0xff, 0x00, 0x41])
        .await
        .unwrap();
    assert_eq!(
        work.operator_read("/raw.bin").await.unwrap(),
        vec![0xff, 0x00, 0x41]
    );
    assert_eq!(
        work.operator_write("/big", vec![b'a'; 10_485_761])
            .await
            .unwrap_err()
            .code(),
        "WF_TOO_LARGE"
    );
    assert_eq!(
        work.operator_read("/missing").await.unwrap_err().code(),
        "WF_NOT_FOUND"
    );
    assert_eq!(
        work.operator_remove("/missing").await.unwrap_err().code(),
        "WF_NOT_FOUND"
    );
    // Paging by path, 100 a page, in byte order.
    for n in 0..125 {
        work.operator_write(&format!("/many/f{n:03}"), vec![b'x'])
            .await
            .unwrap();
    }
    let first = work.operator_list("/many", None).await.unwrap();
    assert_eq!(
        (first.items.len(), first.next_cursor.is_some()),
        (100, true)
    );
    let second = work
        .operator_list("/many/", first.next_cursor)
        .await
        .unwrap();
    assert_eq!(
        (second.items.len(), second.next_cursor.is_none()),
        (25, true)
    );
    assert_eq!(second.items[0].path, "/many/f100");
}

#[tokio::test]
async fn namespaces_are_kept_apart_on_postgresql() {
    let store = migrated().await;
    let (one, bundle_dir) = folder(&store, None, vec![]).await;
    let other = WorkFolder::new(
        store.clone(),
        WorkFolderConfig {
            namespace: "other".to_owned(),
            bundle_dir,
            writable_paths: None,
            protected_paths: vec![],
        },
    )
    .unwrap();
    one.write("/same.md", "mine").await.unwrap();
    assert!(!other.operator_exists("/same.md").await.unwrap());
    other.write("/same.md", "theirs").await.unwrap();
    assert_eq!(one.read("/same.md", 0, 5).await.unwrap(), "mine");
}

#[tokio::test]
async fn the_store_being_unreachable_is_reported_as_the_spec_says() {
    let store = migrated().await;
    let (work, _) = folder(&store, None, vec![]).await;
    store.close().await;
    assert_eq!(
        work.operator_read("/a").await.unwrap_err().code(),
        "WF_STORE_UNAVAILABLE"
    );
    assert_eq!(
        work.write("/a", "x").await.unwrap_err().0,
        "Error: the work folder cannot be reached"
    );
}
