//! Scenario bodies that run on both engines: the three fifty-instance `[pg]` scenarios and the
//! parity transcript.

use std::sync::Arc;

use ordin_run_queue::{
    CancelOutcome, InProgressFilter, LeaseRenewal, NewRun, RunEnd, RunOutcome, RunQueue,
    RunQueueError,
};
use ordin_types::domain::agent_profile::{RunEndKind, RunRecord, RunRecordState, RunStarter};
use ordin_types::time::Timestamp;
use tokio::sync::Barrier;

use super::*;

/// 50 connections call `take` at the same instant, in each of 100 rounds with a new run: exactly
/// one call returns the run and 49 return `None`.
pub async fn fifty_instances_take_one_queued_run(h: &Harness) {
    let mut queues = Vec::new();
    for _ in 0..50 {
        queues.push(Arc::new(h.instance().await.0));
    }
    for round in 0..100 {
        let record = enqueue(&h.queue, go_no_go()).await;
        let barrier = Arc::new(Barrier::new(50));
        let mut tasks = Vec::new();
        for (n, queue) in queues.iter().enumerate() {
            let (queue, barrier) = (queue.clone(), barrier.clone());
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                queue.take(&format!("runs-{n}")).await.expect("take")
            }));
        }
        let mut winners = Vec::new();
        let mut empty = 0;
        for task in tasks {
            match task.await.unwrap() {
                Some(taken) => winners.push(taken.view.record.run_id),
                None => empty += 1,
            }
        }
        assert_eq!(winners, vec![record.run_id.clone()], "round {round}");
        assert_eq!(empty, 49, "round {round}");
    }
    h.finish().await;
}

/// `renew` and `mark_interrupted` of the same lapsed run on two connections at the same instant,
/// in 100 rounds: either renew returns Kept and the state stays Executing, or renew returns Lost
/// and the state is Interrupted.
pub async fn a_renewal_and_the_interruption_have_one_outcome(h: &Harness) {
    let (renewer, _) = h.instance().await;
    let (reaper, _) = h.instance().await;
    let (renewer, reaper) = (Arc::new(renewer), Arc::new(reaper));
    let (mut kept, mut lost) = (0, 0);
    for round in 0..100 {
        let run = taken(&h.queue, "runs-a").await;
        h.lapse_lease(&run.run_id).await;
        let barrier = Arc::new(Barrier::new(2));
        let renew = {
            let (q, b, id) = (renewer.clone(), barrier.clone(), run.run_id.clone());
            tokio::spawn(async move {
                b.wait().await;
                q.renew(&id, "runs-a").await.expect("renew")
            })
        };
        let reap = {
            let (q, b) = (reaper.clone(), barrier.clone());
            tokio::spawn(async move {
                b.wait().await;
                q.mark_interrupted().await.expect("mark_interrupted")
            })
        };
        let renewal = renew.await.unwrap();
        let closed = reap.await.unwrap();
        let state = h
            .queue
            .get(&run.run_id)
            .await
            .unwrap()
            .unwrap()
            .record
            .state;
        match renewal {
            LeaseRenewal::Kept => {
                kept += 1;
                assert_eq!(state, RunRecordState::Executing, "round {round}");
                assert!(!closed.contains(&run.run_id), "round {round}");
            }
            LeaseRenewal::Lost => {
                lost += 1;
                assert_eq!(state, RunRecordState::Interrupted, "round {round}");
                assert!(closed.contains(&run.run_id), "round {round}");
            }
            LeaseRenewal::CancelRequested => panic!("round {round}: no cancel was asked"),
        }
        // Finish the round's run so only the next one is open.
        if state == RunRecordState::Executing {
            h.queue
                .end(&run.run_id, "runs-a", completed("done"))
                .await
                .unwrap();
        }
    }
    eprintln!("renewal rounds: kept {kept}, lost {lost}");
    h.finish().await;
}

