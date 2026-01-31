//! PushService.md *Acceptance Criteria (Gherkin)*: one test per scenario, named after it (X2).
mod support;
use ordin_push::PushNotice;
use ordin_types::domain::places::{PendingPushState, WorkingCopyState};
use serde_json::json;
use support::*;

// Scenario: Happy path — two files approved and sent
#[tokio::test]
async fn happy_path_two_files_approved_and_sent() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "Kestrel-MSA.docx", "v1")
        .await;
    stack
        .mount_sharepoint_upload("01FILEA", "Kestrel-MSA.docx", "v1", "v2")
        .await;
    stack
        .mount_sharepoint_file("01FILEB", "Rates.xlsx", "v1")
        .await;
    stack
        .mount_sharepoint_upload("01FILEB", "Rates.xlsx", "v1", "v2")
        .await;
    let (a, _) = stack.sharepoint_item("Kestrel-MSA.docx").await;
    let (b, _) = stack.sharepoint_item("Rates.xlsx").await;
    let push = stack
        .service
        .propose(proposal(
            stack.workspace,
            &[(a, "New terms"), (b, "New rates")],
        ))
        .await
        .expect("propose");
    assert_eq!(push.state, PendingPushState::Proposed);
    eprintln!("{:?}", stack.graph_requests().await);
    assert_eq!(stack.working.state_of(a), WorkingCopyState::AwaitingPush);
    stack.approve(push.approval_id.expect("card")).await;
    let (pushed, failed) = stack
        .notice(|n| match n {
            PushNotice::PushFinished { pushed, failed, .. } => {
                Some((pushed.clone(), failed.clone()))
            }
            _ => None,
        })
        .await;
    assert!(
        failed.is_empty(),
        "{failed:?} {:?}",
        stack.graph_requests().await
    );
    assert_eq!(pushed.len(), 2);
    eprintln!("{:?}", stack.working.inner.lock().unwrap().states);
    assert_eq!(stack.working.state_of(a), WorkingCopyState::Pushed);
    assert_eq!(stack.working.origin_version_of(a), "v2");
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM external_actions WHERE kind = 'origin_push'")
            .await,
        2
    );
}

// ---- helpers ----

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

// Scenario: Origin changed before the card
#[tokio::test]
async fn origin_changed_before_the_card() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "Kestrel-MSA.docx", "v9")
        .await;
    stack
        .mount_sharepoint_file("01FILEB", "Rates.xlsx", "v1")
        .await;
    let (a, _) = stack.sharepoint_item("Kestrel-MSA.docx").await;
    let (b, _) = stack.sharepoint_item("Rates.xlsx").await;
    let push = stack
        .service
        .propose(proposal(
            stack.workspace,
            &[(a, "New terms"), (b, "New rates")],
        ))
        .await
        .expect("propose");
    assert_eq!(stack.working.state_of(a), WorkingCopyState::OriginChanged);
    assert_eq!(push.items.len(), 1);
    assert_eq!(push.items[0].working_copy_item_id, b);
    let card = stack
        .approvals
        .get(push.approval_id.expect("card"))
        .await
        .expect("the card")
        .card;
    assert_eq!(card.affects.len(), 1);
    // PS-CARD-1: the file is named for itself, and where names the service.
    // spec: ordin-desktop/components/PushService#PS-CARD-1
    match &card.affects[0] {
        ordin_types::wire::approval::AffectedItem::File { label, .. } => {
            assert_eq!(label, "Rates.xlsx")
        }
        other => panic!("{other:?}"),
    }
    assert!(
        card.where_line.starts_with("SharePoint \u{2014} "),
        "{}",
        card.where_line
    );
}

// Scenario: Every origin changed
#[tokio::test]
async fn every_origin_changed() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "Kestrel-MSA.docx", "v9")
        .await;
    let (a, _) = stack.sharepoint_item("Kestrel-MSA.docx").await;
    let error = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "New terms")]))
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_ALL_ORIGINS_CHANGED");
    assert_eq!(stack.working.state_of(a), WorkingCopyState::OriginChanged);
}

// Scenario: Person rejects
#[tokio::test]
async fn person_rejects() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    let b = stack.local_item("b.md", "B");
    let push = stack
        .service
        .propose(proposal(
            stack.workspace,
            &[(a, "A changed"), (b, "B changed")],
        ))
        .await
        .expect("propose");
    stack.reject(push.approval_id.expect("card")).await;
    let (pushed, _, _) = finished(&stack).await;
    assert!(pushed.is_empty());
    assert_eq!(stack.working.state_of(a), WorkingCopyState::Changed);
    assert_eq!(stack.working.state_of(b), WorkingCopyState::Changed);
    assert!(stack
        .service
        .list_pending(stack.workspace)
        .await
        .expect("list")
        .is_empty());
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pending_pushes WHERE state = 'closed'")
            .await,
        1
    );
}

// Scenario: Local edit holds the push (ADR-035)
#[tokio::test]
async fn local_edit_holds_the_push_adr_035() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    let b = stack.local_item("b.md", "B");
    let push = stack
        .service
        .propose(proposal(
            stack.workspace,
            &[(a, "A changed"), (b, "B changed")],
        ))
        .await
        .expect("propose");
    let first_card = push.approval_id.expect("card");
    stack.working.local_edit(a);
    stack
        .notice(|n| matches!(n, PushNotice::PushHeld { .. }).then_some(()))
        .await;
    assert_eq!(
        stack.approvals.get(first_card).await.expect("card").state,
        ordin_types::domain::runs::ApprovalState::Expired
    );
    let held = stack
        .service
        .list_pending(stack.workspace)
        .await
        .expect("list");
    assert_eq!(held[0].state, PendingPushState::Held);
    let again = stack
        .service
        .choose_version(ordin_push::ChooseVersionInput {
            pending_push_id: push.id,
            working_copy_item_id: a,
            choice: ordin_push::HeldChoice::KeepMine,
        })
        .await
        .expect("choose");
    assert_eq!(again.state, PendingPushState::Proposed);
    assert_eq!(again.items.len(), 1);
    assert_ne!(again.approval_id, Some(first_card));
    assert_eq!(stack.working.state_of(a), WorkingCopyState::Changed);
}

// Scenario: A held push is announced as needing the person
#[tokio::test]
async fn a_held_push_is_announced_as_needing_the_person() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A changed")]))
        .await
        .expect("propose");
    stack.working.local_edit(a);
    stack
        .notice(|n| matches!(n, PushNotice::PushHeld { .. }).then_some(()))
        .await;
    stack
        .until("needs_you_changed", || async {
            !stack.events_of("needs_you_changed").await.is_empty()
        })
        .await;
    let events = stack.events_of("needs_you_changed").await;
    assert!(events
        .iter()
        .any(|e| e.data["workspace_id"] == stack.workspace.to_string()));
}

