//! Internal Logic steps not named by a scenario or an Error Table row: the local catch-up of a
//! reloaded window (6.2), window cancellation (3), stream replacement and loss (6.1, 6.7, 8),
//! `subscribe_all`, and quit (`close`).

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use ordin_command_bridge::StreamKind;
use serde_json::json;
use support::*;

#[tokio::test]
async fn a_reloaded_window_catches_up_local_events_from_the_journal() {
    let f = fixture().await;
    for _ in 0..5 {
        f.append().await;
    }
    let (window, opened) = f.open("main", BTreeMap::from([(local(), 2)])).await;
    assert_eq!(opened["local_backend"], LOCAL);
    assert_eq!(opened["reset"], json!([]));
    window.wait_for(3).await;
    // Live events follow the catch-up, each seq once.
    f.append().await;
    window.wait_for(4).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(window.seqs(), vec![3, 4, 5, 6]);
}

#[tokio::test]
async fn a_local_after_beyond_the_journal_is_reset() {
    let f = fixture().await;
    let (probe, _) = f.open("probe", BTreeMap::new()).await;
    f.append().await;
    probe.wait_for(1).await;
    let (window, opened) = f.open("main", BTreeMap::from([(local(), 99)])).await;
    assert_eq!(opened["reset"], json!([LOCAL]));
    f.append().await;
    let events = window.wait_for(1).await;
    assert_eq!(events[0]["seq"], 2, "live events follow");
}

#[tokio::test]
async fn a_server_window_already_up_to_date_is_not_reset() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let (probe, _) = f.open("probe", BTreeMap::new()).await;
    f.server_event(server(), 10).await;
    probe.wait_for(1).await;
    let (_, opened) = f.open("main", BTreeMap::from([(server(), 10)])).await;
    assert_eq!(opened["reset"], json!([]));
    // A server never seen since start cannot be caught up.
    let f2 = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let (_, opened) = f2.open("main", BTreeMap::from([(server(), 10)])).await;
    assert_eq!(opened["reset"], json!([SERVER]));
    f.cancel.cancel();
    f2.cancel.cancel();
}

#[tokio::test]
async fn the_replay_buffer_keeps_the_newest_events_only() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        replay: 3,
        ..Options::default()
    })
    .await;
    let (probe, _) = f.open("probe", BTreeMap::new()).await;
    for seq in 1..=5 {
        f.server_event(server(), seq).await;
    }
    probe.wait_for(5).await;
    let (_, opened) = f.open("a", BTreeMap::from([(server(), 1)])).await;
    assert_eq!(opened["reset"], json!([SERVER]), "seq 2 is no longer kept");
    let (window, opened) = f.open("b", BTreeMap::from([(server(), 2)])).await;
    assert_eq!(opened["reset"], json!([]));
    window.wait_for(3).await;
    assert_eq!(window.seqs(), vec![3, 4, 5]);
    f.cancel.cancel();
}

