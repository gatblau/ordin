//! Playbook §7.2: the Push Service against the real lower-phase crates, with a local bare Git repository,
//! the SharePoint stand-in and the Confluence stand-in. Each push is journalled as exactly one external
//! action whose key is the one the Push Service minted and stored before the send.
mod support;
use ordin_push::PushNotice;
use ordin_types::domain::places::WorkingCopyState;
use support::*;

/// The `file://` address of `path`, with forward slashes: a Windows path has a drive and backslashes.
fn file_url(path: &std::path::Path) -> String {
    let text = path.display().to_string();
    // A canonical Windows path starts `\\?\`, which a URL cannot carry.
    let text = text
        .strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

async fn pushed(stack: &Stack) -> Vec<ordin_types::WorkingCopyItemId> {
    stack
        .notice(|n| match n {
            PushNotice::PushFinished { pushed, failed, .. } if failed.is_empty() => {
                Some(pushed.clone())
            }
            PushNotice::PushFinished { failed, .. } => panic!("failed: {failed:?}"),
            _ => None,
        })
        .await
}

/// The single Journal row of the push, with the key stored on the item.
async fn one_action(stack: &Stack, kind: &str) {
    let key: String = sqlx::query_scalar(
        "SELECT external_action_key FROM pending_push_items WHERE external_action_key IS NOT NULL",
    )
    .fetch_one(&stack.pool)
    .await
    .expect("the key stored on the pushed item");
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT action_key, kind, state FROM external_actions")
            .fetch_all(&stack.pool)
            .await
            .expect("actions");
    assert_eq!(rows, vec![(key, kind.to_owned(), "confirmed".to_owned())]);
}

#[tokio::test]
async fn a_push_to_sharepoint_is_journalled_as_one_external_action_with_its_key() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    stack
        .mount_sharepoint_upload("01FILEA", "A-1.docx", "v1", "v2")
        .await;
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    assert_eq!(pushed(&stack).await, vec![a]);
    one_action(&stack, "origin_push").await;
    let puts = stack
        .graph_requests()
        .await
        .into_iter()
        .filter(|(m, _)| m == "PUT")
        .count();
    assert_eq!(puts, 1);
}

#[tokio::test]
async fn a_push_to_confluence_is_journalled_as_one_external_action_with_its_key() {
    let stack = Stack::new().await;
    let item = stack.confluence_item(7).await;
    stack
        .confluence_reply(
            "PUT",
            &conf(&format!("/api/v2/pages/{PAGE}")),
            200,
            page_json(PAGE, "Rates", 8),
            false,
        )
        .await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(item, "New rates")]))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    assert_eq!(pushed(&stack).await, vec![item]);
    one_action(&stack, "origin_push").await;
    assert_eq!(stack.working.state_of(item), WorkingCopyState::Pushed);
}

#[tokio::test]
async fn a_push_to_a_local_bare_repository_is_journalled_as_one_external_action_with_its_key() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "team-remote");
    let clone = bare_clone_of(&remote, stack.dir.path(), "team-clone");
    let commit = commit_on(&clone, main_of(&clone), "facts.md", "12 new facts");
    let proposal = share_proposal(&stack, &clone, commit, None);
    let card = stack
        .service
        .propose_team_share(proposal)
        .await
        .expect("propose");
    stack.approve(card).await;
    stack
        .notice(|n| matches!(n, PushNotice::TeamShareSent { .. }).then_some(()))
        .await;
    assert_eq!(main_of(&remote), commit);
    let key: String = sqlx::query_scalar("SELECT action_key FROM team_share_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("key");
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT action_key, kind, state FROM external_actions")
            .fetch_all(&stack.pool)
            .await
            .expect("actions");
    assert_eq!(
        rows,
        vec![(key, "origin_push".to_owned(), "confirmed".to_owned())]
    );
}

