//! X1: one test per row of the Error Table. Each provokes the condition and asserts the code, the
//! retryability and, where the row states one, the repair action. The rows of `GithubAccess` are
//! in `hosts.rs`; `git.rs` holds the unit test of the rate-limit reading.

use std::time::Duration;

use ordin_types::{CodedError, RepairAction, RunId};
use serde_json::json;

use super::support::{
    change_two_places, fixture, fixture_with, sh, start_request, worktree, write, NOBODY,
};
use crate::error::ProjectError;
use crate::service::lock;
use crate::{CommandOrigin, EntryDecision, ProjectId};

fn assert_row(error: &ProjectError, code: &str, retryable: bool, repair: Option<RepairAction>) {
    assert_eq!(error.code(), code);
    assert_eq!(error.to_string(), code);
    assert_eq!(error.retryable(), retryable, "{code}");
    assert_eq!(CodedError::retryable(error), retryable, "{code}");
    assert_eq!(CodedError::code(error), code);
    assert_eq!(CodedError::repair(error), repair, "{code}");
}

#[tokio::test]
async fn proj_not_found_for_an_unknown_project_id() {
    let f = fixture().await;
    let error = f.service.project(ProjectId::new()).await.unwrap_err();
    assert_row(&error, "PROJ_NOT_FOUND", false, None);
}

#[tokio::test]
async fn proj_not_a_repository_for_a_git_action_on_a_plain_folder() {
    let f = fixture().await;
    let folder = f.dir.0.join("plain");
    std::fs::create_dir_all(&folder).unwrap();
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    let error = f.service.status(project.id).await.unwrap_err();
    assert_row(&error, "PROJ_NOT_A_REPOSITORY", false, None);
}

#[tokio::test]
async fn proj_place_already_open_when_the_place_is_a_project() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .open_project(project.place_id, f.workspace)
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_PLACE_ALREADY_OPEN", false, None);
}

#[tokio::test]
async fn proj_sign_in_needed_with_no_sign_in() {
    let f = fixture_with(NOBODY).await;
    let (project, folder) = f.project().await;
    let repo = git2::Repository::open(&folder).unwrap();
    repo.remote("origin", "https://github.com/acme/portal.git")
        .unwrap();
    let error = f.service.push(project.id, None).await.unwrap_err();
    assert_row(
        &error,
        "PROJ_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

#[tokio::test]
async fn proj_remote_unreachable_for_an_unsupported_host_and_an_ssh_remote() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let repo = git2::Repository::open(&folder).unwrap();
    repo.remote("origin", "https://git.example.org/acme/portal.git")
        .unwrap();
    let error = f.service.push(project.id, None).await.unwrap_err();
    assert_row(
        &error,
        "PROJ_REMOTE_UNREACHABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    repo.remote_set_url("origin", "git@github.com:acme/portal.git")
        .unwrap();
    let error = f.service.push(project.id, None).await.unwrap_err();
    assert_eq!(error.code(), "PROJ_REMOTE_UNREACHABLE");
    repo.remote_delete("origin").unwrap();
    let error = f.service.push(project.id, None).await.unwrap_err();
    assert_eq!(error.code(), "PROJ_REMOTE_UNREACHABLE");
}

#[test]
fn proj_rate_limited_is_retryable_with_no_action() {
    assert_row(&ProjectError::RateLimited, "PROJ_RATE_LIMITED", true, None);
}

/// No step of Internal Logic raises this code since ruling R12 moved the commits to Git
/// originals into the Push Service; the row is checked on the value.
#[test]
fn proj_origin_changed_is_permanent_for_this_push() {
    let error = ProjectError::OriginChanged {
        remote_commit: "b".repeat(40),
    };
    assert_row(&error, "PROJ_ORIGIN_CHANGED", false, None);
}

#[tokio::test]
async fn proj_nothing_to_commit() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .commit(project.id, "Nothing", None)
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_NOTHING_TO_COMMIT", false, None);
}

#[tokio::test]
async fn proj_undecided_changes_while_an_entry_waits() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "waiting").await;
    change_two_places(&worktree(&started.task));
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let error = f.service.commit(project.id, "No", None).await.unwrap_err();
    assert_row(&error, "PROJ_UNDECIDED_CHANGES", false, None);
}

#[tokio::test]
async fn proj_branch_exists_when_the_name_is_taken() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .create_branch(project.id, "main")
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_BRANCH_EXISTS", false, None);
}

