//! X1: one test per row of the Error Table (CommandBridge.md), and X4: the codes this crate owns.

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use ordin_command_bridge::{BridgeError, StreamKind, StreamSendError, ALL_CODES};
use ordin_journal::JournalEvent;
use ordin_service_api::{EventEnvelope, ServiceApiVersion, ServiceEvent};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::RunProgress;
use ordin_types::{CodedError, RunId};
use serde_json::{json, Value};
use support::*;

fn retryable(code: &str) -> bool {
    lookup(code).expect("a registered code").retryable
}

fn install(capture: &Capture) -> tracing::subscriber::DefaultGuard {
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .event_format(ordin_types::log::JsonLineFormat)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_default(subscriber)
}

fn journal_event(seq: u64, kind: &str, data: Value) -> JournalEvent {
    JournalEvent {
        backend: uuid::Uuid::now_v7(),
        seq,
        at: Timestamp::now(),
        kind: kind.to_owned(),
        data,
    }
}

#[test]
fn every_code_is_registered_and_owned() {
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
        assert_eq!(entry.owner_crate, "ordin-command-bridge", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(owns_prefix("ordin-command-bridge", prefix), "{code}");
    }
    assert_eq!(ALL_CODES.len(), 6);
}

#[tokio::test]
async fn envelope_over_the_body_limit() {
    let f = fixture().await;
    let big = envelope(
        "approval.decide",
        LOCAL,
        json!({ "pad": "x".repeat(1_048_576) }),
    );
    let reply = f.invoke("approval.decide", &big).await;
    assert_eq!(reply["error"]["code"], "SVC_BODY_TOO_LARGE");
    assert_eq!(reply["request_id"], Value::Null);
    assert!(!retryable("SVC_BODY_TOO_LARGE"));
    // The same size to a server is refused here, never forwarded.
    let to_server = envelope(
        "approval.decide",
        SERVER,
        json!({ "pad": "x".repeat(1_048_576) }),
    );
    let reply = f.invoke("approval.decide", &to_server).await;
    assert_eq!(reply["error"]["code"], "SVC_BODY_TOO_LARGE");
    // A text-save command takes up to 9 MiB: the same body passes the size check.
    let save = envelope(
        "files.save_edit",
        LOCAL,
        json!({ "pad": "x".repeat(1_048_576) }),
    );
    let reply = f.invoke("files.save_edit", &save).await;
    assert_ne!(reply["error"]["code"], "SVC_BODY_TOO_LARGE");
    assert!(f.api.calls().is_empty());
}

#[tokio::test]
async fn envelope_unreadable_input_invalid_version_or_name_refused_local() {
    let f = fixture().await;
    // Unreadable.
    let reply = f.invoke("runs.stop", "{ not json").await;
    assert_eq!(reply["error"]["code"], "SVC_INPUT_INVALID");
    // Input invalid.
    let reply = f
        .invoke(
            "runs.stop",
            &envelope("runs.stop", LOCAL, json!({ "run_id": 7 })),
        )
        .await;
    assert_eq!(reply["error"]["code"], "SVC_INPUT_INVALID");
    // Version refused.
    let mut v: Value =
        serde_json::from_str(&envelope("runs.stop", LOCAL, json!({ "run_id": RUN }))).unwrap();
    v["api_version"] = json!({ "major": 2, "minor": 0 });
    let reply = f.invoke("runs.stop", &v.to_string()).await;
    assert_eq!(reply["error"]["code"], "SVC_VERSION_UNSUPPORTED");
    // Name refused: the envelope names another command than the IPC command.
    let reply = f
        .invoke(
            "runs.stop",
            &envelope("runs.cancel_everything", LOCAL, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "SVC_COMMAND_UNKNOWN");
    // A malformed backend is the Service API's own refusal.
    let reply = f
        .invoke(
            "runs.stop",
            &envelope("runs.stop", "not-a-backend", json!({ "run_id": RUN })),
        )
        .await;
    assert_eq!(reply["error"]["code"], "SVC_BACKEND_ID_MALFORMED");
    for code in ["SVC_INPUT_INVALID", "SVC_VERSION_UNSUPPORTED"] {
        assert!(!retryable(code), "{code} is permanent");
    }
    assert!(f.api.calls().is_empty());
}

#[tokio::test]
async fn local_only_command_addressed_to_a_server() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let reply = f
        .invoke("app.quit", &envelope("app.quit", SERVER, json!({})))
        .await;
    assert_eq!(reply["kind"], "error");
    assert_eq!(reply["request_id"], REQUEST);
    assert_eq!(reply["error"]["code"], "SVC_COMMAND_LOCAL_ONLY");
    assert_eq!(
        reply["error"]["message"],
        "Sentence for SVC_COMMAND_LOCAL_ONLY."
    );
    assert!(!retryable("SVC_COMMAND_LOCAL_ONLY"));
    f.cancel.cancel();
}

