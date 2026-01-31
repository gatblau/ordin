//! PushService.md *Error Table*: one test per row, named after its code (X1).
mod support;
use ordin_push::{
    ChooseVersionInput, CopyAction, CopyActionInput, HeldChoice, PushNotice, PushProposal,
    TryAgainInput,
};
use ordin_types::domain::places::{PublishedCopyState, WorkingCopyState};
use serde_json::json;
use support::*;

async fn finished(
    stack: &Stack,
) -> (
    Vec<ordin_types::WorkingCopyItemId>,
    Vec<(ordin_types::WorkingCopyItemId, &'static str)>,
    Vec<ordin_types::WorkingCopyItemId>,
) {
    stack
        .notice(|n| match n {
            PushNotice::PushFinished {
                pushed,
                failed,
                origin_changed,
                ..
            } => Some((pushed.clone(), failed.clone(), origin_changed.clone())),
            _ => None,
        })
        .await
}

/// One SharePoint item proposed and approved; `after` runs between the card and the decision.
async fn sent_with(
    stack: &Stack,
    put: Option<u16>,
) -> (
    ordin_types::WorkingCopyItemId,
    Vec<(ordin_types::WorkingCopyItemId, &'static str)>,
    Vec<ordin_types::WorkingCopyItemId>,
) {
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    if let Some(status) = put {
        stack.graph_put("01FILEA", status).await;
    }
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    let (_, failed, changed) = finished(stack).await;
    (a, failed, changed)
}

#[tokio::test]
async fn push_request_invalid() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    for request in [String::new(), "x".repeat(4001)] {
        let mut p = proposal(stack.workspace, &[(a, "A")]);
        p.request = request;
        assert_eq!(
            stack.service.propose(p).await.expect_err("refused").code,
            "PUSH_REQUEST_INVALID"
        );
    }
    let mut wrong: PushProposal = proposal(stack.workspace, &[(a, "A")]);
    wrong.request = "ok".to_owned();
    assert!(stack.service.propose(wrong).await.is_ok());
}

#[tokio::test]
async fn push_items_out_of_range() {
    let stack = Stack::new().await;
    assert_eq!(
        stack
            .service
            .propose(proposal(stack.workspace, &[]))
            .await
            .expect_err("none")
            .code,
        "PUSH_ITEMS_OUT_OF_RANGE"
    );
    let many: Vec<_> = (0..201)
        .map(|_| (ordin_types::WorkingCopyItemId::new(), "x"))
        .collect();
    assert_eq!(
        stack
            .service
            .propose(proposal(stack.workspace, &many))
            .await
            .expect_err("201")
            .code,
        "PUSH_ITEMS_OUT_OF_RANGE"
    );
}

#[tokio::test]
async fn push_summary_invalid() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    for summary in ["", "line one\nline two", &"s".repeat(121)] {
        let error = stack
            .service
            .propose(proposal(stack.workspace, &[(a, summary)]))
            .await
            .expect_err("refused");
        assert_eq!(error.code, "PUSH_SUMMARY_INVALID", "{summary:?}");
    }
}

#[tokio::test]
async fn push_item_already_pending() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("first");
    // The item is awaiting a push and sits in an open push.
    stack
        .working
        .inner
        .lock()
        .unwrap()
        .entries
        .get_mut(&a)
        .expect("entry")
        .item
        .state = WorkingCopyState::Changed;
    assert_eq!(
        stack
            .service
            .propose(proposal(stack.workspace, &[(a, "A")]))
            .await
            .expect_err("refused")
            .code,
        "PUSH_ITEM_ALREADY_PENDING"
    );
}

#[tokio::test]
async fn push_item_not_changed() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    stack
        .working
        .inner
        .lock()
        .unwrap()
        .entries
        .get_mut(&a)
        .expect("entry")
        .item
        .state = WorkingCopyState::Read;
    assert_eq!(
        stack
            .service
            .propose(proposal(stack.workspace, &[(a, "A")]))
            .await
            .expect_err("refused")
            .code,
        "PUSH_ITEM_NOT_CHANGED"
    );
}

#[tokio::test]
async fn push_all_origins_changed() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v9")
        .await;
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    assert_eq!(
        stack
            .service
            .propose(proposal(stack.workspace, &[(a, "A")]))
            .await
            .expect_err("refused")
            .code,
        "PUSH_ALL_ORIGINS_CHANGED"
    );
}

