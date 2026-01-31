//! X2: one test per Gherkin scenario of ApprovalService.md, under the scenario's name in snake_case.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ordin_approval::{ApprovalError, ApprovalService, ChatChannel, PressOrigin, WaitOutcome};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::wire::approval::{
    AffectedItem, ApprovalAsker, ApprovalContent, PressDecision, RejectStyle,
};
use ordin_types::{ApprovalId, CrewRunId, Cursor, RunId};
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn happy_path_a_run_asks_and_the_person_approves() {
    let f = fixture().await;
    let run_id = RunId::new();
    let ws = workspace();
    // Given a run "R1" in workspace "Sales" asks to push 3 commits
    let card = f
        .service
        .request(request_for(ws, ApprovalAsker::Run { run_id }))
        .await
        .unwrap();
    // Then a card is returned with two 22-character button references
    assert_eq!(card.question, "Push 3 commits to acme/supplier-portal?");
    assert_eq!(card.verb, "Push");
    assert_eq!(ref_text(&card.buttons.approve).len(), 22);
    assert_eq!(ref_text(&card.buttons.reject).len(), 22);
    assert_ne!(
        ref_text(&card.buttons.approve),
        ref_text(&card.buttons.reject)
    );
    assert!(card.buttons.show_details.is_none());
    // And the event approval_requested is journalled with that card
    let requested = f.events_of("approval_requested").await;
    assert_eq!(requested.len(), 1);
    assert_eq!(
        requested[0].data["card"]["approval_id"],
        card.approval_id.to_string()
    );
    assert_eq!(requested[0].data["card"]["workspace_id"], ws.to_string());
    // A waiter is already waiting when the person presses.
    let svc = f.service.clone();
    let id = card.approval_id;
    let waiter = tokio::spawn(async move { svc.wait(id, None).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    // When the person presses the approve reference on this computer
    let outcome = f
        .service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    // Then decide returns state approved with decided_on "Asha's laptop"
    assert_eq!(outcome.state, ApprovalState::Approved);
    assert_eq!(outcome.decided_on, "Asha's laptop");
    assert_eq!(outcome.approval_id, card.approval_id);
    // And every waiter on the approval wakes with Approved
    let woke = waiter.await.unwrap().unwrap();
    assert_eq!(woke, WaitOutcome::Decided(ApprovalState::Approved));
    // And the event approval_decided is journalled
    let decided = f.events_of("approval_decided").await;
    assert_eq!(decided.len(), 1);
    assert_eq!(decided[0].data["state"], "approved");
    assert_eq!(decided[0].data["decided_on"], "Asha's laptop");
    assert_eq!(decided[0].data["approval_id"], card.approval_id.to_string());
    // needs_you_changed fired when the approval became pending and when it stopped being pending (L).
    let needs: Vec<_> = f.events_of("needs_you_changed").await;
    assert_eq!(needs.len(), 2);
    assert!(needs
        .iter()
        .all(|e| e.data["workspace_id"] == ws.to_string()));
    // The stored record carries the proposal, the decision and the person's label.
    let record = f.service.get(card.approval_id).await.unwrap();
    assert_eq!(record.state, ApprovalState::Approved);
    assert_eq!(record.decided_by.as_deref(), Some("person"));
    assert_eq!(record.decided_on.as_deref(), Some("Asha's laptop"));
    assert!(record.decided_at.is_some());
    assert_eq!(record.proposal.action, "Push to GitHub");
    assert_eq!(record.proposal.affects.len(), 1);
    let domain = record.to_domain().expect("a run's approval has a subject");
    assert_eq!(domain.state, ApprovalState::Approved);
    assert_eq!(
        domain.affected_records,
        vec!["3 commits on main".to_owned()]
    );
    // The reject reference is closed, the approve reference used.
    let error = f
        .service
        .decide(reject(&card), PressOrigin::LocalScreen)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_CARD_ALREADY_DECIDED");
}

#[tokio::test(flavor = "multi_thread")]
async fn edge_two_channels_press_at_the_same_instant() {
    let f = fixture().await;
    // Given a pending approval with buttons sent to Telegram and to a paired desktop
    let card = f.service.request(request()).await.unwrap();
    // When both presses arrive within the same millisecond
    let (a, b) = {
        let (s1, s2) = (f.service.clone(), f.service.clone());
        let (p1, p2) = (approve(&card), approve(&card));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let (b1, b2) = (barrier.clone(), barrier);
        let t1 = tokio::spawn(async move {
            b1.wait().await;
            s1.decide(
                p1,
                PressOrigin::PairedChat {
                    channel: ChatChannel::Telegram,
                },
            )
            .await
        });
        let t2 = tokio::spawn(async move {
            b2.wait().await;
            s2.decide(
                p2,
                PressOrigin::PairedDesktop {
                    computer_name: "Dan's desktop".into(),
                },
            )
            .await
        });
        (t1.await.unwrap(), t2.await.unwrap())
    };
    // Then exactly one decide returns approved
    let (won, lost) = match (a, b) {
        (Ok(win), Err(lose)) | (Err(lose), Ok(win)) => (win, lose),
        other => panic!("exactly one press wins, got {other:?}"),
    };
    assert_eq!(won.state, ApprovalState::Approved);
    // And the other returns APPR_CARD_ALREADY_DECIDED naming where it was decided
    assert_eq!(lost.code(), "APPR_CARD_ALREADY_DECIDED");
    match lost {
        ApprovalError::AlreadyDecided { state, decided_on } => {
            assert_eq!(state, ApprovalState::Approved);
            assert_eq!(decided_on, won.decided_on);
        }
        other => panic!("expected AlreadyDecided, got {other:?}"),
    }
    assert_eq!(f.events_of("approval_decided").await.len(), 1);
}

#[tokio::test]
async fn edge_a_pending_approval_survives_a_restart() {
    let f = fixture().await;
    // Given a pending approval "A1"
    let card = f.service.request(request()).await.unwrap();
    // When the application restarts
    let restarted = f.restart().await;
    // Then list_pending returns "A1"
    let page = restarted.list_pending(None, None).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].card.approval_id, card.approval_id);
    // And nothing is re-emitted at start (G).
    assert_eq!(f.events_of("approval_requested").await.len(), 1);
    // And a press of its approve reference after the restart decides it
    let waiter = {
        let svc = restarted.clone();
        let id = card.approval_id;
        tokio::spawn(async move { svc.wait(id, None).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    let outcome = restarted
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    assert_eq!(outcome.state, ApprovalState::Approved);
    assert_eq!(
        waiter.await.unwrap().unwrap(),
        WaitOutcome::Decided(ApprovalState::Approved)
    );
}

#[tokio::test]
async fn edge_pending_approvals_are_paged_oldest_first() {
    let f = fixture().await;
    // Given 150 pending approvals in workspace "W1", created one after another
    let w1 = workspace();
    let other = workspace();
    let mut created = Vec::new();
    for n in 0..150 {
        let mut req = request_for(
            w1,
            ApprovalAsker::Run {
                run_id: RunId::new(),
            },
        );
        req.question = format!("Push batch {n}?");
        created.push(f.service.request(req).await.unwrap().approval_id);
        // Distinct milliseconds, so the creation order is the paging order.
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    f.service
        .request(request_for(other, ApprovalAsker::Person))
        .await
        .unwrap();
    // When list_pending is called for "W1" with no cursor
    let first = f.service.list_pending(Some(w1), None).await.unwrap();
    // Then it returns the 100 oldest, oldest first, and a next_cursor
    let ids: Vec<ApprovalId> = first.items.iter().map(|r| r.card.approval_id).collect();
    assert_eq!(ids, created[..100].to_vec());
    let cursor = first.next_cursor.expect("a next cursor");
    // When list_pending is called for "W1" with that next_cursor
    let second = f
        .service
        .list_pending(Some(w1), Some(cursor))
        .await
        .unwrap();
    // Then it returns the remaining 50 and no next_cursor
    let ids: Vec<ApprovalId> = second.items.iter().map(|r| r.card.approval_id).collect();
    assert_eq!(ids, created[100..].to_vec());
    assert!(second.next_cursor.is_none());
    // Without a workspace filter the other workspace's approval is listed too.
    let all = f.service.list_pending(None, None).await.unwrap();
    assert_eq!(all.items.len(), 100);
    assert!(all.next_cursor.is_some());
}

#[tokio::test]
async fn error_a_malformed_cursor() {
    let f = fixture().await;
    // When list_pending is called with the cursor "not-a-cursor"
    let cursor: Cursor = serde_json::from_str("\"not-a-cursor\"").unwrap();
    // Then it fails with SRV_BAD_CURSOR
    let error = f
        .service
        .list_pending(None, Some(cursor))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
}

#[tokio::test]
async fn edge_the_run_ends_before_anyone_decides() {
    let f = fixture().await;
    let sink = f.service.event_sink();
    f.journal.subscribe(sink);
    // Given a pending approval asked by run "R1"
    let r1 = RunId::new();
    let other_run = RunId::new();
    let card = f
        .service
        .request(request_for(workspace(), ApprovalAsker::Run { run_id: r1 }))
        .await
        .unwrap();
    let untouched = f
        .service
        .request(request_for(
            workspace(),
            ApprovalAsker::Run { run_id: other_run },
        ))
        .await
        .unwrap();
    let mut waiter = f.service.watch(card.approval_id).await.unwrap();
    // A run that is still running changes nothing.
    f.journal
        .append_event(
            "run_state_changed",
            serde_json::json!({ "run_id": r1, "state": "running" }),
        )
        .await
        .unwrap();
    // When the Journal delivers run_state_changed for "R1" with state cancelled
    f.journal
        .append_event(
            "run_state_changed",
            serde_json::json!({ "run_id": r1, "state": "cancelled" }),
        )
        .await
        .unwrap();
    // Then the approval becomes expired
    tokio::time::timeout(
        Duration::from_secs(5),
        waiter.wait_for(|s| *s == ApprovalState::Expired),
    )
    .await
    .expect("the sink expired the approval")
    .unwrap();
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Expired
    );
    assert_eq!(
        f.service.get(untouched.approval_id).await.unwrap().state,
        ApprovalState::Pending
    );
    // And a later press returns APPR_CARD_CLOSED
    let error = f
        .service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_CARD_CLOSED");
    // approval_decided was journalled with the fixed text of F.2. ApprovalService.md C.7 wakes the
    // waiters before C.8 journals the decision, so the event may land a moment after the waiter
    // wakes: wait for it, bounded, rather than read the Journal once.
    let decided = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let decided = f.events_of("approval_decided").await;
            if !decided.is_empty() {
                return decided;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("approval_decided was journalled");
    assert_eq!(decided.len(), 1);
    assert_eq!(decided[0].data["state"], "expired");
    assert_eq!(decided[0].data["decided_on"], "run ended");
}

#[tokio::test]
async fn the_crew_run_ending_expires_its_approvals() {
    // F.3: crew_round_changed with an ended crew state; also expire_for_crew_run directly.
    let f = fixture().await;
    f.journal.subscribe(f.service.event_sink());
    let crew = CrewRunId::new();
    let card = f
        .service
        .request(request_for(
            workspace(),
            ApprovalAsker::CrewRound {
                crew_run_id: crew,
                round: 2,
            },
        ))
        .await
        .unwrap();
    let domain = f
        .service
        .get(card.approval_id)
        .await
        .unwrap()
        .to_domain()
        .unwrap();
    assert!(matches!(
        domain.subject,
        Some(ordin_types::domain::runs::ApprovalSubject::CrewRound {
            round_number: 2,
            ..
        })
    ));
    f.journal
        .append_event(
            "crew_round_changed",
            serde_json::json!({ "crew_run_id": crew, "round": 2, "state": "running" }),
        )
        .await
        .unwrap();
    f.journal
        .append_event(
            "crew_round_changed",
            serde_json::json!({ "crew_run_id": crew, "round": 2, "state": "finished" }),
        )
        .await
        .unwrap();
    let service = f.service.clone();
    let id = card.approval_id;
    tokio::time::timeout(Duration::from_secs(5), async move {
        loop {
            if service.get(id).await.unwrap().state == ApprovalState::Expired {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the crew run's approval expired");
    // A second call finds nothing pending.
    assert!(f
        .service
        .expire_for_crew_run(crew)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn the_sink_ignores_other_kinds_and_logs_a_lag() {
    let f = fixture().await;
    let sink = f.service.event_sink();
    assert_eq!(sink.name(), "approval");
    let run_id = RunId::new();
    let card = f
        .service
        .request(request_for(workspace(), ApprovalAsker::Run { run_id }))
        .await
        .unwrap();
    for (kind, data) in [
        ("run_progress", serde_json::json!({ "run_id": run_id })),
        (
            "run_state_changed",
            serde_json::json!({ "unexpected": true }),
        ),
        (
            "run_state_changed",
            serde_json::json!({ "run_id": run_id, "state": "paused" }),
        ),
    ] {
        let event = f.journal.append_event(kind, data).await.unwrap();
        sink.deliver(&event).await;
    }
    sink.lagged(7).await;
    // No approval is expired on a guess (F.3).
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Pending
    );
}

#[tokio::test]
async fn edge_waiting_with_a_deadline_never_decides() {
    let f = fixture().await;
    // Given a pending approval
    let card = f.service.request(request()).await.unwrap();
    // When a waiter waits with a deadline ahead and nobody presses (150 ms here, not 50 s: the clock
    // is real, and the behaviour does not depend on the length)
    let deadline = tokio::time::Instant::now() + Duration::from_millis(150);
    let outcome = f
        .service
        .wait(card.approval_id, Some(deadline))
        .await
        .unwrap();
    // Then the waiter gets DeadlinePassed
    assert_eq!(outcome, WaitOutcome::DeadlinePassed);
    // And the approval is still pending
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Pending
    );
    // A later decision still wakes a new waiter.
    f.service
        .decide(reject(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    assert_eq!(
        f.service.wait(card.approval_id, None).await.unwrap(),
        WaitOutcome::Decided(ApprovalState::Rejected)
    );
}

#[tokio::test]
async fn error_the_verb_is_ok() {
    let f = fixture().await;
    // When request is called with verb "OK"
    let mut req = request();
    req.verb = "OK".into();
    // Then it returns APPR_VERB_INVALID and nothing is stored
    let error = f.service.request(req).await.unwrap_err();
    assert_eq!(error.code(), "APPR_VERB_INVALID");
    assert!(f
        .service
        .list_pending(None, None)
        .await
        .unwrap()
        .items
        .is_empty());
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM approvals")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
    assert!(f.events_of("approval_requested").await.is_empty());
}

#[tokio::test]
async fn error_the_question_has_no_question_mark() {
    let f = fixture().await;
    // When request is called with question "Publish the brief"
    let mut req = request();
    req.question = "Publish the brief".into();
    // Then it returns APPR_QUESTION_INVALID
    let error = f.service.request(req).await.unwrap_err();
    assert_eq!(error.code(), "APPR_QUESTION_INVALID");
}

#[tokio::test]
async fn error_unknown_reference() {
    let f = fixture().await;
    // When decide is called with a reference that was never minted
    let error = f
        .service
        .decide(
            press(&card_ref("zzzzzzzzzzzzzzzzzzzzzz"), PressDecision::Approve),
            PressOrigin::LocalScreen,
        )
        .await
        .unwrap_err();
    // Then it returns APPR_CARD_REF_UNKNOWN
    assert_eq!(error.code(), "APPR_CARD_REF_UNKNOWN");
}

#[tokio::test]
async fn error_reject_reference_pressed_as_approve() {
    let f = fixture().await;
    let card = f.service.request(request()).await.unwrap();
    // When decide is called with the reject reference and decision approve
    let error = f
        .service
        .decide(
            press(&card.buttons.reject, PressDecision::Approve),
            PressOrigin::LocalScreen,
        )
        .await
        .unwrap_err();
    // Then it returns APPR_CARD_REF_MISMATCH and the approval stays pending
    assert_eq!(error.code(), "APPR_CARD_REF_MISMATCH");
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Pending
    );
    assert!(f.events_of("approval_decided").await.is_empty());
}

#[tokio::test]
async fn agent_profile_the_asker_may_not_approve_an_outside_email() {
    // Given the approvers are the group "support-leads" and the asker "u-123" is in that group
    let f = fixture_with(Some(policy(&[], &["support-leads"], true))).await;
    // And a pending approval with leaves_company true asked by "u-123"
    let mut req = request();
    req.asker = Some("u-123".into());
    req.leaves_company = true;
    let card = f.service.request(req).await.unwrap();
    // When "u-123" presses approve in chat
    let error = f
        .service
        .decide(approve(&card), approver("u-123", &["support-leads"]))
        .await
        .unwrap_err();
    // Then it returns APPR_ASKER_MAY_NOT_APPROVE
    assert_eq!(error.code(), "APPR_ASKER_MAY_NOT_APPROVE");
    // When "u-456" of "support-leads" presses approve
    let outcome = f
        .service
        .decide(approve(&card), approver("u-456", &["support-leads"]))
        .await
        .unwrap();
    // Then the approval is approved with decided_on "u-456"
    assert_eq!(outcome.state, ApprovalState::Approved);
    assert_eq!(outcome.decided_on, "u-456");
    let record = f.service.get(card.approval_id).await.unwrap();
    assert_eq!(record.decided_by.as_deref(), Some("u-456"));
}

#[tokio::test]
async fn agent_profile_someone_who_is_not_an_approver_presses() {
    // Given the approvers are the user "u-456" only
    let f = fixture_with(Some(policy(&["u-456"], &[], false))).await;
    let card = f.service.request(request()).await.unwrap();
    // When "u-789" presses approve
    let error = f
        .service
        .decide(approve(&card), approver("u-789", &[]))
        .await
        .unwrap_err();
    // Then it returns APPR_NOT_AN_APPROVER
    assert_eq!(error.code(), "APPR_NOT_AN_APPROVER");
    // The named user decides.
    f.service
        .decide(approve(&card), approver("u-456", &[]))
        .await
        .unwrap();
}

#[tokio::test]
async fn agent_profile_no_approvers_named_the_operator_decides() {
    // Given the approver policy names no users and no groups
    let f = fixture_with(Some(policy(&[], &[], false))).await;
    let card = f.service.request(request()).await.unwrap();
    // And a chat press on the same card would have returned APPR_NOT_AN_APPROVER
    let error = f
        .service
        .decide(approve(&card), approver("u-456", &["anyone"]))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_NOT_AN_APPROVER");
    // When an operator calls decide_by_operator with approve
    let outcome = f
        .service
        .decide_by_operator(card.approval_id, PressDecision::Approve, "ops-7")
        .await
        .unwrap();
    // Then the approval is approved with decided_on the operator's subject
    assert_eq!(outcome.state, ApprovalState::Approved);
    assert_eq!(outcome.decided_on, "ops-7");
    let record = f.service.get(card.approval_id).await.unwrap();
    assert_eq!(record.decided_by.as_deref(), Some("ops-7"));
    // The operator cannot decide twice.
    let error = f
        .service
        .decide_by_operator(card.approval_id, PressDecision::Reject, "ops-7")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_CARD_ALREADY_DECIDED");
    // A rejected card decided by the operator.
    let second = f.service.request(request()).await.unwrap();
    let outcome = f
        .service
        .decide_by_operator(second.approval_id, PressDecision::Reject, "ops-7")
        .await
        .unwrap();
    assert_eq!(outcome.state, ApprovalState::Rejected);
}

#[tokio::test]
async fn error_operator_decides_while_approvers_are_named() {
    // Given the approvers are the user "u-456"
    let f = fixture_with(Some(policy(&["u-456"], &[], false))).await;
    let card = f.service.request(request()).await.unwrap();
    // When an operator calls decide_by_operator
    let error = f
        .service
        .decide_by_operator(card.approval_id, PressDecision::Approve, "ops-7")
        .await
        .unwrap_err();
    // Then it returns APPR_OPERATOR_NOT_ALLOWED
    assert_eq!(error.code(), "APPR_OPERATOR_NOT_ALLOWED");
}

#[tokio::test]
async fn withdraw_a_pending_approval() {
    let f = fixture().await;
    // Given a pending push approval "A2" in workspace "Sales"
    let sales = workspace();
    let card = f
        .service
        .request(request_for(sales, ApprovalAsker::Person))
        .await
        .unwrap();
    let mut waiter = f.service.watch(card.approval_id).await.unwrap();
    // When the Push Service calls withdraw for "A2"
    f.service.withdraw(card.approval_id).await.unwrap();
    // Then the approval is expired and its buttons are closed
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Expired
    );
    waiter
        .wait_for(|s| *s == ApprovalState::Expired)
        .await
        .expect("waiters wake with Expired");
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM approval_buttons WHERE approval_id = ? AND state <> 'closed'",
    )
    .bind(card.approval_id.to_string())
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(open, 0);
    // And approval_decided with decided_on "withdrawn" and needs_you_changed for "Sales" are journalled
    let decided = f.events_of("approval_decided").await;
    assert_eq!(decided.len(), 1);
    assert_eq!(decided[0].data["decided_on"], "withdrawn");
    assert_eq!(decided[0].data["state"], "expired");
    let needs = f.events_of("needs_you_changed").await;
    assert_eq!(needs.len(), 2);
    assert_eq!(needs[1].data["workspace_id"], sales.to_string());
    // And a later press of its approve reference returns APPR_CARD_CLOSED
    let error = f
        .service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_CARD_CLOSED");
    // When withdraw is called again for "A2"
    f.service.withdraw(card.approval_id).await.unwrap();
    // Then it returns Ok and nothing changes
    assert_eq!(f.events_of("approval_decided").await.len(), 1);
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Expired
    );
}

#[tokio::test]
async fn the_handover_service_withdraws_a_pending_hand_over_card_on_take_back_sa_close_01() {
    let f = fixture().await;
    // Given a pending hand-over approval "A4" asked by the Handover Service
    let mut req = request_for(workspace(), ApprovalAsker::Person);
    req.reject_style = RejectStyle::NotNow;
    let card = f.service.request(req).await.unwrap();
    // When the Handover Service calls withdraw for "A4" from take_back
    f.service.withdraw(card.approval_id).await.unwrap();
    // Then the approval is expired with decided_on "withdrawn"
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Expired
    );
    let decided = f.events_of("approval_decided").await;
    assert_eq!(decided[0].data["decided_on"], "withdrawn");
}

#[tokio::test]
async fn withdraw_after_the_person_decided() {
    let f = fixture().await;
    // Given approval "A3" was approved on "Asha's laptop"
    let card = f.service.request(request()).await.unwrap();
    f.service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    // When the Push Service calls withdraw for "A3"
    let error = f.service.withdraw(card.approval_id).await.unwrap_err();
    // Then it returns APPR_CARD_ALREADY_DECIDED naming "Asha's laptop"
    assert_eq!(error.code(), "APPR_CARD_ALREADY_DECIDED");
    match error {
        ApprovalError::AlreadyDecided { state, decided_on } => {
            assert_eq!(state, ApprovalState::Approved);
            assert_eq!(decided_on, "Asha's laptop");
        }
        other => panic!("expected AlreadyDecided, got {other:?}"),
    }
}

#[tokio::test]
async fn error_the_datastore_is_unavailable() {
    let f = fixture().await;
    // Given the SQLite file is locked by another writer beyond the busy timeout
    let mut writer = f.pool().acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .unwrap();
    // When request is called
    let error = f.service.request(request()).await.unwrap_err();
    // Then it returns APPR_STORE_UNAVAILABLE marked retryable
    assert_eq!(error.code(), "APPR_STORE_UNAVAILABLE");
    assert!(error.retryable());
    sqlx::query("ROLLBACK").execute(&mut *writer).await.unwrap();
    drop(writer);
    // The service recovers once the lock is gone.
    f.service
        .request(request())
        .await
        .expect("the store is back");
}

#[tokio::test]
async fn a_card_naming_a_slack_post_is_decided_like_any_other() {
    let f = fixture().await;
    // Given the Handover Service requested a card whose affects hold a Record "#design-reviews · Slack · will appear as you"
    let label = "#design-reviews · Slack · will appear as you";
    let mut req = request_for(workspace(), ApprovalAsker::Person);
    req.affects = vec![AffectedItem::Record {
        label: label.to_owned(),
    }];
    req.proposal.affects = req.affects.clone();
    req.content = Some(ApprovalContent::Message {
        label: "See what will be sent".into(),
        text: "Priya's assessment is in the workspace.".into(),
        destination_note: Some("Thread of Dan Moss, 09:46".into()),
    });
    req.show_details = true;
    req.reject_style = RejectStyle::NotNow;
    let card = f.service.request(req).await.unwrap();
    assert!(card.buttons.show_details.is_some());
    // When the person presses Hand over
    let outcome = f
        .service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    // Then the approval's state is approved and approval_decided is journalled once
    assert_eq!(outcome.state, ApprovalState::Approved);
    assert_eq!(f.events_of("approval_decided").await.len(), 1);
    // And the card is not changed afterwards
    let record = f.service.get(card.approval_id).await.unwrap();
    assert_eq!(
        serde_json::to_value(&record.card).unwrap(),
        serde_json::to_value(&card).unwrap()
    );
    // A person's card has no ApprovalSubject (to_domain).
    assert!(record.to_domain().is_none());
}

#[tokio::test]
async fn migrations_are_registered_data_for_both_engines() {
    let migrations = ApprovalService::migrations();
    assert_eq!(migrations.module, "approval");
    assert_eq!(ApprovalService::MODULE, "approval");
    assert_eq!(migrations.sqlite.len(), 1);
    assert_eq!(migrations.postgres.len(), 1);
    assert_eq!(migrations.sqlite[0].0, 1);
    assert!(migrations.sqlite[0].2.contains("CREATE TABLE approvals"));
    assert!(migrations.postgres[0]
        .2
        .contains("CREATE TABLE approval_buttons"));
}

#[tokio::test]
async fn pruning_removes_decided_rows_but_never_a_pending_one() {
    let f = fixture().await;
    let busy_run = RunId::new();
    let pending = f.service.request(request()).await.unwrap();
    let decided = f.service.request(request()).await.unwrap();
    f.service
        .decide(approve(&decided), PressOrigin::LocalScreen)
        .await
        .unwrap();
    let in_progress = f
        .service
        .request(request_for(
            workspace(),
            ApprovalAsker::Run { run_id: busy_run },
        ))
        .await
        .unwrap();
    f.service
        .decide(reject(&in_progress), PressOrigin::LocalScreen)
        .await
        .unwrap();
    let participant = f.service.prune_participant();
    assert_eq!(participant.module(), "approval");
    let cutoff = ordin_types::time::Timestamp::from_unix_millis(
        ordin_types::time::Timestamp::now().unix_millis() + 60_000,
    )
    .unwrap();
    let removed = participant
        .prune(
            &f.store,
            &ordin_journal::PruneRequest {
                cutoff,
                runs_in_progress: vec![busy_run],
            },
        )
        .await
        .unwrap();
    assert_eq!(removed, 1);
    assert!(f.service.get(decided.approval_id).await.is_err());
    assert!(f.service.get(in_progress.approval_id).await.is_ok());
    assert!(f.service.get(pending.approval_id).await.is_ok());
    // A closed store reports the crate's own code.
    f.store.close().await;
    let failure = participant
        .prune(
            &f.store,
            &ordin_journal::PruneRequest {
                cutoff,
                runs_in_progress: Vec::new(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "APPR_STORE_UNAVAILABLE");
}

#[tokio::test]
async fn watch_of_a_decided_approval_holds_its_final_state() {
    let f = fixture().await;
    let card = f.service.request(request()).await.unwrap();
    f.service
        .decide(reject(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    // After a restart the channel map is rebuilt from pending rows only; a decided one is built on demand.
    let restarted = f.restart().await;
    let receiver = restarted.watch(card.approval_id).await.unwrap();
    assert_eq!(*receiver.borrow(), ApprovalState::Rejected);
    assert_eq!(
        restarted.wait(card.approval_id, None).await.unwrap(),
        WaitOutcome::Decided(ApprovalState::Rejected)
    );
    let _: Arc<ApprovalService> = restarted;
}
