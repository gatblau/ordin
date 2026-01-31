//! X1: one test per row of AgentFolderManager.md's *Error Table*, provoking the condition and
//! asserting the code, the retryability and that `Display` is the code alone.

use std::time::Duration;

use ordin_types::domain::places::FolderAccess;
use ordin_types::{CodedError, PlaceId, RunId, WorkspaceId};

use super::support::{fixture, fixture_waiting, grant_for, instance, to_instance, TempDir};
use crate::{AfmError, AgentFolderManager, LockMode};

fn check(error: &AfmError, code: &str, retryable: bool) {
    assert_eq!(CodedError::code(error), code);
    assert_eq!(error.retryable(), retryable, "{code}");
    assert_eq!(CodedError::retryable(error), retryable, "{code}");
    assert_eq!(error.to_string(), code, "Display is the code only");
}

// Row: data root cannot be created or written.
#[tokio::test]
async fn data_root_cannot_be_created_or_written() {
    let dir = TempDir::new();
    let journal = super::support::journal_in(&dir).await;
    let file = dir.path().join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let error = AgentFolderManager::new(journal, file.join("data"))
        .await
        .err()
        .unwrap();
    check(&error, "AFM_ROOT_UNWRITABLE", false);
}

// Row: folder or row already exists for the instance (the row).
#[tokio::test]
async fn row_already_exists_for_the_instance() {
    let fx = fixture().await;
    let id = instance(1);
    fx.afm.create(id).await.unwrap();
    let error = fx.afm.create(id).await.unwrap_err();
    assert_eq!(error, AfmError::FolderExists { id });
    check(&error, "AFM_FOLDER_EXISTS", false);
}

// Row: folder or row already exists for the instance (a non-empty `folder/`).
#[tokio::test]
async fn folder_already_holds_files() {
    let fx = fixture().await;
    let id = instance(2);
    let folder = fx.root.join("agents").join(id.to_string()).join("folder");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("left-over.txt"), b"x").unwrap();
    let error = fx.afm.create(id).await.unwrap_err();
    assert_eq!(error, AfmError::FolderExists { id });
    check(&error, "AFM_FOLDER_EXISTS", false);
    assert!(folder.join("left-over.txt").exists(), "nothing was deleted");
    assert_eq!(fx.count("SELECT count(*) FROM agent_folders").await, 0);
}

// Row: no row in state `created` for the instance.
#[tokio::test]
async fn no_row_in_state_created_for_the_instance() {
    let fx = fixture().await;
    let id = instance(3);
    for error in [
        fx.afm.get(id).await.unwrap_err(),
        fx.afm.lock(id, LockMode::Session).await.unwrap_err(),
        fx.afm.remove(id).await.unwrap_err(),
        fx.afm
            .apply_grants(RunId::new(), id, WorkspaceId::new(), None)
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error, AfmError::FolderNotFound { id });
        check(&error, "AFM_FOLDER_NOT_FOUND", false);
    }
    // A row in state `removing` is not found either.
    fx.afm.create(id).await.unwrap();
    fx.sql("UPDATE agent_folders SET state = 'removing'").await;
    check(
        &fx.afm.get(id).await.unwrap_err(),
        "AFM_FOLDER_NOT_FOUND",
        false,
    );
}

// Row: a lease conflicts and the wait ran out.
#[tokio::test]
async fn a_lease_conflicts_and_the_wait_ran_out() {
    let fx = fixture_waiting(Duration::from_millis(200)).await;
    let id = instance(4);
    fx.afm.create(id).await.unwrap();
    let _write = fx.afm.lock(id, LockMode::Write).await.unwrap();
    let error = fx.afm.lock(id, LockMode::Session).await.unwrap_err();
    assert_eq!(error, AfmError::FolderBusy { id, held: "write" });
    check(&error, "AFM_FOLDER_BUSY", true);
    let error = fx.afm.lock(id, LockMode::Write).await.unwrap_err();
    assert_eq!(error, AfmError::FolderBusy { id, held: "write" });
    let error = fx.afm.lock(id, LockMode::SessionUpdate).await.unwrap_err();
    assert_eq!(error, AfmError::FolderBusy { id, held: "write" });
}

// Row: another process holds the OS lock.
#[tokio::test]
async fn another_process_holds_the_os_lock() {
    let fx = fixture_waiting(Duration::from_millis(250)).await;
    let id = instance(5);
    let folder = fx.afm.create(id).await.unwrap();
    // A second handle on the lock file stands in for a second core on the same data root.
    let other = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(folder.meta_path.join("folder.lock"))
        .unwrap();
    other.try_lock().unwrap();
    let error = fx.afm.lock(id, LockMode::Session).await.unwrap_err();
    assert_eq!(
        error,
        AfmError::FolderBusy {
            id,
            held: "another ordin process"
        }
    );
    check(&error, "AFM_FOLDER_BUSY", true);
    let error = fx.afm.lock(id, LockMode::Write).await.unwrap_err();
    assert_eq!(
        error,
        AfmError::FolderBusy {
            id,
            held: "another ordin process"
        }
    );
    // The in-process grant was given back: once the other handle lets go, a lease is granted.
    other.unlock().unwrap();
    fx.afm.lock(id, LockMode::Write).await.unwrap();
}

