//! X1: one test per row of the Error Table (ApprovalService.md), and X4: the codes this crate owns.

mod support;

use ordin_approval::{ApprovalError, PressOrigin, ALL_CODES};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::wire::approval::{
    AffectedItem, ApprovalAsker, ApprovalContent, ContentRef, PressDecision,
};
use ordin_types::{ApprovalId, ArtefactId, CodedError, Cursor, RunId};
use support::*;

/// (code, field, the change that breaks the limit).
type Case = (
    &'static str,
    &'static str,
    Box<dyn Fn(&mut ordin_approval::ApprovalRequest)>,
);

fn assert_invalid(result: Result<impl std::fmt::Debug, ApprovalError>, code: &str, field: &str) {
    match result {
        Err(error @ ApprovalError::Invalid { .. }) => {
            assert_eq!(error.code(), code);
            assert!(!error.retryable(), "{code} is permanent");
            let ApprovalError::Invalid { field: got, .. } = error else {
                unreachable!()
            };
            assert_eq!(got, field, "{code} names its field");
        }
        other => panic!("expected {code}, got {other:?}"),
    }
}

fn message(label: &str, text: &str) -> ApprovalContent {
    ApprovalContent::Message {
        label: label.to_owned(),
        text: text.to_owned(),
        destination_note: None,
    }
}

#[tokio::test]
async fn a_card_field_breaks_its_limit() {
    let f = fixture().await;
    let svc = &f.service;
    let cases: Vec<Case> = vec![
        (
            "APPR_QUESTION_INVALID",
            "question",
            Box::new(|r| r.question = String::new()),
        ),
        (
            "APPR_QUESTION_INVALID",
            "question",
            Box::new(|r| r.question = format!("{}?", "q".repeat(120))),
        ),
        (
            "APPR_QUESTION_INVALID",
            "question",
            Box::new(|r| r.question = "Push the commits".into()),
        ),
        (
            "APPR_VERB_INVALID",
            "verb",
            Box::new(|r| r.verb = String::new()),
        ),
        (
            "APPR_VERB_INVALID",
            "verb",
            Box::new(|r| r.verb = "v".repeat(33)),
        ),
        (
            "APPR_VERB_INVALID",
            "verb",
            Box::new(|r| r.verb = "ok".into()),
        ),
        (
            "APPR_VERB_INVALID",
            "verb",
            Box::new(|r| r.verb = "YES".into()),
        ),
        (
            "APPR_VERB_INVALID",
            "verb",
            Box::new(|r| r.verb = "Continue".into()),
        ),
        (
            "APPR_WHERE_INVALID",
            "where_line",
            Box::new(|r| r.where_line = String::new()),
        ),
        (
            "APPR_WHERE_INVALID",
            "where_line",
            Box::new(|r| r.where_line = "w".repeat(201)),
        ),
        (
            "APPR_AFFECTS_INVALID",
            "affects",
            Box::new(|r| r.affects = Vec::new()),
        ),
        (
            "APPR_AFFECTS_INVALID",
            "affects",
            Box::new(|r| {
                r.affects = (0..51)
                    .map(|n| AffectedItem::Record {
                        label: format!("record {n}"),
                    })
                    .collect()
            }),
        ),
        (
            "APPR_AFFECTS_INVALID",
            "affects",
            Box::new(|r| {
                r.affects = vec![AffectedItem::Record {
                    label: "l".repeat(201),
                }]
            }),
        ),
        (
            "APPR_CHANGES_INVALID",
            "changes",
            Box::new(|r| r.changes = String::new()),
        ),
        (
            "APPR_CHANGES_INVALID",
            "changes",
            Box::new(|r| r.changes = "c".repeat(1_001)),
        ),
        (
            "APPR_CONTENT_LABEL_INVALID",
            "content",
            Box::new(|r| r.content = Some(message("Read this", "text"))),
        ),
        (
            "APPR_CONTENT_TOO_LARGE",
            "content",
            Box::new(|r| r.content = Some(message("See what will be sent", &"t".repeat(100_001)))),
        ),
        (
            "APPR_CONTENT_TOO_LARGE",
            "content",
            Box::new(|r| r.content = Some(message("See what will be sent", ""))),
        ),
        (
            "APPR_CONTENT_TOO_LARGE",
            "content",
            Box::new(|r| {
                r.content = Some(ApprovalContent::Command {
                    label: "See the command".into(),
                    command: "c".repeat(8_001),
                    runs_in: "project".into(),
                })
            }),
        ),
        (
            "APPR_CONTENT_TOO_LARGE",
            "content",
            Box::new(|r| {
                r.content = Some(ApprovalContent::Files {
                    label: "See the files".into(),
                    files: Vec::new(),
                })
            }),
        ),
        (
            "APPR_CONTENT_TOO_LARGE",
            "content",
            Box::new(|r| {
                r.content = Some(ApprovalContent::Files {
                    label: "See the files".into(),
                    files: (0..101)
                        .map(|_| ContentRef::Artefact {
                            artefact_id: ArtefactId::new(),
                        })
                        .collect(),
                })
            }),
        ),
    ];
    for (code, field, change) in cases {
        let mut req = request();
        change(&mut req);
        assert_invalid(svc.request(req).await, code, field);
    }
    // Nothing was stored by any refusal.
    assert!(svc.list_pending(None, None).await.unwrap().items.is_empty());
}