#[tokio::test]
async fn push_approval_failed() {
    let stack = Stack::new().await;
    let mut request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::Handover {
            id: ordin_types::HandoverId::new(),
        },
        ordin_push::PullRequestContent::Files {
            files: vec![("h.md".to_owned(), b"x".to_vec())],
        },
    );
    // The Approval Service refuses a content label that is empty.
    request.card.content = Some(ordin_types::wire::approval::ApprovalContent::Message {
        label: String::new(),
        text: "t".to_owned(),
        destination_note: None,
    });
    let error = stack
        .service
        .propose_pull_request(request)
        .await
        .expect_err("refused");
    assert_eq!(
        (error.code, error.retryable),
        ("PUSH_APPROVAL_FAILED", true)
    );
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pull_request_submissions")
            .await,
        0,
        "the row is deleted"
    );
}

#[tokio::test]
async fn push_approval_missing() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    let request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::Handover {
            id: ordin_types::HandoverId::new(),
        },
        ordin_push::PullRequestContent::Files {
            files: vec![("h.md".to_owned(), b"x".to_vec())],
        },
    );
    let error = stack
        .service
        .send_approved_pull_request(ordin_push::ApprovedPullRequest {
            approval_id: push.approval_id.expect("card"),
            proposal: request,
        })
        .await
        .expect_err("not approved");
    assert_eq!(error.code, "PUSH_APPROVAL_MISSING");
    assert!(
        stack.github.state.lock().unwrap().branch_calls.is_empty(),
        "nothing is sent"
    );
}

#[tokio::test]
async fn push_origin_changed() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    // The document moved at SharePoint after the card.
    stack.graph.reset().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v9")
        .await;
    stack.approve(push.approval_id.expect("card")).await;
    let (pushed, failed, changed) = finished(&stack).await;
    assert!(pushed.is_empty() && failed.is_empty());
    assert_eq!(changed, vec![a]);
    assert_eq!(stack.working.state_of(a), WorkingCopyState::OriginChanged);
    assert_eq!(
        stack.scalar("SELECT count(*) FROM external_actions").await,
        0,
        "nothing was recorded or sent"
    );
}

#[tokio::test]
async fn push_approved_version_changed() {
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
    // The document is saved again on this computer, unseen by the hold, before the send.
    stack.working.edit_unseen(a, b"edited after approval");
    stack.approve(push.approval_id.expect("card")).await;
    let (pushed, failed, changed) = finished(&stack).await;
    assert!(pushed.is_empty() && changed.is_empty());
    assert_eq!(failed, vec![(a, "PUSH_APPROVED_VERSION_CHANGED")]);
    assert!(
        !stack.graph_requests().await.iter().any(|(m, _)| m == "PUT"),
        "nothing reaches SharePoint"
    );
    assert_eq!(
        stack.scalar("SELECT count(*) FROM external_actions").await,
        0,
        "no external action is journalled"
    );
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pending_push_items WHERE result_code = 'PUSH_APPROVED_VERSION_CHANGED' AND pending_action_key IS NULL")
            .await,
        1
    );
    // Back in `changed`, so it can be proposed again under a new card.
    assert_eq!(stack.working.state_of(a), WorkingCopyState::Changed);
    let error = ordin_push::PushError::new("PUSH_APPROVED_VERSION_CHANGED", "");
    assert_eq!((error.retryable, error.action), (false, None));
}

#[tokio::test]
async fn push_origin_not_found() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    stack.graph.reset().await;
    stack.mount_sharepoint_origin().await;
    stack.approve(push.approval_id.expect("card")).await;
    let (_, failed, _) = finished(&stack).await;
    assert_eq!(failed, vec![(a, "PUSH_ORIGIN_NOT_FOUND")]);
}

#[tokio::test]
async fn push_outcome_unknown() {
    let stack = Stack::with(Options {
        confluence_timeout: std::time::Duration::from_millis(300),
        ..Options::default()
    })
    .await;
    let item = stack.confluence_item(7).await;
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("PUT"))
            .and(path(conf(&format!("/api/v2/pages/{PAGE}"))))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(page_json(PAGE, "Rates", 8))
                    .set_delay(std::time::Duration::from_secs(3)),
            )
            .mount(&stack.gateway)
            .await;
    }
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(item, "New rates")]))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    let (_, failed, _) = finished(&stack).await;
    assert_eq!(failed, vec![(item, "PUSH_OUTCOME_UNKNOWN")]);
}

#[tokio::test]
async fn push_destination_refused() {
    let stack = Stack::new().await;
    let (a, failed, _) = sent_with(&stack, Some(400)).await;
    assert_eq!(failed, vec![(a, "PUSH_DESTINATION_REFUSED")]);
}

