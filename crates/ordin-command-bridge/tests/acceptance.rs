//! X2: one test per Gherkin scenario of CommandBridge.md *Acceptance Criteria* and of its amendment of
//! January 2026 (ADR-114), each named after its scenario.

mod support;

use std::collections::BTreeMap;

use ordin_command_bridge::StreamKind;
use ordin_journal::JournalEvent;
use ordin_types::time::Timestamp;
use serde_json::{json, Value};
use support::*;

fn journal_event(seq: u64) -> JournalEvent {
    JournalEvent {
        backend: uuid::Uuid::now_v7(),
        seq,
        at: Timestamp::now(),
        kind: "needs_you_changed".to_owned(),
        data: json!({ "workspace_id": WORKSPACE }),
    }
}

#[tokio::test]
async fn happy_path_a_local_command_reaches_the_desktop_core() {
    let f = fixture().await;
    let reply = f
        .invoke(
            "runs.stop",
            &envelope("runs.stop", LOCAL, json!({ "run_id": RUN })),
        )
        .await;
    assert_eq!(
        f.api.calls(),
        vec![Call {
            method: "runs_stop",
            local_screens: true
        }]
    );
    assert_eq!(reply["kind"], "ok");
    assert_eq!(reply["request_id"], REQUEST);
}

#[tokio::test]
async fn happy_path_a_servers_command_goes_to_the_server_connector() {
    let server_reply = r#"{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"ok","result":{"approval_id":"0192a1c3-0000-7000-8000-000000000001","state":"approved"}}"#;
    let stand_in = RemoteStandIn::start(Answer::Reply(server_reply.to_owned())).await;
    let f = fixture_with(Options {
        seeds: vec![Seed::new(stand_in.port, "connected")],
        ..Options::default()
    })
    .await;
    f.connected().await;
    let sent = envelope(
        "approval.decide",
        SERVER,
        json!({ "card_ref": "q8Zt3VxN0pLk2mRb7sYw1A", "decision": "approve" }),
    );
    let reply = f
        .bridge
        .invoke("main", "approval.decide", &sent)
        .await
        .expect("a reply");
    let commands = stand_in.commands();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].path, "/v1/approval/decide");
    assert_eq!(
        commands[0].body,
        sent.as_bytes(),
        "envelope bytes unchanged"
    );
    assert_eq!(reply, server_reply, "the server's reply bytes unchanged");
    assert!(f.api.calls().is_empty());
    f.cancel.cancel();
}

#[tokio::test]
async fn error_path_a_local_only_command_addressed_to_a_server() {
    let stand_in = RemoteStandIn::start(Answer::Reply("{}".to_owned())).await;
    let f = fixture_with(Options {
        seeds: vec![Seed::new(stand_in.port, "connected")],
        ..Options::default()
    })
    .await;
    f.connected().await;
    let reply = f
        .invoke(
            "servers.connect",
            &envelope("servers.connect", SERVER, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "SVC_COMMAND_LOCAL_ONLY");
    assert!(
        stand_in.commands().is_empty(),
        "the Server Connector is not called"
    );
    f.cancel.cancel();
}

#[tokio::test]
async fn error_path_the_server_is_not_connected() {
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
    assert!(reply["error"].get("action").is_none(), "action absent");
    f.cancel.cancel();
}

#[tokio::test]
async fn error_path_an_unknown_backend() {
    let f = fixture().await;
    let reply = f
        .invoke(
            "approval.decide",
            &envelope(
                "approval.decide",
                "0192ffff-0000-7000-8000-000000000000",
                json!({}),
            ),
        )
        .await;
    assert_eq!(reply["error"]["code"], "SVC_BACKEND_UNKNOWN");
    assert!(f.api.calls().is_empty());
}

#[tokio::test]
async fn happy_path_a_window_reload_catches_up_from_the_replay_buffer() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    // A window already open shows when the server task has read every event into the buffer.
    let (probe, _) = f.open("probe", BTreeMap::new()).await;
    for seq in 48200..=48213 {
        f.server_event(server(), seq).await;
    }
    probe.wait_for(14).await;

    let (window, opened) = f.open("main", BTreeMap::from([(server(), 48210)])).await;
    assert_eq!(opened["reset"], json!([]));
    window.wait_for(3).await;
    let first = window.batches.lock().unwrap()[0].clone();
    let seqs: Vec<u64> = first.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, vec![48211, 48212, 48213]);
    f.cancel.cancel();
}

