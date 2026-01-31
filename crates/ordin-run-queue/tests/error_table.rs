//! X1: one test per row of RunQueue.md *Error Table*, plus X4 (`ALL_CODES` in the registry).
//! The rows are raised through `SqliteRunStore`; PostgreSQL raises the same codes (parity test).

mod support;

use ordin_run_queue::{
    LeaseRenewal, NewRun, RunEnd, RunOutcome, RunQueue, RunQueueError, ALL_CODES,
    MAX_SESSION_RECORD_BYTES,
};
use ordin_types::domain::agent_profile::{RunEndKind, RunRequest, RunStarter};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::CodedError;
use support::*;

fn bad_field<T: std::fmt::Debug>(result: Result<T, RunQueueError>) -> &'static str {
    match result {
        Err(RunQueueError::BadInput { field }) => field,
        other => panic!("expected RUN_BAD_INPUT, got {other:?}"),
    }
}

#[tokio::test]
async fn enqueue_on_a_thread_id_no_thread_has() {
    let h = Harness::sqlite().await;
    let result = h
        .queue
        .enqueue(NewRun {
            thread_id: Some("0192a000-0000-7000-8000-000000000000".into()),
            ..go_no_go()
        })
        .await;
    let error = result.unwrap_err();
    assert!(matches!(error, RunQueueError::ThreadNotFound));
    assert_eq!(error.code(), "RUN_THREAD_NOT_FOUND");
    assert_eq!(error.to_string(), "No thread has this identifier.");
    assert!(!error.retryable());
    // A thread id that is not a UUID is "not found" too.
    expect_code(
        h.queue
            .enqueue(NewRun {
                thread_id: Some("not-a-uuid".into()),
                ..go_no_go()
            })
            .await,
        "RUN_THREAD_NOT_FOUND",
    );
    assert_eq!(h.count("run_records").await, 0);
}

#[tokio::test]
async fn enqueue_on_a_thread_with_a_queued_executing_or_paused_run() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    // queued
    let run = enqueue(q, go_no_go()).await;
    let busy = q
        .enqueue(NewRun {
            thread_id: Some(run.thread_id.clone()),
            ..go_no_go()
        })
        .await;
    let error = busy.unwrap_err();
    assert_eq!(error.code(), "RUN_THREAD_BUSY");
    assert_eq!(
        error.to_string(),
        "The thread has a run that has not ended."
    );
    // executing
    q.take("runs-a").await.unwrap().unwrap();
    expect_code(
        q.enqueue(NewRun {
            thread_id: Some(run.thread_id.clone()),
            ..go_no_go()
        })
        .await,
        "RUN_THREAD_BUSY",
    );
    // paused
    q.pause(
        &run.run_id,
        "runs-a",
        pause_with(vec![tool_pending("t")], b"S"),
    )
    .await
    .unwrap();
    expect_code(
        q.enqueue(NewRun {
            thread_id: Some(run.thread_id.clone()),
            ..go_no_go()
        })
        .await,
        "RUN_THREAD_BUSY",
    );
    assert_eq!(h.count("run_records").await, 1);
}

#[tokio::test]
async fn enqueue_with_a_run_id_a_run_already_has() {
    let h = Harness::sqlite().await;
    let first = enqueue(&h.queue, go_no_go()).await;
    let again = h
        .queue
        .enqueue(NewRun {
            run_id: Some(first.run_id.clone()),
            ..go_no_go()
        })
        .await;
    let error = again.unwrap_err();
    assert_eq!(error.code(), "RUN_DUPLICATE_ID");
    assert_eq!(error.to_string(), "A run has this identifier already.");
    // The new thread of the refused enqueue was rolled back with it.
    assert_eq!(h.count("run_threads").await, 1);
    assert_eq!(h.count("run_records").await, 1);
}

