//! The event stream against a hand-written loopback SSE stand-in that honours RemoteApi.md's
//! event-stream contract (`id:` / `event:` / `data:` frames, `: heartbeat` comments every 20 s, a first
//! `reset` frame for a stream with no `Last-Event-ID`, resume from `Last-Event-ID`). Every wait of the
//! spec is multiplied by a small scale so that the 20 s comment and the 45 s silence rule can be
//! exercised in real time.
//!
//! C-71 (SSE over HTTP/2 passes company proxies with 20 s comments) is a bound assertion that this crate
//! cannot confirm; the tests below prove the two sides of the fallback the spec names: comments every
//! 20 s keep the stream open, and a stream a proxy cuts silent costs one reconnect with `Last-Event-ID`
//! and loses no event.

mod support;

use std::time::{Duration, Instant};

use ordin_server_connector::{ConnectorError, ServerConnector};
use ordin_service_api::commands::ServerView;
use ordin_types::domain::server::ServerConnectionState;
use serde_json::json;
use sqlx::Row as _;
use support::sse::{frame, Session, SseStandIn, Step};
use support::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const AT: &str = "2026-01-10T10:42:00.000Z";

fn live(seq: u64) -> Step {
    frame(
        seq,
        "needs_you_changed",
        event_data(seq, "needs_you_changed", AT),
    )
}

struct Running {
    connector: ServerConnector,
    rx: mpsc::Receiver<ServerEvent>,
    cancel: CancellationToken,
}

use ordin_server_connector::ServerEvent;

async fn run_with(env: &Env, scale: f64, capacity: usize) -> Running {
    let connector = env.connector(scale);
    let (tx, rx) = mpsc::channel(capacity);
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    Running {
        connector,
        rx,
        cancel,
    }
}

async fn views(r: &Running) -> Vec<ServerView> {
    r.connector.list().await.expect("list")
}

#[tokio::test]
async fn a_first_stream_starts_with_reset_and_comments_keep_it_open() {
    let env = Env::new().await;
    // 20 s comments, scaled by 0.01 to 200 ms, for 1.4 s: longer than the scaled 45 s silence of 450 ms.
    let mut steps = vec![frame(100, "reset", event_data(100, "reset", AT))];
    for seq in 101..=103 {
        steps.push(Step::Heartbeat);
        steps.push(Step::Wait(Duration::from_millis(200)));
        steps.push(live(seq));
        steps.push(Step::Wait(Duration::from_millis(200)));
    }
    steps.push(Step::Hold);
    let stand_in = SseStandIn::start(vec![Session::ok(steps)]).await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let mut run = run_with(&env, 0.01, 16).await;
    let got = recv_n(&mut run.rx, 4, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["reset", "101", "102", "103"]);
    // The stream was never dropped: one request, no Last-Event-ID on the first stream, still connected.
    assert_eq!(stand_in.events_requests().len(), 1);
    assert_eq!(stand_in.events_requests()[0].last_event_id, None);
    assert_eq!(views(&run).await[0].state, ServerConnectionState::Connected);
    assert!(env
        .state_events()
        .await
        .iter()
        .all(|e| e.data["state"] == "connected"));
    run.cancel.cancel();
}

#[tokio::test]
async fn a_proxy_that_cuts_the_stream_costs_one_reconnect_and_no_event() {
    let env = Env::new().await;
    // No comments arrive (a proxy that holds the stream silent), then the server accepts the retry.
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![live(1), live(2), live(3), Step::Hold]),
        Session::ok(vec![live(4), live(5), Step::Hold]),
    ])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let mut run = run_with(&env, 0.01, 16).await;
    let got = recv_n(&mut run.rx, 5, Duration::from_secs(10)).await;
    assert_eq!(
        summary(&got),
        ["1", "2", "3", "4", "5"],
        "in order, none lost, none repeated"
    );
    let requests = stand_in.events_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].last_event_id.as_deref(), Some("3"));
    let states: Vec<_> = env
        .state_events()
        .await
        .iter()
        .map(|e| e.data["state"].clone())
        .collect();
    assert_eq!(
        states,
        [
            json!("connected"),
            json!("not_connected"),
            json!("connected")
        ]
    );
    run.cancel.cancel();
}

#[tokio::test]
async fn late_and_repeated_frames_are_ignored() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![Session::ok(vec![
        live(1),
        live(2),
        live(2),
        live(1),
        live(3),
        Step::Hold,
    ])])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let mut run = run_with(&env, 1.0, 16).await;
    let got = recv_n(&mut run.rx, 3, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["1", "2", "3"]);
    assert!(recv_n(&mut run.rx, 1, Duration::from_millis(200))
        .await
        .is_empty());
    run.cancel.cancel();
}