#[tokio::test]
async fn edge_case_the_window_asks_for_events_older_than_the_buffer() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let (probe, _) = f.open("probe", BTreeMap::new()).await;
    for seq in 48200..=48213 {
        f.server_event(server(), seq).await;
    }
    probe.wait_for(14).await;
    let (window, opened) = f.open("main", BTreeMap::from([(server(), 47000)])).await;
    assert_eq!(opened["reset"], json!([SERVER]));
    // Live events follow.
    f.server_event(server(), 48214).await;
    let events = window.wait_for(1).await;
    assert_eq!(events[0]["seq"], 48214);
    assert_eq!(events[0]["backend"], SERVER);
    f.cancel.cancel();
}

#[tokio::test]
async fn edge_case_the_server_sends_reset() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let (window, _) = f.open("main", BTreeMap::new()).await;
    for seq in 1..=3 {
        f.server_event(server(), seq).await;
    }
    window.wait_for(3).await;
    f.server_reset(server()).await;
    let events = window.wait_for(4).await;
    let resets: Vec<&Value> = events.iter().filter(|e| e["kind"] == "reset").collect();
    assert_eq!(resets.len(), 1, "one reset envelope");
    assert_eq!(resets[0]["backend"], SERVER);
    assert_eq!(resets[0]["seq"], 3, "the last seq seen");
    // The buffer is empty: a reloaded window cannot be caught up from it.
    let (_reloaded, opened) = f.open("main", BTreeMap::from([(server(), 1)])).await;
    assert_eq!(opened["reset"], json!([SERVER]));
    f.cancel.cancel();
}

#[tokio::test]
async fn error_path_a_window_that_stops_reading() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .event_format(ordin_types::log::JsonLineFormat)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let f = fixture().await;
    let (window, _) = f.open("main", BTreeMap::new()).await;
    let sink = f.bridge.event_sink();
    // The window's flusher does not run while this loop holds the thread: the queue fills.
    for seq in 1..=8_192 {
        sink.deliver(&journal_event(seq)).await;
    }
    // One more event arrives.
    sink.deliver(&journal_event(8_193)).await;
    let events = window.wait_for(1).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let events_now = window.events();
    assert_eq!(events_now.len(), events.len(), "nothing but the resets");
    assert_eq!(events.len(), 1, "one reset envelope per backend");
    assert_eq!(events[0]["kind"], "reset");
    assert_eq!(events[0]["backend"], LOCAL);
    assert_eq!(events[0]["seq"], 8_193);
    let lines = capture.of("brg.window_lagging");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["code"], "BRG_WINDOW_LAGGING");
    // Later events flow again.
    sink.deliver(&journal_event(8_194)).await;
    let events = window.wait_for(2).await;
    assert_eq!(events[1]["seq"], 8_194);
}

#[tokio::test]
async fn error_path_an_oversized_envelope() {
    let f = fixture().await;
    let mut sent = envelope(
        "approval.decide",
        LOCAL,
        json!({ "card_ref": "q8Zt3VxN0pLk2mRb7sYw1A", "decision": "approve", "pad": "" }),
    );
    let pad = 1_048_577 - sent.len();
    sent = sent.replace("\"pad\":\"\"", &format!("\"pad\":\"{}\"", "x".repeat(pad)));
    assert_eq!(sent.len(), 1_048_577);
    let reply = f.invoke("approval.decide", &sent).await;
    assert_eq!(reply["error"]["code"], "SVC_BODY_TOO_LARGE");
    assert_eq!(reply["request_id"], Value::Null);
    assert!(f.api.calls().is_empty(), "no module is called");
}