// Scenario: Sign-in expired at one file
#[tokio::test]
async fn sign_in_expired_at_one_file() {
    let stack = Stack::new().await;
    for (id, name) in [
        ("01FILEA", "A-1.docx"),
        ("01FILEB", "B-2.docx"),
        ("01FILEC", "C-3.docx"),
    ] {
        stack.mount_sharepoint_file(id, name, "v1").await;
        if id != "01FILEB" {
            stack.mount_sharepoint_upload(id, name, "v1", "v2").await;
        }
    }
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let (b, _) = stack.sharepoint_item("B-2.docx").await;
    let (c, _) = stack.sharepoint_item("C-3.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A"), (b, "B"), (c, "C")]))
        .await
        .expect("propose");
    // The second file's upload is refused after the card: the sign-in expired.
    stack.graph_put("01FILEB", 401).await;
    stack.approve(push.approval_id.expect("card")).await;
    let (pushed, failed, _) = finished(&stack).await;
    assert_eq!(pushed.len(), 2);
    assert_eq!(failed, vec![(b, "PUSH_SIGN_IN_NEEDED")]);
    assert_eq!(stack.working.state_of(b), WorkingCopyState::AwaitingPush);
}

// Scenario: One external action per SharePoint send (single bracket)
#[tokio::test]
async fn one_external_action_per_sharepoint_send_single_bracket() {
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
    finished(&stack).await;
    let key: String = sqlx::query_scalar("SELECT pending_action_key FROM pending_push_items")
        .fetch_one(&stack.pool)
        .await
        .expect("the minted key");
    // One row in the Journal, with the key this service minted and the kind the adapter was given.
    assert_eq!(
        stack.scalar("SELECT count(*) FROM external_actions").await,
        1
    );
    let row: (String, String, String) =
        sqlx::query_as("SELECT action_key, kind, state FROM external_actions")
            .fetch_one(&stack.pool)
            .await
            .expect("the action");
    assert_eq!(row.0, key);
    assert_eq!(row.1, "origin_push");
    assert_eq!(row.2, "confirmed");
}

// Scenario: Confluence item on the ordin server
#[tokio::test]
async fn confluence_item_on_the_ordin_server() {
    let stack = Stack::with(Options {
        with_confluence: false,
        ..Options::default()
    })
    .await;
    let local = stack.local_item("a.md", "A");
    let confluence_entry = stack.entry(
        ordin_types::domain::places::SourceLinkKind::ConfluenceLocation,
        "Supplier pages/Rates.confluence.xhtml",
        "confluence:page/4000".to_owned(),
        None,
    );
    let confluence = confluence_entry.item.id;
    stack.working.add(confluence_entry, b"<p>x</p>");
    let push = stack
        .service
        .propose(proposal(
            stack.workspace,
            &[(local, "A"), (confluence, "Rates")],
        ))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    let (pushed, failed, _) = finished(&stack).await;
    assert_eq!(pushed, vec![local]);
    assert_eq!(failed, vec![(confluence, "PUSH_CONFLUENCE_UNAVAILABLE")]);
}

// ---- team knowledge shares (ADR-103) ----

struct Team {
    remote: std::path::PathBuf,
    clone: std::path::PathBuf,
    commit: git2::Oid,
}

fn team(stack: &Stack) -> Team {
    let remote = bare_with_main(stack.dir.path(), "team-remote");
    let clone = bare_clone_of(&remote, stack.dir.path(), "team-clone");
    let commit = commit_on(&clone, main_of(&clone), "facts.md", "12 new facts");
    Team {
        remote,
        clone,
        commit,
    }
}

async fn share_notice(stack: &Stack) -> PushNotice {
    stack
        .notice(|n| match n {
            PushNotice::TeamShareSent { .. }
            | PushNotice::TeamShareFailed { .. }
            | PushNotice::TeamShareMoved { .. }
            | PushNotice::TeamShareRejected { .. } => Some(n.clone()),
            _ => None,
        })
        .await
}

