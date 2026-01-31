//! Playbook §7.2 integration: an approval event reaches an `EventSink` subscriber once, in
//! sequence order, with the real Journal on a SQLite temporary file.

mod support;

use ordin_approval::PressOrigin;
use ordin_types::wire::approval::ApprovalAsker;
use ordin_types::RunId;
use support::*;

#[tokio::test]
async fn an_approval_event_reaches_a_subscriber_once_in_sequence_order() {
    let f = fixture().await;
    let sink = RecordingSink::new();
    f.journal.subscribe(sink.clone());
    let run_id = RunId::new();

    let card = f
        .service
        .request(request_for(workspace(), ApprovalAsker::Run { run_id }))
        .await
        .unwrap();
    f.service
        .decide(approve(&card), PressOrigin::LocalScreen)
        .await
        .unwrap();

    // approval_requested, needs_you_changed, approval_decided, needs_you_changed.
    eventually(|| sink.seen().len() == 4).await;
    let seen = sink.seen();
    let kinds: Vec<&str> = seen.iter().map(|(_, kind)| kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "approval_requested",
            "needs_you_changed",
            "approval_decided",
            "needs_you_changed"
        ]
    );
    // Strictly increasing and contiguous: each event arrived once, in order.
    let seqs: Vec<u64> = seen.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4]);
    // The subscriber saw exactly what the Journal kept, and nothing arrives later.
    let stored: Vec<(u64, String)> = f
        .events()
        .await
        .into_iter()
        .map(|event| (event.seq, event.kind))
        .collect();
    assert_eq!(stored, seen);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(sink.seen().len(), 4, "no event is delivered twice");

    // The requested event carries the whole card, so the screens need no second read.
    let requested = f.events_of("approval_requested").await;
    assert_eq!(requested[0].data["card"]["question"], card.question);
    assert_eq!(
        requested[0].data["card"]["buttons"]["approve"],
        ref_text(&card.buttons.approve)
    );
}

#[tokio::test]
async fn the_services_own_sink_and_a_second_subscriber_both_see_run_events_once() {
    let f = fixture().await;
    f.journal.subscribe(f.service.event_sink());
    let sink = RecordingSink::new();
    f.journal.subscribe(sink.clone());
    let run_id = RunId::new();
    let card = f
        .service
        .request(request_for(workspace(), ApprovalAsker::Run { run_id }))
        .await
        .unwrap();
    f.journal
        .append_event(
            "run_state_changed",
            serde_json::json!({ "run_id": run_id, "state": "failed" }),
        )
        .await
        .unwrap();
    // The approval's expiry is journalled by the service in answer to the run's event; the other
    // subscriber sees both, in sequence order, once each.
    eventually(|| {
        sink.seen()
            .iter()
            .filter(|(_, kind)| kind == "approval_decided")
            .count()
            == 1
    })
    .await;
    let seen = sink.seen();
    let seqs: Vec<u64> = seen.iter().map(|(seq, _)| *seq).collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(seqs, sorted, "in order, each once");
    let decided = f.events_of("approval_decided").await;
    assert_eq!(decided[0].data["approval_id"], card.approval_id.to_string());
    assert_eq!(decided[0].data["state"], "expired");
}
