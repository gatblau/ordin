//! The `[pg]` scenarios of RunQueue.md on PostgreSQL 18, and the parity scenario. One container for
//! this binary, a copy of the migrated template per test. The paused-container scenario lives in
//! `pg_unavailable.rs` (its own container).

mod support;

use ordin_run_queue::RunQueue;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn fifty_instances_take_one_queued_run_and_exactly_one_receives_it() {
    let h = Harness::postgres().await;
    scenarios::fifty_instances_take_one_queued_run(&h).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_renewal_and_the_interruption_of_the_same_run_have_one_outcome() {
    let h = Harness::postgres().await;
    scenarios::a_renewal_and_the_interruption_have_one_outcome(&h).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_firing_and_its_run_are_recorded_once_across_fifty_instances() {
    let h = Harness::postgres().await;
    scenarios::a_firing_and_its_run_are_recorded_once_across_fifty_instances(&h).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn parity_sqlite_and_postgresql_return_the_same_values() {
    let sqlite = Harness::sqlite().await;
    let postgres = Harness::postgres().await;
    assert_eq!(sqlite.which, Which::Sqlite);
    assert_eq!(postgres.which, Which::Postgres);
    let from_sqlite = scenarios::parity_transcript(&sqlite).await;
    let from_postgres = scenarios::parity_transcript(&postgres).await;
    assert!(from_sqlite.len() > 40, "the parity body is not trivial");
    for (n, (a, b)) in from_sqlite.iter().zip(&from_postgres).enumerate() {
        assert_eq!(
            a, b,
            "line {n} differs between SqliteRunStore and PgRunStore"
        );
    }
    assert_eq!(from_sqlite.len(), from_postgres.len());
}

/// The single-statement guarantees that matter on the scaled shape, beyond the five scenarios: a
/// run is queued, taken, paused and resumed on PostgreSQL with the same values as on SQLite.
#[tokio::test(flavor = "multi_thread")]
async fn a_paused_run_carries_its_session_record_on_postgresql() {
    let h = Harness::postgres().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    q.pause(
        &run.run_id,
        "runs-a",
        pause_with(vec![tool_pending("create_ticket")], b"S1"),
    )
    .await
    .unwrap();
    q.resume(
        &run.run_id,
        serde_json::json!({"decisions": [{"type": "approve"}], "set_aside": []}),
    )
    .await
    .unwrap();
    let next = q.take("runs-b").await.unwrap().unwrap();
    assert_eq!(next.view.record.run_id, run.run_id);
    assert_eq!(next.view.record.started_at, run.started_at);
    assert_eq!(next.session_record.as_deref(), Some(&b"S1"[..]));
    assert_eq!(h.which, Which::Postgres);
}