#[tokio::test]
async fn a_second_call_from_the_same_window_replaces_its_stream() {
    let f = fixture().await;
    let (first, _) = f.open("main", BTreeMap::new()).await;
    let (second, _) = f.open("main", BTreeMap::new()).await;
    f.append().await;
    second.wait_for(1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(first.events().is_empty());
}

#[tokio::test]
async fn a_window_whose_channel_fails_loses_its_stream() {
    let f = fixture().await;
    let (window, _) = f.open("main", BTreeMap::new()).await;
    *window.gone.lock().unwrap() = true;
    f.append().await;
    assert!(
        eventually(Duration::from_secs(10), || async {
            *window.refused.lock().unwrap() > 0
        })
        .await,
        "the failed send is tried"
    );
    *window.gone.lock().unwrap() = false;
    f.append().await;
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(window.events().is_empty(), "the stream was closed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_destroyed_window_cancels_its_local_command() {
    let f = fixture_with(Options {
        api: Mock::slow(Duration::from_secs(5)),
        ..Options::default()
    })
    .await;
    let bridge = f.bridge.clone();
    let call = tokio::spawn(async move {
        bridge
            .invoke(
                "main",
                "runs.stop",
                &envelope("runs.stop", LOCAL, json!({ "run_id": RUN })),
            )
            .await
    });
    assert!(
        eventually(Duration::from_secs(5), || async {
            !f.api.calls().is_empty()
        })
        .await,
        "the command runs"
    );
    let (events, _) = f.open("main", BTreeMap::new()).await;
    let streams = StreamWindow::default();
    f.bridge.open_streams("main", streams.out());
    f.bridge.window_destroyed("main");
    let reply = tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .expect("the call ends at once")
        .expect("joined");
    assert_eq!(reply, None, "the reply is dropped");
    // Its streams closed too.
    f.append().await;
    assert!(f
        .bridge
        .streams()
        .send(uuid::Uuid::now_v7(), StreamKind::Lsp, json!({}))
        .is_err());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(events.events().is_empty());
}

#[tokio::test]
async fn subscribe_all_receives_local_and_server_events() {
    let f = fixture_with(Options {
        seeds: vec![Seed::new(closed_port().await, "disconnected")],
        ..Options::default()
    })
    .await;
    let mut all = f.bridge.subscribe_all();
    f.append().await;
    f.server_event(server(), 41).await;
    f.server_reset(server()).await;
    let mut seen = Vec::new();
    for _ in 0..3 {
        let envelope = tokio::time::timeout(Duration::from_secs(5), all.recv())
            .await
            .expect("in time")
            .expect("an envelope");
        seen.push((
            envelope.backend.to_string(),
            envelope.seq,
            envelope.event.kind(),
        ));
    }
    assert_eq!(seen[0].0, LOCAL);
    assert_eq!(seen[0].1, 1);
    assert_eq!(seen[1].0, SERVER);
    assert_eq!(seen[1].1, 41);
    assert_eq!(seen[2].1, 41, "the reset carries the last seq seen");
    f.cancel.cancel();
}

#[tokio::test]
async fn the_streams_close_message_and_replacement() {
    let f = fixture().await;
    let first = StreamWindow::default();
    f.bridge.open_streams("main", first.out());
    let streams = f.bridge.streams();
    let stream = uuid::Uuid::now_v7();
    streams
        .send(stream, StreamKind::Terminal, json!({ "bytes_b64": "aGk" }))
        .unwrap();
    streams.close(stream, StreamKind::Terminal, "exited");
    let messages = first.wait_for(2).await;
    assert_eq!(messages[1]["payload"], json!({ "closed": "exited" }));
    assert_eq!(messages[1]["kind"], "terminal");
    // A second service_streams call replaces the first.
    let second = StreamWindow::default();
    f.bridge.open_streams("main", second.out());
    streams.send(stream, StreamKind::Lsp, json!({})).unwrap();
    second.wait_for(1).await;
    assert_eq!(first.all().len(), 2);
    // A window whose channel fails loses its stream.
    *second.gone.lock().unwrap() = true;
    streams.send(stream, StreamKind::Lsp, json!({})).unwrap();
    assert!(
        eventually(Duration::from_secs(5), || async {
            streams.send(stream, StreamKind::Lsp, json!({})).is_err()
        })
        .await
    );
    // A stream only another window opened does not feed ScreenStreams.
    f.bridge
        .open_streams("settings", StreamWindow::default().out());
    assert!(streams.send(stream, StreamKind::Lsp, json!({})).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn close_waits_for_commands_in_flight_then_closes_every_stream() {
    let f = fixture_with(Options {
        api: Mock::slow(Duration::from_millis(300)),
        ..Options::default()
    })
    .await;
    let bridge = f.bridge.clone();
    let call = tokio::spawn(async move {
        bridge
            .invoke(
                "main",
                "runs.stop",
                &envelope("runs.stop", LOCAL, json!({ "run_id": RUN })),
            )
            .await
    });
    assert!(
        eventually(Duration::from_secs(5), || async {
            !f.api.calls().is_empty()
        })
        .await
    );
    let (window, _) = f.open("main", BTreeMap::new()).await;
    let streams = StreamWindow::default();
    f.bridge.open_streams("main", streams.out());
    let started = std::time::Instant::now();
    f.bridge.close().await;
    assert!(started.elapsed() >= Duration::from_millis(150), "it waited");
    assert!(started.elapsed() < Duration::from_secs(2));
    let reply = call.await.unwrap().expect("the command in flight finished");
    assert!(reply.contains("\"kind\":\"ok\""));
    // Every stream is closed and the Journal no longer feeds the Bridge.
    f.append().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(window.events().is_empty());
    assert!(f
        .bridge
        .streams()
        .send(uuid::Uuid::now_v7(), StreamKind::Lsp, json!({}))
        .is_err());
}

#[tokio::test]
async fn close_gives_up_after_two_seconds() {
    let f = fixture_with(Options {
        api: Mock::slow(Duration::from_secs(30)),
        // The 2 s wait becomes 100 ms.
        time_scale: 0.05,
        ..Options::default()
    })
    .await;
    let bridge = f.bridge.clone();
    let _call = tokio::spawn(async move {
        bridge
            .invoke(
                "main",
                "runs.stop",
                &envelope("runs.stop", LOCAL, json!({ "run_id": RUN })),
            )
            .await
    });
    assert!(
        eventually(Duration::from_secs(5), || async {
            !f.api.calls().is_empty()
        })
        .await
    );
    let started = std::time::Instant::now();
    f.bridge.close().await;
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn start_twice_subscribes_once() {
    let f = fixture().await;
    f.bridge.start(f.cancel.clone());
    let (window, _) = f.open("main", BTreeMap::new()).await;
    f.append().await;
    window.wait_for(1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(window.seqs(), vec![1]);
}

#[tokio::test]
async fn a_server_reply_that_is_an_error_is_passed_through_unchanged() {
    let body = r#"{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"error","error":{"code":"APPR_CARD_CLOSED","message":"This card is closed."}}"#;
    let stand_in = RemoteStandIn::start(Answer::Reply(body.to_owned())).await;
    let f = fixture_with(Options {
        seeds: vec![Seed::new(stand_in.port, "connected")],
        ..Options::default()
    })
    .await;
    f.connected().await;
    let reply = f
        .bridge
        .invoke(
            "main",
            "approval.decide",
            &envelope("approval.decide", SERVER, json!({})),
        )
        .await
        .unwrap();
    assert_eq!(reply, body);
    // An unknown command name is the Service API's refusal, whatever the backend.
    let reply = f
        .invoke("nothing.here", &envelope("nothing.here", SERVER, json!({})))
        .await;
    assert_eq!(reply["error"]["code"], "SVC_COMMAND_UNKNOWN");
    // A reply from the server with a status the Connector reads as a refusal of the key.
    *stand_in.answer.lock().unwrap() = Answer::Status(401, "{}".to_owned());
    let reply = f
        .invoke(
            "approval.decide",
            &envelope("approval.decide", SERVER, json!({})),
        )
        .await;
    assert_eq!(reply["error"]["code"], "CONN_KEY_REFUSED");
    f.cancel.cancel();
}
