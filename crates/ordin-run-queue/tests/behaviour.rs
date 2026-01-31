//! Interface rules and Internal Logic steps that no single scenario pins down, on SQLite.

mod support;

use ordin_run_queue::{
    CancelOutcome, InProgressFilter, LeaseRenewal, NewRun, RunEnd, RunEventKind, RunOutcome,
    RunQueue, MAX_EVENTS_PER_RUN,
};
use ordin_types::domain::agent_profile::{RunEndKind, RunRecordState, RunRequest, RunStarter};
use ordin_types::time::Timestamp;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn end_writes_each_wire_state_and_the_summary() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    for (kind, wire) in [
        (RunEndKind::Completed, "finished"),
        (RunEndKind::Failed, "failed"),
        (RunEndKind::RunLimit, "ended-at-limit"),
    ] {
        let run = taken(q, "runs-a").await;
        let end = RunEnd {
            end_kind: kind,
            outcome: RunOutcome {
                error_code: Some("RUN_LIMIT".into()),
                safe_reply: true,
                delivery: Some(ordin_run_queue::Delivery::Thread),
                ..Default::default()
            },
            stretch_summary: Some(format!("summary of {wire}")),
        };
        q.end(&run.run_id, "runs-a", end).await.unwrap();
        let view = q.get(&run.run_id).await.unwrap().unwrap();
        assert_eq!(view.record.end_kind, Some(kind));
        assert_eq!(
            view.record.stretch_summaries,
            [format!("summary of {wire}")]
        );
        let events = q.events_after(&run.run_id, 0, 100).await.unwrap();
        assert_eq!(events.last().unwrap().data["state"], wire);
        assert_eq!(
            h.metrics
                .total("wrightrun_runs_ended_total", &[("state", wire)]),
            1
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stretch_summaries_accumulate_across_a_pause() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    let mut pause = pause_with(vec![tool_pending("t")], b"S");
    pause.stretch_summary = Some("first".into());
    q.pause(&run.run_id, "runs-a", pause).await.unwrap();
    q.resume(&run.run_id, serde_json::json!({})).await.unwrap();
    q.take("runs-a").await.unwrap().unwrap();
    let mut end = completed("done");
    end.stretch_summary = Some("second".into());
    q.end(&run.run_id, "runs-a", end).await.unwrap();
    let view = q.get(&run.run_id).await.unwrap().unwrap();
    assert_eq!(view.record.stretch_summaries, ["first", "second"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_on_an_existing_thread_keeps_its_own_starter_and_the_thread_keeps_its_first() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let first = enqueue(q, go_no_go()).await;
    q.take("runs-a").await.unwrap().unwrap();
    q.end(&first.run_id, "runs-a", completed("x"))
        .await
        .unwrap();
    let second = q
        .enqueue(NewRun {
            thread_id: Some(first.thread_id.clone()),
            ..new_run(
                RunStarter::Intake {
                    intake: "tickets".into(),
                },
                command_request("triage", "{}"),
            )
        })
        .await
        .unwrap();
    assert_eq!(
        second.started_by,
        RunStarter::Intake {
            intake: "tickets".into()
        }
    );
    assert_eq!(
        q.thread_owner(&first.thread_id).await.unwrap(),
        Some(caller("svc-release-desk"))
    );
    assert_eq!(q.thread_owner("not-a-uuid").await.unwrap(), None);
    assert_eq!(
        q.thread_owner("0192a000-0000-7000-8000-000000000000")
            .await
            .unwrap(),
        None
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_given_run_id_is_used_and_the_record_id_is_the_same_uuid() {
    let h = Harness::sqlite().await;
    let given = "0192a1c3-0001-7000-8000-000000000001";
    let record = h
        .queue
        .enqueue(NewRun {
            run_id: Some(given.into()),
            ..message("hello")
        })
        .await
        .unwrap();
    assert_eq!(record.run_id, given);
    assert_eq!(record.id.0.to_string(), given);
    assert_eq!(
        record.request,
        RunRequest::Message {
            text: "hello".into()
        }
    );
    assert!(
        h.queue.get(&given.to_uppercase()).await.unwrap().is_some(),
        "the lookup is case-insensitive"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_firing_is_recorded_once_per_entry_and_due_instant() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let due: Timestamp =
        serde_json::from_value(serde_json::json!("2026-01-10T09:00:00.000Z")).unwrap();
    let scope = agent_wide();
    let run = q
        .enqueue_firing(
            "triage/run-triage/0 9 * * 1-5",
            due,
            scope,
            "run-triage",
            "queue=support",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run.started_by,
        RunStarter::ScheduleEntry {
            entry: "triage/run-triage/0 9 * * 1-5".into()
        }
    );
    assert_eq!(run.request, command_request("run-triage", "queue=support"));
    assert!(q
        .enqueue_firing(
            "triage/run-triage/0 9 * * 1-5",
            due,
            scope,
            "run-triage",
            "queue=support"
        )
        .await
        .unwrap()
        .is_none());
    assert_eq!(h.count("run_records").await, 1);
    assert_eq!(
        h.count("run_threads").await,
        1,
        "a refused firing leaves no orphan thread"
    );
    assert_eq!(
        h.metrics
            .total("wrightrun_runs_queued_total", &[("started_by", "schedule")]),
        1
    );
    // Another entry at the same instant, and the same entry at another instant, are new firings.
    let later = Timestamp::from_unix_millis(due.unix_millis() + 60_000).unwrap();
    assert!(q
        .enqueue_firing("other", due, scope, "run-triage", "")
        .await
        .unwrap()
        .is_some());
    assert!(q
        .enqueue_firing(
            "triage/run-triage/0 9 * * 1-5",
            later,
            scope,
            "run-triage",
            ""
        )
        .await
        .unwrap()
        .is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn list_pages_newest_first_a_hundred_at_a_time_and_filters_by_state() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let mut ids = Vec::new();
    for _ in 0..105 {
        ids.push(enqueue(q, message("m")).await.run_id);
    }
    ids.reverse();
    let first = q.list(None, None).await.unwrap();
    assert_eq!(first.items.len(), 100);
    assert_eq!(first.items[0].record.run_id, ids[0]);
    let cursor = serde_json::to_value(first.next_cursor.expect("a next page")).unwrap();
    let second = q.list(None, cursor.as_str()).await.unwrap();
    assert_eq!(second.items.len(), 5);
    assert!(second.next_cursor.is_none());
    assert_eq!(second.items[0].record.run_id, ids[100]);
    assert!(q.list(Some(&[]), None).await.unwrap().items.is_empty());
    assert_eq!(
        q.list(Some(&[RunRecordState::Queued]), None)
            .await
            .unwrap()
            .items
            .len(),
        100
    );
    assert!(q
        .list(Some(&[RunRecordState::Ended]), None)
        .await
        .unwrap()
        .items
        .is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn in_progress_counts_open_runs_matching_every_filter() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let first = enqueue(q, new_run(caller("c"), command_request("run-a", "x"))).await;
    enqueue(
        q,
        new_run(
            RunStarter::Intake {
                intake: "tickets".into(),
            },
            command_request("run-a", "x"),
        ),
    )
    .await;
    enqueue(
        q,
        new_run(
            RunStarter::Intake {
                intake: "mail".into(),
            },
            command_request("run-b", "x"),
        ),
    )
    .await;
    q.take("runs-a").await.unwrap().unwrap();
    q.end(&first.run_id, "runs-a", completed("x"))
        .await
        .unwrap();
    let count = |command: Option<&str>, intake: Option<&str>| {
        let filter = InProgressFilter {
            command: command.map(Into::into),
            intake: intake.map(Into::into),
        };
        async move { q.in_progress(&filter).await.unwrap() }
    };
    assert_eq!(count(None, None).await, 2);
    assert_eq!(count(Some("run-a"), None).await, 1);
    assert_eq!(count(None, Some("tickets")).await, 1);
    assert_eq!(count(Some("run-b"), Some("tickets")).await, 0);
    assert_eq!(count(Some("run-b"), Some("mail")).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_of_a_paused_run_drops_its_session_record_and_cancelling_twice_is_refused() {
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
    assert!(matches!(
        q.cancel(&run.run_id).await.unwrap(),
        CancelOutcome::Cancelled(_)
    ));
    assert_eq!(
        q.cancel(&run.run_id).await.unwrap_err().code(),
        "RUN_NOT_CANCELLABLE"
    );
    assert_eq!(
        q.resume(&run.run_id, serde_json::json!({}))
            .await
            .unwrap_err()
            .code(),
        "RUN_NOT_PAUSED"
    );
    let events = q.events_after(&run.run_id, 0, 100).await.unwrap();
    let states: Vec<_> = events
        .iter()
        .map(|e| e.data["state"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(states, ["queued", "executing", "paused", "cancelled"]);
    // A second request on an executing run keeps the first request's time and logs both.
    let again = taken(q, "runs-a").await;
    q.cancel(&again.run_id).await.unwrap();
    assert!(matches!(
        q.cancel(&again.run_id).await.unwrap(),
        CancelOutcome::Requested(_)
    ));
    assert!(logs()
        .lines()
        .iter()
        .any(|l| l["event"] == "run_cancel_requested"));
}

#[tokio::test(flavor = "multi_thread")]
async fn events_after_clamps_its_limit_and_answers_unknown_runs_with_nothing() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    for n in 0..5 {
        q.append_progress(&run.run_id, "runs-a", &format!("line {n}"))
            .await
            .unwrap();
    }
    assert_eq!(
        q.events_after(&run.run_id, 0, 0).await.unwrap().len(),
        1,
        "a limit of 0 is raised to 1"
    );
    assert_eq!(
        q.events_after(&run.run_id, 0, 10_000).await.unwrap().len(),
        7
    );
    assert_eq!(
        q.events_after(&run.run_id, 5, 10)
            .await
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect::<Vec<_>>(),
        [6, 7]
    );
    assert_eq!(
        q.events_after(&run.run_id, u64::MAX, 10)
            .await
            .unwrap()
            .len(),
        0
    );
    assert!(q
        .events_after("0192a000-0000-7000-8000-000000000000", 0, 10)
        .await
        .unwrap()
        .is_empty());
    assert!(q.events_after("junk", 0, 10).await.unwrap().is_empty());
    assert!(
        !q.append_progress(&run.run_id, "runs-a", "   ")
            .await
            .unwrap(),
        "an empty line writes nothing"
    );
    assert_eq!(
        q.events_after(&run.run_id, 0, 100)
            .await
            .unwrap()
            .last()
            .unwrap()
            .kind,
        RunEventKind::RunProgress
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_stops_taking_progress_lines_at_the_event_limit_and_logs_it_once() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    h.exec(
        "WITH RECURSIVE n(i) AS (SELECT 3 UNION ALL SELECT i + 1 FROM n WHERE i < 10000) \
         INSERT INTO run_events (run_id, seq, kind, data, at) SELECT $1, i, 'run_progress', '{}', '2026-01-10T09:00:00.000Z' FROM n",
        "",
        Some(&run.run_id),
    )
    .await;
    assert_eq!(MAX_EVENTS_PER_RUN, 10_000);
    assert!(!q
        .append_progress(&run.run_id, "runs-a", "one too many")
        .await
        .unwrap());
    assert!(!q
        .append_progress(&run.run_id, "runs-a", "and another")
        .await
        .unwrap());
    let full = logs()
        .lines()
        .iter()
        .filter(|l| l["event"] == "run_events_full" && l["run_id"] == run.run_id.as_str())
        .count();
    assert_eq!(full, 1);
    assert_eq!(h.count("run_events").await, 10_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_in_scope_removes_whole_threads_in_any_state_and_the_executing_instance_learns_it() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let scope = agent_wide();
    let other = agent_wide();
    let executing = enqueue(
        q,
        NewRun {
            scope_id: scope,
            ..go_no_go()
        },
    )
    .await;
    q.take("runs-a").await.unwrap().unwrap();
    let keep = enqueue(
        q,
        NewRun {
            scope_id: other,
            ..go_no_go()
        },
    )
    .await;
    let queued = enqueue(
        q,
        NewRun {
            scope_id: scope,
            ..go_no_go()
        },
    )
    .await;
    let counted = q.count_in_scope(scope).await.unwrap();
    assert_eq!(counted.runs, 2);
    let deleted = q.delete_in_scope(scope).await.unwrap();
    assert_eq!(deleted.runs, 2);
    assert_eq!(deleted.thread_ids.len(), 2);
    assert_eq!(
        q.renew(&executing.run_id, "runs-a").await.unwrap(),
        LeaseRenewal::Lost
    );
    assert!(q.get(&queued.run_id).await.unwrap().is_none());
    assert!(q.get(&keep.run_id).await.unwrap().is_some());
    assert_eq!(h.count("run_threads").await, 1);
    assert_eq!(q.delete_in_scope(scope).await.unwrap().runs, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_limit_is_clamped_and_takes_threads_in_thread_id_order() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let mut threads = Vec::new();
    for _ in 0..3 {
        let run = taken(q, "runs-a").await;
        q.end(&run.run_id, "runs-a", completed("x")).await.unwrap();
        h.age_end(&run.run_id).await;
        threads.push(run.thread_id);
    }
    threads.sort();
    let cutoff =
        Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 90 * 86_400_000).unwrap();
    let first = q.delete_ended_before(cutoff, 0).await.unwrap();
    assert_eq!(
        first.thread_ids,
        vec![threads[0].clone()],
        "a limit of 0 is raised to 1"
    );
    let rest = q.delete_ended_before(cutoff, 10_000).await.unwrap();
    assert_eq!(rest.thread_ids, threads[1..]);
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupting_a_run_logs_it_and_a_late_renewal_after_the_close_is_lost() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    h.lapse_lease(&run.run_id).await;
    q.mark_interrupted().await.unwrap();
    assert_eq!(
        q.renew(&run.run_id, "runs-a").await.unwrap(),
        LeaseRenewal::Lost
    );
    let line = logs()
        .lines()
        .into_iter()
        .find(|l| l["event"] == "run_interrupted" && l["run_id"] == run.run_id.as_str())
        .expect("run_interrupted was logged");
    assert_eq!(line["level"], "WARNING");
    assert_eq!(line["instance"], "runs-a");
}

#[tokio::test(flavor = "multi_thread")]
async fn log_events_carry_their_fields_and_never_the_payloads() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let secret = "very-secret-argument-text";
    let record = q
        .enqueue(new_run(caller("svc"), command_request("run-x", secret)))
        .await
        .unwrap();
    let taken = q.take("runs-a").await.unwrap().unwrap();
    q.pause(
        &record.run_id,
        "runs-a",
        pause_with(vec![tool_pending("t")], b"session-bytes"),
    )
    .await
    .unwrap();
    q.resume(&record.run_id, serde_json::json!({"note": "decision-text"}))
        .await
        .unwrap();
    q.take("runs-a").await.unwrap().unwrap();
    q.append_progress(&record.run_id, "runs-a", "progress-line-text")
        .await
        .unwrap();
    q.end(&record.run_id, "runs-a", completed("reply-text"))
        .await
        .unwrap();
    let all = logs().lines();
    let mine: Vec<_> = all
        .iter()
        .filter(|l| l["run_id"] == record.run_id.as_str())
        .collect();
    for event in [
        "run_queued",
        "run_taken",
        "run_paused",
        "run_resumed",
        "run_ended",
    ] {
        assert!(
            mine.iter().any(|l| l["event"] == event),
            "{event} was logged"
        );
    }
    let queued = mine.iter().find(|l| l["event"] == "run_queued").unwrap();
    assert_eq!(queued["started_by"], "caller");
    assert_eq!(queued["command"], "run-x");
    assert_eq!(queued["thread_id"], record.thread_id.as_str());
    assert_eq!(
        mine.iter().find(|l| l["event"] == "run_paused").unwrap()["pending"],
        1
    );
    assert_eq!(
        mine.iter().find(|l| l["event"] == "run_ended").unwrap()["state"],
        "finished"
    );
    assert_eq!(taken.view.record.run_id, record.run_id);
    let text = logs().text();
    for payload in [
        secret,
        "session-bytes",
        "decision-text",
        "progress-line-text",
        "reply-text",
    ] {
        assert!(!text.contains(payload), "a log line holds {payload:?}");
    }
}