// Scenario: Happy path — a direct push after the card
#[tokio::test]
async fn happy_path_a_direct_push_after_the_card() {
    let stack = Stack::new().await;
    let t = team(&stack);
    let proposal = share_proposal(&stack, &t.clone, t.commit, None);
    let share = proposal.share_id;
    let card = stack
        .service
        .propose_team_share(proposal)
        .await
        .expect("propose");
    stack.approve(card).await;
    match share_notice(&stack).await {
        PushNotice::TeamShareSent {
            share_id,
            commit,
            pull_request,
        } => {
            assert_eq!(share_id, share);
            assert_eq!(commit, t.commit.to_string());
            assert!(pull_request.is_none());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(main_of(&t.remote), t.commit, "pushed without force");
    let row: (String, String) = sqlx::query_as("SELECT kind, state FROM external_actions")
        .fetch_one(&stack.pool)
        .await
        .expect("action");
    assert_eq!(row, ("origin_push".to_owned(), "confirmed".to_owned()));
}

// Scenario: Review — the branch requires review
#[tokio::test]
async fn review_the_branch_requires_review() {
    let stack = Stack::new().await;
    let t = team(&stack);
    let proposal = share_proposal(
        &stack,
        &t.clone,
        t.commit,
        Some("ordin/team-knowledge/1a2b3c4d"),
    );
    let card = stack
        .service
        .propose_team_share(proposal)
        .await
        .expect("propose");
    stack.approve(card).await;
    match share_notice(&stack).await {
        PushNotice::TeamShareSent { pull_request, .. } => {
            assert_eq!(pull_request.expect("a pull request").number, 41)
        }
        other => panic!("{other:?}"),
    }
    assert!(has_branch(&t.remote, "ordin/team-knowledge/1a2b3c4d"));
    assert_ne!(
        main_of(&t.remote),
        t.commit,
        "the linked branch is untouched"
    );
    let opened: Vec<(String, String)> = {
        let state = stack.github.state.lock().unwrap();
        state
            .opened
            .iter()
            .map(|o| (o.head.clone(), o.base.clone()))
            .collect()
    };
    assert_eq!(
        opened,
        vec![(
            "ordin/team-knowledge/1a2b3c4d".to_owned(),
            "main".to_owned()
        )]
    );
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM external_actions WHERE kind = 'pull_request_opened'")
            .await,
        1
    );
}

// Scenario: Edge — the branch moved after the card, then resent
#[tokio::test]
async fn edge_the_branch_moved_after_the_card_then_resent() {
    let stack = Stack::new().await;
    let t = team(&stack);
    let proposal = share_proposal(&stack, &t.clone, t.commit, None);
    let share = proposal.share_id;
    let card = stack
        .service
        .propose_team_share(proposal)
        .await
        .expect("propose");
    let colleague = colleague_pushes(&t.remote, stack.dir.path());
    stack.approve(card).await;
    assert!(matches!(
        share_notice(&stack).await,
        PushNotice::TeamShareMoved { .. }
    ));
    assert_eq!(
        stack
            .scalar("SELECT moves FROM team_share_submissions")
            .await,
        1
    );
    let refused: String = sqlx::query_scalar("SELECT refused_keys FROM team_share_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("keys");
    assert!(
        refused.contains('-'),
        "the refused attempt's key is kept: {refused}"
    );
    // The Team Knowledge Service rebuilds on the new tip: the colleague's commit reaches the clone first.
    {
        let repo = git2::Repository::open_bare(&t.clone).expect("clone");
        repo.find_remote("origin")
            .expect("remote")
            .fetch(&["+refs/heads/main:refs/heads/main"], None, None)
            .expect("fetch");
    }
    let second = commit_on(&t.clone, colleague, "facts.md", "12 new facts, rebuilt");
    let cards_before = stack.scalar("SELECT count(*) FROM approvals").await;
    stack
        .service
        .resend_team_share(share, second.to_string())
        .await
        .expect("resend");
    match share_notice(&stack).await {
        PushNotice::TeamShareSent { commit, .. } => assert_eq!(commit, second.to_string()),
        other => panic!("{other:?}"),
    }
    assert_eq!(main_of(&t.remote), second);
    assert_eq!(
        stack.scalar("SELECT count(*) FROM approvals").await,
        cards_before,
        "no new card"
    );
}

// Scenario: Edge — a share withdrawn before its decision
#[tokio::test]
async fn edge_a_share_withdrawn_before_its_decision() {
    let stack = Stack::new().await;
    let t = team(&stack);
    let proposal = share_proposal(&stack, &t.clone, t.commit, None);
    let share = proposal.share_id;
    let card = stack
        .service
        .propose_team_share(proposal)
        .await
        .expect("propose");
    let mut notices = stack.service.subscribe();
    stack
        .service
        .withdraw_team_share(share)
        .await
        .expect("withdraw");
    assert_eq!(
        stack.approvals.get(card).await.expect("card").state,
        ordin_types::domain::runs::ApprovalState::Expired
    );
    let state: String = sqlx::query_scalar("SELECT state FROM team_share_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("state");
    assert_eq!(state, "withdrawn");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    while let Ok(notice) = notices.try_recv() {
        assert!(
            !matches!(notice, PushNotice::TeamShareRejected { .. }),
            "no rejection notice"
        );
    }
}

// Scenario: Error — resend from a state that does not allow it
#[tokio::test]
async fn error_resend_from_a_state_that_does_not_allow_it() {
    let stack = Stack::new().await;
    let t = team(&stack);
    let proposal = share_proposal(&stack, &t.clone, t.commit, None);
    let share = proposal.share_id;
    let card = stack
        .service
        .propose_team_share(proposal)
        .await
        .expect("propose");
    stack.approve(card).await;
    assert!(matches!(
        share_notice(&stack).await,
        PushNotice::TeamShareSent { .. }
    ));
    let again = commit_on(&t.clone, t.commit, "facts.md", "more");
    let error = stack
        .service
        .resend_team_share(share, again.to_string())
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_TEAM_SHARE_STATE");
}

// Scenario: Error — the commit is not in the clone
#[tokio::test]
async fn error_the_commit_is_not_in_the_clone() {
    let stack = Stack::new().await;
    let t = team(&stack);
    let mut proposal = share_proposal(&stack, &t.clone, t.commit, None);
    proposal.commit = "0123456789abcdef0123456789abcdef01234567".to_owned();
    let error = stack
        .service
        .propose_team_share(proposal)
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_TEAM_COMMIT_MISSING");
    assert_eq!(
        stack.scalar("SELECT count(*) FROM approvals").await,
        0,
        "no card is raised"
    );
}

// Scenario: Recovery — a push whose answer was lost
#[tokio::test]
async fn recovery_a_push_whose_answer_was_lost() {
    for arrived in [true, false] {
        let stack = Stack::new().await;
        // The first run is over before the crash state is written: its own recovery would
        // otherwise settle the share when its task is scheduled late, and announce it on its own
        // channel, leaving the restarted service nothing to recover.
        stack.stop().await;
        let t = team(&stack);
        let proposal = share_proposal(&stack, &t.clone, t.commit, None);
        let share = proposal.share_id;
        stack
            .service
            .propose_team_share(proposal)
            .await
            .expect("propose");
        // The share was `sending` with key K begun when ordin stopped before the answer.
        let key = ordin_types::ExternalActionKey::new();
        stack
            .journal
            .begin_external_action(ordin_journal::NewExternalAction {
                action_key: key,
                run_id: None,
                kind: ordin_types::domain::runs::ExternalActionKind::OriginPush,
                destination: "github.com/acme/supplier-team#main".to_owned(),
            })
            .await
            .expect("begin");
        sqlx::query("UPDATE team_share_submissions SET state = 'sending', action_key = ?1, approval_id = approval_id WHERE share_id = ?2")
            .bind(key.to_string())
            .bind(share.to_string())
            .execute(&stack.pool)
            .await
            .expect("sending");
        if arrived {
            let repo = git2::Repository::open_bare(&t.clone).expect("clone");
            repo.find_remote("origin")
                .expect("remote")
                .push(&[format!("{}:refs/heads/main", t.commit)], None)
                .expect("push");
        }
        let restarted = stack.restart();
        let mut notices = restarted.subscribe();
        let token = CancellationTokenGuard::new();
        let runner = restarted.clone();
        let stop = token.0.clone();
        let mut running = tokio::spawn(async move { runner.run(stop).await });
        let notice = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                tokio::select! {
                    received = notices.recv() => match received {
                        Ok(
                            n @ (PushNotice::TeamShareSent { .. }
                            | PushNotice::TeamShareFailed { .. }),
                        ) => return n,
                        Ok(_) => {}
                        Err(e) => panic!("{e}"),
                    },
                    // A run that ends before the notice (a failed recovery) fails here, not at the timeout.
                    ended = &mut running => panic!("the restarted service stopped: {ended:?}"),
                }
            }
        })
        .await
        .expect("a notice");
        if arrived {
            assert!(matches!(notice, PushNotice::TeamShareSent { .. }));
            let state: String =
                sqlx::query_scalar("SELECT state FROM external_actions WHERE action_key = ?1")
                    .bind(key.to_string())
                    .fetch_one(&stack.pool)
                    .await
                    .expect("row");
            assert_ne!(state, "confirmed", "the row stays for the person");
        } else {
            assert!(matches!(
                notice,
                PushNotice::TeamShareFailed {
                    code: "PUSH_OUTCOME_UNKNOWN",
                    ..
                }
            ));
            assert_ne!(main_of(&t.remote), t.commit, "nothing is pushed again");
        }
    }
}

struct CancellationTokenGuard(tokio_util::sync::CancellationToken);

impl CancellationTokenGuard {
    fn new() -> Self {
        CancellationTokenGuard(tokio_util::sync::CancellationToken::new())
    }
}

impl Drop for CancellationTokenGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

// ---- pull requests ----