#[tokio::test]
async fn input_breaks_a_rule_of_step_1_or_step_6_pause_with_no_pending_item_or_end_control_without_ended_by_control(
) {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let long = "x".repeat(513);
    let slug_too_long = "a".repeat(65);
    let cases: Vec<(&str, NewRun)> = vec![
        (
            "started_by",
            new_run(caller(""), command_request("run-x", "a")),
        ),
        (
            "started_by",
            new_run(caller(&long), command_request("run-x", "a")),
        ),
        (
            "started_by",
            new_run(
                RunStarter::ChatConnection {
                    connection: "c".into(),
                    asker: "".into(),
                },
                RunRequest::Message { text: "hi".into() },
            ),
        ),
        ("text", message("")),
        ("text", message("   \n\t")),
        ("text", message("a\0b")),
        ("text", message(&"y".repeat(262_145))),
        (
            "command",
            new_run(caller("c"), command_request("Run-X", "a")),
        ),
        ("command", new_run(caller("c"), command_request("", "a"))),
        (
            "command",
            new_run(caller("c"), command_request(&slug_too_long, "a")),
        ),
        (
            "arguments",
            new_run(caller("c"), command_request("run-x", "a\0b")),
        ),
        (
            "arguments",
            new_run(
                caller("c"),
                command_request("run-x", &"z".repeat(1_048_577)),
            ),
        ),
        (
            "run_id",
            NewRun {
                run_id: Some("0192a000-0000-4000-8000-000000000000".into()),
                ..go_no_go()
            },
        ),
        (
            "run_id",
            NewRun {
                run_id: Some("x".into()),
                ..go_no_go()
            },
        ),
        (
            "gathered_context",
            NewRun {
                gathered_context: serde_json::json!([1]),
                ..go_no_go()
            },
        ),
    ];
    for (field, new) in cases {
        assert_eq!(bad_field(q.enqueue(new).await), field);
    }
    assert_eq!(h.count("run_records").await, 0, "nothing was written");
    assert_eq!(h.count("run_threads").await, 0);
    let error = RunQueueError::BadInput { field: "x" };
    assert_eq!(error.code(), "RUN_BAD_INPUT");
    assert_eq!(
        error.to_string(),
        "A run starts with a command or with a message, not both."
    );

    // Step 6.
    let due = ordin_types::time::Timestamp::now();
    for (key, command, arguments, field) in [
        ("", "run-x", "a", "entry_key"),
        ("k", "Run X", "a", "command"),
        ("k", "run-x", &*"a".repeat(4_097), "arguments"),
    ] {
        assert_eq!(
            bad_field(
                q.enqueue_firing(key, due, agent_wide(), command, arguments)
                    .await
            ),
            field
        );
    }
    assert_eq!(h.count("run_records").await, 0);

    // pause with no pending item; end Control without ended_by_control.
    let run = taken(q, "runs-a").await;
    assert_eq!(
        bad_field(
            q.pause(&run.run_id, "runs-a", pause_with(vec![], b"S"))
                .await
        ),
        "pending"
    );
    let control = RunEnd {
        end_kind: RunEndKind::Control,
        outcome: RunOutcome::default(),
        stretch_summary: None,
    };
    assert_eq!(
        bad_field(q.end(&run.run_id, "runs-a", control).await),
        "end_kind"
    );
    // paused_with_command with more than 100 commands.
    let many: Vec<String> = (0..101).map(|n| format!("c{n}")).collect();
    assert_eq!(
        bad_field(q.paused_with_command(&many, &run.run_id).await),
        "commands"
    );
    assert_eq!(
        q.get(&run.run_id)
            .await
            .unwrap()
            .unwrap()
            .record
            .lease_holder
            .as_deref(),
        Some("runs-a")
    );
}

