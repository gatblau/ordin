//! Paths the scenarios and Error Table rows do not reach: the person's actions on copies, Try again,
//! the choices on a held push, and recovery of copy operations.
mod support;
use ordin_push::{
    ChooseVersionInput, CopyAction, CopyActionInput, HeldChoice, PushNotice, TryAgainInput,
};
use ordin_types::domain::places::{CopyOriginal, PublishedCopyState, WorkingCopyState};
use serde_json::json;
use support::*;

async fn act(
    stack: &Stack,
    copy: ordin_types::PublishedCopyId,
    action: CopyAction,
) -> Result<ordin_types::domain::places::PublishedCopy, ordin_push::PushError> {
    stack
        .service
        .copy_action(CopyActionInput {
            published_copy_id: copy,
            action,
        })
        .await
}

async fn state(stack: &Stack, copy: ordin_types::PublishedCopyId) -> PublishedCopyState {
    stack.files.published_copy(copy).await.expect("copy").state
}

/// A kept-up-to-date copy whose destination page changed after ordin's push, so it waits for the person.
async fn changed_at_destination(stack: &Stack) -> ordin_types::PublishedCopyId {
    let copy = stack.published_copy(true).await;
    stack.confluence_page("5200", "Rates copy", 5).await;
    stack.person_saves("rates v2\n").await;
    stack
        .until("changed at the destination", || async {
            state(stack, copy).await == PublishedCopyState::ChangedAtDestination
        })
        .await;
    copy
}

#[tokio::test]
async fn leave_it_keeps_the_destination_and_the_next_update_replaces_it() {
    let stack = Stack::new().await;
    let copy = changed_at_destination(&stack).await;
    let now = act(&stack, copy, CopyAction::LeaveIt)
        .await
        .expect("leave it");
    assert_eq!(
        (now.state, now.destination_version_pushed.as_deref()),
        (PublishedCopyState::Current, Some("5"))
    );
    let again = changed_at_destination_again(&stack, copy).await;
    assert_eq!(again.state, PublishedCopyState::Current);
}

/// After `leave it`, the destination is at the version ordin recorded, so the next change updates it.
async fn changed_at_destination_again(
    stack: &Stack,
    copy: ordin_types::PublishedCopyId,
) -> ordin_types::domain::places::PublishedCopy {
    stack
        .confluence_reply(
            "PUT",
            &conf("/api/v2/pages/5200"),
            200,
            page_json("5200", "Rates copy", 6),
            false,
        )
        .await;
    stack.person_saves("rates v3\n").await;
    stack
        .until("updated", || async {
            stack
                .files
                .published_copy(copy)
                .await
                .map(|c| c.destination_version_pushed.as_deref() == Some("6"))
                .unwrap_or(false)
        })
        .await;
    stack.files.published_copy(copy).await.expect("copy")
}

#[tokio::test]
async fn bring_it_in_saves_the_destination_page_as_the_persons_edit_of_the_original() {
    let stack = Stack::new().await;
    let copy = changed_at_destination(&stack).await;
    let before = stack
        .files
        .versions(stack.place.id, "rates.md")
        .await
        .expect("versions")
        .len();
    let now = act(&stack, copy, CopyAction::BringItIn)
        .await
        .expect("bring it in");
    assert_eq!(now.state, PublishedCopyState::Current);
    let versions = stack
        .files
        .versions(stack.place.id, "rates.md")
        .await
        .expect("versions");
    assert_eq!(versions.len(), before + 1, "a new version of the original");
}