/// `enqueue_firing` on 50 connections at once, in 60 rounds with a new due instant: exactly one
/// call returns a run; 60 runs on 60 threads, each started by the schedule entry.
pub async fn a_firing_and_its_run_are_recorded_once_across_fifty_instances(h: &Harness) {
    const ENTRY: &str = "triage/run-triage/0 9 * * 1-5";
    let first: Timestamp =
        serde_json::from_value(serde_json::json!("2026-01-10T09:00:00.000Z")).unwrap();
    let scope = agent_wide();
    let mut queues = Vec::new();
    for _ in 0..50 {
        queues.push(Arc::new(h.instance().await.0));
    }
    for round in 0..60i64 {
        let due = Timestamp::from_unix_millis(first.unix_millis() + round * 86_400_000).unwrap();
        let barrier = Arc::new(Barrier::new(50));
        let mut tasks = Vec::new();
        for queue in &queues {
            let (queue, barrier) = (queue.clone(), barrier.clone());
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                queue
                    .enqueue_firing(ENTRY, due, scope, "run-triage", "queue=support")
                    .await
                    .expect("enqueue_firing")
            }));
        }
        let mut returned = 0;
        for task in tasks {
            if task.await.unwrap().is_some() {
                returned += 1;
            }
        }
        assert_eq!(returned, 1, "round {round}");
    }
    assert_eq!(h.count("run_records").await, 60);
    assert_eq!(h.count("run_threads").await, 60);
    let page = h.queue.list(None, None).await.unwrap();
    assert_eq!(page.items.len(), 60);
    let mut threads = std::collections::HashSet::new();
    for view in &page.items {
        assert_eq!(
            view.record.started_by,
            RunStarter::ScheduleEntry {
                entry: ENTRY.to_owned()
            }
        );
        threads.insert(view.record.thread_id.clone());
    }
    assert_eq!(threads.len(), 60);
    h.finish().await;
}

// ---- parity ----

const R1: &str = "0192a1c3-0001-7000-8000-000000000001";
const R2: &str = "0192a1c3-0002-7000-8000-000000000002";
const R3: &str = "0192a1c3-0003-7000-8000-000000000003";
const R4: &str = "0192a1c3-0004-7000-8000-000000000004";

fn res<T>(result: &Result<T, RunQueueError>, show: impl Fn(&T) -> String) -> String {
    match result {
        Ok(value) => show(value),
        Err(error) => format!("err {} retryable={}", error.code(), error.retryable()),
    }
}

fn rec(record: &RunRecord) -> String {
    format!(
        "{} {:?} end={:?} lease={} started={} ended={} result={:?} summaries={:?}",
        record.run_id,
        record.state,
        record.end_kind,
        record.lease_holder.as_deref().unwrap_or("-"),
        record.started_at.is_some(),
        record.ended_at.is_some(),
        record.result,
        record.stretch_summaries,
    )
}

fn view(view: &ordin_run_queue::RunView) -> String {
    format!(
        "{} outcome={} cancel={}",
        rec(&view.record),
        serde_json::to_string(&view.outcome).unwrap(),
        view.cancel_requested
    )
}

fn taken_text(taken: &Option<ordin_run_queue::TakenRun>) -> String {
    match taken {
        None => "none".to_owned(),
        Some(x) => format!(
            "{} decision={:?} session={:?}",
            view(&x.view),
            x.decision,
            x.session_record
        ),
    }
}

fn deleted(d: &ordin_run_queue::DeletedRuns) -> String {
    let mut ids = d.run_ids.clone();
    ids.sort();
    format!("runs={} threads={} ids={ids:?}", d.runs, d.thread_ids.len())
}