#[tokio::test]
async fn proj_working_copy_dirty_when_a_switch_would_overwrite() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    f.service.create_branch(project.id, "other").await.unwrap();
    f.service.switch_branch(project.id, "other").await.unwrap();
    write(&folder, "README.md", "on other\n");
    f.service
        .commit(project.id, "On other", None)
        .await
        .unwrap();
    f.service.switch_branch(project.id, "main").await.unwrap();
    write(&folder, "README.md", "uncommitted\n");
    let error = f
        .service
        .switch_branch(project.id, "other")
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_WORKING_COPY_DIRTY", false, None);
    assert_eq!(super::support::read(&folder, "README.md"), "uncommitted\n");
}

#[tokio::test]
async fn proj_too_many_attempts_above_the_cap() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let group = crate::AttemptGroupId::new();
    let mut narrow = super::support::settings();
    narrow.max_attempts = 1;
    f.settings.send(narrow).unwrap();
    f.start_in_group(&project, "one", Some(group)).await;
    let error = f
        .service
        .start_coding_task(start_request(&project, "two", Some(group)))
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_TOO_MANY_ATTEMPTS", false, None);
}

#[tokio::test]
async fn proj_task_not_found_and_task_state_invalid() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .end_coding_run(crate::CodingTaskId::new(), RunId::new())
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_TASK_NOT_FOUND", false, None);
    let started = f.start_task(&project, "once").await;
    change_two_places(&worktree(&started.task));
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let error = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_TASK_STATE_INVALID", false, None);
}

#[tokio::test]
async fn proj_worktree_failed_when_the_folder_cannot_be_made_and_the_branch_goes() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let root = f.data_root.join("worktrees");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join(project.id.to_string()), "in the way").unwrap();
    let error = f
        .service
        .start_coding_task(start_request(&project, "blocked", None))
        .await
        .unwrap_err();
    assert_row(
        &error,
        "PROJ_WORKTREE_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    let repo = git2::Repository::open(&folder).unwrap();
    assert!(repo
        .find_branch("ordin/blocked", git2::BranchType::Local)
        .is_err());
}

#[tokio::test]
async fn proj_entry_not_found_and_decision_invalid() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let error = f
        .service
        .decide_entry(crate::ChangeSetEntryId::new(), EntryDecision::Accept)
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_ENTRY_NOT_FOUND", false, None);

    let started = f.start_task(&project, "stale").await;
    change_two_places(&worktree(&started.task));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    // A hunk that no longer applies to the ancestor.
    sqlx::query("UPDATE change_set_entries SET hunk = '@@ -1 +1 @@\n-nothing like this\n+x\n' WHERE id = ?1")
        .bind(set.entries[0].id.to_string())
        .execute(&f.service.pool)
        .await
        .unwrap();
    let error = f
        .service
        .decide_entry(set.entries[0].id, EntryDecision::Accept)
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_DECISION_INVALID", false, None);
    let _ = folder;
    // A decided entry cannot be decided again.
    f.service
        .decide_entry(set.entries[1].id, EntryDecision::Reject)
        .await
        .unwrap();
    let again = f
        .service
        .decide_entry(set.entries[1].id, EntryDecision::Accept)
        .await
        .unwrap_err();
    assert_eq!(again.code(), "PROJ_DECISION_INVALID");
}

#[tokio::test]
async fn proj_check_denied_by_the_rules() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .run_check(project.id, None, "rm -rf /", CommandOrigin::Person)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_CHECK_DENIED");
    assert!(!error.retryable());
    assert_eq!(CodedError::repair(&error), None);
}

#[tokio::test]
async fn proj_check_timeout_when_the_check_runs_too_long() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    lock(&f.service.tunables).check_timeout = Duration::from_millis(200);
    f.service
        .set_check_commands(project.id, vec![sh::SLOW.to_owned()])
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let error = f
        .service
        .run_check(project.id, None, sh::SLOW, CommandOrigin::Person)
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_CHECK_TIMEOUT", false, None);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn proj_lsp_unavailable_for_a_language_off_failed_or_not_confirmed() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, folder) = f.project().await;
    let error = f
        .service
        .instance(
            crate::lsp::InstanceKey::Editor(project.id, "java".to_owned()),
            &folder,
            "java",
        )
        .await
        .err()
        .unwrap();
    assert_row(&error, "PROJ_LSP_UNAVAILABLE", true, None);
    f.service
        .configure_language("go", None, false, json!({}))
        .await
        .unwrap();
    let off = f
        .service
        .instance(
            crate::lsp::InstanceKey::Editor(project.id, "go".to_owned()),
            &folder,
            "go",
        )
        .await
        .err()
        .unwrap();
    assert_eq!(off.code(), "PROJ_LSP_UNAVAILABLE");
}