#[tokio::test]
async fn a_control_character_is_refused_with_the_code_of_its_field() {
    let f = fixture().await;
    let mut req = request();
    req.question = "Push\u{7}?".into();
    assert_invalid(
        f.service.request(req).await,
        "APPR_QUESTION_INVALID",
        "question",
    );
    let mut req = request();
    req.changes = "line\r\nbreak".into();
    assert_invalid(
        f.service.request(req).await,
        "APPR_CHANGES_INVALID",
        "changes",
    );
    // A newline and a tab are allowed.
    let mut req = request();
    req.changes = "line one\n\tline two".into();
    f.service.request(req).await.expect("newline and tab pass");
}

#[tokio::test]
async fn text_is_stored_nfc_normalised() {
    let f = fixture().await;
    let mut req = request();
    // "e" followed by a combining acute accent.
    req.question = "Push the caf\u{65}\u{301} files?".into();
    let card = f.service.request(req).await.unwrap();
    assert_eq!(card.question, "Push the caf\u{e9} files?");
}

#[tokio::test]
async fn a_proposal_field_breaks_its_limit() {
    let f = fixture().await;
    let svc = &f.service;
    let cases: Vec<Case> = vec![
        (
            "APPR_ACTION_INVALID",
            "action",
            Box::new(|r| r.proposal.action = String::new()),
        ),
        (
            "APPR_ACTION_INVALID",
            "action",
            Box::new(|r| r.proposal.action = "a".repeat(501)),
        ),
        (
            "APPR_DESTINATION_INVALID",
            "destination",
            Box::new(|r| r.proposal.destination = String::new()),
        ),
        (
            "APPR_DESTINATION_INVALID",
            "destination",
            Box::new(|r| r.proposal.destination = "d".repeat(2_049)),
        ),
        (
            "APPR_CONSEQUENCE_INVALID",
            "consequence",
            Box::new(|r| r.proposal.consequence = String::new()),
        ),
        (
            "APPR_CONSEQUENCE_INVALID",
            "consequence",
            Box::new(|r| r.proposal.consequence = "c".repeat(2_001)),
        ),
        (
            "APPR_RECORDS_TOO_MANY",
            "affects",
            Box::new(|r| {
                r.proposal.affects = (0..1_001)
                    .map(|n| AffectedItem::Record {
                        label: format!("record {n}"),
                    })
                    .collect()
            }),
        ),
    ];
    for (code, field, change) in cases {
        let mut req = request();
        change(&mut req);
        assert_invalid(svc.request(req).await, code, field);
    }
}

#[tokio::test]
async fn show_details_without_content() {
    let f = fixture().await;
    let mut req = request();
    req.show_details = true;
    assert_invalid(
        f.service.request(req).await,
        "APPR_SHOW_DETAILS_WITHOUT_CONTENT",
        "show_details",
    );
}

