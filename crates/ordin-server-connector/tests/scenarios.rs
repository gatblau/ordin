//! X2: one test per Gherkin scenario of ServerConnector.md, named after the scenario.

mod support;

use std::time::{Duration, Instant};

use ordin_server_connector::ConnectorError;
use ordin_service_api::commands::{ConnectServer, OlderSide};
use ordin_service_api::ServiceApiVersion;
use ordin_types::domain::server::ServerConnectionState;
use ordin_types::ServerConnectionId;
use serde_json::{json, Value};
use sqlx::Row as _;
use support::sse::{frame, Session, SseStandIn, Step};
use support::*;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::Mock;

const AT: &str = "2026-01-10T10:42:00.000Z";

fn address_and_code(address: &str, code: &str) -> ConnectServer {
    ConnectServer::AddressAndCode {
        address: address.to_owned(),
        code: code.to_owned(),
    }
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn happy_path_pairing_with_an_address_and_a_code() {
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    let connector = env.connector(1.0);
    // The code is typed in lower case with a hyphen, as the setup command shows it.
    let view = connector
        .connect(address_and_code(&env.mock_address(), "k7qm-2xpa"))
        .await
        .expect("pairing");

    let requests = env.mock.received_requests().await.unwrap();
    let pairing = requests
        .iter()
        .find(|r| r.url.path() == "/v1/pairing")
        .expect("a pairing request");
    let body: Value = serde_json::from_slice(&pairing.body).unwrap();
    assert_eq!(
        body,
        json!({"api_version": {"major": 1, "minor": 0}, "code": "K7QM2XPA", "computer_name": "Asha's laptop"})
    );

    let id = view.server_connection_id;
    let keychain_ref = format!("server-connector/desktop-key/{id}");
    assert!(env.keys.has(&keychain_ref));
    assert_eq!(env.keys.items.lock().unwrap()[&keychain_ref], KEY_BYTES);

    let row = env.row(id).await.expect("a row");
    assert_eq!(row.get::<String, _>("state"), "connected");
    assert_eq!(row.get::<i64, _>("port"), i64::from(env.mock_port()));
    assert_eq!(row.get::<String, _>("keychain_ref"), keychain_ref);
    assert_eq!(
        row.get::<Option<String>, _>("service_api_version")
            .as_deref(),
        Some("1.0")
    );
    assert_eq!(row.get::<i64, _>("last_sequence_received"), 0);

    let events = env.state_events().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data["state"], "connected");
    assert_eq!(events[0].data["server_connection_id"], id.to_string());
    assert_eq!(view.state, ServerConnectionState::Connected);
    assert_eq!(view.display_name, SERVER_NAME);
    assert_eq!(view.backend, Some(backend()));
    assert_eq!(connector.connection_for(backend()), Some(id));
    assert_eq!(
        connector.display_name(backend()).as_deref(),
        Some(SERVER_NAME)
    );
}

#[tokio::test]
async fn happy_path_pairing_with_a_pairing_line_pins_the_certificate() {
    let env = Env::new().await;
    let server = tls::TlsStandIn::start(
        "a",
        200,
        pair_reply(SERVER_NAME, KEY_TEXT, (1, 0)).to_string(),
    )
    .await;
    let fingerprint = tls::fingerprint("a");
    let line = format!(
        "ordin-pair:1:127.0.0.1:{}:{fingerprint}:{CODE}",
        server.port
    );
    let view = env
        .tls_connector(env.config(), 1.0)
        .connect(ConnectServer::PairingLine { line })
        .await
        .expect("a certificate no authority signed is accepted because the fingerprint matches");
    let pin: String = sqlx::query(
        "SELECT fingerprint_sha256 FROM server_connection_pins WHERE server_connection_id = ?1",
    )
    .bind(view.server_connection_id.to_string())
    .fetch_one(&env.pool)
    .await
    .expect("a pin row")
    .get(0);
    assert_eq!(pin, fingerprint);
    assert_eq!(state_of(&env, view.server_connection_id).await, "connected");
}

#[tokio::test]
async fn error_path_a_wrong_code() {
    let env = Env::new().await;
    mock_pairing(
        &env.mock,
        json_response(403, &error_body("DESK_CODE_INVALID")),
    )
    .await;
    let err = env
        .connector(1.0)
        .connect(address_and_code(&env.mock_address(), "K7QM-2XPA"))
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::CodeRefused));
    assert_eq!(ordin_types::CodedError::code(&err), "CONN_CODE_REFUSED");
    assert_eq!(env.count("server_connections").await, 0);
    assert_eq!(env.keys.count(), 0);
}