#[tokio::test]
async fn resume_of_a_run_that_is_not_paused_or_does_not_exist() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let queued = enqueue(q, go_no_go()).await;
    let error = q
        .resume(&queued.run_id, serde_json::json!({}))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "RUN_NOT_PAUSED");
    assert_eq!(error.to_string(), "The run is not paused.");
    expect_code(
        q.resume(
            "0192a000-0000-7000-8000-000000000000",
            serde_json::json!({}),
        )
        .await,
        "RUN_NOT_PAUSED",
    );
    expect_code(
        q.resume("not-a-uuid", serde_json::json!({})).await,
        "RUN_NOT_PAUSED",
    );
}

#[tokio::test]
async fn cancel_of_a_run_that_has_ended_or_does_not_exist() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    q.end(&run.run_id, "runs-a", completed("Go."))
        .await
        .unwrap();
    let error = q.cancel(&run.run_id).await.unwrap_err();
    assert_eq!(error.code(), "RUN_NOT_CANCELLABLE");
    assert_eq!(
        error.to_string(),
        "The run has ended and cannot be cancelled."
    );
    expect_code(
        q.cancel("0192a000-0000-7000-8000-000000000000").await,
        "RUN_NOT_CANCELLABLE",
    );
    expect_code(q.cancel("not-a-uuid").await, "RUN_NOT_CANCELLABLE");
}

#[tokio::test]
async fn end_pause_record_cancelled_and_append_progress_by_an_instance_that_no_longer_holds_the_lease(
) {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    let id = run.run_id.as_str();
    assert_eq!(
        q.end(id, "runs-b", completed("x"))
            .await
            .unwrap_err()
            .code(),
        "RUN_LEASE_LOST"
    );
    assert_eq!(
        q.pause(id, "runs-b", pause_with(vec![tool_pending("t")], b"S"))
            .await
            .unwrap_err()
            .code(),
        "RUN_LEASE_LOST"
    );
    assert_eq!(
        q.record_cancelled(id, "runs-b", RunOutcome::default())
            .await
            .unwrap_err()
            .code(),
        "RUN_LEASE_LOST"
    );
    assert_eq!(
        q.append_progress(id, "runs-b", "line")
            .await
            .unwrap_err()
            .code(),
        "RUN_LEASE_LOST"
    );
    // The holder itself cannot record a cancel nobody asked for.
    assert_eq!(
        q.record_cancelled(id, "runs-a", RunOutcome::default())
            .await
            .unwrap_err()
            .code(),
        "RUN_LEASE_LOST"
    );
    // A run that is not a UUID, or does not exist, is answered the same way.
    for unknown in ["not-a-uuid", "0192a000-0000-7000-8000-000000000000"] {
        assert_eq!(
            q.end(unknown, "runs-a", completed("x"))
                .await
                .unwrap_err()
                .code(),
            "RUN_LEASE_LOST"
        );
        assert_eq!(
            q.append_progress(unknown, "runs-a", "x")
                .await
                .unwrap_err()
                .code(),
            "RUN_LEASE_LOST"
        );
        assert_eq!(
            q.renew(unknown, "runs-a").await.unwrap(),
            LeaseRenewal::Lost
        );
    }
    assert_eq!(
        RunQueueError::LeaseLost.to_string(),
        "The run is no longer executing under this instance's lease."
    );
    // The run is untouched.
    assert_eq!(
        q.get(id)
            .await
            .unwrap()
            .unwrap()
            .record
            .lease_holder
            .as_deref(),
        Some("runs-a")
    );
}