#[tokio::test]
async fn a_push_to_a_local_worktree_remote_is_journalled_as_one_pull_request_action() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "agents-remote");
    let worktree = clone_of(&remote, stack.dir.path(), "worktree", true);
    std::fs::write(worktree.join("agent.md"), "# Supplier agent\n").expect("write");
    let request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::AgentDraft {
            id: ordin_types::AgentDraftId::new(),
        },
        ordin_push::PullRequestContent::Worktree {
            path: worktree,
            paths: vec!["agent.md".to_owned()],
        },
    );
    stack
        .service
        .propose_pull_request(request)
        .await
        .expect("propose");
    let card: String = sqlx::query_scalar("SELECT approval_id FROM pull_request_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .notice(|n| matches!(n, PushNotice::PullRequestOpened { .. }).then_some(()))
        .await;
    assert!(has_branch(&remote, "draft/agent-1"));
    let key: String = sqlx::query_scalar("SELECT action_key FROM pull_request_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("key");
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT action_key, kind, state FROM external_actions")
            .fetch_all(&stack.pool)
            .await
            .expect("actions");
    assert_eq!(
        rows,
        vec![(
            key,
            "pull_request_opened".to_owned(),
            "confirmed".to_owned()
        )]
    );
}

#[tokio::test]
async fn a_git_document_changed_after_approval_reaches_neither_the_clone_nor_the_bare_repository() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "docs-remote");
    let clone = clone_of(&remote, stack.dir.path(), "docs-clone", true);
    let before = (main_of(&remote), tip(&clone, "main"));
    std::fs::write(clone.join("rates.md"), "approved rates\n").expect("write");
    let entry = stack.entry(
        ordin_types::domain::places::SourceLinkKind::GitRepositoryFolder,
        "rates.md",
        clone.display().to_string(),
        Some("main"),
    );
    let id = entry.item.id;
    stack.working.add(entry, b"approved rates\n");
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(id, "Rates")]))
        .await
        .expect("propose");
    // After the card, the file is edited on disk and saved as a new version.
    std::fs::write(clone.join("rates.md"), "edited after approval\n").expect("write");
    stack.working.edit_unseen(id, b"edited after approval\n");
    stack.approve(push.approval_id.expect("card")).await;
    let failed = stack
        .notice(|n| match n {
            PushNotice::PushFinished { failed, .. } => Some(failed.clone()),
            _ => None,
        })
        .await;
    assert_eq!(failed, vec![(id, "PUSH_APPROVED_VERSION_CHANGED")]);
    assert_eq!(stack.working.state_of(id), WorkingCopyState::Changed);
    assert_eq!(
        (main_of(&remote), tip(&clone, "main")),
        before,
        "nothing committed or pushed"
    );
    assert_eq!(
        stack.scalar("SELECT count(*) FROM external_actions").await,
        0
    );
}

#[tokio::test]
async fn a_git_document_is_sent_to_a_local_bare_repository_through_the_project_service() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "rates-remote");
    let clone = clone_of(&remote, stack.dir.path(), "rates-clone", true);
    // `origin` keeps the HTTPS address a real project has; `insteadOf` carries the traffic to the bare
    // repository, and the Project Service is told that one local address stands in for the host.
    {
        let repo = git2::Repository::open(&clone).expect("open");
        let mut config = repo.config().expect("config");
        config
            .set_str("remote.origin.url", "https://github.com/acme/rates.git")
            .expect("origin");
        config
            .set_str(
                &format!("url.{}.insteadOf", file_url(&remote)),
                "https://github.com/acme/rates.git",
            )
            .expect("instead of");
    }
    stack.projects.allow_local_remote(file_url(&remote));
    let before = main_of(&remote);
    std::fs::write(clone.join("rates.md"), "approved rates\n").expect("write");
    let place = stack
        .files
        .add_place(ordin_files::AddPlace {
            path: clone.clone(),
            friendly_name: Some("Rates repository".to_owned()),
            kind: ordin_types::domain::places::PlaceKind::WorkspaceFolder,
        })
        .await
        .expect("the clone as a place");
    let mut entry = stack.entry(
        ordin_types::domain::places::SourceLinkKind::GitRepositoryFolder,
        "rates.md",
        clone.display().to_string(),
        Some("main"),
    );
    entry.place_id = place.id;
    entry.item.origin_version_read = before.to_string();
    let id = entry.item.id;
    stack.working.add(entry, b"approved rates\n");
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(id, "Rates")]))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    assert_eq!(pushed(&stack).await, vec![id]);
    assert_ne!(main_of(&remote), before, "the bare repository moved");
    assert_eq!(tip(&clone, "main"), main_of(&remote));
    assert_eq!(stack.working.state_of(id), WorkingCopyState::Pushed);
    one_action(&stack, "origin_push").await;
}