/// The parity body: enqueue, take, renew, pause, resume, cancel, end, list, count and delete. The
/// transcript holds every returned value and every error code, with the minted thread ids left out.
pub async fn parity_transcript(h: &Harness) -> Vec<String> {
    let q = &h.queue;
    let mut t: Vec<String> = Vec::new();
    let scope = agent_wide();

    let r = q
        .enqueue(NewRun {
            run_id: Some(R1.into()),
            scope_id: scope,
            ..go_no_go()
        })
        .await;
    t.push(format!("enqueue R1: {}", res(&r, rec)));
    let thread1 = r.as_ref().unwrap().thread_id.clone();
    let r = q
        .enqueue(NewRun {
            run_id: Some(R2.into()),
            scope_id: scope,
            ..new_run(
                RunStarter::ChatConnection {
                    connection: "team-slack".into(),
                    asker: "U1".into(),
                },
                ordin_types::domain::agent_profile::RunRequest::Message {
                    text: "hello".into(),
                },
            )
        })
        .await;
    t.push(format!("enqueue R2: {}", res(&r, rec)));
    let r = q
        .enqueue(NewRun {
            thread_id: Some(thread1.clone()),
            ..go_no_go()
        })
        .await;
    t.push(format!("enqueue busy: {}", res(&r, rec)));
    let r = q
        .enqueue(NewRun {
            run_id: Some(R1.into()),
            ..go_no_go()
        })
        .await;
    t.push(format!("enqueue duplicate: {}", res(&r, rec)));
    let r = q
        .enqueue(NewRun {
            run_id: Some("nope".into()),
            ..go_no_go()
        })
        .await;
    t.push(format!("enqueue bad run id: {}", res(&r, rec)));
    let r = q
        .enqueue(NewRun {
            thread_id: Some("0192a000-0000-7000-8000-000000000000".into()),
            ..go_no_go()
        })
        .await;
    t.push(format!("enqueue no thread: {}", res(&r, rec)));
    t.push(format!(
        "thread owner: {:?}",
        q.thread_owner(&thread1).await.unwrap()
    ));
    t.push(format!(
        "in_progress: {}",
        q.in_progress(&InProgressFilter::default()).await.unwrap()
    ));
    t.push(format!(
        "in_progress by command: {}",
        q.in_progress(&InProgressFilter {
            command: Some("run-go-no-go".into()),
            intake: None
        })
        .await
        .unwrap()
    ));

    let r = q.take("i1").await;
    t.push(format!("take: {}", res(&r, taken_text)));
    t.push(format!("renew: {:?}", q.renew(R1, "i1").await.unwrap()));
    t.push(format!(
        "renew other: {:?}",
        q.renew(R1, "i2").await.unwrap()
    ));
    t.push(format!(
        "progress: {:?}",
        q.append_progress(R1, "i1", "  Reading 3 tickets  ")
            .await
            .unwrap()
    ));
    t.push(format!(
        "progress other: {}",
        res(&q.append_progress(R1, "i2", "x").await, |b| b.to_string())
    ));
    let r = q
        .pause(
            R1,
            "i1",
            pause_with(vec![tool_pending("create_ticket")], b"S1"),
        )
        .await;
    t.push(format!("pause: {}", res(&r, |_| "ok".into())));
    t.push(format!(
        "paused_with_command: {}",
        q.paused_with_command(&["run-go-no-go".into()], R2)
            .await
            .unwrap()
    ));
    t.push(format!(
        "paused_with_command self: {}",
        q.paused_with_command(&["run-go-no-go".into()], R1)
            .await
            .unwrap()
    ));
    t.push(format!(
        "get paused: {}",
        res(&q.get(R1).await.map(|v| v.unwrap()), view)
    ));
    let r = q
        .resume(
            R1,
            serde_json::json!({"decisions": [{"type": "approve"}], "set_aside": []}),
        )
        .await;
    t.push(format!("resume: {}", res(&r, rec)));
    t.push(format!(
        "resume again: {}",
        res(&q.resume(R1, serde_json::json!({})).await, rec)
    ));
    t.push(format!("decision: {:?}", q.decision(R1).await.unwrap()));
    let r = q.cancel(R2).await;
    t.push(format!(
        "cancel queued: {}",
        res(&r, |o| match o {
            CancelOutcome::Cancelled(v) => format!("Cancelled {}", view(v)),
            CancelOutcome::Requested(v) => format!("Requested {}", view(v)),
        })
    ));
    let r = q.take("i2").await;
    t.push(format!("take resumed: {}", res(&r, taken_text)));
    let r = q.cancel(R1).await;
    t.push(format!(
        "cancel executing: {}",
        res(&r, |o| match o {
            CancelOutcome::Cancelled(v) => format!("Cancelled {}", view(v)),
            CancelOutcome::Requested(v) => format!("Requested {}", view(v)),
        })
    ));
    t.push(format!(
        "renew cancel: {:?}",
        q.renew(R1, "i2").await.unwrap()
    ));
    let r = q
        .record_cancelled(
            R1,
            "i2",
            RunOutcome {
                ending: Some("cancelled".into()),
                ..Default::default()
            },
        )
        .await;
    t.push(format!("record_cancelled: {}", res(&r, |_| "ok".into())));
    t.push(format!(
        "cancel ended: {}",
        res(&q.cancel(R1).await, |_| "ok".into())
    ));

    let r = q
        .enqueue(NewRun {
            run_id: Some(R3.into()),
            scope_id: scope,
            ..go_no_go()
        })
        .await;
    t.push(format!("enqueue R3: {}", res(&r, rec)));
    let r = q.take("i3").await;
    t.push(format!("take R3: {}", res(&r, taken_text)));
    let mut end = completed("Go.");
    end.stretch_summary = Some("{\"stretch\":1}".into());
    t.push(format!(
        "end: {}",
        res(&q.end(R3, "i3", end).await, |_| "ok".into())
    ));
    t.push(format!(
        "end again: {}",
        res(&q.end(R3, "i3", completed("again")).await, |_| "ok".into())
    ));
    let bad = RunEnd {
        end_kind: RunEndKind::Control,
        outcome: RunOutcome::default(),
        stretch_summary: None,
    };
    t.push(format!(
        "end bad control: {}",
        res(&q.end(R3, "i3", bad).await, |_| "ok".into())
    ));
    let r = q
        .enqueue(NewRun {
            run_id: Some(R4.into()),
            scope_id: scope,
            ..go_no_go()
        })
        .await;
    t.push(format!("enqueue R4: {}", res(&r, rec)));

    for (name, states) in [
        ("all", None),
        ("ended", Some(vec![RunRecordState::Ended])),
        (
            "cancelled+queued",
            Some(vec![RunRecordState::Cancelled, RunRecordState::Queued]),
        ),
        ("none", Some(vec![])),
    ] {
        let page = q.list(states.as_deref(), None).await;
        t.push(format!(
            "list {name}: {}",
            res(&page, |p| format!(
                "{:?} next={}",
                p.items
                    .iter()
                    .map(|v| format!("{}:{:?}", v.record.run_id, v.record.state))
                    .collect::<Vec<_>>(),
                p.next_cursor.is_some()
            ))
        ));
    }
    let bad = ordin_types::Cursor::encode(&["abc"]);
    let bad = serde_json::to_value(&bad).unwrap();
    t.push(format!(
        "list bad cursor: {}",
        res(&q.list(None, bad.as_str()).await, |_| "ok".into())
    ));
    let events = q.events_after(R1, 0, 100).await.unwrap();
    t.push(format!(
        "events R1: {:?}",
        events
            .iter()
            .map(|e| format!("{}:{:?}:{}", e.seq, e.kind, e.data))
            .collect::<Vec<_>>()
    ));
    let events = q.events_after(R3, 1, 100).await.unwrap();
    t.push(format!(
        "events R3 after 1: {:?}",
        events
            .iter()
            .map(|e| (e.seq, e.data.to_string()))
            .collect::<Vec<_>>()
    ));

    let cutoff = Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 3_600_000).unwrap();
    t.push(format!(
        "count ended: {}",
        res(&q.count_ended_before(cutoff, 500).await, deleted)
    ));
    t.push(format!(
        "count asker: {}",
        res(&q.count_started_by_asker("team-slack", "U1").await, deleted)
    ));
    t.push(format!(
        "count scope: {}",
        res(&q.count_in_scope(scope).await, deleted)
    ));
    t.push(format!(
        "delete asker: {}",
        res(
            &q.delete_started_by_asker("team-slack", "U1").await,
            deleted
        )
    ));
    t.push(format!(
        "delete ended: {}",
        res(&q.delete_ended_before(cutoff, 500).await, deleted)
    ));
    t.push(format!(
        "delete scope: {}",
        res(&q.delete_in_scope(scope).await, deleted)
    ));
    t.push(format!(
        "get after delete: {}",
        res(&q.get(R3).await.map(|v| v.is_some()), |b| b.to_string())
    ));
    t.push(format!(
        "tables: {} {} {}",
        h.count("run_records").await,
        h.count("run_threads").await,
        h.count("run_events").await
    ));
    t
}
