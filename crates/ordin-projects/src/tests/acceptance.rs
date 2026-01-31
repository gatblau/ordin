//! X2: the scenarios of *Acceptance Criteria* at this module's boundary, each under the scenario's
//! name. The scenarios on remotes are in `remotes.rs`, those on repository hosts in `hosts.rs`, and
//! the terminal's in `tests/logging.rs`.

use std::time::Duration;

use ordin_types::{AttemptGroupId, ProjectId, RunId};
use serde_json::json;

use super::support::{
    change_two_places, fixture, pin, read, repository, settings, sh, start_request, worktree, write,
};
use crate::service::lock;
use crate::{
    ApprovedServers, ChangeEntryState, ChangeSetState, CodingTaskState, DecisionOutcome,
    EntryDecision, LanguagePluginStatus, ProjectState,
};

#[tokio::test]
async fn the_project_open_on_a_place_is_found_for_a_coding_task() {
    let f = fixture().await;
    let (rates, _) = f.project().await;
    let other = f.dir.0.join("other");
    repository(&other, &[("a.txt", "a\n")]);
    let p2 = f.place(&other).await;
    let closed = f.service.open_project(p2, f.workspace).await.unwrap();
    f.service.close_project(closed.id).await.unwrap();

    let found = f.service.project_for_place(rates.place_id).await.unwrap();
    assert_eq!(found.unwrap().id, rates.id);
    assert!(f.service.project_for_place(p2).await.unwrap().is_none());
}

// spec: ordin-desktop/components/ProjectService#PROJ-ORIG-1
#[tokio::test]
async fn edge_case_a_coding_task_s_run_is_told_apart_by_its_origin() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let request = start_request(&project, "origin", None);
    let origin = request.origin;
    f.service.start_coding_task(request).await.unwrap();
    assert!(f.service.is_coding_task_origin(origin).await.unwrap());
    let other = ordin_types::domain::RunOrigin::Task {
        task_id: ordin_types::TaskId::new(),
    };
    assert!(!f.service.is_coding_task_origin(other).await.unwrap());
}

#[tokio::test]
async fn happy_path_the_engineer_sets_the_project_s_check_commands() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let commands = vec![
        "cargo test".to_owned(),
        " cargo test ".to_owned(),
        "cargo clippy".to_owned(),
    ];
    let updated = f
        .service
        .set_check_commands(project.id, commands)
        .await
        .unwrap();
    assert_eq!(updated.check_commands, ["cargo test", "cargo clippy"]);
    let stored = f.count("SELECT COUNT(*) FROM projects WHERE check_commands = '[\"cargo test\",\"cargo clippy\"]'").await;
    assert_eq!(stored, 1);
    let entries = f.entries_of("check_commands_set").await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].data["count"], 2);
    assert!(!entries[0].data.to_string().contains("cargo"));
}

#[tokio::test]
async fn error_path_too_many_check_commands_are_refused() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["one".to_owned()])
        .await
        .unwrap();
    let commands = (0..21).map(|n| format!("check {n}")).collect();
    let error = f
        .service
        .set_check_commands(project.id, commands)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_CHECKS_INVALID");
    assert_eq!(
        f.service.project(project.id).await.unwrap().check_commands,
        ["one"]
    );
}

#[tokio::test]
async fn edge_case_a_project_with_no_check_commands_runs_no_checks() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "rates-retry").await;
    change_two_places(&worktree(&started.task));
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let task = f.service.task_row(started.task.id).await.unwrap().task;
    assert_eq!(task.test_outcome, None);
    assert!(f.entries_of("check_run").await.is_empty());
}

#[tokio::test]
async fn happy_path_a_coding_task_gets_its_own_worktree_and_branch() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let started = f.start_task(&project, "Rates retry").await;
    let task = &started.task;
    assert_eq!(task.branch, "ordin/rates-retry");
    assert_eq!(task.base_branch, "main");
    assert_eq!(task.state, CodingTaskState::Working);

    let repo = git2::Repository::open(&folder).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap().id();
    assert_eq!(task.base_commit, head.to_string());
    assert_eq!(
        repo.refname_to_id("refs/heads/ordin/rates-retry").unwrap(),
        head
    );

    let path = worktree(task);
    assert!(path.starts_with(&f.data_root));
    assert_eq!(read(&path, "src/rates.rs"), read(&folder, "src/rates.rs"));
    assert!(path.ends_with(format!("worktrees/{}/{}", project.id, task.id)));
    let place = f.files.place(started.worktree_place_id).await.unwrap();
    assert_eq!(place.kind, ordin_files::PlaceKind::CodingTaskWorktree);
    assert_eq!(place.friendly_name, "ordin/rates-retry");
    assert!(f.service.status(project.id).await.unwrap().is_empty());
    assert_eq!(f.entries_of("coding_task_started").await.len(), 1);
}