async fn pr_notice(stack: &Stack) -> PushNotice {
    stack
        .notice(|n| match n {
            PushNotice::PullRequestOpened { .. }
            | PushNotice::PullRequestFailed { .. }
            | PushNotice::PullRequestRejected { .. } => Some(n.clone()),
            _ => None,
        })
        .await
}

// Scenario: Pull request for an agent draft
#[tokio::test]
async fn pull_request_for_an_agent_draft() {
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
            path: worktree.clone(),
            paths: vec!["agent.md".to_owned()],
        },
    );
    let submission = stack
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
    match pr_notice(&stack).await {
        PushNotice::PullRequestOpened {
            submission: s,
            pull_request,
            ..
        } => {
            assert_eq!(s, submission);
            assert_eq!(pull_request.number, 41);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        has_branch(&remote, "draft/agent-1"),
        "the branch is committed and pushed"
    );
    assert_eq!(stack.github.state.lock().unwrap().opened.len(), 1);
    assert_eq!(stack.scalar("SELECT count(*) FROM external_actions WHERE kind = 'pull_request_opened' AND state = 'confirmed'").await, 1);
}

// Scenario: A workflow is shared to a repository folder
#[tokio::test]
async fn a_workflow_is_shared_to_a_repository_folder() {
    let stack = Stack::new().await;
    let request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::WorkflowShare {
            workspace_id: stack.workspace,
            file_name: "weekly-report.workflow.yaml".to_owned(),
        },
        ordin_push::PullRequestContent::Files {
            files: vec![(
                "weekly-report.workflow.yaml".to_owned(),
                b"name: weekly\n".to_vec(),
            )],
        },
    );
    stack
        .service
        .propose_pull_request(request)
        .await
        .expect("propose");
    assert_eq!(
        stack.scalar("SELECT count(*) FROM approvals").await,
        1,
        "one approval card"
    );
    let card: String = sqlx::query_scalar("SELECT approval_id FROM pull_request_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    assert!(matches!(
        pr_notice(&stack).await,
        PushNotice::PullRequestOpened { .. }
    ));
    let state = stack.github.state.lock().unwrap();
    assert_eq!(
        state.branch_calls,
        vec![(
            "draft/agent-1".to_owned(),
            "main".to_owned(),
            vec!["weekly-report.workflow.yaml".to_owned()]
        )]
    );
    assert_eq!(state.opened.len(), 1);
}

// Scenario: No permission to open a review
#[tokio::test]
async fn no_permission_to_open_a_review() {
    let stack = Stack::new().await;
    stack.github.state.lock().unwrap().branch_files =
        Some(Err(ordin_projects::GithubError::NotFound));
    let request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::Handover {
            id: ordin_types::HandoverId::new(),
        },
        ordin_push::PullRequestContent::Files {
            files: vec![("handover.md".to_owned(), b"x".to_vec())],
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
    match pr_notice(&stack).await {
        PushNotice::PullRequestFailed { code, .. } => assert_eq!(code, "PUSH_NO_REVIEW_RIGHT"),
        other => panic!("{other:?}"),
    }
}

// ---- Confluence ----

// Scenario: Confluence update passes the version read (R19)
#[tokio::test]
async fn confluence_update_passes_the_version_read_r19() {
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
    let (pushed, failed, _) = finished(&stack).await;
    assert!(
        failed.is_empty(),
        "{failed:?} {:?}",
        stack.confluence_requests().await
    );
    assert_eq!(pushed, vec![item]);
    let bodies = stack
        .confluence_bodies("PUT", &conf(&format!("/api/v2/pages/{PAGE}")))
        .await;
    assert_eq!(bodies.len(), 1);
    // The service passed "7" unchanged; the adapter, the one place that adds 1, sent 8.
    assert_eq!(bodies[0]["version"]["number"], 8);
    assert_eq!(stack.working.origin_version_of(item), "8");
}

// Scenario: Outcome unknown is not resent
#[tokio::test]
async fn outcome_unknown_is_not_resent() {
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
    let (pushed, failed, _) = finished(&stack).await;
    assert!(pushed.is_empty());
    assert_eq!(failed, vec![(item, "PUSH_OUTCOME_UNKNOWN")]);
    assert_eq!(stack.working.state_of(item), WorkingCopyState::AwaitingPush);
    // The adapter's own single retry is its business; the service sends nothing more by itself.
    let count = || async {
        stack
            .confluence_requests()
            .await
            .into_iter()
            .filter(|(m, _)| m == "PUT")
            .count()
    };
    let puts = count().await;
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    assert_eq!(count().await, puts, "no second update is sent");
    assert_eq!(
        stack.scalar("SELECT count(*) FROM external_actions").await,
        1
    );
    let code: String = sqlx::query_scalar("SELECT result_code FROM pending_push_items")
        .fetch_one(&stack.pool)
        .await
        .expect("code");
    assert_eq!(code, "PUSH_OUTCOME_UNKNOWN");
}

// ---- recovery of documents ----

/// An approved push of one SharePoint item whose send was begun (key K recorded in the Journal and on the
/// item) when ordin stopped: the stand-in's file is at `now` while the item read `v1`.
async fn interrupted_send(
    stack: &Stack,
    now: &str,
) -> (
    ordin_types::WorkingCopyItemId,
    ordin_types::ExternalActionKey,
) {
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    let key = ordin_types::ExternalActionKey::new();
    stack
        .journal
        .begin_external_action(ordin_journal::NewExternalAction {
            action_key: key,
            run_id: None,
            kind: ordin_types::domain::runs::ExternalActionKind::OriginPush,
            destination: "Contracts".to_owned(),
        })
        .await
        .expect("begin");
    sqlx::query("UPDATE pending_pushes SET state = 'approved' WHERE id = ?1")
        .bind(push.id.to_string())
        .execute(&stack.pool)
        .await
        .expect("approved");
    sqlx::query("UPDATE pending_push_items SET pending_action_key = ?1")
        .bind(key.to_string())
        .execute(&stack.pool)
        .await
        .expect("key");
    if now != "v1" {
        // The stand-in now answers with the new version (the first mounts answer first, so a fresh stand-in).
        stack.graph.reset().await;
        stack
            .mount_sharepoint_file("01FILEA", "A-1.docx", now)
            .await;
        stack
            .graph_get(
                &graph(&format!("/drives/{DRIVE}/items/01FILEA")),
                200,
                file_json("01FILEA", "A-1.docx", now),
            )
            .await;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("GET"))
            .and(path(graph(&format!(
                "/drives/{DRIVE}/items/01FILEA/content"
            ))))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(b"new text of A-1.docx".to_vec()),
            )
            .mount(&stack.graph)
            .await;
    }
    (a, key)
}

async fn run_restarted(stack: &Stack) -> tokio_util::sync::CancellationToken {
    let restarted = stack.restart();
    stack.journal.subscribe(restarted.event_sink());
    let token = tokio_util::sync::CancellationToken::new();
    let stop = token.clone();
    tokio::spawn(async move {
        let _ = restarted.run(stop).await;
    });
    token
}