#[tokio::test]
async fn list_pending_cursor_that_does_not_decode() {
    let f = fixture().await;
    let cursor: Cursor = serde_json::from_str("\"not-a-cursor\"").unwrap();
    let error = f
        .service
        .list_pending(None, Some(cursor))
        .await
        .unwrap_err();
    assert!(matches!(error, ApprovalError::BadCursor));
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
    assert!(!error.retryable());
}

#[tokio::test]
async fn press_with_an_unknown_reference() {
    let f = fixture().await;
    let error = f
        .service
        .decide(
            press(&card_ref("AAAAAAAAAAAAAAAAAAAAAA"), PressDecision::Approve),
            PressOrigin::LocalScreen,
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ApprovalError::CardRefUnknown));
    assert_eq!(error.code(), "APPR_CARD_REF_UNKNOWN");
    assert!(!error.retryable());
}

#[tokio::test]
async fn reference_of_the_other_button() {
    let f = fixture().await;
    let card = f.service.request(request()).await.unwrap();
    // The reject reference pressed as approve, and the approve reference pressed as reject.
    for wrong in [
        press(&card.buttons.reject, PressDecision::Approve),
        press(&card.buttons.approve, PressDecision::Reject),
    ] {
        let error = f
            .service
            .decide(wrong, PressOrigin::LocalScreen)
            .await
            .unwrap_err();
        assert!(matches!(error, ApprovalError::CardRefMismatch));
        assert_eq!(error.code(), "APPR_CARD_REF_MISMATCH");
        assert!(!error.retryable());
    }
    assert_eq!(
        f.service.get(card.approval_id).await.unwrap().state,
        ApprovalState::Pending
    );
}

#[tokio::test]
async fn a_show_details_reference_cannot_decide() {
    let f = fixture().await;
    let mut req = request();
    req.content = Some(message("See what will be sent", "the text"));
    req.show_details = true;
    let card = f.service.request(req).await.unwrap();
    let details = card.buttons.show_details.clone().expect("a See button");
    for decision in [PressDecision::Approve, PressDecision::Reject] {
        let error = f
            .service
            .decide(press(&details, decision), PressOrigin::LocalScreen)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "APPR_CARD_REF_MISMATCH");
    }
}

#[tokio::test]
async fn already_decided_elsewhere() {
    let f = fixture().await;
    let card = f.service.request(request()).await.unwrap();
    f.service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    // The same reference again, and the other button of the decided card.
    for again in [approve(&card), reject(&card)] {
        let error = f
            .service
            .decide(
                again,
                PressOrigin::PairedDesktop {
                    computer_name: "Dan's desktop".into(),
                },
            )
            .await
            .unwrap_err();
        match &error {
            ApprovalError::AlreadyDecided { state, decided_on } => {
                assert_eq!(*state, ApprovalState::Approved);
                assert_eq!(decided_on, COMPUTER);
            }
            other => panic!("expected AlreadyDecided, got {other:?}"),
        }
        assert_eq!(error.code(), "APPR_CARD_ALREADY_DECIDED");
        assert!(!error.retryable());
    }
}

#[tokio::test]
async fn the_run_ended_first() {
    let f = fixture().await;
    let run_id = RunId::new();
    let card = f
        .service
        .request(request_for(workspace(), ApprovalAsker::Run { run_id }))
        .await
        .unwrap();
    f.service.expire_for_run(run_id).await.unwrap();
    for press in [approve(&card), reject(&card)] {
        let error = f
            .service
            .decide(press, PressOrigin::LocalScreen)
            .await
            .unwrap_err();
        assert!(matches!(error, ApprovalError::CardClosed));
        assert_eq!(error.code(), "APPR_CARD_CLOSED");
        assert!(!error.retryable());
    }
}