#[tokio::test]
async fn push_git_branch_unknown() {
    let stack = Stack::new().await;
    let local = stack.local_item("a.md", "A");
    let git = stack.entry(
        ordin_types::domain::places::SourceLinkKind::GitRepositoryFolder,
        "b.md",
        "/x".to_owned(),
        None,
    );
    let git_id = git.item.id;
    stack.working.add(git, b"B");
    stack
        .service
        .propose(proposal(stack.workspace, &[(local, "A"), (git_id, "B")]))
        .await
        .expect("propose");
    let (_, failed, _) = finished(&stack).await;
    assert_eq!(failed, vec![(git_id, "PUSH_GIT_BRANCH_UNKNOWN")]);
}

#[tokio::test]
async fn push_sign_in_needed() {
    let stack = Stack::new().await;
    let (a, failed, _) = sent_with(&stack, Some(401)).await;
    assert_eq!(failed, vec![(a, "PUSH_SIGN_IN_NEEDED")]);
    assert_eq!(stack.working.state_of(a), WorkingCopyState::AwaitingPush);
    let error = ordin_push::PushError::new("PUSH_SIGN_IN_NEEDED", "");
    assert_eq!(
        (error.retryable, error.action),
        (true, Some(ordin_types::RepairAction::SignInAgain))
    );
}

#[tokio::test]
async fn push_forbidden() {
    let stack = Stack::new().await;
    let (a, failed, _) = sent_with(&stack, Some(403)).await;
    assert_eq!(failed, vec![(a, "PUSH_FORBIDDEN")]);
}

#[tokio::test]
async fn push_too_large() {
    let stack = Stack::new().await;
    let (a, failed, _) = sent_with(&stack, Some(413)).await;
    assert_eq!(failed, vec![(a, "PUSH_TOO_LARGE")]);
}

#[tokio::test]
async fn push_origin_unreachable() {
    let stack = Stack::new().await;
    let (a, failed, _) = sent_with(&stack, Some(503)).await;
    assert_eq!(failed, vec![(a, "PUSH_ORIGIN_UNREACHABLE")]);
    assert!(ordin_push::PushError::new("PUSH_ORIGIN_UNREACHABLE", "").retryable);
    // PS-REF-1: the destination refused it, so its external action is refused, never left for recovery.
    // spec: ordin-desktop/components/PushService#PS-REF-1
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM external_actions WHERE kind = 'origin_push' AND state = 'refused'")
            .await,
        1
    );
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM external_actions WHERE state = 'sent'")
            .await,
        0
    );
}

#[tokio::test]
async fn push_git_identity_missing() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "agents-remote");
    let worktree = clone_of(&remote, stack.dir.path(), "worktree", false);
    {
        // An empty identity in the repository's own configuration hides any global one.
        let mut config = git2::Repository::open(&worktree)
            .expect("repo")
            .config()
            .expect("config");
        config.set_str("user.name", "").expect("name");
        config.set_str("user.email", "").expect("email");
    }
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
    let code = stack
        .notice(|n| match n {
            PushNotice::PullRequestFailed { code, .. } => Some(*code),
            _ => None,
        })
        .await;
    assert_eq!(code, "PUSH_GIT_IDENTITY_MISSING");
    assert!(!has_branch(&remote, "draft/agent-1"), "nothing is pushed");
}

#[tokio::test]
async fn push_not_held() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    let error = stack
        .service
        .choose_version(ChooseVersionInput {
            pending_push_id: push.id,
            working_copy_item_id: a,
            choice: HeldChoice::KeepMine,
        })
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_NOT_HELD");
}

#[tokio::test]
async fn push_unknown() {
    let stack = Stack::new().await;
    let ghost = ordin_types::PendingPushId::new();
    let item = ordin_types::WorkingCopyItemId::new();
    assert_eq!(
        stack
            .service
            .choose_version(ChooseVersionInput {
                pending_push_id: ghost,
                working_copy_item_id: item,
                choice: HeldChoice::KeepMine
            })
            .await
            .expect_err("unknown")
            .code,
        "PUSH_UNKNOWN"
    );
    assert_eq!(
        stack
            .service
            .try_again(TryAgainInput {
                pending_push_id: ghost,
                working_copy_item_id: item
            })
            .await
            .expect_err("unknown")
            .code,
        "PUSH_UNKNOWN"
    );
    let copy = ordin_types::PublishedCopyId::new();
    assert_eq!(
        stack
            .service
            .copy_action(CopyActionInput {
                published_copy_id: copy,
                action: CopyAction::Remove
            })
            .await
            .expect_err("unknown")
            .code,
        "PUSH_UNKNOWN"
    );
}