// Scenario: Restart between record and confirm
#[tokio::test]
async fn restart_between_record_and_confirm() {
    let stack = Stack::new().await;
    let (a, key) = interrupted_send(&stack, "v1").await;
    let before = stack
        .graph_requests()
        .await
        .into_iter()
        .filter(|(m, _)| m == "PUT")
        .count();
    let token = run_restarted(&stack).await;
    stack
        .until("the item is marked unknown", || async {
            stack.scalar("SELECT count(*) FROM pending_push_items WHERE result_code = 'PUSH_OUTCOME_UNKNOWN'").await == 1
        })
        .await;
    token.cancel();
    assert_eq!(stack.working.state_of(a), WorkingCopyState::AwaitingPush);
    let puts = stack
        .graph_requests()
        .await
        .into_iter()
        .filter(|(m, _)| m == "PUT")
        .count();
    assert_eq!(puts, before, "nothing is sent");
    let _ = key;
}

// Scenario: Recovery finds the change arrived
#[tokio::test]
async fn recovery_finds_the_change_arrived() {
    let stack = Stack::new().await;
    let (a, key) = interrupted_send(&stack, "v2").await;
    let token = run_restarted(&stack).await;
    stack
        .until("the item is pushed", || async {
            stack.working.state_of(a) == WorkingCopyState::Pushed
        })
        .await;
    token.cancel();
    assert_eq!(stack.working.origin_version_of(a), "v2");
    let state: String =
        sqlx::query_scalar("SELECT state FROM external_actions WHERE action_key = ?1")
            .bind(key.to_string())
            .fetch_one(&stack.pool)
            .await
            .expect("row");
    assert_ne!(state, "confirmed", "the Journal row stays for the person");
}

// PS-REC-1: found in the Phase 5 walk — a Send back pressed while the service's loops were not running
// stayed `proposed` after a restart, and nothing was sent.
// spec: ordin-desktop/components/PushService#PS-REC-1
#[tokio::test]
async fn recovery_carries_out_a_card_decided_while_the_service_was_stopped() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "Kestrel-MSA.docx", "v1")
        .await;
    stack
        .mount_sharepoint_upload("01FILEA", "Kestrel-MSA.docx", "v1", "v2")
        .await;
    let (a, _) = stack.sharepoint_item("Kestrel-MSA.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "New terms")]))
        .await
        .expect("propose");
    stack.stop().await;
    stack.approve(push.approval_id.expect("card")).await;
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pending_pushes WHERE state = 'proposed'")
            .await,
        1,
        "no running service acted on the decision"
    );
    let token = run_restarted(&stack).await;
    stack
        .until("the item is pushed", || async {
            stack.working.state_of(a) == WorkingCopyState::Pushed
        })
        .await;
    token.cancel();
    assert_eq!(stack.working.origin_version_of(a), "v2");
}

// PS-REC-1: a card still waiting keeps its push waiting after a restart.
#[tokio::test]
async fn recovery_leaves_a_waiting_card_waiting() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "Kestrel-MSA.docx", "v1")
        .await;
    let (a, _) = stack.sharepoint_item("Kestrel-MSA.docx").await;
    stack
        .service
        .propose(proposal(stack.workspace, &[(a, "New terms")]))
        .await
        .expect("propose");
    stack.stop().await;
    let token = run_restarted(&stack).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    token.cancel();
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pending_pushes WHERE state = 'proposed'")
            .await,
        1
    );
    assert_eq!(stack.working.state_of(a), WorkingCopyState::AwaitingPush);
}

// ---- published copies (ADR-069) ----
// The copies here go to a Confluence page. The Word, PDF and slides copies SharePoint takes need a
// conversion app; `conversion.rs` tests them with a fake one (FU-105). The rules under test (the
// destination's version, the audience, the name) are the same for both destinations.

use ordin_types::domain::places::PublishedCopyState;

// Scenario: Format not ready (C-28)
#[tokio::test]
async fn format_not_ready_c_28() {
    let stack = Stack::new().await;
    let mut request = stack.copy_request(false);
    request.format = ordin_types::domain::places::CopyFormat::Word;
    request.destination = ordin_types::domain::places::CopyDestination::SharepointFile {
        site: "s".to_owned(),
        library: DRIVE.to_owned(),
        folder: FOLDER.to_owned(),
    };
    let error = stack
        .service
        .publish_copy(request)
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_FORMAT_NOT_READY");
}

// Scenario: Copy kept up to date after a merge on the followed branch
#[tokio::test]
async fn copy_kept_up_to_date_after_a_merge_on_the_followed_branch() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(true).await;
    stack.confluence_page("5200", "Rates copy", 1).await;
    stack
        .confluence_reply(
            "PUT",
            &conf("/api/v2/pages/5200"),
            200,
            page_json("5200", "Rates copy", 2),
            false,
        )
        .await;
    stack.person_saves("rates v2\n").await;
    match stack
        .notice(|n| match n {
            PushNotice::CopiesUpdated {
                updated,
                held,
                failed,
                from_version,
                ..
            } => Some((*updated, *held, *failed, from_version.clone())),
            _ => None,
        })
        .await
    {
        (1, 0, 0, version) => assert_eq!(version, "2"),
        other => panic!("{other:?}"),
    }
    let now = stack.files.published_copy(copy).await.expect("copy");
    assert_eq!(now.state, PublishedCopyState::Current);
    assert_eq!(now.destination_version_pushed.as_deref(), Some("2"));
    assert_eq!(now.original_version_published.as_deref(), Some("2"));
    let bodies = stack
        .confluence_bodies("PUT", &conf("/api/v2/pages/5200"))
        .await;
    assert_eq!(bodies.len(), 1, "uploaded without a card");
    assert!(
        bodies[0]["body"]["value"]
            .as_str()
            .unwrap_or("")
            .contains("rates v2"),
        "made from the original at that version"
    );
}