#[tokio::test]
async fn proj_lsp_method_not_allowed_outside_the_list() {
    let f = fixture().await;
    let error = f
        .service
        .lsp_send(
            crate::StreamId(uuid::Uuid::now_v7()),
            json!({"method": "workspace/applyEdit"}),
        )
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_LSP_METHOD_NOT_ALLOWED", false, None);
}

#[tokio::test]
async fn proj_server_install_failed_when_the_installer_refuses() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    f.service.editor_open(project.id, "a.go").await.unwrap();
    *f.installer.program.lock().unwrap() = None;
    let error = f.service.restart_language_server("go").await.unwrap_err();
    assert_row(&error, "PROJ_SERVER_INSTALL_FAILED", true, None);
    let session = f.service.editor_open(project.id, "b.go").await.unwrap();
    assert_eq!(session.server_line, "highlighted · no language server");
}

#[tokio::test]
async fn proj_ide_not_found_and_ide_start_failed() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .open_in_ide(project.id, "no-such-ide", None)
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_IDE_NOT_FOUND", false, None);
    assert_eq!(
        f.service
            .set_ide_enabled("no-such-ide", false)
            .await
            .unwrap_err()
            .code(),
        "PROJ_IDE_NOT_FOUND"
    );
    assert_row(
        &ProjectError::IdeStartFailed,
        "PROJ_IDE_START_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
}

#[tokio::test]
async fn proj_terminal_failed_when_the_shell_cannot_start() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    lock(&f.service.tunables).shell = Some("/no/such/shell".to_owned());
    let error = f
        .service
        .terminal_open(project.id, 80, 24)
        .await
        .unwrap_err();
    assert_row(
        &error,
        "PROJ_TERMINAL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    let gone = crate::StreamId(uuid::Uuid::now_v7());
    assert_eq!(
        f.service
            .terminal_input(gone, vec![1])
            .await
            .unwrap_err()
            .code(),
        "PROJ_TERMINAL_FAILED"
    );
    assert_eq!(
        f.service
            .terminal_resize(gone, 80, 24)
            .await
            .unwrap_err()
            .code(),
        "PROJ_TERMINAL_FAILED"
    );
}

#[tokio::test]
async fn proj_git_failed_for_any_other_git_error() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .switch_branch(project.id, "no-such-branch")
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_GIT_FAILED", false, None);
    let read = f
        .service
        .read_at_commit(project.place_id, &"0".repeat(40), "README.md")
        .await
        .unwrap_err();
    assert_eq!(read.code(), "PROJ_GIT_FAILED");
}

#[tokio::test]
async fn proj_files_failed_when_the_file_service_refuses() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let stale = crate::Sha256Hex("0".repeat(64));
    let error = f
        .service
        .save_file(project.id, "README.md", "new\n".to_owned(), Some(stale))
        .await
        .unwrap_err();
    assert_row(
        &error,
        "PROJ_FILES_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    assert_eq!(super::support::read(&folder, "README.md"), "hello\n");
}

#[tokio::test]
async fn proj_too_many_followed_above_twenty_repositories() {
    let f = fixture().await;
    for _ in 0..crate::MAX_FOLLOWED_REPOSITORIES {
        f.service
            .follow_original(crate::PlaceId::new(), "rates.md", "main")
            .await
            .unwrap();
    }
    let known = crate::store::followed_branches(&f.service.pool)
        .await
        .unwrap();
    let error = f
        .service
        .follow_original(crate::PlaceId::new(), "rates.md", "main")
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_TOO_MANY_FOLLOWED", false, None);
    // Another path of a repository already followed is no new repository.
    f.service
        .follow_original(known[0].place_id, "other.md", "main")
        .await
        .unwrap();
}