#[tokio::test]
async fn happy_path_the_change_set_comes_from_the_worktree_diff() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "rates-retry").await;
    change_two_places(&worktree(&started.task));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    assert_eq!(set.state, ChangeSetState::InReview);
    assert_eq!(set.entries.len(), 2);
    assert!(set.entries.iter().all(|e| e.file == "src/rates.rs"));
    assert!(set.entries[0].hunk.starts_with("@@ "));
    assert!(set.entries[0].hunk.contains("+line two"));
    let review = f.service.review(project.id).await.unwrap();
    assert_eq!(review.len(), 1);
    assert_eq!(review[0].task.state, CodingTaskState::ReadyForReview);
    assert_eq!(review[0].change_set.as_ref().unwrap().entries.len(), 2);
}

#[tokio::test]
async fn happy_path_accepting_an_entry_applies_it_to_the_engineer_s_working_copy() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let started = f.start_task(&project, "rates-retry").await;
    change_two_places(&worktree(&started.task));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let before = git2::Repository::open(&folder)
        .unwrap()
        .head()
        .unwrap()
        .target();

    let outcome = f
        .service
        .decide_entry(set.entries[0].id, EntryDecision::Accept)
        .await
        .unwrap();
    assert!(matches!(outcome, DecisionOutcome::Applied { .. }));
    let text = read(&folder, "src/rates.rs");
    assert!(text.contains("line two\n"));
    assert!(!text.contains("line fifteen"), "only this hunk");
    let version = f
        .files
        .current_version(project.place_id, "src/rates.rs")
        .await
        .unwrap();
    assert!(version.is_some(), "recorded as a new version");
    let after = git2::Repository::open(&folder)
        .unwrap()
        .head()
        .unwrap()
        .target();
    assert_eq!(before, after, "nothing is committed");

    // The second entry merges cleanly over the first.
    f.service
        .decide_entry(set.entries[1].id, EntryDecision::Accept)
        .await
        .unwrap();
    let text = read(&folder, "src/rates.rs");
    assert!(text.contains("line two\n") && text.contains("line fifteen\n"));
}

#[tokio::test]
async fn error_path_the_engineer_changed_the_same_lines() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let started = f.start_task(&project, "rates-retry").await;
    change_two_places(&worktree(&started.task));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let mine = read(&folder, "src/rates.rs").replace("line 2\n", "line deux\n");
    write(&folder, "src/rates.rs", &mine);

    let outcome = f
        .service
        .decide_entry(set.entries[0].id, EntryDecision::Accept)
        .await
        .unwrap();
    match outcome {
        DecisionOutcome::Conflict { yours, agents } => {
            assert!(yours.contains("line deux"), "{yours}");
            assert!(agents.contains("line two"), "{agents}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        read(&folder, "src/rates.rs"),
        mine,
        "the working copy stays"
    );
    let stored = f.service.review(project.id).await.unwrap();
    let entries = &stored[0].change_set.as_ref().unwrap().entries;
    assert_eq!(entries[0].state, ChangeEntryState::Pending);

    // The two ways out of a conflict.
    f.service
        .decide_entry(set.entries[0].id, EntryDecision::UseTheAgents)
        .await
        .unwrap();
    assert!(read(&folder, "src/rates.rs").contains("line two\n"));
    f.service
        .decide_entry(set.entries[1].id, EntryDecision::KeepYours)
        .await
        .unwrap();
    assert!(!read(&folder, "src/rates.rs").contains("line fifteen"));
}

#[tokio::test]
async fn edge_case_the_last_decision_settles_and_cleans_up() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let started = f.start_task(&project, "rates-retry").await;
    let path = worktree(&started.task);
    change_two_places(&path);
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    f.service
        .decide_entry(set.entries[0].id, EntryDecision::Reject)
        .await
        .unwrap();
    assert!(path.exists(), "one entry is still pending");
    let last = f
        .service
        .decide_entry(set.entries[1].id, EntryDecision::Reject)
        .await
        .unwrap();
    assert!(matches!(last, DecisionOutcome::Dropped));

    let task = f.service.task_row(started.task.id).await.unwrap();
    assert_eq!(task.task.state, CodingTaskState::Settled);
    let stored = crate::store::change_set_of(&f.service.pool, started.task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.state, ChangeSetState::Settled);
    assert!(!path.exists());
    let repo = git2::Repository::open(&folder).unwrap();
    assert!(repo
        .find_branch("ordin/rates-retry", git2::BranchType::Local)
        .is_err());
    assert_eq!(f.entries_of("coding_task_removed").await.len(), 1);
    assert!(f.files.place(started.worktree_place_id).await.is_err());
    assert_eq!(read(&folder, "src/rates.rs"), super::support::rates_file());
}