// Scenario: Error path — the person's save does not update a linked folder's copy (SA-PUSH-LINKED-01;
// PushService.md amendment 2026-01-24)
#[tokio::test]
async fn error_path_the_persons_save_does_not_update_a_linked_folders_copy() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(true).await;
    let linked = ordin_types::PublishedCopyId::new();
    stack
        .files
        .record_published_copy(ordin_types::domain::places::PublishedCopy {
            id: linked,
            original: ordin_types::domain::places::CopyOriginal::PlaceFile {
                place_id: stack.place.id,
                path: "rates.md".to_owned(),
            },
            destination: ordin_types::domain::places::CopyDestination::ConfluencePage {
                space: "PROC".to_owned(),
                parent_page_id: PARENT.to_owned(),
            },
            destination_item_id: Some("5300".to_owned()),
            format: ordin_types::domain::places::CopyFormat::Page,
            maker: ordin_types::domain::places::CopyMaker::Ordin,
            published_via: ordin_types::domain::places::PublishedVia::ConfluenceLink {
                confluence_link_id: ordin_types::ConfluenceLinkId::new(),
            },
            followed_branch: None,
            keep_up_to_date: true,
            audience: "Procurement team".to_owned(),
            original_version_published: Some("1".to_owned()),
            destination_version_pushed: Some("1".to_owned()),
            last_updated_at: Some(ordin_types::time::Timestamp::now()),
            state: PublishedCopyState::Current,
            failure_reason: None,
        })
        .await
        .expect("record the linked folder's copy");
    for (id, title) in [("5200", "Rates copy"), ("5300", "rates")] {
        stack.confluence_page(id, title, 1).await;
        stack
            .confluence_reply(
                "PUT",
                &conf(&format!("/api/v2/pages/{id}")),
                200,
                page_json(id, title, 2),
                false,
            )
            .await;
    }
    stack.person_saves("rates v2\n").await;
    let counts = stack
        .notice(|n| match n {
            PushNotice::CopiesUpdated {
                updated,
                held,
                failed,
                ..
            } => Some((*updated, *held, *failed)),
            _ => None,
        })
        .await;
    assert_eq!(counts, (1, 0, 0), "only the Publish a copy copy is updated");
    assert_eq!(
        stack.files.published_copy(copy).await.expect("copy").state,
        PublishedCopyState::Current
    );
    let unchanged = stack.files.published_copy(linked).await.expect("linked");
    assert_eq!(unchanged.state, PublishedCopyState::Current);
    assert_eq!(unchanged.destination_version_pushed.as_deref(), Some("1"));
    assert_eq!(unchanged.original_version_published.as_deref(), Some("1"));
    assert!(
        stack
            .confluence_bodies("PUT", &conf("/api/v2/pages/5300"))
            .await
            .is_empty(),
        "no request is sent for the linked folder's page"
    );
}

/// A copy of `rates.md` recorded as `failed` (or `state`), made `via` the given route, at page `item`.
async fn recorded_copy(
    stack: &Stack,
    via: ordin_types::domain::places::PublishedVia,
    item: Option<&str>,
    state: PublishedCopyState,
    keep: bool,
) -> ordin_types::PublishedCopyId {
    let id = ordin_types::PublishedCopyId::new();
    stack
        .files
        .record_published_copy(ordin_types::domain::places::PublishedCopy {
            id,
            original: ordin_types::domain::places::CopyOriginal::PlaceFile {
                place_id: stack.place.id,
                path: "rates.md".to_owned(),
            },
            destination: ordin_types::domain::places::CopyDestination::ConfluencePage {
                space: "PROC".to_owned(),
                parent_page_id: PARENT.to_owned(),
            },
            destination_item_id: item.map(str::to_owned),
            format: ordin_types::domain::places::CopyFormat::Page,
            maker: ordin_types::domain::places::CopyMaker::Ordin,
            published_via: via,
            followed_branch: None,
            keep_up_to_date: keep,
            audience: "Procurement team".to_owned(),
            original_version_published: item.map(|_| "1".to_owned()),
            destination_version_pushed: item.map(|_| "1".to_owned()),
            last_updated_at: Some(ordin_types::time::Timestamp::now()),
            state,
            failure_reason: (state == PublishedCopyState::Failed)
                .then(|| "ordin couldn't publish this to Confluence.".to_owned()),
        })
        .await
        .expect("record the copy");
    id
}

fn linked() -> ordin_types::domain::places::PublishedVia {
    ordin_types::domain::places::PublishedVia::ConfluenceLink {
        confluence_link_id: ordin_types::ConfluenceLinkId::new(),
    }
}

async fn try_again(
    stack: &Stack,
    copy: ordin_types::PublishedCopyId,
) -> Result<ordin_types::domain::places::PublishedCopy, ordin_push::PushError> {
    stack
        .service
        .copy_action(ordin_push::CopyActionInput {
            published_copy_id: copy,
            action: ordin_push::CopyAction::TryAgain,
        })
        .await
}

/// Every request to Confluence that would change something.
async fn confluence_writes(stack: &Stack) -> Vec<(String, String)> {
    stack
        .confluence_requests()
        .await
        .into_iter()
        .filter(|(verb, _)| verb != "GET")
        .collect()
}

// Scenario: Error path — Try again on a failed linked folder's copy is refused (SA-PUSH-LINKED-02;
// PushService.md amendment 2026-01-24)
#[tokio::test]
async fn error_path_try_again_on_a_failed_linked_folders_copy_is_refused() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack.confluence_page("5300", "rates", 1).await;
    stack
        .confluence_reply(
            "PUT",
            &conf("/api/v2/pages/5300"),
            200,
            page_json("5300", "rates", 2),
            false,
        )
        .await;
    stack.write_original("rates v2 by an agent\n").await;
    let copy = recorded_copy(
        &stack,
        linked(),
        Some("5300"),
        PublishedCopyState::Failed,
        true,
    )
    .await;
    let before = stack.files.published_copy(copy).await.expect("copy");

    let error = try_again(&stack, copy).await.expect_err("refused");

    assert_eq!(error.code, "PUSH_COPY_LINKED");
    let after = stack.files.published_copy(copy).await.expect("copy");
    assert_eq!(after.state, PublishedCopyState::Failed);
    assert_eq!(after.failure_reason, before.failure_reason);
    assert_eq!(after.original_version_published.as_deref(), Some("1"));
    assert_eq!(after.destination_version_pushed.as_deref(), Some("1"));
    assert!(
        confluence_writes(&stack).await.is_empty(),
        "no request is sent for its page"
    );
}

// Scenario: Error path — Try again on a linked folder's copy whose first publish failed is refused
// (SA-PUSH-LINKED-02; PushService.md amendment 2026-01-24)
#[tokio::test]
async fn error_path_try_again_on_a_linked_folders_copy_whose_first_publish_failed_is_refused() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack
        .confluence_reply(
            "POST",
            &conf("/api/v2/pages"),
            200,
            page_json("5300", "rates", 1),
            false,
        )
        .await;
    stack.write_original("rates v1\n").await;
    let copy = recorded_copy(&stack, linked(), None, PublishedCopyState::Failed, true).await;

    let error = try_again(&stack, copy).await.expect_err("refused");

    assert_eq!(error.code, "PUSH_COPY_LINKED");
    let after = stack.files.published_copy(copy).await.expect("copy");
    assert_eq!(after.state, PublishedCopyState::Failed);
    assert_eq!(after.destination_item_id, None);
    assert!(
        confluence_writes(&stack).await.is_empty(),
        "no page is created"
    );
}

