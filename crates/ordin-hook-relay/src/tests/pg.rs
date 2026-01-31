//! The PostgreSQL slice of the module on PostgreSQL 18 (plan 01, "PostgreSQL 18 in tests"): the
//! crate's own statements against the scaled shape of `hook_late_decisions`, and a late decision
//! end to end. One container for this test binary; each test copies the migrated template.
//! The helper is `tests/support/pg.rs`, shared by file with the migration test.

#[path = "../../tests/support/pg.rs"]
mod support;

use std::time::Duration;

use ordin_journal::{Journal, JournalOptions, SystemClock};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::time::Timestamp;
use ordin_types::wire::hook::HarnessKind;
use ordin_types::{ApprovalId, RunId};

use super::*;
use crate::store::{self, LateKind, LateRow};

async fn pg_store() -> Datastore {
    support::open_url(support::migrated_database().await, 10).await
}

fn row(kind: LateKind) -> LateRow {
    LateRow {
        approval_id: ApprovalId::new(),
        run_id: RunId::new(),
        kind,
    }
}

#[tokio::test]
async fn the_statements_run_on_postgresql() {
    let store = pg_store().await;
    let (tool, gate) = (row(LateKind::ToolCall), row(LateKind::StopGate));
    store::insert(&store, &tool, Timestamp::now())
        .await
        .unwrap();
    store::insert(&store, &tool, Timestamp::now())
        .await
        .unwrap();
    let thirty_one_days = Timestamp::now().unix_millis() - 31 * 24 * 3_600 * 1_000;
    store::insert(
        &store,
        &gate,
        Timestamp::from_unix_millis(thirty_one_days).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(store::list(&store).await.unwrap().len(), 2);
    let got = store::get(&store, tool.approval_id)
        .await
        .unwrap()
        .expect("row");
    assert_eq!((got.run_id, got.kind), (tool.run_id, LateKind::ToolCall));
    let cutoff =
        Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 30 * 24 * 3_600 * 1_000)
            .unwrap();
    assert_eq!(store::delete_older_than(&store, cutoff).await.unwrap(), 1);
    assert!(store::get(&store, gate.approval_id)
        .await
        .unwrap()
        .is_none());
    store::delete(&store, tool.approval_id).await.unwrap();
    assert!(store::list(&store).await.unwrap().is_empty());
    store.close().await;
}

#[tokio::test]
async fn a_late_decision_resumes_the_run_on_postgresql() {
    let dir = TempDir::new();
    let store = pg_store().await;
    let journal = Arc::new(
        Journal::start(
            store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal on PostgreSQL"),
    );
    let rig = Rig::over(
        dir,
        store.clone(),
        journal,
        fast(),
        ScriptedApprovals::new(),
        ScriptedSupervisor::new(),
    );
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(
        deny_reason(&replies[1]),
        "waiting for the person's approval"
    );
    assert!(store::get(&store, approval).await.unwrap().is_some());
    rig.approvals.decide(approval, ApprovalState::Approved);
    eventually("the run is resumed", || {
        !rig.supervisor.resumes.lock().unwrap().is_empty()
    })
    .await;
    for _ in 0..250 {
        if store::get(&store, approval).await.unwrap().is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(store::get(&store, approval).await.unwrap().is_none());
    // The refusal entry reaches the PostgreSQL journal too.
    let request = signed(&opened, ordin_types::HookCallId::new(), pre("Read", None));
    let mut bad = request.clone();
    bad.mac = mac_of(&[3u8; 32], &bad.message);
    let mut bytes = serde_json::to_vec(&bad).unwrap();
    bytes.push(b'\n');
    let _ = rig.send(&bytes).await;
    assert_eq!(rig.refusals().await.len(), 1);
    store.close().await;
}