#[tokio::test]
async fn edge_case_keeping_one_attempt_sets_the_others_aside() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let group = AttemptGroupId::new();
    let first = f.start_in_group(&project, "attempt", Some(group)).await;
    let second = f.start_in_group(&project, "attempt", Some(group)).await;
    assert_eq!(second.task.branch, "ordin/attempt-2");

    f.service.keep_attempt(first.task.id).await.unwrap();
    let row = f.service.task_row(second.task.id).await.unwrap();
    assert_eq!(row.task.state, CodingTaskState::SetAside);
    assert!(!worktree(&second.task).exists());
    let repo = git2::Repository::open(&folder).unwrap();
    assert!(repo
        .find_branch("ordin/attempt-2", git2::BranchType::Local)
        .is_err());
    assert!(repo
        .find_branch("ordin/attempt", git2::BranchType::Local)
        .is_ok());
    assert_eq!(
        f.service.task_row(first.task.id).await.unwrap().task.state,
        CodingTaskState::Working
    );
    let kept = f.entries_of("attempt_kept").await;
    assert_eq!(kept[0].data["task_id"], first.task.id.to_string());
}

#[tokio::test]
async fn error_path_attempts_above_the_cap() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let group = AttemptGroupId::new();
    for n in 0..3 {
        f.start_in_group(&project, &format!("try-{n}"), Some(group))
            .await;
    }
    let error = f
        .service
        .start_coding_task(start_request(&project, "try-3", Some(group)))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_TOO_MANY_ATTEMPTS");
}

#[tokio::test]
async fn happy_path_the_workspace_s_languages_come_from_its_projects() {
    let f = fixture().await;
    let folder = f.dir.0.join("languages");
    repository(
        &folder,
        &[("go.mod", "module x\n"), ("tsconfig.json", "{}\n")],
    );
    let place = f.place(&folder).await;
    f.service.open_project(place, f.workspace).await.unwrap();
    assert_eq!(
        f.service.workspace_languages(f.workspace).await.unwrap(),
        ["go", "typescript"]
    );
    let empty = f
        .service
        .workspace_languages(ordin_types::WorkspaceId::new())
        .await
        .unwrap();
    assert!(empty.is_empty());
}

#[tokio::test]
async fn error_path_commit_with_undecided_changes() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "rates-retry").await;
    change_two_places(&worktree(&started.task));
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let error = f
        .service
        .commit(project.id, "Change", None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_UNDECIDED_CHANGES");
}

#[tokio::test]
async fn error_path_switching_off_approval_above_the_company_s_ceiling() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let error = f
        .service
        .set_git_host_approval(project.id, "merge", false)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "GHOP_ABOVE_CEILING");
}

#[tokio::test]
async fn happy_path_new_problems_compare_with_the_base_branch() {
    let f = fixture().await;
    {
        let mut tunables = lock(&f.service.tunables);
        tunables.worktree_confirmed.insert("rust".to_owned());
        tunables.diagnostics_settle = Duration::from_millis(200);
    }
    f.service.reconcile_settings().await.unwrap();
    let folder = f.dir.0.join("problems");
    let base = "fn fetch() {\n    // WARN slow\n}\n";
    repository(&folder, &[("src/rates.rs", base)]);
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    let started = f.start_task(&project, "rates").await;
    let path = worktree(&started.task);
    write(
        &path,
        "src/rates.rs",
        "fn fetch() {\n    // WARN slow\n    // ERROR new\n}\n",
    );
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();

    let row = f.service.task_row(started.task.id).await.unwrap();
    assert_eq!(row.task.new_problems, 1);
    assert_eq!(row.problems[0].severity, "error");
    assert_eq!(row.problems[0].code.as_deref(), Some("E1"));
    assert_eq!(row.problems[0].file, "src/rates.rs");
    let review = f.service.review(project.id).await.unwrap();
    assert_eq!(review[0].new_problems.len(), 1);
    let base_folder = f
        .data_root
        .join("worktrees")
        .join(project.id.to_string())
        .join(format!("base-{}", started.task.id));
    assert!(!base_folder.exists(), "the base worktree is removed");
}

#[tokio::test]
async fn error_path_invalid_language_server_settings_are_refused() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let error = f
        .service
        .configure_language("go", None, true, json!([1, 2]))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "LANG_SETTINGS_INVALID");
    let plugin = crate::store::plugin(&f.service.pool, "go")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(plugin.server_settings, json!({}));
    assert_eq!(plugin.status, LanguagePluginStatus::Available);
}