// Scenario: Happy path — Try again on a failed copy made by "Publish a copy" still sends it
#[tokio::test]
async fn happy_path_try_again_on_a_failed_copy_made_by_publish_a_copy_still_sends_it() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack.confluence_page("5200", "Rates copy", 1).await;
    stack
        .confluence_reply(
            "PUT",
            &conf("/api/v2/pages/5200"),
            200,
            page_json("5200", "Rates copy", 2),
            false,
        )
        .await;
    stack.write_original("rates v2\n").await;
    let copy = recorded_copy(
        &stack,
        ordin_types::domain::places::PublishedVia::PublishACopy,
        Some("5200"),
        PublishedCopyState::Failed,
        true,
    )
    .await;

    let again = try_again(&stack, copy).await.expect("try again");

    assert_eq!(again.state, PublishedCopyState::Current);
    assert_eq!(again.destination_version_pushed.as_deref(), Some("2"));
    let bodies = stack
        .confluence_bodies("PUT", &conf("/api/v2/pages/5200"))
        .await;
    assert_eq!(bodies.len(), 1, "the page is updated once");
    assert!(
        bodies[0]["body"]["value"]
            .as_str()
            .unwrap_or("")
            .contains("rates v2"),
        "made from the original as it stands"
    );
}

// SA-PUSH-LINKED-03: a linked folder's out-of-date copy is not put on an Update copies card.
#[tokio::test]
async fn update_copies_leaves_out_a_linked_folders_copy() {
    let stack = Stack::new().await;
    stack.write_original("rates v2\n").await;
    recorded_copy(
        &stack,
        linked(),
        Some("5300"),
        PublishedCopyState::OutOfDate,
        false,
    )
    .await;

    let card = stack
        .service
        .update_copies(
            ordin_types::domain::places::CopyOriginal::PlaceFile {
                place_id: stack.place.id,
                path: "rates.md".to_owned(),
            },
            stack.workspace,
        )
        .await
        .expect("update_copies");

    assert!(card.is_none(), "no card is raised");
    assert_eq!(
        stack.scalar("SELECT count(*) FROM copy_operations").await,
        0,
        "no copy operation is stored"
    );
}

// Scenario: Copy changed at its destination
#[tokio::test]
async fn copy_changed_at_its_destination() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(true).await;
    stack.confluence_page("5200", "Rates copy", 5).await;
    stack.person_saves("rates v2\n").await;
    stack
        .until("changed at the destination", || async {
            stack
                .files
                .published_copy(copy)
                .await
                .map(|c| c.state == PublishedCopyState::ChangedAtDestination)
                .unwrap_or(false)
        })
        .await;
    let puts = stack
        .confluence_requests()
        .await
        .into_iter()
        .filter(|(m, _)| m == "PUT")
        .count();
    assert_eq!(puts, 0, "not overwritten");
}

// Scenario: Audience grew
#[tokio::test]
async fn audience_grew() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(true).await;
    stack.confluence_page("5200", "Rates copy", 1).await;
    stack
        .confluence_restrictions(&[("Procurement team", "g-1"), ("Finance", "g-2")], false)
        .await;
    let cards_before = stack.scalar("SELECT count(*) FROM approvals").await;
    stack.person_saves("rates v2\n").await;
    let held = stack
        .notice(|n| match n {
            PushNotice::CopiesUpdated { held, .. } => Some(*held),
            _ => None,
        })
        .await;
    assert_eq!(held, 1);
    assert_eq!(
        stack.scalar("SELECT count(*) FROM approvals").await,
        cards_before + 1,
        "a new card names the change"
    );
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM copy_operations WHERE kind = 'audience_hold'")
            .await,
        1
    );
    let puts = stack
        .confluence_requests()
        .await
        .into_iter()
        .filter(|(m, _)| m == "PUT")
        .count();
    assert_eq!(puts, 0, "the update is held");
    assert_ne!(
        stack.files.published_copy(copy).await.expect("copy").state,
        PublishedCopyState::Updating
    );
}

// Scenario: A copy name already used
#[tokio::test]
async fn a_copy_name_already_used() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack
        .confluence_reply(
            "POST",
            &conf("/api/v2/pages"),
            400,
            json!({"errors": [{"title": "A page with this title already exists"}]}),
            false,
        )
        .await;
    stack.write_original("rates v1\n").await;
    let copy = stack
        .service
        .publish_copy(stack.copy_request(false))
        .await
        .expect("publish_copy");
    let card: String = sqlx::query_scalar("SELECT approval_id FROM copy_operations")
        .fetch_one(&stack.pool)
        .await
        .expect("card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .until("the copy failed", || async {
            stack
                .files
                .published_copy(copy)
                .await
                .map(|c| c.state == PublishedCopyState::Failed)
                .unwrap_or(false)
        })
        .await;
    let failed = stack.files.published_copy(copy).await.expect("copy");
    assert_eq!(
        failed.failure_reason.as_deref(),
        Some("Something with that name is already there.")
    );
    assert_eq!(failed.destination_item_id, None, "nothing is overwritten");
}

// Scenario: A document changed after approval is not sent
#[tokio::test]
async fn a_document_changed_after_approval_is_not_sent() {
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
    stack
        .working
        .edit_unseen(item, b"<p>Edited after approval</p>");
    stack.approve(push.approval_id.expect("card")).await;
    let (_, failed, _) = finished(&stack).await;
    assert_eq!(failed, vec![(item, "PUSH_APPROVED_VERSION_CHANGED")]);
    // Try again goes through the same check.
    let again = stack
        .service
        .try_again(ordin_push::TryAgainInput {
            pending_push_id: push.id,
            working_copy_item_id: item,
        })
        .await
        .expect("try again");
    assert!(again.items[0].external_action_key.is_none());
    assert!(
        !stack
            .confluence_requests()
            .await
            .iter()
            .any(|(m, _)| m == "PUT"),
        "nothing reaches Confluence"
    );
    assert_eq!(
        stack.scalar("SELECT count(*) FROM external_actions").await,
        0
    );
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pending_push_items WHERE result_code = 'PUSH_APPROVED_VERSION_CHANGED'")
            .await,
        1
    );
    assert_eq!(stack.working.state_of(item), WorkingCopyState::Changed);
}

// ---- restart: state that used to live only in memory ----

/// Stops the running service so that only a restarted one handles the next card press.
async fn restarted_after_stop(stack: &Stack) -> tokio_util::sync::CancellationToken {
    stack.stop().await;
    let restarted = stack.restart();
    stack.journal.subscribe(restarted.event_sink());
    let token = tokio_util::sync::CancellationToken::new();
    let stop = token.clone();
    tokio::spawn(async move {
        let _ = restarted.run(stop).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    token
}

#[tokio::test]
async fn handover_files_survive_a_restart_before_the_card_is_approved() {
    let stack = Stack::new().await;
    let request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::Handover {
            id: ordin_types::HandoverId::new(),
        },
        ordin_push::PullRequestContent::Files {
            files: vec![("handover.md".to_owned(), b"kept\n".to_vec())],
        },
    );
    stack
        .service
        .propose_pull_request(request)
        .await
        .expect("propose");
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pull_request_files")
            .await,
        1
    );
    let token = restarted_after_stop(&stack).await;
    let card: String = sqlx::query_scalar("SELECT approval_id FROM pull_request_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .until("the pull request is sent", || async {
            stack
                .scalar("SELECT count(*) FROM pull_request_submissions WHERE state = 'sent'")
                .await
                == 1
        })
        .await;
    // The state turns to 'sent' a moment before the stored bytes are deleted, so wait for both.
    stack
        .until("the bytes are removed once sent", || async {
            stack
                .scalar("SELECT count(*) FROM pull_request_files")
                .await
                == 0
        })
        .await;
    token.cancel();
    let calls = stack.github.state.lock().unwrap().branch_calls.clone();
    assert_eq!(
        calls,
        vec![(
            "draft/agent-1".to_owned(),
            "main".to_owned(),
            vec!["handover.md".to_owned()]
        )]
    );
}