#[tokio::test]
async fn stop_keeping_up_to_date_and_keep_up_to_date_again() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(true).await;
    let stopped = act(&stack, copy, CopyAction::StopKeepingUpToDate)
        .await
        .expect("stop");
    assert_eq!(
        (stopped.state, stopped.keep_up_to_date),
        (PublishedCopyState::NotKeptUpToDate, false)
    );
    assert_eq!(
        act(&stack, copy, CopyAction::StopKeepingUpToDate)
            .await
            .expect_err("twice")
            .code,
        "PUSH_COPY_STATE"
    );
    let kept = act(&stack, copy, CopyAction::KeepUpToDate)
        .await
        .expect("keep");
    assert_eq!(
        (kept.state, kept.keep_up_to_date),
        (PublishedCopyState::OutOfDate, true)
    );
}

#[tokio::test]
async fn remove_deletes_the_page_under_a_key_and_the_copy_is_removed() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(false).await;
    stack.confluence_page("5200", "Rates copy", 1).await;
    stack
        .confluence_reply(
            "DELETE",
            &conf("/api/v2/pages/5200"),
            204,
            json!(null),
            false,
        )
        .await;
    let removed = act(&stack, copy, CopyAction::Remove).await.expect("remove");
    assert_eq!(removed.state, PublishedCopyState::Removed);
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM copy_operations WHERE kind = 'remove' AND state = 'done'")
            .await,
        1
    );
    assert_eq!(
        act(&stack, copy, CopyAction::Remove)
            .await
            .expect_err("twice")
            .code,
        "PUSH_COPY_STATE"
    );
}

#[tokio::test]
async fn a_copy_not_kept_up_to_date_goes_out_of_date_and_one_card_updates_it() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(false).await;
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
    stack
        .until("out of date", || async {
            state(&stack, copy).await == PublishedCopyState::OutOfDate
        })
        .await;
    let original = CopyOriginal::PlaceFile {
        place_id: stack.place.id,
        path: "rates.md".to_owned(),
    };
    let card = stack
        .service
        .update_copies(original.clone(), stack.workspace)
        .await
        .expect("card")
        .expect("a card");
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM copy_operations WHERE kind = 'update_copies'")
            .await,
        1
    );
    stack.approve(card).await;
    stack
        .until("updated", || async {
            stack
                .files
                .published_copy(copy)
                .await
                .map(|c| c.destination_version_pushed.as_deref() == Some("2"))
                .unwrap_or(false)
        })
        .await;
    assert_eq!(state(&stack, copy).await, PublishedCopyState::Current);
    assert!(
        stack
            .service
            .update_copies(original, stack.workspace)
            .await
            .expect("none")
            .is_none(),
        "nothing is out of date now"
    );
}

#[tokio::test]
async fn try_again_publishes_a_copy_whose_first_publish_failed() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack
        .confluence_reply(
            "POST",
            &conf("/api/v2/pages"),
            400,
            json!({"errors": [{"title": "already exists"}]}),
            true,
        )
        .await;
    stack
        .confluence_reply(
            "POST",
            &conf("/api/v2/pages"),
            200,
            page_json("5200", "Rates copy", 1),
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
            state(&stack, copy).await == PublishedCopyState::Failed
        })
        .await;
    assert_eq!(
        act(&stack, copy, CopyAction::LeaveIt)
            .await
            .expect_err("wrong state")
            .code,
        "PUSH_COPY_STATE"
    );
    let again = act(&stack, copy, CopyAction::TryAgain)
        .await
        .expect("try again");
    assert_eq!(
        (again.state, again.destination_item_id.as_deref()),
        (PublishedCopyState::Current, Some("5200"))
    );
}