#[tokio::test]
async fn happy_path_a_dictation_update_reaches_the_main_window_unjournalled() {
    let f = fixture().await;
    let window = StreamWindow::default();
    f.bridge.open_streams("main", window.out());
    let before = f.journal.last_seq();
    let session = uuid::Uuid::parse_str("0192b3c4-1111-7222-8333-444455556666").unwrap();
    f.bridge
        .streams()
        .send(
            session,
            StreamKind::Dictation,
            json!({ "kind": "partial", "text": "Please summarise" }),
        )
        .expect("queued");
    let messages = window.wait_for(1).await;
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0]["stream"],
        "0192b3c4-1111-7222-8333-444455556666"
    );
    assert_eq!(messages[0]["kind"], "dictation");
    assert_eq!(messages[0]["payload"]["text"], "Please summarise");
    assert_eq!(f.journal.last_seq(), before, "the Journal records nothing");
}

#[tokio::test]
async fn error_path_a_stream_message_with_no_window() {
    let f = fixture().await;
    let error = f
        .bridge
        .streams()
        .send(uuid::Uuid::now_v7(), StreamKind::Terminal, json!({}))
        .unwrap_err();
    assert_eq!(error.to_string(), "BRG_STREAM_NO_WINDOW");
}

#[tokio::test]
async fn happy_path_events_are_batched_in_order() {
    let f = fixture().await;
    let (window, _) = f.open("main", BTreeMap::new()).await;
    let sink = f.bridge.event_sink();
    // "Within 10 ms" is what keeps the flusher out: this test's one thread does the same, because
    // the flusher cannot run until the loop lets go of it.
    for seq in 1..=120 {
        sink.deliver(&journal_event(seq)).await;
    }
    window.wait_for(120).await;
    assert_eq!(window.sizes(), vec![50, 50, 20]);
    let seqs = window.seqs();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "strictly increasing");
}

#[tokio::test]
async fn a_command_of_a_group_this_build_does_not_answer() {
    static WITHOUT_MAIL: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    let groups = WITHOUT_MAIL.get_or_init(|| {
        every_group()
            .iter()
            .copied()
            .filter(|g| *g != "mail")
            .collect()
    });
    let f = fixture_with(Options {
        api: Mock::ok().with_groups(groups),
        ..Options::default()
    })
    .await;
    let reply = f
        .invoke(
            "mail.list_drafts",
            &envelope("mail.list_drafts", LOCAL, json!({ "workspace": WORKSPACE })),
        )
        .await;
    assert_eq!(reply["kind"], "error");
    assert_eq!(reply["error"]["code"], "SVC_NOT_IN_THIS_VERSION");
    assert!(f.api.calls().is_empty(), "no module is called");
}

#[tokio::test]
async fn a_command_of_a_group_this_build_answers() {
    static APPROVAL_ONLY: [&str; 2] = ["approval", "service"];
    let f = fixture_with(Options {
        api: Mock::new(|_, _| {
            Box::pin(async { Ok(json!({ "items": [], "next_cursor": "c29tZS1jdXJzb3I" })) })
        })
        .with_groups(&APPROVAL_ONLY),
        ..Options::default()
    })
    .await;
    let reply = f
        .invoke(
            "approval.list_pending",
            &envelope("approval.list_pending", LOCAL, json!({})),
        )
        .await;
    assert_eq!(f.api.calls().len(), 1, "the ServiceApi is called with it");
    assert_eq!(f.api.calls()[0].method, "approval_list_pending");
    assert_eq!(reply["kind"], "ok");
    assert_eq!(
        reply["result"],
        json!({ "items": [], "next_cursor": "c29tZS1jdXJzb3I" })
    );
}
