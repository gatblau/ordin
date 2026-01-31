//! Observability (CommandBridge.md Internal Logic 7; Logging.md R-2): the log events of the Bridge,
//! their fields, and what they never hold. The only test in its own binary: `tracing` caches per call
//! site whether anything listens, so one process-wide subscriber is installed before anything runs and
//! no other test shares the process (plan FU-017).

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use ordin_command_bridge::StreamKind;
use ordin_journal::JournalEvent;
use ordin_types::time::Timestamp;
use serde_json::{json, Value};
use support::*;
use tracing_subscriber::layer::SubscriberExt;

/// A value that must never reach a log line: it stands for personal data in an input or a payload.
const SECRET: &str = "Asha-private-note-7c1e";

#[tokio::test(flavor = "multi_thread")]
async fn each_event_is_logged_with_its_fields_and_never_with_a_payload() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    tracing::subscriber::set_global_default(subscriber).expect("install the subscriber");

    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "not_connected")],
        ..Options::default()
    })
    .await;

    // brg.command: a local command and a server command, each carrying a private value.
    f.invoke(
        "runs.stop",
        &envelope("runs.stop", LOCAL, json!({ "run_id": RUN, "note": SECRET })),
    )
    .await;
    f.invoke(
        "approval.decide",
        &envelope("approval.decide", SERVER, json!({ "note": SECRET })),
    )
    .await;

    // brg.stream (open) and brg.event_dropped.
    let (window, _) = f.open("main", BTreeMap::new()).await;
    f.bridge
        .event_sink()
        .deliver(&JournalEvent {
            backend: uuid::Uuid::now_v7(),
            seq: 1,
            at: Timestamp::now(),
            kind: "needs_you_changed".to_owned(),
            data: json!({ "note": SECRET }),
        })
        .await;

    // brg.journal_lagged.
    f.bridge.event_sink().lagged(1_000).await;
    window.wait_for(1).await;

    // brg.stream_lagging: the unjournalled queue is filled while its window does not read.
    let streams_window = StreamWindow::default();
    *streams_window.paused.lock().unwrap() = true;
    f.bridge.open_streams("main", streams_window.out());
    let streams = f.bridge.streams();
    let stream = uuid::Uuid::now_v7();
    let mut lagging = false;
    for _ in 0..5_000 {
        if streams
            .send(stream, StreamKind::Dictation, json!({ "text": SECRET }))
            .is_err()
        {
            lagging = true;
            break;
        }
    }
    assert!(lagging);
    *streams_window.paused.lock().unwrap() = false;

    // brg.window_lagging: the event queue of a window that does not read overflows.
    let slow = Window::default();
    *slow.paused.lock().unwrap() = true;
    f.bridge
        .open_events("slow", BTreeMap::new(), slow.out())
        .await
        .expect("opens");
    let sink = f.bridge.event_sink();
    for seq in 2..=8_400 {
        sink.deliver(&JournalEvent {
            backend: uuid::Uuid::now_v7(),
            seq,
            at: Timestamp::now(),
            kind: "needs_you_changed".to_owned(),
            data: json!({ "workspace_id": WORKSPACE }),
        })
        .await;
    }
    *slow.paused.lock().unwrap() = false;

    let ok = eventually(Duration::from_secs(10), || async {
        !capture.of("brg.window_lagging").is_empty()
    })
    .await;
    assert!(ok, "brg.window_lagging is logged");
    // brg.stream (closed).
    f.bridge.close().await;

    let all = capture.lines();
    for line in &all {
        assert!(line.get("dropped_fields").is_none(), "{line}");
    }
    assert!(!capture.text().contains(SECRET), "never a payload or input");

    let commands = capture.of("brg.command");
    let local = commands
        .iter()
        .find(|l| l["command"] == "runs.stop")
        .expect("the local command");
    assert_eq!(local["level"], "INFO");
    assert_eq!(local["request_id"], REQUEST);
    assert_eq!(local["backend_kind"], "local");
    assert!(local["outcome"].is_string());
    assert!(local["elapsed_ms"].is_u64());
    let server = commands
        .iter()
        .find(|l| l["command"] == "approval.decide")
        .expect("the server command");
    assert_eq!(server["backend_kind"], "server");
    assert_eq!(server["outcome"], "CONN_NOT_CONNECTED");

    let dropped: Vec<&Value> = all
        .iter()
        .filter(|l| l["event"] == "brg.event_dropped")
        .collect();
    assert_eq!(dropped[0]["level"], "WARNING");
    assert_eq!(dropped[0]["backend"], LOCAL);
    assert_eq!(dropped[0]["seq"], 1);
    assert_eq!(dropped[0]["kind"], "needs_you_changed");
    assert_eq!(dropped[0]["code"], "SVC_EVENT_INVALID");

    let opened = capture.of("brg.stream");
    assert!(opened
        .iter()
        .any(|l| l["window"] == "main" && l["state"] == "open" && l["reset_count"] == 0));
    assert!(opened.iter().any(|l| l["state"] == "closed"));

    let lagging = capture.of("brg.stream_lagging");
    assert_eq!(lagging.len(), 1, "once per stream id per second");
    assert_eq!(lagging[0]["level"], "WARNING");
    assert_eq!(lagging[0]["stream"], stream.to_string());
    assert_eq!(lagging[0]["kind"], "dictation");

    let journal = capture.of("brg.journal_lagged");
    assert_eq!(journal[0]["code"], "BRG_JOURNAL_LAGGED");
    let window_lagging = capture.of("brg.window_lagging");
    assert_eq!(window_lagging[0]["code"], "BRG_WINDOW_LAGGING");
    assert_eq!(window_lagging[0]["window"], "slow");
    f.cancel.cancel();
}