#[tokio::test]
async fn pause_with_a_session_record_over_64_mib() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let run = taken(q, "runs-a").await;
    let big = vec![0u8; MAX_SESSION_RECORD_BYTES + 1];
    let error = q
        .pause(
            &run.run_id,
            "runs-a",
            pause_with(vec![tool_pending("t")], &big),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "RUN_SESSION_RECORD_TOO_LARGE");
    assert_eq!(error.to_string(), "The session record exceeds 64 MiB.");
    assert_eq!(
        q.get(&run.run_id)
            .await
            .unwrap()
            .unwrap()
            .record
            .lease_holder
            .as_deref(),
        Some("runs-a")
    );
    // Exactly 64 MiB is accepted.
    let exact = vec![7u8; MAX_SESSION_RECORD_BYTES];
    q.pause(
        &run.run_id,
        "runs-a",
        pause_with(vec![tool_pending("t")], &exact),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn list_with_a_cursor_that_cannot_be_decoded_or_is_not_one_integer() {
    let h = Harness::sqlite().await;
    let q = &h.queue;
    let not_an_integer = serde_json::to_value(ordin_types::Cursor::encode(&["abc"])).unwrap();
    let error = q.list(None, not_an_integer.as_str()).await.unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
    assert_eq!(
        error.to_string(),
        "the cursor is not one this listing returned"
    );
    for junk in ["!!not base64!!", "", &"A".repeat(1_025)] {
        expect_code(q.list(None, Some(junk)).await, "SRV_BAD_CURSOR");
    }
    assert!(!error.retryable());
}

#[tokio::test]
async fn the_datastore_does_not_answer() {
    let h = Harness::sqlite().await;
    h.datastore.close().await;
    let error = h.queue.enqueue(go_no_go()).await.unwrap_err();
    assert_eq!(error.code(), "DB_UNAVAILABLE");
    assert_eq!(error.to_string(), "The runtime database did not answer.");
    assert!(error.retryable());
    // Every method that reaches the datastore answers the same way.
    assert_eq!(
        h.queue.take("runs-a").await.unwrap_err().code(),
        "DB_UNAVAILABLE"
    );
    assert_eq!(
        h.queue
            .get("0192a000-0000-7000-8000-000000000000")
            .await
            .unwrap_err()
            .code(),
        "DB_UNAVAILABLE"
    );
    assert_eq!(
        h.queue.mark_interrupted().await.unwrap_err().code(),
        "DB_UNAVAILABLE"
    );
    let warned = logs()
        .lines()
        .iter()
        .any(|l| l["event"] == "run_queue_unavailable" && l["operation"] == "enqueue");
    assert!(
        warned,
        "run_queue_unavailable was logged with its operation"
    );
}

// ---- X4 ----

#[test]
fn every_code_of_this_crate_is_registered_under_a_prefix_it_owns() {
    const PACKAGE: &str = "ordin-run-queue";
    assert_eq!(ALL_CODES.len(), 14);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, PACKAGE, "{code}");
        let (prefix, _) = code.split_once('_').expect("a code has a prefix");
        assert!(
            owns_prefix(PACKAGE, prefix),
            "{PACKAGE} does not own {prefix}_"
        );
        assert_eq!(entry.retryable, *code == "DB_UNAVAILABLE", "{code}");
    }
}

#[test]
fn every_error_variant_has_a_registered_code() {
    let variants = [
        (RunQueueError::ThreadNotFound, "RUN_THREAD_NOT_FOUND"),
        (RunQueueError::ThreadBusy, "RUN_THREAD_BUSY"),
        (RunQueueError::DuplicateId, "RUN_DUPLICATE_ID"),
        (RunQueueError::BadInput { field: "f" }, "RUN_BAD_INPUT"),
        (RunQueueError::NotPaused, "RUN_NOT_PAUSED"),
        (RunQueueError::NotCancellable, "RUN_NOT_CANCELLABLE"),
        (RunQueueError::LeaseLost, "RUN_LEASE_LOST"),
        (
            RunQueueError::SessionRecordTooLarge,
            "RUN_SESSION_RECORD_TOO_LARGE",
        ),
        (RunQueueError::BadCursor, "SRV_BAD_CURSOR"),
        (RunQueueError::Unavailable, "DB_UNAVAILABLE"),
    ];
    for (error, expected) in variants {
        assert_eq!(error.code(), expected);
        assert_eq!(CodedError::code(&error), expected);
        assert_eq!(CodedError::retryable(&error), error.retryable());
        assert!(lookup(expected).is_some(), "{expected}");
        assert_eq!(error.retryable(), expected == "DB_UNAVAILABLE");
    }
}