#[tokio::test]
async fn error_path_the_editor_cannot_send_arbitrary_lsp_methods() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    let session = f
        .service
        .editor_open(project.id, "src/rates.go")
        .await
        .unwrap();
    let error = f
        .service
        .lsp_send(
            session.stream,
            json!({"jsonrpc": "2.0", "id": 1, "method": "workspace/executeCommand"}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_LSP_METHOD_NOT_ALLOWED");
    assert!(!error.retryable());
}

#[tokio::test]
async fn without_a_profile_entry_the_built_in_table_approves_the_language_server() {
    let f = fixture().await;
    // The program fills `language_servers` from `language-servers.json` for a language with no
    // profile entry (SA-CLOSE-02): the table's one entry for go, gopls, is approved and default.
    let mut built = settings();
    built.language_servers.insert(
        "go".to_owned(),
        ApprovedServers {
            approved: vec![pin("gopls", "0.22.1")],
            default: "gopls".to_owned(),
        },
    );
    f.settings.send(built).unwrap();
    f.service.reconcile_settings().await.unwrap();
    let plugin = crate::store::plugin(&f.service.pool, "go")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(plugin.approved_servers, ["gopls"]);
    assert_eq!(plugin.default_server, "gopls");
    assert_eq!(plugin.pinned_version.as_deref(), Some("0.22.1"));
}

// ---- around the scenarios

#[tokio::test]
async fn a_closed_project_reopens_as_the_same_record() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["make".to_owned()])
        .await
        .unwrap();
    let closed = f.service.close_project(project.id).await.unwrap();
    assert_eq!(closed.state, ProjectState::Closed);
    assert!(f.service.close_project(project.id).await.is_err());
    let again = f
        .service
        .open_project(project.place_id, f.workspace)
        .await
        .unwrap();
    assert_eq!(again.id, project.id);
    assert_ne!(again.state, ProjectState::Closed);
    assert_eq!(again.check_commands, ["make"]);
}

#[tokio::test]
async fn a_folder_with_no_repository_is_still_a_project() {
    let f = fixture().await;
    let folder = f.dir.0.join("plain");
    std::fs::create_dir_all(&folder).unwrap();
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    assert!(!project.is_git_repository);
    assert_eq!(project.current_branch, None);
    let calls = f.registry.0.lock().unwrap().clone();
    assert_eq!(calls, ["github", "gitlab"]);
}

#[tokio::test]
async fn an_unchanged_worktree_settles_with_no_change_set() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "nothing").await;
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    assert!(set.entries.is_empty());
    let row = f.service.task_row(started.task.id).await.unwrap();
    assert_eq!(row.task.state, CodingTaskState::Settled);
    assert!(!worktree(&started.task).exists());
    assert_eq!(f.count("SELECT COUNT(*) FROM change_sets").await, 0);
}

#[tokio::test]
async fn untracked_binary_and_oversized_files_each_make_one_entry() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let started = f.start_task(&project, "files").await;
    let path = worktree(&started.task);
    write(&path, "src/new.rs", "fn new() {}\n");
    std::fs::write(path.join("logo.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    let big = "x\n".repeat(600_000);
    write(&path, "big.txt", &big);
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let hunk_of = |file: &str| {
        set.entries
            .iter()
            .find(|e| e.file == file)
            .map(|e| e.hunk.clone())
            .unwrap()
    };
    assert!(hunk_of("src/new.rs").contains("+fn new() {}"));
    assert_eq!(hunk_of("logo.bin"), crate::BINARY_HUNK);
    assert_eq!(hunk_of("big.txt"), crate::TOO_LARGE_HUNK);
    let id_of = |file: &str| set.entries.iter().find(|e| e.file == file).unwrap().id;

    f.service
        .decide_entry(id_of("src/new.rs"), EntryDecision::Accept)
        .await
        .unwrap();
    assert_eq!(read(&folder, "src/new.rs"), "fn new() {}\n");
    f.service
        .decide_entry(id_of("big.txt"), EntryDecision::Accept)
        .await
        .unwrap();
    assert_eq!(read(&folder, "big.txt"), big);
    let outcome = f
        .service
        .decide_entry(id_of("logo.bin"), EntryDecision::Accept)
        .await
        .unwrap();
    assert!(matches!(outcome, DecisionOutcome::Applied { .. }));
    assert_eq!(std::fs::read(folder.join("logo.bin")).unwrap().len(), 6);
}

#[tokio::test]
async fn a_binary_file_the_engineer_changed_is_a_conflict() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    std::fs::write(folder.join("logo.bin"), [0u8, 1]).unwrap();
    let started = f.start_task(&project, "files").await;
    std::fs::write(worktree(&started.task).join("logo.bin"), [0u8, 2, 3]).unwrap();
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let entry = set.entries[0].id;
    let outcome = f
        .service
        .decide_entry(entry, EntryDecision::Accept)
        .await
        .unwrap();
    assert!(matches!(outcome, DecisionOutcome::Conflict { .. }));
    f.service
        .decide_entry(entry, EntryDecision::UseTheAgents)
        .await
        .unwrap();
    assert_eq!(std::fs::read(folder.join("logo.bin")).unwrap(), [0, 2, 3]);
}