#[tokio::test]
async fn proj_datastore_unavailable_when_the_tables_cannot_be_reached() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service.pool.close().await;
    let error = f.service.project(project.id).await.unwrap_err();
    assert_row(
        &error,
        "PROJ_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// ---- the 2C rules (one row, a code each)

#[tokio::test]
async fn code_request_invalid_outside_its_length() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let mut request = start_request(&project, "x", None);
    request.request = String::new();
    let empty = f.service.start_coding_task(request).await.unwrap_err();
    assert_row(&empty, "CODE_REQUEST_INVALID", false, None);
    let mut request = start_request(&project, "x", None);
    request.request = "x".repeat(4001);
    let long = f.service.start_coding_task(request).await.unwrap_err();
    assert_eq!(long.code(), "CODE_REQUEST_INVALID");
}

#[tokio::test]
async fn code_branch_invalid_when_no_name_can_be_made() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let name = "x".repeat(100);
    f.start_task(&project, &name).await;
    // `-2` takes the slug past 100 characters.
    let error = f
        .service
        .start_coding_task(start_request(&project, &name, None))
        .await
        .unwrap_err();
    assert_row(&error, "CODE_BRANCH_INVALID", false, None);
    let empty = f
        .service
        .start_coding_task(start_request(&project, "", None))
        .await
        .unwrap_err();
    assert_eq!(empty.code(), "CODE_BRANCH_INVALID");
}

#[tokio::test]
async fn code_commit_invalid_when_the_base_is_not_forty_hex() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "good").await;
    let mut task = started.task.clone();
    task.id = crate::CodingTaskId::new();
    task.base_commit = "not-a-commit".to_owned();
    let error = crate::store::insert_task(&f.service.pool, &task, started.worktree_place_id)
        .await
        .unwrap_err();
    assert_row(&error, "CODE_COMMIT_INVALID", false, None);
}

#[test]
fn code_worktree_outside_root_when_the_path_leaves_the_data_root() {
    use std::path::Path;
    assert!(crate::git::under_root(
        Path::new("/data/worktrees/a"),
        Path::new("/data")
    ));
    assert!(!crate::git::under_root(
        Path::new("/elsewhere/a"),
        Path::new("/data")
    ));
    assert!(!crate::git::under_root(
        Path::new("/data/../etc"),
        Path::new("/data")
    ));
    assert_row(
        &ProjectError::Invalid("CODE_WORKTREE_OUTSIDE_ROOT"),
        "CODE_WORKTREE_OUTSIDE_ROOT",
        false,
        None,
    );
}

#[tokio::test]
async fn code_group_too_small_when_keeping_from_a_one_task_group() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let alone = f
        .start_in_group(&project, "alone", Some(crate::AttemptGroupId::new()))
        .await;
    let error = f.service.keep_attempt(alone.task.id).await.unwrap_err();
    assert_row(&error, "CODE_GROUP_TOO_SMALL", false, None);
    let no_group = f.start_task(&project, "no-group").await;
    let error = f.service.keep_attempt(no_group.task.id).await.unwrap_err();
    assert_eq!(error.code(), "CODE_GROUP_TOO_SMALL");
}

#[tokio::test]
async fn cset_duplicate_for_a_second_change_set_of_a_task() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "twice").await;
    change_two_places(&worktree(&started.task));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let mut again = set.clone();
    again.id = crate::ChangeSetId::new();
    let mut tx = f.service.pool.begin().await.unwrap();
    let error = crate::store::insert_change_set(&mut tx, &again, &Default::default())
        .await
        .unwrap_err();
    assert_row(&error, "CSET_DUPLICATE", false, None);
}

#[tokio::test]
async fn cset_hunk_too_large_for_an_entry_over_one_mib() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "large").await;
    let set = crate::ChangeSet {
        id: crate::ChangeSetId::new(),
        coding_task_id: started.task.id,
        run_id: RunId::new(),
        project_id: project.id,
        entries: vec![crate::ChangeSetEntry {
            id: crate::ChangeSetEntryId::new(),
            file: "a".to_owned(),
            hunk: "x".repeat(crate::MAX_HUNK_BYTES + 1),
            state: crate::ChangeEntryState::Pending,
        }],
        state: crate::ChangeSetState::InReview,
        created_at: crate::Timestamp::now(),
    };
    let mut tx = f.service.pool.begin().await.unwrap();
    let error = crate::store::insert_change_set(&mut tx, &set, &Default::default())
        .await
        .unwrap_err();
    assert_row(&error, "CSET_HUNK_TOO_LARGE", false, None);
}