#[tokio::test]
async fn backend_neither_local_nor_paired() {
    let f = fixture().await;
    let reply = f
        .invoke(
            "runs.stop",
            &envelope(
                "runs.stop",
                "0192ffff-0000-7000-8000-000000000000",
                json!({ "run_id": RUN }),
            ),
        )
        .await;
    assert_eq!(reply["error"]["code"], "SVC_BACKEND_UNKNOWN");
    assert_eq!(reply["request_id"], REQUEST);
    assert!(!retryable("SVC_BACKEND_UNKNOWN"));
}

#[tokio::test]
async fn server_not_connected() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "not_connected")],
        ..Options::default()
    })
    .await;
    let reply = f
        .invoke(
            "approval.decide",
            &envelope("approval.decide", SERVER, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "CONN_NOT_CONNECTED");
    assert_eq!(
        reply["error"]["message"],
        "Sentence for CONN_NOT_CONNECTED."
    );
    assert!(retryable("CONN_NOT_CONNECTED"));
    f.cancel.cancel();
}

#[tokio::test]
async fn server_unreachable_while_sending() {
    let stand_in = RemoteStandIn::start(Answer::Drop).await;
    let f = fixture_with(Options {
        seeds: vec![Seed::new(stand_in.port, "connected")],
        ..Options::default()
    })
    .await;
    f.connected().await;
    let reply = f
        .invoke(
            "approval.decide",
            &envelope("approval.decide", SERVER, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "CONN_UNREACHABLE");
    assert_eq!(reply["error"]["action"], "try_again");
    assert!(retryable("CONN_UNREACHABLE"));
    f.cancel.cancel();
}

#[tokio::test]
async fn server_refused_the_desktop_key() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let reply = f
        .invoke(
            "approval.decide",
            &envelope("approval.decide", SERVER, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "CONN_KEY_REFUSED");
    assert!(reply["error"].get("action").is_none());
    assert!(!retryable("CONN_KEY_REFUSED"));
    f.cancel.cancel();
}

#[tokio::test]
async fn server_version_unusable() {
    let mut seed = Seed::new(closed_port().await, "refused_for_version");
    seed.version = Some("2.0");
    let f = fixture_with(Options {
        seeds: vec![seed],
        ..Options::default()
    })
    .await;
    let reply = f
        .invoke(
            "approval.decide",
            &envelope("approval.decide", SERVER, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "CONN_REFUSED_FOR_VERSION");
    assert!(!retryable("CONN_REFUSED_FOR_VERSION"));
    f.cancel.cancel();
}

#[tokio::test]
async fn server_did_not_answer_by_the_deadline() {
    let stand_in =
        RemoteStandIn::start(Answer::Late(Duration::from_secs(3), "{}".to_owned())).await;
    let f = fixture_with(Options {
        seeds: vec![Seed::new(stand_in.port, "connected")],
        // The 10 s deadline becomes 200 ms.
        time_scale: 0.02,
        ..Options::default()
    })
    .await;
    f.connected().await;
    let started = std::time::Instant::now();
    let reply = f
        .invoke(
            "approval.decide",
            &envelope("approval.decide", SERVER, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "CONN_DEADLINE");
    assert_eq!(reply["error"]["action"], "try_again");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(retryable("CONN_DEADLINE"));
    f.cancel.cancel();
}

#[tokio::test]
async fn service_events_names_an_unknown_backend() {
    let f = fixture().await;
    let window = Window::default();
    let error = f
        .bridge
        .open_events(
            "main",
            BTreeMap::from([(
                ordin_types::BackendId::parse("0192ffff-0000-7000-8000-000000000000").unwrap(),
                5,
            )]),
            window.out(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        BridgeError {
            code: "BRG_BACKEND_UNKNOWN"
        }
    );
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        json!({ "code": "BRG_BACKEND_UNKNOWN" })
    );
    assert!(!error.retryable());
    assert_eq!(CodedError::code(&error), "BRG_BACKEND_UNKNOWN");
}

#[tokio::test]
async fn the_bridges_journal_queue_overflowed() {
    let capture = Capture::default();
    let _guard = install(&capture);
    let f = fixture().await;
    let (window, _) = f.open("main", BTreeMap::new()).await;
    for _ in 0..5 {
        f.append().await;
    }
    window.wait_for(5).await;
    // The Journal reports that events from seq 3 on were not delivered: they are re-read, and the
    // ones already sent are not sent twice.
    f.bridge.event_sink().lagged(3).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(window.seqs(), vec![1, 2, 3, 4, 5]);
    let lines = capture.of("brg.journal_lagged");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["code"], "BRG_JOURNAL_LAGGED");
    assert!(!retryable("BRG_JOURNAL_LAGGED"));
    // When the missed events cannot be read, every window gets a reset for the local backend.
    f.bridge.event_sink().lagged(1_000).await;
    let events = window.wait_for(6).await;
    assert_eq!(events[5]["kind"], "reset");
    assert_eq!(events[5]["backend"], LOCAL);
    assert_eq!(events[5]["seq"], 5);
}

#[tokio::test]
async fn the_bridges_journal_queue_overflowed_with_events_missed() {
    let f = fixture().await;
    let (window, _) = f.open("main", BTreeMap::new()).await;
    let sink = f.bridge.event_sink();
    // The sink delivers seq 1 and 2; 3 to 5 are dropped from its queue and re-read on `lagged`.
    for _ in 0..5 {
        f.append().await;
    }
    window.wait_for(5).await;
    let delivered = window.seqs();
    assert_eq!(delivered, vec![1, 2, 3, 4, 5]);
    sink.lagged(1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(window.seqs().len(), 5, "each seq reaches the window once");
}

#[tokio::test]
async fn command_after_close_began() {
    let f = fixture().await;
    f.bridge.close().await;
    let reply = f
        .invoke(
            "runs.stop",
            &envelope("runs.stop", LOCAL, json!({ "run_id": RUN })),
        )
        .await;
    assert_eq!(reply["kind"], "error");
    assert_eq!(reply["request_id"], REQUEST);
    assert_eq!(reply["error"]["code"], "BRG_CLOSING");
    assert!(!retryable("BRG_CLOSING"));
    assert!(f.api.calls().is_empty());
}

#[tokio::test]
async fn window_not_reading_events() {
    let capture = Capture::default();
    let _guard = install(&capture);
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let (probe, _) = f.open("probe", BTreeMap::new()).await;
    f.server_event(server(), 77).await;
    probe.wait_for(1).await;
    f.bridge.window_destroyed("probe");
    let (window, _) = f.open("main", BTreeMap::new()).await;
    let sink = f.bridge.event_sink();
    for seq in 1..=8_193 {
        sink.deliver(&journal_event(
            seq,
            "needs_you_changed",
            json!({ "workspace_id": WORKSPACE }),
        ))
        .await;
    }
    let events = window.wait_for(2).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        window.events().len(),
        2,
        "one reset per backend, nothing else"
    );
    let mut backends: Vec<&str> = events
        .iter()
        .map(|e| {
            assert_eq!(e["kind"], "reset");
            e["backend"].as_str().unwrap()
        })
        .collect();
    backends.sort_unstable();
    let mut expected = vec![LOCAL, SERVER];
    expected.sort_unstable();
    assert_eq!(backends, expected);
    assert_eq!(capture.of("brg.window_lagging").len(), 1);
    assert!(!retryable("BRG_WINDOW_LAGGING"));
    f.cancel.cancel();
}

#[tokio::test]
async fn screen_streams_send_with_no_open_channel() {
    let f = fixture().await;
    let error = f
        .bridge
        .streams()
        .send(uuid::Uuid::now_v7(), StreamKind::Lsp, json!({}))
        .unwrap_err();
    assert_eq!(error, StreamSendError::NoWindow);
    assert_eq!(error.code(), "BRG_STREAM_NO_WINDOW");
    assert!(!error.retryable());
    assert!(!retryable("BRG_STREAM_NO_WINDOW"));
}

#[tokio::test]
async fn screen_streams_send_with_the_queue_full() {
    let capture = Capture::default();
    let _guard = install(&capture);
    let f = fixture().await;
    let window = StreamWindow::default();
    f.bridge.open_streams("main", window.out());
    let streams = f.bridge.streams();
    let stream = uuid::Uuid::now_v7();
    // The sender task does not run while this loop holds the thread: the queue fills.
    for n in 0..4_096 {
        streams
            .send(stream, StreamKind::Terminal, json!({ "n": n }))
            .expect("queued");
    }
    let error = streams
        .send(stream, StreamKind::Terminal, json!({ "n": 4_096 }))
        .unwrap_err();
    assert_eq!(error, StreamSendError::Lagging);
    assert_eq!(error.code(), "BRG_STREAM_LAGGING");
    assert!(!error.retryable());
    // Logged once per stream id per second.
    let _ = streams.send(stream, StreamKind::Terminal, json!({}));
    let lines = capture.of("brg.stream_lagging");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["stream"], stream.to_string());
    assert_eq!(lines[0]["kind"], "terminal");
    // Nothing queued is dropped.
    let all = window.wait_for(4_096).await;
    assert_eq!(all.len(), 4_096);
    assert_eq!(all[4_095]["payload"]["n"], 4_095);
    assert!(!retryable("BRG_STREAM_LAGGING"));
}

#[tokio::test]
async fn event_record_not_decodable_or_too_large() {
    let capture = Capture::default();
    let _guard = install(&capture);
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let (window, _) = f.open("main", BTreeMap::new()).await;
    // Not decodable: the data does not fit the kind.
    f.bridge
        .event_sink()
        .deliver(&journal_event(1, "needs_you_changed", json!({ "x": 1 })))
        .await;
    // Too large: a server's event of a kind that cannot be substituted.
    let huge = EventEnvelope {
        api_version: ServiceApiVersion::CURRENT,
        backend: server(),
        seq: 9,
        at: Timestamp::now(),
        event: ServiceEvent::RunProgress {
            run_id: RunId::new(),
            progress: RunProgress {
                line: "x".repeat(300_000),
                count: None,
            },
        },
    };
    f.server_tx
        .send(ordin_server_connector::ServerEvent::Event {
            backend: server(),
            envelope: std::sync::Arc::new(huge),
        })
        .await
        .unwrap();
    // A good event after them still arrives: the bad ones were skipped.
    f.bridge
        .event_sink()
        .deliver(&journal_event(
            2,
            "needs_you_changed",
            json!({ "workspace_id": WORKSPACE }),
        ))
        .await;
    let ok = eventually(Duration::from_secs(5), || async {
        capture.of("brg.event_dropped").len() == 2
    })
    .await;
    assert!(ok, "both records are logged");
    let events = window.wait_for(1).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["seq"], 2);
    let dropped = capture.of("brg.event_dropped");
    let codes: Vec<&str> = dropped
        .iter()
        .map(|l| l["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"SVC_EVENT_INVALID"));
    assert!(codes.contains(&"SVC_EVENT_TOO_LARGE"));
    for line in &dropped {
        assert!(line.get("data").is_none());
        assert!(!line.to_string().contains("xxxxxxxx"), "never event data");
    }
    assert!(!retryable("SVC_EVENT_INVALID"));
    f.cancel.cancel();
}