#[tokio::test]
async fn a_full_channel_slows_the_stream_and_drops_nothing() {
    let env = Env::new().await;
    let mut steps: Vec<Step> = (1..=30).map(live).collect();
    steps.push(Step::Hold);
    let stand_in = SseStandIn::start(vec![Session::ok(steps)]).await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let mut run = run_with(&env, 1.0, 1).await;
    let mut seen = Vec::new();
    while seen.len() < 30 {
        let got = recv_n(&mut run.rx, 1, Duration::from_secs(5)).await;
        assert_eq!(got.len(), 1, "an event is still to come after {seen:?}");
        seen.extend(summary(&got));
        tokio::time::sleep(Duration::from_millis(3)).await;
    }
    let want: Vec<String> = (1..=30).map(|n| n.to_string()).collect();
    assert_eq!(seen, want);
    run.cancel.cancel();
}

#[tokio::test]
async fn a_stream_that_ends_is_reopened_with_the_last_event_id() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![live(1), Step::Close]),
        Session::ok(vec![live(2), Step::Hold]),
    ])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let mut run = run_with(&env, 0.01, 16).await;
    let got = recv_n(&mut run.rx, 2, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["1", "2"]);
    assert_eq!(
        stand_in.events_requests()[1].last_event_id.as_deref(),
        Some("1")
    );
    run.cancel.cancel();
}

#[tokio::test]
async fn a_locked_address_waits_ten_minutes_of_the_scaled_clock() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::status(429, &error_body("DESK_ADDRESS_LOCKED").to_string()),
        Session::ok(vec![live(1), Step::Hold]),
    ])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let started = Instant::now();
    let mut run = run_with(&env, 0.001, 16).await;
    let got = recv_n(&mut run.rx, 1, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["1"]);
    // Ten minutes times 0.001 is 0.6 s.
    assert!(
        started.elapsed() >= Duration::from_millis(550),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(stand_in.events_requests().len(), 2);
    run.cancel.cancel();
}

#[tokio::test]
async fn a_conflict_refuses_the_server_for_its_version_and_stops() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![Session::status(
        409,
        &error_body("SVC_VERSION_UNSUPPORTED").to_string(),
    )])
    .await;
    let id = env.seed(Seed::new(stand_in.port, "connected")).await;
    let run = run_with(&env, 0.01, 4).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            state_of(&env, id).await == "refused_for_version"
        })
        .await
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        stand_in.events_requests().len(),
        1,
        "the supervisor stopped"
    );
    assert_eq!(
        views(&run).await[0].older_side,
        Some(ordin_service_api::commands::OlderSide::Desktop)
    );
    run.cancel.cancel();
}

#[tokio::test]
async fn an_unauthorised_stream_disconnects_the_server_and_stops() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![Session::status(
        401,
        &error_body("SVC_UNAUTHENTICATED").to_string(),
    )])
    .await;
    let id = env.seed(Seed::new(stand_in.port, "connected")).await;
    let run = run_with(&env, 0.01, 4).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            state_of(&env, id).await == "disconnected"
        })
        .await
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(stand_in.events_requests().len(), 1);
    run.cancel.cancel();
}

#[tokio::test]
async fn an_answer_without_a_version_header_is_retried() {
    let env = Env::new().await;
    let mut headerless = Session::ok(vec![Step::Hold]);
    headerless.version = None;
    let stand_in =
        SseStandIn::start(vec![headerless, Session::ok(vec![live(1), Step::Hold])]).await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let mut run = run_with(&env, 0.01, 4).await;
    let got = recv_n(&mut run.rx, 1, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["1"]);
    assert_eq!(stand_in.events_requests().len(), 2);
    run.cancel.cancel();
}

#[tokio::test]
async fn the_sequence_number_is_persisted_while_the_stream_runs() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![Session::ok(vec![
        live(1),
        live(2),
        live(3),
        Step::Hold,
    ])])
    .await;
    let id = env.seed(Seed::new(stand_in.port, "connected")).await;
    // Five seconds times 0.01 is 50 ms.
    let mut run = run_with(&env, 0.01, 16).await;
    let _ = recv_n(&mut run.rx, 3, Duration::from_secs(5)).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            env.row(id)
                .await
                .unwrap()
                .get::<i64, _>("last_sequence_received")
                == 3
        })
        .await
    );
    run.cancel.cancel();
}

#[tokio::test]
async fn a_key_missing_from_the_keychain_disconnects_at_start() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![Session::ok(vec![Step::Hold])]).await;
    let mut seed = Seed::new(stand_in.port, "connected");
    seed.with_key = false;
    let id = env.seed(seed).await;
    let run = run_with(&env, 1.0, 4).await;
    assert_eq!(state_of(&env, id).await, "disconnected");
    assert_eq!(
        views(&run).await[0].state,
        ServerConnectionState::Disconnected
    );
    assert_eq!(
        env.state_events().await.last().unwrap().data["state"],
        "disconnected"
    );
    assert!(stand_in.requests().is_empty());
    run.cancel.cancel();
}