#[tokio::test]
async fn a_rejected_copy_card_publishes_nothing() {
    let stack = Stack::new().await;
    stack.mount_confluence_site().await;
    stack.write_original("rates v1\n").await;
    let copy = stack
        .service
        .publish_copy(stack.copy_request(true))
        .await
        .expect("publish");
    let card: String = sqlx::query_scalar("SELECT approval_id FROM copy_operations")
        .fetch_one(&stack.pool)
        .await
        .expect("card");
    stack
        .reject(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .until("removed", || async {
            state(&stack, copy).await == PublishedCopyState::Removed
        })
        .await;
    assert!(stack
        .confluence_requests()
        .await
        .iter()
        .all(|(m, _)| m != "POST"));
}

#[tokio::test]
async fn a_copy_update_that_arrived_before_a_restart_is_recorded() {
    let stack = Stack::new().await;
    let copy = stack.published_copy(true).await;
    // An update was begun (key K) and the application stopped before the answer; the page is at version 2.
    let key = ordin_types::ExternalActionKey::new();
    stack
        .journal
        .begin_external_action(ordin_journal::NewExternalAction {
            action_key: key,
            run_id: None,
            kind: ordin_types::domain::runs::ExternalActionKind::CopyPublish,
            destination: "Rates copy".to_owned(),
        })
        .await
        .expect("begin");
    sqlx::query("INSERT INTO copy_operations (id, published_copy_id, kind, action_key, original_version, state, created_at) VALUES ('op-1', ?1, 'update', ?2, '2', 'sending', '2026-01-01T00:00:00.000Z')")
        .bind(copy.to_string())
        .bind(key.to_string())
        .execute(&stack.pool)
        .await
        .expect("operation");
    stack
        .files
        .set_published_copy_state(copy, PublishedCopyState::OutOfDate, None)
        .await
        .expect("out of date");
    stack
        .files
        .set_published_copy_state(copy, PublishedCopyState::Updating, None)
        .await
        .expect("updating");
    stack.confluence_page("5200", "Rates copy", 2).await;
    let restarted = stack.restart();
    let token = tokio_util::sync::CancellationToken::new();
    let stop = token.clone();
    tokio::spawn(async move {
        let _ = restarted.run(stop).await;
    });
    stack
        .until("recorded", || async {
            state(&stack, copy).await == PublishedCopyState::Current
        })
        .await;
    token.cancel();
    assert_eq!(
        stack
            .files
            .published_copy(copy)
            .await
            .expect("copy")
            .destination_version_pushed
            .as_deref(),
        Some("2")
    );
}

// ---- Try again and the choices of a held push ----

#[tokio::test]
async fn try_again_sends_an_item_that_failed_under_the_same_approval() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    stack.graph_put("01FILEA", 503).await;
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    stack
        .notice(|n| match n {
            PushNotice::PushFinished { failed, .. } if !failed.is_empty() => Some(()),
            _ => None,
        })
        .await;
    // The destination is back: the upload now works.
    stack.graph.reset().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    stack
        .mount_sharepoint_upload("01FILEA", "A-1.docx", "v1", "v2")
        .await;
    let again = stack
        .service
        .try_again(TryAgainInput {
            pending_push_id: push.id,
            working_copy_item_id: a,
        })
        .await
        .expect("try again");
    assert!(again.items[0].external_action_key.is_some());
    assert_eq!(stack.working.state_of(a), WorkingCopyState::Pushed);
    // Nothing to retry now.
    assert_eq!(
        stack
            .service
            .try_again(TryAgainInput {
                pending_push_id: push.id,
                working_copy_item_id: a
            })
            .await
            .expect_err("done")
            .code,
        "PUSH_UNKNOWN"
    );
}