/// A finished run whose only change is the binary `logo.bin`, in a folder `assets/`.
async fn binary_entry(
    f: &super::support::Fixture,
    project: &crate::Project,
) -> (std::path::PathBuf, ordin_types::ChangeSetEntryId) {
    let started = f.start_task(project, "files").await;
    let path = worktree(&started.task);
    std::fs::create_dir_all(path.join("assets")).unwrap();
    std::fs::write(path.join("assets/logo.bin"), BINARY).unwrap();
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    assert_eq!(set.entries[0].hunk, crate::BINARY_HUNK);
    (path, set.entries[0].id)
}

const BINARY: [u8; 6] = [0, 159, 146, 150, 0, 1];

/// The entry cannot be applied, the engineer's copy stays as it was, and it can still be rejected.
async fn assert_refused(
    f: &super::support::Fixture,
    folder: &std::path::Path,
    entry: ordin_types::ChangeSetEntryId,
) {
    for decision in [EntryDecision::Accept, EntryDecision::UseTheAgents] {
        let error = f.service.decide_entry(entry, decision).await.unwrap_err();
        assert_eq!(error.code(), "PROJ_DECISION_INVALID");
    }
    assert!(!folder.join("assets/logo.bin").exists());
    f.service
        .decide_entry(entry, EntryDecision::Reject)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_whole_file_entry_records_its_content_hash_and_a_hunk_does_not() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "files").await;
    let path = worktree(&started.task);
    write(&path, "src/new.rs", "fn new() {}\n");
    std::fs::write(path.join("logo.bin"), BINARY).unwrap();
    write(&path, "big.txt", &"x\n".repeat(600_000));
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let recorded = "SELECT COUNT(*) FROM change_set_entries \
                    WHERE content_hash IS NOT NULL AND hunk IN ('Binary file changed', \
                    'too large to review here')";
    assert_eq!(f.count(recorded).await, 2);
    let hunks = "SELECT COUNT(*) FROM change_set_entries \
                 WHERE content_hash IS NULL AND file = 'src/new.rs'";
    assert_eq!(f.count(hunks).await, 1);
}

#[tokio::test]
async fn a_binary_entry_whose_file_changed_after_capture_is_refused() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let (path, entry) = binary_entry(&f, &project).await;
    std::fs::write(path.join("assets/logo.bin"), [0u8, 9, 9]).unwrap();
    assert_refused(&f, &folder, entry).await;
}

#[tokio::test]
async fn an_oversized_entry_whose_file_changed_after_capture_is_refused() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let started = f.start_task(&project, "files").await;
    let path = worktree(&started.task);
    write(&path, "big.txt", &"x\n".repeat(600_000));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    assert_eq!(set.entries[0].hunk, crate::TOO_LARGE_HUNK);
    write(&path, "big.txt", &"y\n".repeat(600_000));
    let error = f
        .service
        .decide_entry(set.entries[0].id, EntryDecision::Accept)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_DECISION_INVALID");
    assert!(!folder.join("big.txt").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_binary_entry_replaced_by_a_link_to_the_same_bytes_is_refused() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let (path, entry) = binary_entry(&f, &project).await;
    std::fs::rename(path.join("assets/logo.bin"), path.join("assets/copy.bin")).unwrap();
    std::os::unix::fs::symlink("copy.bin", path.join("assets/logo.bin")).unwrap();
    assert_refused(&f, &folder, entry).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_binary_entry_replaced_by_a_link_out_of_the_worktree_is_refused() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let (path, entry) = binary_entry(&f, &project).await;
    let outside = f.dir.0.join("outside.bin");
    std::fs::write(&outside, BINARY).unwrap();
    std::fs::remove_file(path.join("assets/logo.bin")).unwrap();
    std::os::unix::fs::symlink(&outside, path.join("assets/logo.bin")).unwrap();
    assert_refused(&f, &folder, entry).await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_binary_entry_behind_a_linked_folder_is_refused() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let (path, entry) = binary_entry(&f, &project).await;
    std::fs::rename(path.join("assets"), path.join("moved")).unwrap();
    std::os::unix::fs::symlink("moved", path.join("assets")).unwrap();
    assert_refused(&f, &folder, entry).await;
}

#[tokio::test]
async fn a_binary_entry_with_no_recorded_hash_is_refused() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let (_, entry) = binary_entry(&f, &project).await;
    sqlx::query("UPDATE change_set_entries SET content_hash = NULL WHERE id = ?1")
        .bind(entry.to_string())
        .execute(f.store.sqlite().unwrap())
        .await
        .unwrap();
    assert_refused(&f, &folder, entry).await;
}

