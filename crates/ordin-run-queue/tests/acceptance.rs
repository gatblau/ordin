//! X2: one test per Gherkin scenario of RunQueue.md, named after it, on SQLite. The `[pg]`
//! scenarios run here on SQLite and in `pg_scenarios.rs` / `pg_unavailable.rs` on PostgreSQL 18.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ordin_run_queue::{
    CancelOutcome, Delivery, LeaseRenewal, NewRun, RunEnd, RunEventKind, RunOutcome, RunQueue,
};
use ordin_types::domain::agent_profile::{RunEndKind, RunRecordState, RunRequest, RunStarter};
use ordin_types::time::Timestamp;
use ordin_types::ApprovalId;
use support::*;

async fn pause_briefly() {
    // Timestamps have millisecond precision; ordering by `queued_at` needs distinct milliseconds.
    tokio::time::sleep(Duration::from_millis(5)).await;
}

fn states(events: &[ordin_run_queue::StoredRunEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.kind == RunEventKind::RunStateChanged)
        .map(|e| e.data["state"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn happy_path_a_run_is_recorded_taken_ended_and_read_back() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let record = enqueue(q, go_no_go()).await;
    assert_eq!(record.state, RunRecordState::Queued);
    assert_eq!(record.id.0.hyphenated().to_string(), record.run_id);
    let taken = q.take("runs-a").await.unwrap().unwrap();
    assert_eq!(taken.view.record.state, RunRecordState::Executing);
    assert_eq!(taken.view.record.lease_holder.as_deref(), Some("runs-a"));
    assert!(taken.decision.is_none() && taken.session_record.is_none());
    q.end(&record.run_id, "runs-a", completed("Go."))
        .await
        .unwrap();

    let view = q.get(&record.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Ended);
    assert_eq!(view.record.end_kind, Some(RunEndKind::Completed));
    assert_eq!(view.record.result.as_deref(), Some("Go."));
    assert!(view.record.started_at.is_some() && view.record.ended_at.is_some());
    assert_eq!(view.created_at, view.record.created_at);
    assert_eq!(view.started_at, view.record.started_at);
    assert_eq!(view.ended_at, view.record.ended_at);
    assert!(view.record.lease_holder.is_none() && view.record.lease_expires_at.is_none());
    assert_eq!(view.outcome.unwrap().reply.as_deref(), Some("Go."));

    let events = q.events_after(&record.run_id, 0, 10).await.unwrap();
    assert_eq!(states(&events), ["queued", "executing", "finished"]);
    assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3]);
    let m = &h.metrics;
    assert_eq!(
        m.total("wrightrun_runs_queued_total", &[("started_by", "caller")]),
        1
    );
    assert_eq!(
        m.total("wrightrun_runs_started_total", &[("started_by", "caller")]),
        1
    );
    assert_eq!(
        m.total("wrightrun_runs_ended_total", &[("state", "finished")]),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_a_control_ended_is_recorded_with_the_end_kind_control() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    let end = RunEnd {
        end_kind: RunEndKind::Completed,
        outcome: RunOutcome {
            ended_by_control: true,
            ..Default::default()
        },
        stretch_summary: None,
    };
    q.end(&run.run_id, "runs-a", end).await.unwrap();
    let view = q.get(&run.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Ended);
    assert_eq!(view.record.end_kind, Some(RunEndKind::Control));
    let events = q.events_after(&run.run_id, 0, 10).await.unwrap();
    assert_eq!(
        states(&events).last().map(String::as_str),
        Some("ended-by-control")
    );
    assert_eq!(
        h.metrics.total(
            "wrightrun_runs_ended_total",
            &[("state", "ended-by-control")]
        ),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_the_end_kind_control_without_a_control_ending() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    let end = RunEnd {
        end_kind: RunEndKind::Control,
        outcome: RunOutcome::default(),
        stretch_summary: None,
    };
    match q.end(&run.run_id, "runs-a", end).await {
        Err(ordin_run_queue::RunQueueError::BadInput { field }) => assert_eq!(field, "end_kind"),
        other => panic!("{other:?}"),
    }
    let view = q.get(&run.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Executing);
}

#[tokio::test(flavor = "multi_thread")]
async fn fifty_instances_take_one_queued_run_and_exactly_one_receives_it() {
    let h = Harness::sqlite().await;
    scenarios::fifty_instances_take_one_queued_run(&h).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_are_taken_oldest_first_and_a_resumed_run_joins_the_end_of_the_line() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let a = enqueue(q, go_no_go()).await;
    pause_briefly().await;
    let b = enqueue(q, go_no_go()).await;
    pause_briefly().await;
    let c = enqueue(q, go_no_go()).await;
    pause_briefly().await;
    let first = q.take("runs-a").await.unwrap().unwrap();
    assert_eq!(first.view.record.run_id, a.run_id);
    q.pause(
        &a.run_id,
        "runs-a",
        pause_with(vec![tool_pending("t")], b"S"),
    )
    .await
    .unwrap();
    q.resume(&a.run_id, serde_json::json!({"decisions": []}))
        .await
        .unwrap();
    let order: Vec<String> = [
        q.take("runs-a").await.unwrap().unwrap(),
        q.take("runs-a").await.unwrap().unwrap(),
        q.take("runs-a").await.unwrap().unwrap(),
    ]
    .into_iter()
    .map(|t| t.view.record.run_id)
    .collect();
    assert_eq!(order, [b.run_id, c.run_id, a.run_id]);
    assert!(q.take("runs-a").await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_run_carries_its_session_record_to_the_next_instance() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    q.pause(
        &run.run_id,
        "runs-a",
        pause_with(vec![tool_pending("create_ticket")], b"S1"),
    )
    .await
    .unwrap();
    let paused = q.get(&run.run_id).await.unwrap().unwrap();
    assert_eq!(paused.record.state, RunRecordState::Paused);
    assert!(paused.record.lease_holder.is_none() && paused.record.lease_expires_at.is_none());
    let decision = serde_json::json!({"decisions": [{"type": "approve"}], "set_aside": []});
    q.resume(&run.run_id, decision.clone()).await.unwrap();
    assert_eq!(
        q.decision(&run.run_id).await.unwrap(),
        Some(decision.clone())
    );
    let next = q.take("runs-b").await.unwrap().unwrap();
    assert_eq!(next.view.record.run_id, run.run_id);
    assert_eq!(next.view.record.started_at, run.started_at);
    assert_eq!(next.decision, Some(decision));
    assert_eq!(next.session_record.as_deref(), Some(&b"S1"[..]));
    assert_eq!(next.view.record.lease_holder.as_deref(), Some("runs-b"));
    // Paused again, the decision is cleared.
    q.pause(
        &run.run_id,
        "runs-b",
        pause_with(vec![tool_pending("t")], b"S2"),
    )
    .await
    .unwrap();
    assert_eq!(q.decision(&run.run_id).await.unwrap(), None);
    assert_eq!(
        h.metrics.total(
            "wrightrun_tool_calls_paused_for_approval_total",
            &[("tool", "create_ticket")]
        ),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_item_keeps_its_approval_id_across_pause_and_get() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    let id = ApprovalId::parse("0192a1c3-4455-7667-8889-aabbccddeeff").unwrap();
    let mut item = tool_pending("create_ticket");
    item.approval_id = Some(id);
    q.pause(&run.run_id, "runs-a", pause_with(vec![item], b"S"))
        .await
        .unwrap();
    let view = q.get(&run.run_id).await.unwrap().unwrap();
    assert_eq!(view.outcome.unwrap().pending[0].approval_id, Some(id));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_ending_cannot_carry_an_approval_id() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    let item = ordin_run_queue::PendingItem {
        kind: ordin_run_queue::PendingKind::RefusedEnding,
        tool: None,
        gate: None,
        reason: "refused-ending".into(),
        approval_id: Some(ApprovalId::new()),
    };
    match q
        .pause(&run.run_id, "runs-a", pause_with(vec![item], b"S"))
        .await
    {
        Err(ordin_run_queue::RunQueueError::BadInput { field }) => {
            assert_eq!(field, "pending.approval_id")
        }
        other => panic!("{other:?}"),
    }
    // Without the id the same item is accepted.
    let item = ordin_run_queue::PendingItem {
        kind: ordin_run_queue::PendingKind::RefusedEnding,
        tool: None,
        gate: None,
        reason: "refused-ending".into(),
        approval_id: None,
    };
    q.pause(&run.run_id, "runs-a", pause_with(vec![item], b"S"))
        .await
        .unwrap();
    assert_eq!(
        h.metrics
            .total("wrightrun_tool_calls_paused_for_approval_total", &[]),
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_whose_lease_lapses_is_recorded_as_interrupted_and_never_taken_again() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    h.lapse_lease(&run.run_id).await;
    let closed = q.mark_interrupted().await.unwrap();
    assert_eq!(closed, vec![run.run_id.clone()]);
    let view = q.get(&run.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Interrupted);
    assert!(view.record.ended_at.is_some());
    assert_eq!(
        view.record.lease_holder.as_deref(),
        Some("runs-a"),
        "the instance name is kept"
    );
    assert!(view.record.lease_expires_at.is_none());
    assert!(q.take("runs-b").await.unwrap().is_none());
    assert_eq!(h.metrics.total("wrightrun_runs_interrupted_total", &[]), 1);
    let events = q.events_after(&run.run_id, 0, 10).await.unwrap();
    assert_eq!(
        states(&events).last().map(String::as_str),
        Some("interrupted")
    );
    // Nothing more to close.
    assert!(q.mark_interrupted().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_renewal_and_the_interruption_of_the_same_run_have_one_outcome() {
    let h = Harness::sqlite().await;
    scenarios::a_renewal_and_the_interruption_have_one_outcome(&h).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_an_instance_that_lost_its_lease_cannot_end_the_run() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    h.lapse_lease(&run.run_id).await;
    q.mark_interrupted().await.unwrap();
    let error = q
        .end(&run.run_id, "runs-a", completed("late"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "RUN_LEASE_LOST");
    let view = q.get(&run.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Interrupted);
    assert_eq!(
        q.renew(&run.run_id, "runs-a").await.unwrap(),
        LeaseRenewal::Lost
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_a_second_run_on_a_thread_whose_run_has_not_ended() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    q.pause(
        &run.run_id,
        "runs-a",
        pause_with(vec![tool_pending("t")], b"S"),
    )
    .await
    .unwrap();
    let on_thread = || NewRun {
        thread_id: Some(run.thread_id.clone()),
        ..go_no_go()
    };
    assert_eq!(
        q.enqueue(on_thread()).await.unwrap_err().code(),
        "RUN_THREAD_BUSY"
    );
    match q.cancel(&run.run_id).await.unwrap() {
        CancelOutcome::Cancelled(view) => assert_eq!(view.record.state, RunRecordState::Cancelled),
        other => panic!("{other:?}"),
    }
    let second = q.enqueue(on_thread()).await.unwrap();
    assert_eq!(second.thread_id, run.thread_id);
    assert_ne!(second.run_id, run.run_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_a_run_on_a_thread_that_does_not_exist() {
    let h = Harness::sqlite().await;
    let error = h
        .queue
        .enqueue(NewRun {
            thread_id: Some("0192a000-0000-7000-8000-000000000000".into()),
            ..go_no_go()
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "RUN_THREAD_NOT_FOUND");
    assert_eq!(h.count("run_records").await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_two_resumes_at_the_same_instant() {
    let h = Harness::sqlite().await;
    let run = taken(&h.queue, "runs-a").await;
    h.queue
        .pause(
            &run.run_id,
            "runs-a",
            pause_with(vec![tool_pending("t")], b"S"),
        )
        .await
        .unwrap();
    let (queue, _) = h.instance().await;
    let queue = Arc::new(queue);
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let (queue, barrier, id) = (queue.clone(), barrier.clone(), run.run_id.clone());
            tokio::spawn(async move {
                barrier.wait().await;
                queue
                    .resume(&id, serde_json::json!({"decisions": []}))
                    .await
            })
        })
        .collect();
    let (mut won, mut refused) = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            Ok(record) => {
                assert_eq!(record.state, RunRecordState::Queued);
                won += 1;
            }
            Err(error) => {
                assert_eq!(error.code(), "RUN_NOT_PAUSED");
                refused += 1;
            }
        }
    }
    assert_eq!((won, refused), (1, 9));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_a_queued_run_and_request_the_cancel_of_an_executing_run() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let e = taken(q, "runs-a").await;
    let queued = enqueue(q, go_no_go()).await;
    match q.cancel(&queued.run_id).await.unwrap() {
        CancelOutcome::Cancelled(view) => assert_eq!(view.record.state, RunRecordState::Cancelled),
        other => panic!("{other:?}"),
    }
    let view = q.get(&queued.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Cancelled);
    assert!(view.record.ended_at.is_some());
    match q.cancel(&e.run_id).await.unwrap() {
        CancelOutcome::Requested(view) => {
            assert_eq!(view.record.state, RunRecordState::Executing);
            assert!(view.cancel_requested);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        q.renew(&e.run_id, "runs-a").await.unwrap(),
        LeaseRenewal::CancelRequested
    );
    // The lease was extended by that renewal.
    assert!(q
        .get(&e.run_id)
        .await
        .unwrap()
        .unwrap()
        .record
        .lease_expires_at
        .is_some());
    let outcome = RunOutcome {
        ending: Some("cancelled".into()),
        ..Default::default()
    };
    q.record_cancelled(&e.run_id, "runs-a", outcome.clone())
        .await
        .unwrap();
    let view = q.get(&e.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Cancelled);
    assert!(view.record.ended_at.is_some());
    assert!(view.record.lease_holder.is_none());
    assert_eq!(view.outcome, Some(outcome));
    assert_eq!(
        h.metrics
            .total("wrightrun_runs_ended_total", &[("state", "cancelled")]),
        2
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_cancel_of_an_ended_run() {
    let h = Harness::sqlite().await;
    let run = taken(&h.queue, "runs-a").await;
    h.queue
        .end(&run.run_id, "runs-a", completed("Go."))
        .await
        .unwrap();
    assert_eq!(
        h.queue.cancel(&run.run_id).await.unwrap_err().code(),
        "RUN_NOT_CANCELLABLE"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn edge_case_an_executing_run_asked_to_cancel_whose_instance_vanished_is_recorded_as_cancelled(
) {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let e = taken(q, "runs-a").await;
    q.cancel(&e.run_id).await.unwrap();
    h.lapse_lease(&e.run_id).await;
    let closed = q.mark_interrupted().await.unwrap();
    assert_eq!(closed, vec![e.run_id.clone()]);
    let view = q.get(&e.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.state, RunRecordState::Cancelled);
    assert!(view.record.ended_at.is_some());
    assert_eq!(h.metrics.total("wrightrun_runs_interrupted_total", &[]), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_firing_and_its_run_are_recorded_once_across_fifty_instances() {
    let h = Harness::sqlite().await;
    scenarios::a_firing_and_its_run_are_recorded_once_across_fifty_instances(&h).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_a_firing_with_an_empty_entry_key_writes_nothing() {
    let h = Harness::sqlite().await;
    let due = Timestamp::now();
    let error = h
        .queue
        .enqueue_firing("", due, agent_wide(), "run-triage", "queue=support")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "RUN_BAD_INPUT");
    assert_eq!(h.count("run_records").await, 0);
    assert_eq!(h.count("run_threads").await, 0);
    assert_eq!(h.count("run_events").await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paused_run_with_a_duty_s_command_is_found_the_calling_run_excluded() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let duty = || new_run(caller("svc"), command_request("watch-readiness", "x"));
    let p = enqueue(q, duty()).await;
    q.take("runs-a").await.unwrap().unwrap();
    q.pause(
        &p.run_id,
        "runs-a",
        pause_with(vec![tool_pending("t")], b"S"),
    )
    .await
    .unwrap();
    let qr = enqueue(q, duty()).await;
    q.take("runs-a").await.unwrap().unwrap();
    let commands = ["watch-readiness".to_owned()];
    assert!(!q.paused_with_command(&commands, &p.run_id).await.unwrap());
    assert!(q
        .paused_with_command(&["watch-readiness".into(), "triage".into()], &qr.run_id)
        .await
        .unwrap());
    // An except id that is not a UUID excludes nothing.
    assert!(q
        .paused_with_command(&commands, "not-a-uuid")
        .await
        .unwrap());
    assert!(!q
        .paused_with_command(&["triage".into()], &qr.run_id)
        .await
        .unwrap());
    // An empty list never reaches the datastore: it answers even when the datastore is closed.
    h.datastore.close().await;
    assert!(!q.paused_with_command(&[], &qr.run_id).await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn progress_lines_are_kept_in_order_and_bounded() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    assert!(q
        .append_progress(&run.run_id, "runs-a", "Reading 3 tickets")
        .await
        .unwrap());
    assert!(q
        .append_progress(&run.run_id, "runs-a", &"x".repeat(400))
        .await
        .unwrap());
    let lines: Vec<String> = q
        .events_after(&run.run_id, 0, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == RunEventKind::RunProgress)
        .map(|e| e.data["line"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "Reading 3 tickets");
    assert_eq!(lines[1].chars().count(), 160);
    assert_eq!(
        q.append_progress(&run.run_id, "runs-b", "nope")
            .await
            .unwrap_err()
            .code(),
        "RUN_LEASE_LOST"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_deletes_a_thread_only_when_all_its_runs_ended_before_the_cutoff() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    async fn end(h: &Harness, id: String) {
        h.queue.end(&id, "runs-a", completed("done")).await.unwrap();
        h.age_end(&id).await;
    }
    // T1: two runs, both ended 100 days ago.
    let t1a = taken(q, "runs-a").await;
    end(&h, t1a.run_id.clone()).await;
    let t1b = q
        .enqueue(NewRun {
            thread_id: Some(t1a.thread_id.clone()),
            ..go_no_go()
        })
        .await
        .unwrap();
    q.take("runs-a").await.unwrap().unwrap();
    end(&h, t1b.run_id.clone()).await;
    // T2: one run ended 100 days ago, a second one paused.
    let t2a = taken(q, "runs-a").await;
    end(&h, t2a.run_id.clone()).await;
    let t2b = q
        .enqueue(NewRun {
            thread_id: Some(t2a.thread_id.clone()),
            ..go_no_go()
        })
        .await
        .unwrap();
    q.take("runs-a").await.unwrap().unwrap();
    q.pause(
        &t2b.run_id,
        "runs-a",
        pause_with(vec![tool_pending("t")], b"S"),
    )
    .await
    .unwrap();
    // T3: one run that ended just now.
    let t3 = taken(q, "runs-a").await;
    q.end(&t3.run_id, "runs-a", completed("done"))
        .await
        .unwrap();

    let cutoff =
        Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 90 * 86_400_000).unwrap();
    let counted = q.count_ended_before(cutoff, 500).await.unwrap();
    assert_eq!(counted.runs, 2);
    assert_eq!(h.count("run_records").await, 5, "a count deletes nothing");
    let deleted = q.delete_ended_before(cutoff, 500).await.unwrap();
    assert_eq!(deleted, counted);
    assert_eq!(deleted.runs, 2);
    assert_eq!(deleted.thread_ids, vec![t1a.thread_id.clone()]);
    let mut ids = deleted.run_ids.clone();
    ids.sort();
    let mut expected = vec![t1a.run_id.clone(), t1b.run_id.clone()];
    expected.sort();
    assert_eq!(ids, expected);
    assert!(q.get(&t1a.run_id).await.unwrap().is_none());
    for id in [&t2a.run_id, &t2b.run_id, &t3.run_id] {
        assert!(q.get(id).await.unwrap().is_some(), "{id}");
    }
    // The events of a deleted run went with it.
    assert!(q.events_after(&t1a.run_id, 0, 10).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn erasure_by_person_deletes_what_a_chat_user_started_and_nothing_a_caller_started() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let chat = |asker: &str| {
        new_run(
            RunStarter::ChatConnection {
                connection: "team-slack".into(),
                asker: asker.into(),
            },
            RunRequest::Message {
                text: "hello".into(),
            },
        )
    };
    let mine = enqueue(q, chat("U024BE7LH")).await;
    let other = enqueue(q, chat("U999")).await;
    let callers = enqueue(
        q,
        new_run(
            caller("U024BE7LH"),
            RunRequest::Message { text: "hi".into() },
        ),
    )
    .await;

    let counted = q
        .count_started_by_asker("team-slack", "U024BE7LH")
        .await
        .unwrap();
    assert_eq!(counted.runs, 1);
    for run in [&mine, &other, &callers] {
        assert!(q.get(&run.run_id).await.unwrap().is_some());
    }
    let deleted = q
        .delete_started_by_asker("team-slack", "U024BE7LH")
        .await
        .unwrap();
    assert_eq!(deleted, counted);
    assert_eq!(deleted.run_ids, vec![mine.run_id.clone()]);
    assert!(q.get(&mine.run_id).await.unwrap().is_none());
    assert!(q.get(&other.run_id).await.unwrap().is_some());
    assert!(q.get(&callers.run_id).await.unwrap().is_some());
    let text = logs().text();
    assert!(!text.contains("U024BE7LH"), "the asker is never logged");
    assert!(logs()
        .lines()
        .iter()
        .any(|l| l["event"] == "runs_deleted" && l["reason"] == "erase-person"));
    assert_eq!(q.thread_owner(&mine.thread_id).await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn error_path_a_cursor_of_another_listing() {
    let h = Harness::sqlite().await;
    let cursor = serde_json::to_value(ordin_types::Cursor::encode(&["abc"])).unwrap();
    let error = h.queue.list(None, cursor.as_str()).await.unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
}

#[test]
fn delivery_serialises_in_snake_case() {
    assert_eq!(
        serde_json::to_string(&Delivery::Asker).unwrap(),
        "\"asker\""
    );
}