#[tokio::test]
async fn try_again_does_not_send_a_document_changed_after_approval() {
    let stack = Stack::new().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    stack.graph_put("01FILEA", 503).await;
    let (a, _) = stack.sharepoint_item("A-1.docx").await;
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A")]))
        .await
        .expect("propose");
    stack.approve(push.approval_id.expect("card")).await;
    stack
        .notice(|n| match n {
            PushNotice::PushFinished { failed, .. } if !failed.is_empty() => Some(()),
            _ => None,
        })
        .await;
    // After approval, the person saves a new version; then the destination is back.
    stack.working.edit_unseen(a, b"edited after approval");
    stack.graph.reset().await;
    stack
        .mount_sharepoint_file("01FILEA", "A-1.docx", "v1")
        .await;
    stack
        .mount_sharepoint_upload("01FILEA", "A-1.docx", "v1", "v2")
        .await;
    let actions_before = stack.scalar("SELECT count(*) FROM external_actions").await;
    let again = stack
        .service
        .try_again(TryAgainInput {
            pending_push_id: push.id,
            working_copy_item_id: a,
        })
        .await
        .expect("try again");
    assert!(again.items[0].external_action_key.is_none());
    assert!(
        !stack.graph_requests().await.iter().any(|(m, _)| m == "PUT"),
        "nothing reaches SharePoint"
    );
    assert_eq!(
        stack.scalar("SELECT count(*) FROM external_actions").await,
        actions_before,
        "no further external action is journalled"
    );
    assert_eq!(
        stack
            .scalar("SELECT count(*) FROM pending_push_items WHERE result_code = 'PUSH_APPROVED_VERSION_CHANGED'")
            .await,
        1
    );
    assert_eq!(stack.working.state_of(a), WorkingCopyState::Changed);
}

#[tokio::test]
async fn keep_ordins_restores_the_version_and_apply_ordins_to_mine_waits_for_a_new_proposal() {
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
    stack.working.local_edit(a);
    stack
        .notice(|n| matches!(n, PushNotice::PushHeld { .. }).then_some(()))
        .await;
    let kept = stack
        .service
        .choose_version(ChooseVersionInput {
            pending_push_id: push.id,
            working_copy_item_id: a,
            choice: HeldChoice::KeepOrdins,
        })
        .await
        .expect("keep ordin's");
    assert_eq!(stack.working.inner.lock().unwrap().restored, vec![(a, 3)]);
    assert_eq!(kept.items.len(), 2, "both items go back on a new card");
    // The person edits again; this time the Ordin Host re-runs the change.
    stack.working.local_edit(b);
    stack
        .notice(|n| matches!(n, PushNotice::PushHeld { .. }).then_some(()))
        .await;
    let held = stack
        .service
        .choose_version(ChooseVersionInput {
            pending_push_id: push.id,
            working_copy_item_id: b,
            choice: HeldChoice::ApplyOrdinsToMine,
        })
        .await
        .expect("apply");
    assert_eq!(
        held.state,
        ordin_types::domain::places::PendingPushState::Held
    );
    // The Ordin Host proposes again once the change is re-made: the held push is superseded.
    stack
        .working
        .inner
        .lock()
        .unwrap()
        .entries
        .get_mut(&b)
        .expect("entry")
        .item
        .state = WorkingCopyState::Changed;
    let next = stack
        .service
        .propose(proposal(stack.workspace, &[(b, "B re-made")]))
        .await
        .expect("propose again");
    assert_ne!(next.id, push.id);
    assert_eq!(stack.scalar("SELECT count(*) FROM pending_pushes WHERE id = (SELECT id FROM pending_pushes ORDER BY created_at LIMIT 1) AND state = 'closed'").await, 1);
}

#[tokio::test]
async fn keeping_the_persons_version_of_the_only_item_closes_the_push() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A changed")]))
        .await
        .expect("propose");
    stack.working.local_edit(a);
    stack
        .notice(|n| matches!(n, PushNotice::PushHeld { .. }).then_some(()))
        .await;
    let closed = stack
        .service
        .choose_version(ChooseVersionInput {
            pending_push_id: push.id,
            working_copy_item_id: a,
            choice: HeldChoice::KeepMine,
        })
        .await
        .expect("keep mine");
    assert_eq!(
        closed.state,
        ordin_types::domain::places::PendingPushState::Closed
    );
    assert!(stack
        .service
        .list_pending(stack.workspace)
        .await
        .expect("list")
        .is_empty());
}