#[tokio::test]
async fn push_format_not_ready() {
    let stack = Stack::new().await;
    let mut request = stack.copy_request(false);
    request.format = ordin_types::domain::places::CopyFormat::Pdf;
    request.destination = ordin_types::domain::places::CopyDestination::SharepointFile {
        site: "s".to_owned(),
        library: DRIVE.to_owned(),
        folder: FOLDER.to_owned(),
    };
    assert_eq!(
        stack
            .service
            .publish_copy(request)
            .await
            .expect_err("refused")
            .code,
        "PUSH_FORMAT_NOT_READY"
    );
}

#[tokio::test]
async fn push_name_exists() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack
        .confluence_reply(
            "POST",
            &conf("/api/v2/pages"),
            400,
            json!({"errors": [{"title": "already exists"}]}),
            false,
        )
        .await;
    stack.write_original("rates v1\n").await;
    let copy = stack
        .service
        .publish_copy(stack.copy_request(false))
        .await
        .expect("publish");
    let card: String = sqlx::query_scalar("SELECT approval_id FROM copy_operations")
        .fetch_one(&stack.pool)
        .await
        .expect("card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .until("failed", || async {
            stack
                .files
                .published_copy(copy)
                .await
                .map(|c| c.state == PublishedCopyState::Failed)
                .unwrap_or(false)
        })
        .await;
    assert_eq!(
        stack
            .files
            .published_copy(copy)
            .await
            .expect("copy")
            .failure_reason
            .as_deref(),
        Some("Something with that name is already there.")
    );
}

#[tokio::test]
async fn push_original_unreadable() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack.write_original("rates v1\n").await;
    let mut request = stack.copy_request(false);
    request.original_version = "9".to_owned(); // no such version
    let copy = stack.service.publish_copy(request).await.expect("publish");
    let card: String = sqlx::query_scalar("SELECT approval_id FROM copy_operations")
        .fetch_one(&stack.pool)
        .await
        .expect("card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .until("failed", || async {
            stack
                .files
                .published_copy(copy)
                .await
                .map(|c| c.state == PublishedCopyState::Failed)
                .unwrap_or(false)
        })
        .await;
    assert_eq!(
        stack
            .files
            .published_copy(copy)
            .await
            .expect("copy")
            .failure_reason
            .as_deref(),
        Some("ordin couldn't read the original at that version.")
    );
    assert!(
        stack
            .confluence_requests()
            .await
            .iter()
            .all(|(m, _)| m != "POST"),
        "nothing is sent"
    );
}

#[tokio::test]
async fn push_copy_cannot_bring_back() {
    let stack = Stack::new().await;
    let id = ordin_types::PublishedCopyId::new();
    stack
        .files
        .record_published_copy(ordin_types::domain::places::PublishedCopy {
            id,
            original: ordin_types::domain::places::CopyOriginal::PlaceFile {
                place_id: stack.place.id,
                path: "rates.md".to_owned(),
            },
            destination: ordin_types::domain::places::CopyDestination::SharepointFile {
                site: "s".to_owned(),
                library: DRIVE.to_owned(),
                folder: FOLDER.to_owned(),
            },
            destination_item_id: Some("01FILEA".to_owned()),
            format: ordin_types::domain::places::CopyFormat::Pdf,
            maker: ordin_types::domain::places::CopyMaker::PdfCraft,
            published_via: ordin_types::domain::places::PublishedVia::PublishACopy,
            followed_branch: None,
            keep_up_to_date: true,
            audience: "Procurement team".to_owned(),
            original_version_published: Some("1".to_owned()),
            destination_version_pushed: Some("v1".to_owned()),
            last_updated_at: Some(ordin_types::time::Timestamp::now()),
            state: PublishedCopyState::ChangedAtDestination,
            failure_reason: None,
        })
        .await
        .expect("record");
    let error = stack
        .service
        .copy_action(CopyActionInput {
            published_copy_id: id,
            action: CopyAction::BringItIn,
        })
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_COPY_CANNOT_BRING_BACK");
}

#[tokio::test]
async fn push_copy_state() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(false).await;
    let error = stack
        .service
        .copy_action(CopyActionInput {
            published_copy_id: copy,
            action: CopyAction::LeaveIt,
        })
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_COPY_STATE");
}

