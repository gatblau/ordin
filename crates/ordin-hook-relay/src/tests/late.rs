//! G. Late decisions: the watcher, the Journal sink, the stale-row clean-up and the resume errors.

use ordin_journal::JournalEvent;
use ordin_session_supervisor::ResumeDecision;
use ordin_types::domain::runs::ApprovalState;
use ordin_types::time::Timestamp;
use ordin_types::wire::hook::HarnessKind;
use ordin_types::ApprovalId;
use serde_json::json;

use super::*;
use crate::store::{self, LateKind, LateRow};

async fn held_past_deadline(rig: &Rig, opened: &Opened) -> ApprovalId {
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let replies = rig.call(opened, pre("Bash", None)).await;
    assert_eq!(
        deny_reason(&replies[1]),
        "waiting for the person's approval"
    );
    approval
}

#[tokio::test]
async fn a_late_rejection_tells_the_model_the_person_declined() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = held_past_deadline(&rig, &opened).await;
    rig.approvals.decide(approval, ApprovalState::Rejected);
    eventually("the run is resumed", || {
        !rig.supervisor.resumes.lock().unwrap().is_empty()
    })
    .await;
    let resumes = rig.supervisor.resumes.lock().unwrap().clone();
    match &resumes[0].1 {
        ResumeDecision::Reject {
            approval_id,
            reason,
        } => {
            assert_eq!(*approval_id, approval);
            assert_eq!(reason.as_deref(), Some("The person chose not to do this."));
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
    wait_for_rows(&rig, 0).await;
}

#[tokio::test]
async fn a_late_expiry_resumes_nothing_and_the_row_goes() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = held_past_deadline(&rig, &opened).await;
    rig.approvals.decide(approval, ApprovalState::Expired);
    wait_for_rows(&rig, 0).await;
    assert!(rig.supervisor.resumes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_resume_error_is_logged_and_the_row_still_goes() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = held_past_deadline(&rig, &opened).await;
    rig.supervisor
        .fail_resume
        .store(true, std::sync::atomic::Ordering::SeqCst);
    rig.approvals.decide(approval, ApprovalState::Approved);
    wait_for_rows(&rig, 0).await;
    assert!(rig.supervisor.resumes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_late_stop_gate_rejection_settles_the_gates_before_the_resume() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    *rig.rules.end.lock().unwrap() = EndScript::Ask(approval);
    let _ = rig.call(&opened, stop()).await;
    rig.approvals.decide(approval, ApprovalState::Rejected);
    eventually("the run is resumed", || {
        !rig.supervisor.resumes.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(
        *rig.rules.settled.lock().unwrap(),
        vec![(opened.run_id, false)]
    );
}

#[tokio::test]
async fn the_journal_sink_settles_a_row_whose_watcher_is_gone() {
    // After a restart the sink also delivers `approval_decided`: an id in the table triggers G.1 directly.
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    store::insert(
        &rig.store,
        &LateRow {
            approval_id: approval,
            run_id: opened.run_id,
            kind: LateKind::ToolCall,
        },
        Timestamp::now(),
    )
    .await
    .unwrap();
    let sink = rig.relay.event_sink();
    assert_eq!(sink.name(), "hook_relay");
    let event = |kind: &str, id: String, state: &str| JournalEvent {
        backend: uuid::Uuid::now_v7(),
        seq: 1,
        at: Timestamp::now(),
        kind: kind.to_owned(),
        data: json!({"approval_id": id, "state": state}),
    };
    // Other events, an unknown approval, an unknown state and a malformed id change nothing.
    sink.deliver(&event(
        "run_state_changed",
        approval.to_string(),
        "approved",
    ))
    .await;
    sink.deliver(&event(
        "approval_decided",
        ApprovalId::new().to_string(),
        "approved",
    ))
    .await;
    sink.deliver(&event("approval_decided", approval.to_string(), "pending"))
        .await;
    sink.deliver(&event("approval_decided", "nope".into(), "approved"))
        .await;
    assert!(rig.supervisor.resumes.lock().unwrap().is_empty());
    assert_eq!(rig.late_rows().await, 1);
    sink.deliver(&event("approval_decided", approval.to_string(), "approved"))
        .await;
    assert_eq!(rig.supervisor.resumes.lock().unwrap().len(), 1);
    assert_eq!(rig.late_rows().await, 0);
    // A second delivery finds no row and resumes nothing more.
    sink.deliver(&event("approval_decided", approval.to_string(), "approved"))
        .await;
    assert_eq!(rig.supervisor.resumes.lock().unwrap().len(), 1);
    sink.lagged(5).await;
}

#[tokio::test]
async fn rows_older_than_thirty_days_are_deleted_at_start() {
    let rig = Rig::new().await;
    let (old, young) = (ApprovalId::new(), ApprovalId::new());
    let thirty_one_days = 31 * 24 * 3_600 * 1_000;
    for (id, age) in [(old, thirty_one_days), (young, 1_000)] {
        let at = Timestamp::from_unix_millis(Timestamp::now().unix_millis() - age).unwrap();
        store::insert(
            &rig.store,
            &LateRow {
                approval_id: id,
                run_id: ordin_types::RunId::new(),
                kind: LateKind::ToolCall,
            },
            at,
        )
        .await
        .unwrap();
    }
    rig.relay.start().await.expect("start");
    let rows = store::list(&rig.store).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].approval_id, young);
    // The surviving row has a watcher again (A.3).
    eventually("a watcher waits on the young row", || {
        rig.approvals.waits.lock().unwrap().contains(&young)
    })
    .await;
    rig.relay.shutdown().await;
}

#[tokio::test]
async fn a_row_is_written_once_and_read_back() {
    let rig = Rig::new().await;
    let id = ApprovalId::new();
    let row = LateRow {
        approval_id: id,
        run_id: ordin_types::RunId::new(),
        kind: LateKind::StopGate,
    };
    store::insert(&rig.store, &row, Timestamp::now())
        .await
        .unwrap();
    store::insert(&rig.store, &row, Timestamp::now())
        .await
        .unwrap();
    let got = store::get(&rig.store, id).await.unwrap().expect("the row");
    assert_eq!(got.run_id, row.run_id);
    assert_eq!(got.kind, LateKind::StopGate);
    assert_eq!(rig.late_rows().await, 1);
    store::delete(&rig.store, id).await.unwrap();
    assert!(store::get(&rig.store, id).await.unwrap().is_none());
}

#[tokio::test]
async fn the_table_refuses_a_kind_outside_the_two() {
    let rig = Rig::new().await;
    let bad = sqlx::query(
        "INSERT INTO hook_late_decisions (approval_id, run_id, kind, created_at) VALUES (?1, ?2, 'other', 'now')",
    )
    .bind(ApprovalId::new().to_string())
    .bind(ordin_types::RunId::new().to_string())
    .execute(rig.store.sqlite().unwrap())
    .await;
    assert!(bad.is_err());
    let short = sqlx::query(
        "INSERT INTO hook_late_decisions (approval_id, run_id, kind, created_at) VALUES ('x', 'y', 'tool_call', 'now')",
    )
    .execute(rig.store.sqlite().unwrap())
    .await;
    assert!(short.is_err());
}

#[test]
fn the_module_registers_one_step_for_each_engine() {
    let migrations = HookRelay::migrations();
    assert_eq!(migrations.module, "hook_relay");
    assert_eq!(HookRelay::MODULE, "hook_relay");
    assert_eq!(migrations.sqlite.len(), 1);
    assert_eq!(migrations.postgres.len(), 1);
    assert!(migrations.sqlite[0].2.contains("hook_late_decisions"));
    assert!(migrations.postgres[0].2.contains("timestamptz"));
    for sql in [migrations.sqlite[0].2, migrations.postgres[0].2] {
        for forbidden in ["secret", "payload", "arguments"] {
            assert!(!sql.contains(forbidden), "the table holds no {forbidden}");
        }
    }
}