#[tokio::test]
async fn the_engineer_s_git_views_read_the_repository() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    write(&folder, "README.md", "changed\n");
    write(&folder, "new.txt", "new\n");
    std::fs::remove_file(folder.join("src/rates.rs")).unwrap();
    write(&folder, ".gitignore", "ignored.txt\n");
    write(&folder, "ignored.txt", "x\n");
    let status = f.service.status(project.id).await.unwrap();
    let word = |path: &str| {
        status
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.status.as_str())
    };
    assert_eq!(word("README.md"), Some("modified"));
    assert_eq!(word("new.txt"), Some("new"));
    assert_eq!(word("src/rates.rs"), Some("deleted"));
    assert_eq!(word("ignored.txt"), None);

    let diff = f.service.diff(project.id, Some("README.md")).await.unwrap();
    assert!(
        diff.contains("+changed") && diff.contains("-hello"),
        "{diff}"
    );
    assert!(!diff.contains("new.txt"));
    let all = f.service.diff(project.id, None).await.unwrap();
    assert!(all.contains("new.txt"));

    let commit = f.service.commit(project.id, "Tidy up", None).await.unwrap();
    assert_eq!(commit.subject, "Tidy up");
    assert_eq!(commit.author, "Asha Rao");
    let history = f.service.history(project.id, None, 10).await.unwrap();
    assert_eq!(history.len(), 2);
    let one = f
        .service
        .history(project.id, Some("README.md"), 1)
        .await
        .unwrap();
    assert_eq!(one[0].subject, "Tidy up");
    let filtered = f
        .service
        .history(project.id, Some("new.txt"), 10)
        .await
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(f.entries_of("git_committed").await.len(), 1);
}

#[tokio::test]
async fn branches_hide_ordin_s_own_and_a_switch_moves_head() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    f.start_task(&project, "hidden").await;
    f.service
        .create_branch(project.id, "feature")
        .await
        .unwrap();
    assert_eq!(
        f.service.branches(project.id).await.unwrap(),
        ["feature", "main"]
    );
    f.service
        .switch_branch(project.id, "feature")
        .await
        .unwrap();
    let repo = git2::Repository::open(&folder).unwrap();
    assert_eq!(repo.head().unwrap().shorthand().unwrap(), "feature");
    assert_eq!(
        f.service
            .project(project.id)
            .await
            .unwrap()
            .current_branch
            .as_deref(),
        Some("feature")
    );
}

#[tokio::test]
async fn a_detached_head_shows_the_short_commit() {
    let f = fixture().await;
    let folder = f.dir.0.join("detached");
    let repo = repository(&folder, &[("a.txt", "a\n")]);
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    let branch = project.current_branch.unwrap();
    assert!(head.to_string().starts_with(&branch), "{branch}");
}

#[tokio::test]
async fn saving_a_file_makes_a_new_version_the_status_shows() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    f.service
        .save_file(project.id, "README.md", "saved\n".to_owned(), None)
        .await
        .unwrap();
    assert_eq!(read(&folder, "README.md"), "saved\n");
    let status = f.service.status(project.id).await.unwrap();
    assert_eq!(status[0].path, "README.md");
}

#[tokio::test]
async fn the_engineer_s_commit_is_told_to_the_terminal_stream() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    write(&folder, "README.md", "changed\n");
    let stream = crate::StreamId(uuid::Uuid::now_v7());
    f.service
        .commit(project.id, "Say \"hi\"", Some(stream))
        .await
        .unwrap();
    let text = f.streams.terminal_text(stream);
    assert!(
        text.contains("[ordin] git commit -m \"Say \\\"hi\\\"\""),
        "{text}"
    );
    assert!(text.contains("[ordin] committed "));
}

#[tokio::test]
async fn a_person_runs_a_declared_check_and_the_journal_keeps_no_output() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["echo checked".to_owned()])
        .await
        .unwrap();
    let result = f
        .service
        .run_check(
            project.id,
            None,
            "echo checked",
            crate::CommandOrigin::Person,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.output, format!("checked{}", sh::NL));
    assert!(!result.truncated);
    let entries = f.entries_of("check_run").await;
    assert_eq!(entries.len(), 1);
    assert!(!entries[0].data.to_string().contains("checked"));
}