#[tokio::test]
async fn push_branch_moved() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "agents-remote");
    let worktree = clone_of(&remote, stack.dir.path(), "worktree", true);
    // A colleague pushed to the review's branch after the draft was started.
    {
        let other = clone_of(&remote, stack.dir.path(), "other", true);
        std::fs::write(other.join("theirs.md"), "theirs\n").expect("write");
        let repo = git2::Repository::open(&other).expect("repo");
        let mut index = repo.index().expect("index");
        index
            .add_path(std::path::Path::new("theirs.md"))
            .expect("add");
        index.write().expect("write");
        let tree = repo
            .find_tree(index.write_tree().expect("tree"))
            .expect("find");
        let parent = repo.head().expect("head").peel_to_commit().expect("commit");
        let sig = signature();
        repo.commit(Some("HEAD"), &sig, &sig, "theirs", &tree, &[&parent])
            .expect("commit");
        repo.find_remote("origin")
            .expect("remote")
            .push(&["refs/heads/main:refs/heads/draft/agent-1"], None)
            .expect("push");
    }
    std::fs::write(worktree.join("agent.md"), "# Supplier agent\n").expect("write");
    let mut request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::AgentDraft {
            id: ordin_types::AgentDraftId::new(),
        },
        ordin_push::PullRequestContent::Worktree {
            path: worktree,
            paths: vec!["agent.md".to_owned()],
        },
    );
    request.existing = Some(pull_request(40));
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
    let code = stack
        .notice(|n| match n {
            PushNotice::PullRequestFailed { code, .. } => Some(*code),
            _ => None,
        })
        .await;
    assert_eq!(code, "PUSH_BRANCH_MOVED");
}

#[tokio::test]
async fn push_confluence_unavailable() {
    let stack = Stack::with(Options {
        with_confluence: false,
        ..Options::default()
    })
    .await;
    let mut request = stack.copy_request(false);
    request.destination = ordin_types::domain::places::CopyDestination::ConfluencePage {
        space: "PROC".to_owned(),
        parent_page_id: PARENT.to_owned(),
    };
    stack.write_original("rates v1\n").await;
    assert_eq!(
        stack
            .service
            .publish_copy(request)
            .await
            .expect_err("refused")
            .code,
        "PUSH_CONFLUENCE_UNAVAILABLE"
    );
}

#[tokio::test]
async fn push_store_failed() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    stack.working.inner.lock().unwrap().fail_entry = true;
    let error = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect_err("refused");
    assert_eq!((error.code, error.retryable), ("PUSH_STORE_FAILED", true));
}

#[tokio::test]
async fn push_team_share_unknown() {
    let stack = Stack::new().await;
    let ghost = ordin_types::TeamShareId::new();
    assert_eq!(
        stack
            .service
            .withdraw_team_share(ghost)
            .await
            .expect_err("unknown")
            .code,
        "PUSH_TEAM_SHARE_UNKNOWN"
    );
    assert_eq!(
        stack
            .service
            .resend_team_share(ghost, "a".repeat(40))
            .await
            .expect_err("unknown")
            .code,
        "PUSH_TEAM_SHARE_UNKNOWN"
    );
}

#[tokio::test]
async fn push_team_share_state() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "team-remote");
    let clone = bare_clone_of(&remote, stack.dir.path(), "team-clone");
    let commit = commit_on(&clone, main_of(&clone), "facts.md", "12 new facts");
    let proposal = share_proposal(&stack, &clone, commit, None);
    stack
        .service
        .propose_team_share(proposal.clone())
        .await
        .expect("first");
    // Proposed twice.
    assert_eq!(
        stack
            .service
            .propose_team_share(proposal)
            .await
            .expect_err("twice")
            .code,
        "PUSH_TEAM_SHARE_STATE"
    );
}

#[tokio::test]
async fn push_team_commit_missing() {
    let stack = Stack::new().await;
    let remote = bare_with_main(stack.dir.path(), "team-remote");
    let clone = bare_clone_of(&remote, stack.dir.path(), "team-clone");
    let mut proposal = share_proposal(&stack, &clone, main_of(&clone), None);
    proposal.commit = "f".repeat(40);
    assert_eq!(
        stack
            .service
            .propose_team_share(proposal)
            .await
            .expect_err("missing")
            .code,
        "PUSH_TEAM_COMMIT_MISSING"
    );
}

// PUSH_AUDIENCE_GREW, PUSH_NO_REVIEW_RIGHT, PUSH_TEAM_*: the `acceptance` scenarios of the same names
// ("Audience grew", "No permission to open a review") carry them. PUSH_COPY_MAKER_FAILED is tested end to end
// in `conversion.rs` with a fake conversion app (FU-105), and at the message level in `registry.rs`.