#[tokio::test]
async fn cset_undecided_entries_guards_settling() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "undecided").await;
    change_two_places(&worktree(&started.task));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let error = crate::store::settle_change_set(&f.service.pool, set.id)
        .await
        .unwrap_err();
    assert_row(&error, "CSET_UNDECIDED_ENTRIES", false, None);
}

#[tokio::test]
async fn ghop_duplicate_for_a_second_row_of_a_kind() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let rows = f.service.git_host_settings(project.id).await.unwrap();
    let mut second = rows[0].clone();
    second.id = ordin_types::GithubOperationSettingId::new();
    let error = crate::store::insert_setting(&f.service.pool, &second)
        .await
        .unwrap_err();
    assert_row(&error, "GHOP_DUPLICATE", false, None);
}

#[tokio::test]
async fn ghop_above_ceiling_for_a_kind_the_company_requires() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .set_git_host_approval(project.id, "merge", false)
        .await
        .unwrap_err();
    assert_row(&error, "GHOP_ABOVE_CEILING", false, None);
}

#[tokio::test]
async fn lang_duplicate_for_a_second_plug_in_of_a_language() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let mut second = crate::store::plugin(&f.service.pool, "go")
        .await
        .unwrap()
        .unwrap();
    second.id = ordin_types::LanguagePluginId::new();
    let error = crate::store::insert_plugin(&f.service.pool, &second)
        .await
        .unwrap_err();
    assert_row(&error, "LANG_DUPLICATE", false, None);
}

#[tokio::test]
async fn lang_server_not_approved_for_a_server_off_the_list() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let error = f
        .service
        .configure_language("go", Some("rogue".to_owned()), true, json!({}))
        .await
        .unwrap_err();
    assert_row(&error, "LANG_SERVER_NOT_APPROVED", false, None);
    let unknown = f
        .service
        .configure_language("cobol", None, true, json!({}))
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), "LANG_SERVER_NOT_APPROVED");
}

#[tokio::test]
async fn lang_settings_invalid_over_sixty_four_kib_or_not_an_object() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let big = json!({"a": "x".repeat(crate::MAX_SERVER_SETTINGS_BYTES)});
    let error = f
        .service
        .configure_language("go", None, true, big)
        .await
        .unwrap_err();
    assert_row(&error, "LANG_SETTINGS_INVALID", false, None);
}

#[tokio::test]
async fn proj_checks_invalid_past_the_limits() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .set_check_commands(project.id, vec!["x".repeat(1001)])
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_CHECKS_INVALID", false, None);
}

#[tokio::test]
async fn proj_branch_mismatch_when_the_branch_and_the_repository_flag_disagree() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let mut row = crate::store::project(&f.service.pool, project.id)
        .await
        .unwrap()
        .unwrap();
    row.project.id = ProjectId::new();
    row.project.is_git_repository = false;
    let error = crate::store::insert_project(&f.service.pool, &row)
        .await
        .unwrap_err();
    assert_row(&error, "PROJ_BRANCH_MISMATCH", false, None);
    let reopened = crate::store::reopen_project(
        &f.service.pool,
        project.id,
        true,
        None,
        crate::Timestamp::now(),
    )
    .await
    .unwrap_err();
    assert_eq!(reopened.code(), "PROJ_BRANCH_MISMATCH");
}

#[test]
fn every_code_of_the_enum_is_in_all_codes() {
    use ProjectError::*;
    let values = [
        NotFound,
        NotARepository,
        PlaceAlreadyOpen,
        SignInNeeded,
        RemoteUnreachable,
        RateLimited,
        PushRefused,
        NothingToCommit,
        UndecidedChanges,
        BranchExists,
        WorkingCopyDirty,
        TooManyAttempts,
        TaskNotFound,
        TaskStateInvalid,
        WorktreeFailed,
        EntryNotFound,
        DecisionInvalid,
        CheckTimeout,
        LspUnavailable,
        LspMethodNotAllowed,
        ServerInstallFailed,
        IdeNotFound,
        IdeStartFailed,
        TerminalFailed,
        GitFailed,
        FilesFailed,
        TooManyFollowed,
        DatastoreUnavailable,
    ];
    for value in values {
        assert!(crate::ALL_CODES.contains(&value.code()), "{value}");
    }
}