// Row: a grant's path is missing, not a directory, or inside ordin's data root or an agent folder
// (not an error: left out of the stored set or of the session).
#[tokio::test]
async fn a_grant_path_that_cannot_be_used_is_not_an_error() {
    let fx = fixture().await;
    let (a, w) = (instance(6), WorkspaceId::new());
    fx.afm.create(a).await.unwrap();
    let file = fx.dir.path().join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let missing = fx.dir.path().join("missing");
    let inside = fx.afm.get(a).await.unwrap().folder_path;
    let (p_file, p_missing, p_inside) = (PlaceId::new(), PlaceId::new(), PlaceId::new());
    fx.afm
        .set_workspace_grants(
            w,
            vec![
                grant_for(w, to_instance(a), p_file, FolderAccess::Read, &file),
                grant_for(w, to_instance(a), p_missing, FolderAccess::Read, &missing),
                grant_for(w, to_instance(a), p_inside, FolderAccess::Read, &inside),
            ],
        )
        .await
        .expect("the call succeeds");
    // The path inside the data root is left out of the stored set; the other two are stored and
    // left out of the session because they are not directories.
    assert_eq!(
        fx.count("SELECT count(*) FROM workspace_folder_grants")
            .await,
        2
    );
    let access = fx.afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    assert!(access.grants.is_empty());
}

// Row: `set_workspace_grants` with a grant of another workspace.
#[tokio::test]
async fn set_workspace_grants_with_a_grant_of_another_workspace() {
    let fx = fixture().await;
    let (w, v) = (WorkspaceId::new(), WorkspaceId::new());
    let place = fx.dir.dir("x");
    let foreign = grant_for(
        v,
        to_instance(instance(7)),
        PlaceId::new(),
        FolderAccess::Read,
        &place,
    );
    let folder_grant_id = foreign.grant.id;
    let error = fx
        .afm
        .set_workspace_grants(w, vec![foreign])
        .await
        .unwrap_err();
    assert_eq!(error, AfmError::GrantInvalid { folder_grant_id });
    check(&error, "AFM_GRANT_INVALID", false);
}

// Row: `session_access` for a run with no applied grants and no instance.
#[tokio::test]
async fn session_access_for_a_run_with_no_applied_grants_and_no_instance() {
    let fx = fixture().await;
    let run_id = RunId::new();
    let error = fx.afm.session_access(run_id).await.unwrap_err();
    assert_eq!(error, AfmError::SessionUnknown { run_id });
    check(&error, "AFM_SESSION_UNKNOWN", false);
}

// Row: file-system operation fails (creating the folders).
#[tokio::test]
async fn file_system_operation_fails_on_create() {
    let fx = fixture().await;
    let id = instance(8);
    // A file where the instance's directory belongs.
    std::fs::write(fx.root.join("agents").join(id.to_string()), b"x").unwrap();
    let error = fx.afm.create(id).await.unwrap_err();
    assert_eq!(error, AfmError::IoFailed { retryable: true });
    check(&error, "AFM_IO_FAILED", true);
    assert_eq!(
        fx.count("SELECT count(*) FROM agent_folders").await,
        0,
        "the row was rolled back"
    );
}

// Row: file-system operation fails (removing); the state stays `removing` and `recover` finishes it.
#[cfg(unix)]
#[tokio::test]
async fn file_system_operation_fails_on_remove() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture().await;
    let id = instance(9);
    let folder = fx.afm.create(id).await.unwrap();
    let sealed = folder.folder_path.join("sealed");
    std::fs::create_dir(&sealed).unwrap();
    std::fs::write(sealed.join("file.txt"), b"x").unwrap();
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o500)).unwrap();

    let error = fx.afm.remove(id).await.unwrap_err();
    assert_eq!(error, AfmError::IoFailed { retryable: true });
    check(&error, "AFM_IO_FAILED", true);
    assert_eq!(
        fx.count("SELECT count(*) FROM agent_folders WHERE state = 'removing'")
            .await,
        1
    );

    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o700)).unwrap();
    let report = fx.afm.recover().await.unwrap();
    assert_eq!(report.removals_finished, 1);
    assert!(!folder.folder_path.exists());
}

// Row: datastore busy or locked.
#[tokio::test]
async fn datastore_busy_or_locked() {
    let fx = fixture().await;
    let pool = fx.journal.datastore().sqlite().unwrap();
    // Another connection holds the write lock for longer than the busy timeout.
    let mut holder = pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *holder)
        .await
        .unwrap();
    let error = fx.afm.create(instance(10)).await.unwrap_err();
    assert_eq!(error, AfmError::DatastoreFailed { retryable: true });
    check(&error, "AFM_DATASTORE_FAILED", true);
    sqlx::query("ROLLBACK").execute(&mut *holder).await.unwrap();
}

// Row: other datastore failure.
#[tokio::test]
async fn other_datastore_failure() {
    let fx = fixture().await;
    fx.sql("DROP TABLE session_folder_access").await;
    fx.sql("DROP TABLE agent_folders").await;
    let error = fx.afm.get(instance(11)).await.unwrap_err();
    assert_eq!(error, AfmError::DatastoreFailed { retryable: false });
    check(&error, "AFM_DATASTORE_FAILED", false);
}