#[tokio::test]
async fn error_path_the_server_already_has_ten_desktops() {
    let env = Env::new().await;
    mock_pairing(&env.mock, json_response(403, &error_body("DESK_TOO_MANY"))).await;
    let err = env
        .connector(1.0)
        .connect(address_and_code(&env.mock_address(), "K7QM-2XPA"))
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::TooManyDesktops));
    assert_eq!(
        ordin_types::CodedError::code(&err),
        "CONN_TOO_MANY_DESKTOPS"
    );
    assert_eq!(env.count("server_connections").await, 0);
    assert_eq!(env.count("server_connection_backends").await, 0);
    assert_eq!(env.keys.count(), 0);
}

#[tokio::test]
async fn error_path_the_pinned_certificate_changes() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    // The server now presents certificate b; the stored pin is certificate a's fingerprint.
    let server = tls::TlsStandIn::start(
        "b",
        200,
        pair_reply(SERVER_NAME, KEY_TEXT, (1, 0)).to_string(),
    )
    .await;
    let mut seed = Seed::new(server.port, "connected");
    seed.pin = Some(tls::fingerprint("a"));
    let id = env.seed(seed).await;
    let connector = env.tls_connector(env.config(), 0.001);
    let (tx, _rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    assert!(
        eventually(Duration::from_secs(5), || async {
            !capture.of("conn.reconnect").is_empty()
        })
        .await,
        "the supervisor tried and failed"
    );
    let views = connector.list().await.expect("list");
    assert_eq!(views[0].server_connection_id, id);
    assert_eq!(views[0].state, ServerConnectionState::NotConnected);
    assert!(env
        .state_events()
        .await
        .iter()
        .all(|e| e.data["state"] != "connected"));
    cancel.cancel();

    // A new pairing with the old fingerprint is refused with its own code.
    let line = format!(
        "ordin-pair:1:127.0.0.1:{}:{}:{CODE}",
        server.port,
        tls::fingerprint("a")
    );
    let fresh = Env::new().await;
    let err = fresh
        .tls_connector(fresh.config(), 1.0)
        .connect(ConnectServer::PairingLine { line })
        .await
        .unwrap_err();
    assert!(
        matches!(err, ConnectorError::FingerprintMismatch),
        "{err:?}"
    );
    assert_eq!(
        ordin_types::CodedError::code(&err),
        "CONN_FINGERPRINT_MISMATCH"
    );
}

#[tokio::test]
async fn edge_case_45_seconds_of_silence() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    // One stream that delivers an event and then says nothing; afterwards the server refuses connections.
    let stand_in = SseStandIn::start(vec![Session::ok(vec![
        frame(
            1,
            "needs_you_changed",
            event_data(1, "needs_you_changed", AT),
        ),
        Step::Hold,
    ])])
    .await;
    let id = env.seed(Seed::new(stand_in.port, "connected")).await;
    let connector = env.connector(0.01);
    let (tx, _rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");

    assert!(
        eventually(Duration::from_secs(10), || async {
            capture.of("conn.reconnect").len() >= 7
        })
        .await,
        "seven reconnect waits: {:?}",
        capture.of("conn.reconnect")
    );
    let view = connector.list().await.expect("list").remove(0);
    assert_eq!(view.state, ServerConnectionState::NotConnected);
    assert_eq!(view.as_of.expect("as_of").to_wire(), AT);
    assert_eq!(state_of(&env, id).await, "not_connected");

    // The waits are 1, 2, 4, 8, 16, 32 and 60 seconds, each within +-20 %, times the stand-in's 0.01.
    let base_ms = [10.0_f64, 20.0, 40.0, 80.0, 160.0, 320.0, 600.0];
    let lines = capture.of("conn.reconnect");
    for (i, base) in base_ms.iter().enumerate() {
        let attempt = lines[i]["attempt"].as_u64().expect("attempt");
        let delay = lines[i]["delay_ms"].as_f64().expect("delay_ms");
        assert_eq!(attempt, i as u64 + 1);
        assert!(
            delay >= (base * 0.8).floor() && delay <= (base * 1.2).ceil(),
            "attempt {attempt}: {delay} ms"
        );
    }
    let events = env.state_events().await;
    assert_eq!(events.last().unwrap().data["state"], "not_connected");
    assert_eq!(events.last().unwrap().data["as_of"], AT);
    cancel.cancel();
}

