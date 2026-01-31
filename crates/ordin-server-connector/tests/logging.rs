//! Logging (ServerConnector.md *Security*, *Observability*): the log lines carry the server connection
//! id, host, port-free state changes, status codes and sequence numbers; never the desktop key, the
//! pairing code, the pairing line, command bodies, replies or event data. The events named in the spec
//! appear.

mod support;

use std::time::{Duration, Instant};

use ordin_service_api::commands::ConnectServer;
use serde_json::json;
use support::sse::{frame, Session, SseStandIn, Step};
use support::*;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const AT: &str = "2026-01-10T10:42:00.000Z";
const NOTE: &str = "top-secret-note-4711";
const WORKSPACE: &str = "0192a1d0-1111-7222-8333-444455556667";

fn assert_clean(capture: &Capture, extra: &[&str]) {
    let text = capture.text().to_lowercase();
    let mut never = vec![
        KEY_TEXT,
        CODE,
        "K7QM-2XPA",
        "bearer",
        "authorization",
        "ordin-pair",
        NOTE,
        WORKSPACE,
        "not json at all",
    ];
    never.extend_from_slice(extra);
    for needle in never {
        assert!(
            !text.contains(&needle.to_lowercase()),
            "{needle} is in the log:\n{}",
            capture.text()
        );
    }
}

#[tokio::test]
async fn pairing_commands_and_disconnect_log_ids_and_codes_only() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    Mock::given(method("POST"))
        .and(path("/v1/approval/decide"))
        .respond_with(json_response(200, &reply_ok("r1")))
        .mount(&env.mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/pairing/end"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&env.mock)
        .await;
    let connector = env.connector(1.0);
    let view = connector
        .connect(ConnectServer::PairingLine {
            line: format!(
                "ordin-pair:1:127.0.0.1:{}:{}:{CODE}",
                env.mock_port(),
                "9f".repeat(32)
            ),
        })
        .await;
    // The pin does not matter on plain HTTP; a pairing line still pairs.
    let view = view.expect("pairing");
    connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({"note": NOTE})),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .expect("a reply");
    connector
        .disconnect(view.server_connection_id)
        .await
        .expect("disconnect");

    let paired = capture.of("conn.paired");
    assert_eq!(paired.len(), 1);
    assert_eq!(
        paired[0]["server_connection_id"],
        view.server_connection_id.to_string()
    );
    assert_eq!(paired[0]["host"], "127.0.0.1");
    assert_eq!(paired[0]["component"], "server_connector");

    let command = capture.of("conn.command");
    assert_eq!(command.len(), 1);
    assert_eq!(command[0]["command"], "approval.decide");
    assert_eq!(command[0]["status"], 200);
    assert_eq!(command[0]["retries"], 0);
    assert_eq!(command[0]["level"], "DEBUG");

    let states = capture.of("conn.state");
    assert_eq!(states.last().unwrap()["from"], "connected");
    assert_eq!(states.last().unwrap()["to"], "disconnected");
    assert!(states.last().unwrap().get("reason_code").is_some());

    let unconfirmed = capture.of("conn.end_unconfirmed");
    assert_eq!(unconfirmed.len(), 1);
    assert_eq!(
        unconfirmed[0]["server_connection_id"],
        view.server_connection_id.to_string()
    );
    assert_clean(&capture, &[]);
}

#[tokio::test]
async fn stream_frames_log_sequence_numbers_never_their_data() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    let data = |seq: u64| event_data(seq, "needs_you_changed", AT);
    let stand_in = SseStandIn::start(vec![Session::ok(vec![
        frame(1, "needs_you_changed", data(1)),
        frame(2, "needs_you_changed", "not json at all".to_owned()),
        frame(5, "needs_you_changed", data(5)),
        Step::Hold,
    ])])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let connector = env.connector(1.0);
    let (tx, mut rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    let got = recv_n(&mut rx, 3, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["1", "reset", "5"]);

    let unreadable = capture.of("conn.event_unreadable");
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0]["seq"], 2);
    assert_eq!(unreadable[0]["code"], "CONN_EVENT_UNREADABLE");
    let gap = capture.of("conn.event_gap");
    assert_eq!(gap.len(), 1);
    assert_eq!(gap[0]["code"], "SVC_EVENT_GAP");
    assert_eq!(gap[0]["expected"], 2);
    assert_eq!(gap[0]["got"], 5);
    assert_clean(&capture, &[]);
    cancel.cancel();
}