#[tokio::test]
async fn a_check_s_environment_holds_no_credential_and_long_output_is_cut() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let folder = f.dir.0.join("supplier-portal");
    std::env::set_var("GH_TOKEN", "must-not-reach-the-check");
    // `cmd.exe` leaves a variable that is not set as written.
    let (echo, empty) = if cfg!(windows) {
        ("echo [%GH_TOKEN%]", "[%GH_TOKEN%]\r\n")
    } else {
        ("echo \"[$GH_TOKEN]\"", "[]\n")
    };
    let probe = crate::checks::execute(&folder, echo, Duration::from_secs(30))
        .await
        .unwrap();
    std::env::remove_var("GH_TOKEN");
    assert_eq!(probe.output, empty);
    // 20,000 lines of 62 bytes on Windows: more than 1 MiB.
    let many = if cfg!(windows) {
        "for /L %i in (1,1,20000) do @echo xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
    } else {
        "head -c 1200000 /dev/zero | tr '\\0' x"
    };
    let long = crate::checks::execute(&folder, many, Duration::from_secs(30))
        .await
        .unwrap();
    assert!(long.truncated);
    assert!(long.output.ends_with("only the first 1 MiB is shown.]\n"));
    let _ = project;
}

#[tokio::test]
async fn a_failing_or_denied_task_check_is_a_failed_outcome() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["exit 3".to_owned()])
        .await
        .unwrap();
    let started = f.start_task(&project, "checks").await;
    change_two_places(&worktree(&started.task));
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let task = f.service.task_row(started.task.id).await.unwrap().task;
    assert_eq!(task.test_outcome, Some(crate::TestOutcome::Failed));
}

#[tokio::test]
async fn approval_settings_are_seeded_and_pushed() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let rows = f.service.git_host_settings(project.id).await.unwrap();
    assert_eq!(rows.len(), crate::OPERATION_KINDS.len());
    let row = |kind: &str| rows.iter().find(|r| r.operation_kind == kind).unwrap();
    assert!(!row("read").approval_required && !row("read").is_write);
    assert!(row("comment").approval_required);
    assert_eq!(
        row("merge").set_by,
        crate::GithubSettingSource::OrganisationProfile
    );
    let changed = f
        .service
        .set_git_host_approval(project.id, "comment", false)
        .await
        .unwrap();
    assert!(!changed.approval_required);
    assert_eq!(changed.set_by, crate::GithubSettingSource::Engineer);
    assert_eq!(f.entries_of("git_host_approval_set").await.len(), 1);
    // `read` cannot be set to require approval.
    let read_kind = f
        .service
        .set_git_host_approval(project.id, "read", true)
        .await
        .unwrap();
    assert!(!read_kind.approval_required);
    let unknown = f
        .service
        .set_git_host_approval(project.id, "fly", true)
        .await;
    assert_eq!(unknown.unwrap_err().code(), "PROJ_NOT_FOUND");
}

#[tokio::test]
async fn a_profile_change_forces_the_ceiling_on_existing_rows() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_git_host_approval(project.id, "close", false)
        .await
        .unwrap();
    f.service.start().await.unwrap();
    let mut tighter = settings();
    tighter.always_approved_kinds.push("close".to_owned());
    f.settings.send(tighter).unwrap();
    let mut forced = false;
    for _ in 0..100 {
        let rows = f.service.git_host_settings(project.id).await.unwrap();
        let close = rows.iter().find(|r| r.operation_kind == "close").unwrap();
        if close.approval_required {
            forced = true;
            assert_eq!(
                close.set_by,
                crate::GithubSettingSource::OrganisationProfile
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    f.service.shutdown().await;
    assert!(forced);
}

#[tokio::test]
async fn crash_recovery_removes_the_worktrees_of_finished_tasks() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let started = f.start_task(&project, "left-behind").await;
    crate::store::set_task_state(&f.service.pool, started.task.id, CodingTaskState::SetAside)
        .await
        .unwrap();
    let working = f.start_task(&project, "still-working").await;
    f.service.recover_worktrees().await;
    assert!(!worktree(&started.task).exists());
    assert!(worktree(&working.task).exists());
    let repo = git2::Repository::open(&folder).unwrap();
    assert!(repo
        .find_branch("ordin/left-behind", git2::BranchType::Local)
        .is_err());
}

#[tokio::test]
async fn the_engineer_s_edit_marks_a_pending_entry_and_the_decision_clears_it() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "marks").await;
    change_two_places(&worktree(&started.task));
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let change = ordin_files::PlaceChange {
        place_id: project.place_id,
        path: "src/rates.rs".to_owned(),
        change: ordin_files::PlaceChangeKind::Modified,
        observed_at: crate::Timestamp::now(),
        by_ordin: false,
        version: None,
        origin: None,
    };
    f.service.place_changed(&change).await.unwrap();
    assert_eq!(lock(&f.service.marked_entries).len(), 2);
    f.service
        .decide_entry(set.entries[0].id, EntryDecision::Reject)
        .await
        .unwrap();
    assert_eq!(lock(&f.service.marked_entries).len(), 1);
}

#[tokio::test]
async fn the_project_turns_indexed_when_the_file_service_covers_it() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .mark_indexed_when_covered(project.id, project.place_id)
        .await
        .unwrap();
    let state = f.service.project(project.id).await.unwrap().state;
    assert_eq!(state, ProjectState::Indexed);
    let unknown = f.service.project(ProjectId::new()).await;
    assert!(unknown.is_err());
}