#[tokio::test]
async fn happy_path_catching_up_after_reconnecting() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![Session::ok(vec![
        frame(
            48213,
            "needs_you_changed",
            event_data(48213, "needs_you_changed", AT),
        ),
        frame(
            48214,
            "needs_you_changed",
            event_data(48214, "needs_you_changed", AT),
        ),
        frame(
            48215,
            "needs_you_changed",
            event_data(48215, "needs_you_changed", AT),
        ),
        Step::Hold,
    ])])
    .await;
    let mut seed = Seed::new(stand_in.port, "connected");
    seed.last_seq = 48212;
    let id = env.seed(seed).await;
    let connector = env.connector(1.0);
    let (tx, mut rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    let got = recv_n(&mut rx, 3, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["48213", "48214", "48215"]);
    let request = &stand_in.events_requests()[0];
    assert_eq!(request.last_event_id.as_deref(), Some("48212"));
    assert_eq!(
        request.authorization.as_deref(),
        Some(format!("Bearer {KEY_TEXT}").as_str())
    );
    cancel.cancel();
    // On quit the sequence number reached is persisted, and connected is written as not_connected.
    assert!(
        eventually(Duration::from_secs(5), || async {
            let row = env.row(id).await.unwrap();
            row.get::<i64, _>("last_sequence_received") == 48215
                && row.get::<String, _>("state") == "not_connected"
        })
        .await
    );
}

#[tokio::test]
async fn edge_case_the_server_sends_reset() {
    let env = Env::new().await;
    // The event stream delivers a reset with id 50001 and closes; the second connection is a command.
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![
            Step::Wait(Duration::from_millis(500)),
            frame(50001, "reset", event_data(50001, "reset", AT)),
            Step::Close,
        ]),
        Session::status(200, &reply_ok("r1").to_string()),
    ])
    .await;
    let mut seed = Seed::new(stand_in.port, "connected");
    seed.last_seq = 48212;
    let id = env.seed(seed).await;
    let connector = env.connector(1.0);
    let (tx, mut rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    assert!(
        eventually(Duration::from_secs(5), || async {
            !stand_in.events_requests().is_empty()
        })
        .await
    );
    assert_eq!(
        stand_in.events_requests()[0].last_event_id.as_deref(),
        Some("48212")
    );

    // Fill the read cache while the stream is up.
    let input = json!({"limit": 10});
    let sent = envelope("workspaces.list", "r1", input.clone());
    for _ in 0..40 {
        if let Ok(reply) = connector
            .send_command(backend(), meta("workspaces.list"), sent.clone(), soon())
            .await
        {
            assert!(String::from_utf8_lossy(&reply).contains("\"ok\""));
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let got = recv_n(&mut rx, 1, Duration::from_secs(5)).await;
    assert_eq!(summary(&got), ["reset"]);
    assert!(
        eventually(Duration::from_secs(5), || async {
            env.row(id)
                .await
                .unwrap()
                .get::<i64, _>("last_sequence_received")
                == 50001
        })
        .await,
        "last_sequence_received becomes 50001"
    );
    // The stream is gone and the cache is empty: the same read has no cached reply to return.
    assert!(
        eventually(Duration::from_secs(5), || async {
            matches!(
                connector
                    .send_command(backend(), meta("workspaces.list"), sent.clone(), soon())
                    .await,
                Err(ConnectorError::NotConnected)
            )
        })
        .await
    );
    cancel.cancel();
}

#[tokio::test]
async fn edge_case_reading_while_not_connected() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![Step::Wait(Duration::from_millis(500)), Step::Close]),
        Session::status(200, &reply_ok("r1").to_string()),
    ])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let connector = env.connector(1.0);
    let (tx, _rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    assert!(
        eventually(Duration::from_secs(5), || async {
            !stand_in.events_requests().is_empty()
        })
        .await
    );

    // workspaces.list is answered while the server is connected.
    let sent = envelope("workspaces.list", "r1", json!({"limit": 10}));
    let mut first = None;
    for _ in 0..40 {
        match connector
            .send_command(backend(), meta("workspaces.list"), sent.clone(), soon())
            .await
        {
            Ok(reply) => {
                first = Some(reply);
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(25)).await,
        }
    }
    let first = first.expect("a reply while connected");

    // The stream closes and the server refuses connections: it is not_connected.
    assert!(
        eventually(Duration::from_secs(5), || async {
            connector.list().await.unwrap()[0].state == ServerConnectionState::NotConnected
        })
        .await
    );
    // The same read, with a different request id, returns the cached reply.
    let again = envelope("workspaces.list", "another", json!({"limit": 10}));
    let cached = connector
        .send_command(backend(), meta("workspaces.list"), again, soon())
        .await
        .expect("the cached reply");
    assert_eq!(cached, first);
    // A change command is refused.
    let refused = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r2", json!({})),
            soon(),
        )
        .await;
    assert!(matches!(refused, Err(ConnectorError::NotConnected)));
    cancel.cancel();
}