#[tokio::test]
async fn press_from_someone_who_is_not_an_approver() {
    // Agent profile: the user "u-456" only.
    let f = fixture_with(Some(policy(&["u-456"], &[], false))).await;
    let card = f.service.request(request()).await.unwrap();
    let error = f
        .service
        .decide(approve(&card), approver("u-789", &["staff"]))
        .await
        .unwrap_err();
    assert!(matches!(error, ApprovalError::NotAnApprover));
    assert_eq!(error.code(), "APPR_NOT_AN_APPROVER");
    assert!(!error.retryable());
    // A local screen is not an approver in the agent profile either (C.3).
    let error = f
        .service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_NOT_AN_APPROVER");
    // Desktop profile: an `Approver` origin is refused (C.4).
    let desktop = fixture().await;
    let card = desktop.service.request(request()).await.unwrap();
    let error = desktop
        .service
        .decide(approve(&card), approver("u-456", &[]))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_NOT_AN_APPROVER");
}

#[tokio::test]
async fn asker_approves_own_action_that_leaves_the_company() {
    let f = fixture_with(Some(policy(&[], &["support-leads"], true))).await;
    let mut req = request();
    req.asker = Some("u-123".into());
    req.leaves_company = true;
    let card = f.service.request(req).await.unwrap();
    let error = f
        .service
        .decide(approve(&card), approver("u-123", &["support-leads"]))
        .await
        .unwrap_err();
    assert!(matches!(error, ApprovalError::AskerMayNotApprove));
    assert_eq!(error.code(), "APPR_ASKER_MAY_NOT_APPROVE");
    assert!(!error.retryable());
    // The same asker may approve an action inside the company when the profile allows it ...
    let mut req = request();
    req.asker = Some("u-123".into());
    req.leaves_company = false;
    let card = f.service.request(req).await.unwrap();
    f.service
        .decide(approve(&card), approver("u-123", &["support-leads"]))
        .await
        .expect("asker may approve inside the company");
    // ... and may not when it does not.
    let strict = fixture_with(Some(policy(&["u-123"], &[], false))).await;
    let mut req = request();
    req.asker = Some("u-123".into());
    req.leaves_company = false;
    let card = strict.service.request(req).await.unwrap();
    let error = strict
        .service
        .decide(approve(&card), approver("u-123", &[]))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_ASKER_MAY_NOT_APPROVE");
}

#[tokio::test]
async fn operator_decides_while_approvers_are_named() {
    let f = fixture_with(Some(policy(&["u-456"], &[], false))).await;
    let card = f.service.request(request()).await.unwrap();
    let error = f
        .service
        .decide_by_operator(card.approval_id, PressDecision::Approve, "ops-1")
        .await
        .unwrap_err();
    assert!(matches!(error, ApprovalError::OperatorNotAllowed));
    assert_eq!(error.code(), "APPR_OPERATOR_NOT_ALLOWED");
    assert!(!error.retryable());
    // The desktop profile (no policy at all) has no operator either.
    let desktop = fixture().await;
    let card = desktop.service.request(request()).await.unwrap();
    let error = desktop
        .service
        .decide_by_operator(card.approval_id, PressDecision::Approve, "ops-1")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_OPERATOR_NOT_ALLOWED");
}

#[tokio::test]
async fn unknown_approval_id() {
    let f = fixture().await;
    let missing = ApprovalId::new();
    let errors = [
        f.service.get(missing).await.unwrap_err(),
        f.service.watch(missing).await.unwrap_err(),
        f.service.wait(missing, None).await.unwrap_err(),
    ];
    for error in errors {
        assert!(matches!(error, ApprovalError::NotFound));
        assert_eq!(error.code(), "APPR_NOT_FOUND");
        assert!(!error.retryable());
    }
    // D.2: an operator deciding an unknown approval.
    let agent = fixture_with(Some(policy(&[], &[], false))).await;
    let error = agent
        .service
        .decide_by_operator(missing, PressDecision::Approve, "ops-1")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "APPR_NOT_FOUND");
}