#[tokio::test]
async fn an_audience_hold_is_approved_after_a_restart() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(true).await;
    stack.confluence_page("5200", "Rates copy", 1).await;
    stack
        .confluence_restrictions(&[("Procurement team", "g-1"), ("Finance", "g-2")], false)
        .await;
    stack
        .confluence_reply(
            "PUT",
            &conf("/api/v2/pages/5200"),
            200,
            page_json("5200", "Rates copy", 2),
            false,
        )
        .await;
    stack.person_saves("rates v2\n").await;
    stack
        .notice(|n| matches!(n, PushNotice::CopiesUpdated { .. }).then_some(()))
        .await;
    let token = restarted_after_stop(&stack).await;
    let card: String =
        sqlx::query_scalar("SELECT approval_id FROM copy_operations WHERE kind = 'audience_hold'")
            .fetch_one(&stack.pool)
            .await
            .expect("hold card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .until("the held update is sent", || async {
            stack
                .scalar("SELECT count(*) FROM copy_operations WHERE kind = 'audience_hold' AND state = 'done'")
                .await
                == 1
        })
        .await;
    token.cancel();
    let puts = stack
        .confluence_requests()
        .await
        .into_iter()
        .filter(|(m, _)| m == "PUT")
        .count();
    assert_eq!(puts, 1, "the update went out for copy {copy:?}");
}

// The card question is the localiser's output for `push-card-question-many`, not text built in the crate.
#[tokio::test]
async fn the_card_question_is_the_localisers_output() {
    use ordin_localisation::{id, ArgValue, MessageArgs};
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    let b = stack.local_item("b.md", "B");
    let push = stack
        .service
        .propose(proposal(
            stack.workspace,
            &[(a, "A changed"), (b, "B changed")],
        ))
        .await
        .expect("propose");
    let card = stack
        .approvals
        .get(push.approval_id.expect("card"))
        .await
        .expect("card");
    let question = card.card.question;
    assert!(question.chars().count() <= 120, "{question}");
    assert!(question.ends_with('?'));
    let expected = stack.localisation.message(
        id::PUSH_CARD_QUESTION_MANY,
        &MessageArgs::new().with("count", ArgValue::Count(2)),
    );
    assert_eq!(question, expected);
}

// ---- a team workspace's results (ADR-121) ----

// Scenario: Happy path — a result published to the team's SharePoint folder (PS-TW-1)
// spec: ordin-desktop/components/PushService#PS-TW-1
// spec: ordin-desktop/phase-2c-shared-types-domain#PUB_FORMAT_MAKER_MISMATCH
#[tokio::test]
async fn happy_path_a_result_published_to_the_teams_sharepoint_folder() {
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, ResponseTemplate};
    let stack = Stack::new().await;
    // W's results place is the SharePoint folder; its audience is unknown (403, the C-42 fallback).
    stack.mount_sharepoint_origin().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/permissions$"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&stack.graph)
        .await;
    let name = "Kestrel quote — comparison.md";
    let request = ordin_push::PublishCopyRequest {
        workspace_id: stack.workspace,
        original: ordin_types::domain::places::CopyOriginal::PlaceFile {
            place_id: stack.place.id,
            path: name.to_owned(),
        },
        original_version: "1".to_owned(),
        original_name: name.to_owned(),
        page_file: None,
        destination: ordin_types::domain::places::CopyDestination::SharepointFile {
            site: "s".to_owned(),
            library: DRIVE.to_owned(),
            folder: FOLDER.to_owned(),
        },
        format: ordin_types::domain::places::CopyFormat::ExactFile,
        followed_branch: None,
        keep_up_to_date: false,
        file_name: name.to_owned(),
    };
    let copy = stack
        .service
        .publish_copy(request)
        .await
        .expect("publish_copy");
    let cards: Vec<String> = sqlx::query_scalar("SELECT approval_id FROM copy_operations")
        .fetch_all(&stack.pool)
        .await
        .expect("the cards");
    assert_eq!(cards.len(), 1, "one card");
    let card = stack
        .approvals
        .get(ordin_types::ApprovalId::parse(&cards[0]).expect("id"))
        .await
        .expect("card");
    assert_eq!(
        card.card.question,
        "Publish a copy of Kestrel quote — comparison.md?"
    );
    // Nothing is sent before it is approved.
    let sent = stack
        .graph
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.method.as_str() != "GET")
        .count();
    assert_eq!(sent, 0, "nothing is sent before approval");
    assert_eq!(
        stack.files.published_copy(copy).await.expect("copy").state,
        PublishedCopyState::AwaitingApproval
    );
}

// Scenario: Error — an exact file to a Confluence page is still refused (PS-TW-1, 2C)
// spec: ordin-desktop/components/PushService#PS-TW-1
#[tokio::test]
async fn error_an_exact_file_to_a_confluence_page_is_still_refused() {
    let stack = Stack::new().await;
    let name = "Kestrel quote — comparison.md";
    let request = ordin_push::PublishCopyRequest {
        workspace_id: stack.workspace,
        original: ordin_types::domain::places::CopyOriginal::PlaceFile {
            place_id: stack.place.id,
            path: name.to_owned(),
        },
        original_version: "1".to_owned(),
        original_name: name.to_owned(),
        page_file: None,
        destination: ordin_types::domain::places::CopyDestination::ConfluencePage {
            space: "PROC".to_owned(),
            parent_page_id: "4001".to_owned(),
        },
        format: ordin_types::domain::places::CopyFormat::ExactFile,
        followed_branch: None,
        keep_up_to_date: false,
        file_name: name.to_owned(),
    };
    let err = stack
        .service
        .publish_copy(request)
        .await
        .expect_err("refused");
    assert_eq!(err.code, "PUSH_REQUEST_INVALID");
    assert_eq!(err.detail, "PUB_FORMAT_MAKER_MISMATCH");
    let cards: i64 = sqlx::query_scalar("SELECT count(*) FROM copy_operations")
        .fetch_one(&stack.pool)
        .await
        .expect("the count");
    assert_eq!(cards, 0, "no card");
}