#[tokio::test]
async fn error_path_the_server_ended_this_computers_pairing() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::ok(vec![Step::Hold]),
        Session::status(401, &error_body("SVC_UNAUTHENTICATED").to_string()),
    ])
    .await;
    let id = env.seed(Seed::new(stand_in.port, "connected")).await;
    let connector = env.connector(1.0);
    let (tx, _rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    assert!(
        eventually(Duration::from_secs(5), || async {
            !stand_in.events_requests().is_empty()
        })
        .await
    );
    assert!(
        eventually(Duration::from_secs(5), || async {
            connector.list().await.unwrap()[0].state == ServerConnectionState::Connected
        })
        .await
    );
    let err = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            soon(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::KeyRefused));
    assert_eq!(state_of(&env, id).await, "disconnected");
    assert_eq!(
        connector.list().await.unwrap()[0].state,
        ServerConnectionState::Disconnected
    );
    // The stream stops: no second events request is made.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(stand_in.events_requests().len(), 1);
    cancel.cancel();
}

#[tokio::test]
async fn error_path_a_server_two_minor_versions_behind() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![Session::ok(vec![Step::Hold])]).await;
    let id = env.seed(Seed::new(stand_in.port, "connected")).await;
    // This desktop is at Service API 1.2; the server's responses carry Ordin-Api-Version 1.0.
    let connector = env.connector_with(env.config(), 1.0, ServiceApiVersion { major: 1, minor: 2 });
    let (tx, _rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    assert!(
        eventually(Duration::from_secs(5), || async {
            state_of(&env, id).await == "refused_for_version"
        })
        .await
    );
    let view = connector.list().await.unwrap().remove(0);
    assert_eq!(view.state, ServerConnectionState::RefusedForVersion);
    assert_eq!(
        view.older_side,
        Some(OlderSide::Server),
        "server_older is true"
    );
    let err = connector
        .send_command(
            backend(),
            meta("workspaces.list"),
            envelope("workspaces.list", "r1", json!({})),
            soon(),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ConnectorError::RefusedForVersion { server_older: true }
    ));
    cancel.cancel();
}

#[tokio::test]
async fn happy_path_a_change_command_survives_one_dropped_connection() {
    let env = Env::new().await;
    let stand_in = SseStandIn::start(vec![
        Session::status(200, &pair_reply(SERVER_NAME, KEY_TEXT, (1, 0)).to_string()),
        Session::dropped(),
        Session::status(200, &reply_ok("req-42").to_string()),
    ])
    .await;
    let connector = env.connector(1.0);
    connector
        .connect(address_and_code(
            &format!("127.0.0.1:{}", stand_in.port),
            "K7QM-2XPA",
        ))
        .await
        .expect("pairing");
    let sent = envelope("approval.decide", "req-42", json!({"approval_id": "x"}));
    let started = Instant::now();
    let reply = connector
        .send_command(backend(), meta("approval.decide"), sent.clone(), soon())
        .await
        .expect("the second attempt is answered");
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "sent again after 200 ms"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&reply).unwrap()["request_id"],
        "req-42"
    );
    let commands: Vec<_> = stand_in
        .requests()
        .into_iter()
        .filter(|r| r.path == "/v1/approval/decide")
        .collect();
    assert_eq!(commands.len(), 2);
    assert_eq!(
        commands[0].body, commands[1].body,
        "the same body, so the same request_id"
    );
    assert!(commands[0].body.contains("req-42"));
}