#[tokio::test]
async fn a_server_paired_after_start_gets_its_own_stream() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::status(200, &pair_reply(SERVER_NAME, KEY_TEXT, (1, 0)).to_string()),
        Session::ok(vec![live(1), Step::Hold]),
    ])
    .await;
    let mut run = run_with(&env, 1.0, 4).await;
    run.connector
        .connect(ordin_service_api::commands::ConnectServer::AddressAndCode {
            address: format!("127.0.0.1:{}", stand_in.port),
            code: "K7QM-2XPA".to_owned(),
        })
        .await
        .expect("pairing");
    let got = recv_n(&mut run.rx, 1, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["1"]);
    run.cancel.cancel();
}

#[tokio::test]
async fn disconnecting_ends_the_pairing_and_stops_the_stream() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![Step::Hold]),
        Session::status(200, &json!({"kind": "ok", "result": {}}).to_string()),
    ])
    .await;
    let id = env.seed(Seed::new(stand_in.port, "connected")).await;
    let run = run_with(&env, 0.01, 4).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            stand_in.events_requests().len() == 1
        })
        .await
    );
    run.connector.disconnect(id).await.expect("disconnect");
    let end: Vec<_> = stand_in
        .requests()
        .into_iter()
        .filter(|r| r.path == "/v1/pairing/end")
        .collect();
    assert_eq!(end.len(), 1);
    assert_eq!(
        end[0].authorization.as_deref(),
        Some(format!("Bearer {KEY_TEXT}").as_str())
    );
    assert_eq!(state_of(&env, id).await, "disconnected");
    assert_eq!(env.keys.count(), 0);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        stand_in.events_requests().len(),
        1,
        "no reconnect after the disconnect"
    );
    // Disconnecting twice is not an error, and a send now says the key was refused.
    run.connector
        .disconnect(id)
        .await
        .expect("already disconnected");
    let refused = run
        .connector
        .send_command(
            backend(),
            meta("workspaces.list"),
            envelope("workspaces.list", "r", json!({})),
            Instant::now() + Duration::from_secs(1),
        )
        .await;
    assert!(matches!(refused, Err(ConnectorError::KeyRefused)));
    run.cancel.cancel();
}

#[tokio::test]
async fn a_wake_drops_a_connected_stream_that_has_gone_quiet() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![live(1), Step::Hold]),
        Session::ok(vec![live(2), Step::Hold]),
    ])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    // Scale 0.05: a stream silent for more than 1 s (20 s) is stale; 2.25 s (45 s) is the silence limit.
    let mut run = run_with(&env, 0.05, 16).await;
    let _ = recv_n(&mut run.rx, 1, Duration::from_secs(5)).await;
    // A wake while the stream is fresh does nothing.
    run.connector.on_wake();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(stand_in.events_requests().len(), 1);
    assert_eq!(capture.of("conn.wake")[0]["woken"], 0);
    // Past 20 s of the scaled clock, still short of 45 s: the wake drops the stream and reconnects.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    run.connector.on_wake();
    let got = recv_n(&mut run.rx, 1, Duration::from_secs(1)).await;
    assert_eq!(summary(&got), ["2"]);
    assert_eq!(
        stand_in.events_requests()[1].last_event_id.as_deref(),
        Some("1")
    );
    assert_eq!(capture.of("conn.wake")[1]["woken"], 1);
    run.cancel.cancel();
}

#[tokio::test]
async fn a_wake_before_start_or_after_quit_does_nothing() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    let connector = env.connector(1.0);
    connector.on_wake();
    let (tx, _rx) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    cancel.cancel();
    connector.on_wake();
    assert!(capture.of("conn.wake").is_empty());
}

#[tokio::test]
async fn a_command_that_loses_its_connection_makes_the_supervisor_probe() {
    let env = Env::new().await;
    // The stream is up; the command connection is dropped three times (one try and two retries).
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![Step::Hold]),
        Session::dropped(),
        Session::dropped(),
        Session::dropped(),
        Session::ok(vec![Step::Hold]),
    ])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let run = run_with(&env, 0.1, 4).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            stand_in.events_requests().len() == 1
        })
        .await
    );
    let err = run
        .connector
        .send_command(
            backend(),
            meta("workspaces.list"),
            envelope("workspaces.list", "r1", json!({})),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::Unreachable));
    let commands = stand_in
        .requests()
        .into_iter()
        .filter(|r| r.path == "/v1/workspaces/list")
        .count();
    assert_eq!(commands, 3, "a read is tried once and retried twice");
    // The supervisor was told to probe: it opens the stream again at once.
    assert!(
        eventually(Duration::from_secs(2), || async {
            stand_in.events_requests().len() == 2
        })
        .await
    );
    run.cancel.cancel();
}