#[tokio::test]
async fn a_task_of_another_project_is_not_a_place_for_a_check() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let other = f.dir.0.join("other");
    repository(&other, &[("a.txt", "a\n")]);
    let place = f.place(&other).await;
    let second = f.service.open_project(place, f.workspace).await.unwrap();
    let started = f.start_task(&project, "mine").await;
    let error = f
        .service
        .run_check(
            second.id,
            Some(started.task.id),
            "true",
            crate::CommandOrigin::Person,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_TASK_NOT_FOUND");
}

#[tokio::test]
async fn an_entry_of_an_attempt_set_aside_can_no_longer_be_decided_or_hold_a_commit() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    let group = AttemptGroupId::new();
    let kept = f.start_in_group(&project, "attempt", Some(group)).await;
    let other = f.start_in_group(&project, "attempt", Some(group)).await;
    change_two_places(&worktree(&other.task));
    let set = f
        .service
        .end_coding_run(other.task.id, RunId::new())
        .await
        .unwrap();
    f.service.keep_attempt(kept.task.id).await.unwrap();
    let error = f
        .service
        .decide_entry(set.entries[0].id, EntryDecision::Reject)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_TASK_STATE_INVALID");
    write(&folder, "README.md", "changed\n");
    assert!(f.service.commit(project.id, "Free", None).await.is_ok());
    let again = f.service.keep_attempt(other.task.id).await.unwrap_err();
    assert_eq!(again.code(), "PROJ_TASK_STATE_INVALID");
}

#[test]
fn slugs_and_branches_keep_to_the_pattern() {
    use crate::tasks::{branch_valid, slug};
    assert_eq!(slug("Rates retry"), "rates-retry");
    assert_eq!(slug("A  B__C"), "a-b-c");
    assert_eq!(slug(&"x".repeat(150)).len(), 100);
    assert!(branch_valid("ordin/rates-retry"));
    assert!(!branch_valid("ordin/"));
    assert!(!branch_valid("ordin/UPPER"));
    assert!(!branch_valid("other/name"));
    assert!(!branch_valid(&format!("ordin/{}", "x".repeat(101))));
}

#[test]
fn check_commands_are_cleaned_as_the_spec_says() {
    use crate::projects::clean_check_commands;
    assert_eq!(
        clean_check_commands(vec!["  ".to_owned(), "a".to_owned(), " a".to_owned()]).unwrap(),
        ["a"]
    );
    assert!(clean_check_commands(vec!["x".repeat(1001)]).is_err());
    assert!(clean_check_commands(vec!["x".repeat(1000)]).is_ok());
    assert!(clean_check_commands((0..20).map(|n| n.to_string()).collect()).is_ok());
}

/// Gherkin "Happy path — the terminal starts in the project folder on Windows": the place is added
/// with the verbatim form `\\?\C:\...` the operating system resolves to, and `cmd.exe`'s `cd`
/// prints the plain folder, not `C:\Windows`, where it starts when handed the verbatim form.
#[cfg(windows)]
// spec: ordin-desktop/components/ProjectService#PROJ-PATH-1
#[tokio::test]
async fn happy_path_the_terminal_starts_in_the_project_folder_on_windows() {
    let f = fixture().await;
    let folder = f.dir.0.join("supplier-portal");
    repository(&folder, &[("README.md", "hello\n")]);
    let verbatim = std::fs::canonicalize(&folder).unwrap();
    assert!(
        verbatim.to_string_lossy().starts_with(r"\\?\"),
        "{verbatim:?}"
    );
    let place = f
        .files
        .add_place(ordin_files::AddPlace {
            path: verbatim,
            friendly_name: None,
            kind: ordin_files::PlaceKind::PersonsFolder,
        })
        .await
        .unwrap()
        .id;
    f.scanned(place).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    // Wide enough that the folder's line never wraps.
    let stream = f.service.terminal_open(project.id, 400, 24).await.unwrap();
    // The Windows console first asks where the cursor is (`ESC [6n`); Enter is a carriage return.
    f.service
        .terminal_input(stream, b"\x1b[1;1Rcd\r".to_vec())
        .await
        .unwrap();
    let plain = folder.to_string_lossy().into_owned();
    let mut seen = false;
    for _ in 0..500 {
        if f.streams.terminal_text(stream).contains(&plain) {
            seen = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let text = f.streams.terminal_text(stream);
    f.service.terminal_close(stream).await;
    assert!(seen, "the terminal starts in {plain}: {text}");
    assert!(!text.contains(r"\\?\"), "{text}");
    assert!(!text.contains(r"C:\Windows>"), "{text}");
}