#[tokio::test]
async fn happy_path_waking_from_sleep_reconnects_at_once() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    // The first attempt is refused with 503; the stand-in then waits for the wake-up connection.
    let stand_in = SseStandIn::start(vec![
        Session::status(503, "{}"),
        Session::ok(vec![Step::Hold]),
    ])
    .await;
    let mut seed = Seed::new(stand_in.port, "not_connected");
    seed.last_seq = 48212;
    env.seed(seed).await;
    // Every wait of the spec is multiplied by 100: the first backoff is about 100 seconds.
    let connector = env.connector(100.0);
    let (tx, _rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    assert!(
        eventually(Duration::from_secs(5), || async {
            stand_in.events_requests().len() == 1
        })
        .await
    );
    assert!(
        eventually(Duration::from_secs(5), || async {
            !capture.of("conn.reconnect").is_empty()
        })
        .await,
        "the supervisor is waiting its backoff"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    let woke = Instant::now();
    connector.on_wake();
    assert!(
        eventually(Duration::from_secs(1), || async {
            stand_in.events_requests().len() == 2
        })
        .await,
        "GET /v1/events is opened within 1 second"
    );
    assert!(woke.elapsed() < Duration::from_secs(1));
    assert_eq!(
        stand_in.events_requests()[1].last_event_id.as_deref(),
        Some("48212")
    );
    let wakes = capture.of("conn.wake");
    assert_eq!(wakes.len(), 1);
    assert_eq!(wakes[0]["woken"], 1);
    cancel.cancel();
}

#[tokio::test]
async fn edge_case_the_version_screen_names_the_side_to_update() {
    let env = Env::new().await;
    let mut refused = Seed::new(2001, "refused_for_version");
    refused.name = "Alpha";
    refused.version = Some("1.0");
    env.seed(refused).await;
    let mut connected = Seed::new(2002, "connected");
    connected.name = "Bravo";
    env.seed(connected).await;
    let mut unknown = Seed::new(2003, "refused_for_version");
    unknown.name = "Charlie";
    unknown.version = None;
    env.seed(unknown).await;
    let mut disconnected = Seed::new(2004, "disconnected");
    disconnected.name = "Delta";
    env.seed(disconnected).await;

    // This desktop is at Service API 1.2.
    let connector = env.connector_with(env.config(), 1.0, ServiceApiVersion { major: 1, minor: 2 });
    let views = connector.list().await.expect("servers.list");
    let names: Vec<_> = views.iter().map(|v| v.display_name.as_str()).collect();
    assert_eq!(
        names,
        ["Alpha", "Bravo", "Charlie", "Delta"],
        "ordered by display name"
    );
    assert_eq!(views[0].older_side, Some(OlderSide::Server));
    assert_eq!(views[1].older_side, None);
    assert_eq!(
        views[2].older_side,
        Some(OlderSide::Desktop),
        "a missing stored version gives desktop"
    );
    assert_eq!(views[3].older_side, None);
}

#[tokio::test]
async fn happy_path_disconnecting() {
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    Mock::given(method("POST"))
        .and(path("/v1/pairing/end"))
        .respond_with(json_response(200, &json!({"kind": "ok", "result": {}})))
        .mount(&env.mock)
        .await;
    let connector = env.connector(1.0);
    let view = connector
        .connect(address_and_code(&env.mock_address(), "K7QM-2XPA"))
        .await
        .expect("pairing");
    let id: ServerConnectionId = view.server_connection_id;
    connector.disconnect(id).await.expect("disconnect");

    let requests = env.mock.received_requests().await.unwrap();
    let end = requests
        .iter()
        .find(|r| r.url.path() == "/v1/pairing/end")
        .expect("pairing/end");
    assert_eq!(
        end.headers.get("authorization").unwrap().to_str().unwrap(),
        format!("Bearer {KEY_TEXT}")
    );
    assert_eq!(env.keys.count(), 0, "the keychain item is deleted");
    assert_eq!(state_of(&env, id).await, "disconnected");
    let states: Vec<_> = env
        .state_events()
        .await
        .iter()
        .map(|e| e.data["state"].clone())
        .collect();
    assert_eq!(states, [json!("connected"), json!("disconnected")]);

    // Remove this server deletes the rows of the three tables.
    connector.remove(id).await.expect("remove");
    assert_eq!(env.count("server_connections").await, 0);
    assert_eq!(env.count("server_connection_backends").await, 0);
    assert!(connector.connection_for(backend()).is_none());
}