#[tokio::test]
async fn store_read_or_write_fails() {
    let f = fixture().await;
    let card = f.service.request(request()).await.unwrap();
    f.store.close().await;
    let errors = [
        f.service.request(request()).await.map(|_| ()).unwrap_err(),
        f.service
            .get(card.approval_id)
            .await
            .map(|_| ())
            .unwrap_err(),
        f.service
            .decide(approve(&card), PressOrigin::LocalScreen)
            .await
            .map(|_| ())
            .unwrap_err(),
        f.service
            .list_pending(None, None)
            .await
            .map(|_| ())
            .unwrap_err(),
    ];
    for error in errors {
        assert!(matches!(error, ApprovalError::Store(_)));
        assert_eq!(error.code(), "APPR_STORE_UNAVAILABLE");
        assert!(error.retryable());
        assert_eq!(error.repair(), Some(ordin_types::RepairAction::TryAgain));
    }
}

#[tokio::test]
async fn journal_emit_fails_twice() {
    let f = fixture().await;
    // Dropping the Journal's event table makes every append fail; the approval still succeeds.
    sqlx::query("DROP TABLE journal_events")
        .execute(f.pool())
        .await
        .unwrap();
    let card = f
        .service
        .request(request())
        .await
        .expect("a failed emit never fails the call");
    let outcome = f
        .service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .expect("a failed emit never fails the decision");
    assert_eq!(outcome.state, ApprovalState::Approved);
    let record = f.service.get(card.approval_id).await.unwrap();
    assert_eq!(record.state, ApprovalState::Approved);
}

#[tokio::test]
async fn withdraw_of_an_approval_the_person_already_decided() {
    let f = fixture().await;
    let card = f.service.request(request()).await.unwrap();
    f.service
        .decide(reject(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();
    let error = f.service.withdraw(card.approval_id).await.unwrap_err();
    match &error {
        ApprovalError::AlreadyDecided { state, decided_on } => {
            assert_eq!(*state, ApprovalState::Rejected);
            assert_eq!(decided_on, COMPUTER);
        }
        other => panic!("expected AlreadyDecided, got {other:?}"),
    }
    assert_eq!(error.code(), "APPR_CARD_ALREADY_DECIDED");
    assert!(!error.retryable());
}

#[tokio::test]
async fn withdraw_of_an_unknown_approval() {
    let f = fixture().await;
    let error = f.service.withdraw(ApprovalId::new()).await.unwrap_err();
    assert!(matches!(error, ApprovalError::NotFound));
    assert_eq!(error.code(), "APPR_NOT_FOUND");
}

#[test]
fn journal_variant_reports_the_journal_code() {
    let error = ApprovalError::Journal(ordin_journal::JournalError::Unavailable);
    assert_eq!(error.code(), "JRNL_DATASTORE_UNAVAILABLE");
    assert!(error.retryable());
    let error = ApprovalError::from(ordin_journal::JournalError::NotFound);
    assert!(!error.retryable());
}

// X4: ErrorHandling.md Internal Logic 11.
#[test]
fn every_owned_code_is_registered_and_the_prefix_is_owned() {
    assert!(!ALL_CODES.is_empty());
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-approval", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(owns_prefix("ordin-approval", prefix), "{prefix}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, ALL_CODES,
        "ALL_CODES is sorted and has no duplicates"
    );
}

// Every code the crate returns is in ALL_CODES, except the one `ordin-types` owns.
#[test]
fn every_returned_code_is_owned_here_or_is_the_cursor_code() {
    let errors = [
        ApprovalError::Invalid {
            code: "APPR_QUESTION_INVALID",
            field: "question",
        },
        ApprovalError::CardRefUnknown,
        ApprovalError::CardRefMismatch,
        ApprovalError::AlreadyDecided {
            state: ApprovalState::Approved,
            decided_on: "x".into(),
        },
        ApprovalError::CardClosed,
        ApprovalError::NotAnApprover,
        ApprovalError::AskerMayNotApprove,
        ApprovalError::OperatorNotAllowed,
        ApprovalError::NotFound,
        ApprovalError::Store(sqlx::Error::PoolClosed),
    ];
    for error in errors {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
    }
    assert_eq!(ApprovalError::BadCursor.code(), "SRV_BAD_CURSOR");
}