#[tokio::test]
async fn a_pull_request_card_that_is_rejected_is_reported_and_nothing_is_sent() {
    let stack = Stack::new().await;
    let request = pr_proposal(
        &stack,
        ordin_push::SubmissionOrigin::PartDraft {
            id: ordin_types::PartDraftId::new(),
        },
        ordin_push::PullRequestContent::Files {
            files: vec![("part.md".to_owned(), b"x".to_vec())],
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
        .reject(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    stack
        .notice(|n| matches!(n, PushNotice::PullRequestRejected { .. }).then_some(()))
        .await;
    assert!(stack.github.state.lock().unwrap().branch_calls.is_empty());
}

#[tokio::test]
async fn a_submission_approved_when_the_application_stopped_is_found_on_the_host_or_failed() {
    for found in [true, false] {
        let stack = Stack::new().await;
        let request = pr_proposal(
            &stack,
            ordin_push::SubmissionOrigin::Handover {
                id: ordin_types::HandoverId::new(),
            },
            ordin_push::PullRequestContent::Worktree {
                path: stack.dir.path().to_path_buf(),
                paths: vec![],
            },
        );
        stack
            .service
            .propose_pull_request(request)
            .await
            .expect("propose");
        // The first service must be stopped before the row is approved: its own recovery could
        // otherwise settle the row first, leaving the restarted service nothing to find.
        stack.stop().await;
        sqlx::query("UPDATE pull_request_submissions SET state = 'approved'")
            .execute(&stack.pool)
            .await
            .expect("approved");
        stack.github.state.lock().unwrap().for_branch = found.then(|| pull_request(77));
        let restarted = stack.restart();
        let mut notices = restarted.subscribe();
        let token = tokio_util::sync::CancellationToken::new();
        let stop = token.clone();
        tokio::spawn(async move {
            let _ = restarted.run(stop).await;
        });
        let notice = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                match notices.recv().await {
                    Ok(
                        n @ (PushNotice::PullRequestOpened { .. }
                        | PushNotice::PullRequestFailed { .. }),
                    ) => return n,
                    Ok(_) => {}
                    Err(error) => panic!("{error}"),
                }
            }
        })
        .await
        .expect("a notice");
        token.cancel();
        match (found, notice) {
            (true, PushNotice::PullRequestOpened { pull_request, .. }) => {
                assert_eq!(pull_request.number, 77)
            }
            (false, PushNotice::PullRequestFailed { code, .. }) => {
                assert_eq!(code, "PUSH_OUTCOME_UNKNOWN")
            }
            (_, other) => panic!("{other:?}"),
        }
    }
}

// FU-113: the card raised after "keep Ordin's version" counts for the version now on it.
#[tokio::test]
async fn a_re_raised_card_after_keep_ordins_is_not_blocked_by_the_approved_version_check() {
    let stack = Stack::new().await;
    let a = stack.local_item("a.md", "A");
    let push = stack
        .service
        .propose(proposal(stack.workspace, &[(a, "A changed")]))
        .await
        .expect("propose");
    stack.working.local_edit(a);
    stack
        .notice(|n| matches!(n, PushNotice::PushHeld { .. }).then_some(()))
        .await;
    let kept = stack
        .service
        .choose_version(ChooseVersionInput {
            pending_push_id: push.id,
            working_copy_item_id: a,
            choice: HeldChoice::KeepOrdins,
        })
        .await
        .expect("keep ordin's");
    let now = stack.working.inner.lock().unwrap().entries[&a]
        .item
        .local_version;
    assert_eq!(now, 4, "the restore wrote a new version");
    assert_eq!(
        stack
            .scalar("SELECT ordins_local_version FROM pending_push_items")
            .await,
        i64::from(now),
        "the item counts for the version on the new card"
    );
    stack.approve(kept.approval_id.expect("new card")).await;
    let failed = stack
        .notice(|n| match n {
            PushNotice::PushFinished { failed, .. } => Some(failed.clone()),
            _ => None,
        })
        .await;
    assert!(failed.is_empty(), "{failed:?}");
}
