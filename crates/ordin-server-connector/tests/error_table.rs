//! X1: one test per row of the Error Table (ServerConnector.md), named after the row's code.

mod support;

use std::time::{Duration, Instant};

use ordin_server_connector::{ConnectorError, ServerConnector};
use ordin_service_api::commands::ConnectServer;
use ordin_service_api::ServiceApiVersion;
use ordin_types::{CodedError, ServerConnectionId};
use serde_json::json;
use support::sse::{frame, Session, SseStandIn, Step};
use support::*;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn address_and_code(address: &str) -> ConnectServer {
    ConnectServer::AddressAndCode {
        address: address.to_owned(),
        code: "K7QM-2XPA".to_owned(),
    }
}

async fn connect_mock(env: &Env, connector: &ServerConnector) -> Result<(), ConnectorError> {
    connector
        .connect(address_and_code(&env.mock_address()))
        .await
        .map(|_| ())
}

async fn nothing_written(env: &Env) {
    assert_eq!(env.count("server_connections").await, 0);
    assert_eq!(env.count("server_connection_backends").await, 0);
    assert_eq!(env.keys.count(), 0);
}

#[tokio::test]
async fn conn_address_invalid() {
    let env = Env::new().await;
    let err = env
        .connector(1.0)
        .connect(address_and_code("bad_host"))
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::AddressInvalid));
    assert_eq!(err.code(), "CONN_ADDRESS_INVALID");
    assert!(!err.retryable());
    nothing_written(&env).await;
}

#[tokio::test]
async fn conn_port_out_of_range() {
    let env = Env::new().await;
    for address in ["127.0.0.1:80", "127.0.0.1:70000", "127.0.0.1:1024"] {
        let err = env
            .connector(1.0)
            .connect(address_and_code(address))
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectorError::PortOutOfRange), "{address}");
        assert_eq!(err.code(), "CONN_PORT_OUT_OF_RANGE");
    }
}

#[tokio::test]
async fn conn_duplicate() {
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    let connector = env.connector(1.0);
    connect_mock(&env, &connector).await.expect("first pairing");
    let err = connect_mock(&env, &connector).await.unwrap_err();
    assert!(matches!(err, ConnectorError::Duplicate));
    assert_eq!(err.code(), "CONN_DUPLICATE");
    assert_eq!(env.count("server_connections").await, 1);
}

#[tokio::test]
async fn conn_pairing_line_invalid() {
    let env = Env::new().await;
    let err = env
        .connector(1.0)
        .connect(ConnectServer::PairingLine {
            line: "ordin-pair:1:nonsense".to_owned(),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::PairingLineInvalid));
    assert_eq!(err.code(), "CONN_PAIRING_LINE_INVALID");
}

#[tokio::test]
async fn conn_code_refused() {
    let env = Env::new().await;
    for code in ["DESK_CODE_INVALID", "DESK_CODE_EXPIRED"] {
        env.mock.reset().await;
        mock_pairing(&env.mock, json_response(403, &error_body(code))).await;
        let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
        assert!(matches!(err, ConnectorError::CodeRefused), "{code}");
        assert_eq!(err.code(), "CONN_CODE_REFUSED");
    }
    // A code that is malformed is refused before any request is sent.
    let before = env.mock.received_requests().await.unwrap().len();
    let err = env
        .connector(1.0)
        .connect(ConnectServer::AddressAndCode {
            address: env.mock_address(),
            code: "not a code".to_owned(),
        })
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::CodeRefused));
    assert_eq!(env.mock.received_requests().await.unwrap().len(), before);
    nothing_written(&env).await;
}

#[tokio::test]
async fn conn_pairing_stopped() {
    let env = Env::new().await;
    mock_pairing(
        &env.mock,
        json_response(403, &error_body("DESK_PAIRING_STOPPED")),
    )
    .await;
    let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
    assert!(matches!(err, ConnectorError::PairingStopped));
    assert_eq!(err.code(), "CONN_PAIRING_STOPPED");
}

#[tokio::test]
async fn conn_too_many_desktops() {
    let env = Env::new().await;
    mock_pairing(&env.mock, json_response(403, &error_body("DESK_TOO_MANY"))).await;
    let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
    assert!(matches!(err, ConnectorError::TooManyDesktops));
    assert_eq!(err.code(), "CONN_TOO_MANY_DESKTOPS");
    nothing_written(&env).await;
}

#[tokio::test]
async fn conn_computer_name_invalid() {
    let env = Env::new().await;
    mock_pairing(
        &env.mock,
        json_response(422, &error_body("DESK_NAME_INVALID")),
    )
    .await;
    let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
    assert!(matches!(err, ConnectorError::ComputerNameInvalid));
    assert_eq!(err.code(), "CONN_COMPUTER_NAME_INVALID");
    // A computer name outside 1-63 characters is refused at construction.
    let mut cfg = env.config();
    cfg.computer_name = String::new();
    let refused = ServerConnector::new(cfg, env.journal.clone(), env.keys.clone());
    assert!(matches!(refused, Err(ConnectorError::ComputerNameInvalid)));
}

#[tokio::test]
async fn conn_address_locked() {
    let env = Env::new().await;
    mock_pairing(
        &env.mock,
        json_response(429, &error_body("DESK_ADDRESS_LOCKED")),
    )
    .await;
    let connector = env.connector(1.0);
    let err = connect_mock(&env, &connector).await.unwrap_err();
    assert!(matches!(err, ConnectorError::AddressLocked));
    assert_eq!(err.code(), "CONN_ADDRESS_LOCKED");
    assert!(err.retryable());

    // The same code on a command: a 429 whose error is DESK_ADDRESS_LOCKED.
    env.mock.reset().await;
    mock_pairing_ok(&env.mock).await;
    connect_mock(&env, &connector).await.expect("pairing");
    Mock::given(method("POST"))
        .and(path("/v1/approval/decide"))
        .respond_with(json_response(429, &error_body("DESK_ADDRESS_LOCKED")))
        .mount(&env.mock)
        .await;
    let sent = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            Instant::now() + Duration::from_secs(5),
        )
        .await;
    assert!(matches!(sent, Err(ConnectorError::AddressLocked)));
}

#[tokio::test]
async fn conn_unreachable() {
    let env = Env::new().await;
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let err = env
        .connector(1.0)
        .connect(address_and_code(&format!("127.0.0.1:{port}")))
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::Unreachable));
    assert_eq!(err.code(), "CONN_UNREACHABLE");
    assert!(err.retryable());
    assert!(matches!(
        err.repair(),
        Some(ordin_types::RepairAction::TryAgain)
    ));
}

#[tokio::test]
async fn conn_cert_untrusted() {
    let env = Env::new().await;
    let server = tls::TlsStandIn::start(
        "a",
        200,
        pair_reply(SERVER_NAME, KEY_TEXT, (1, 0)).to_string(),
    )
    .await;
    let err = env
        .tls_connector(env.config(), 1.0)
        .connect(address_and_code(&format!("127.0.0.1:{}", server.port)))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ConnectorError::CertificateUntrusted),
        "{err:?}"
    );
    assert_eq!(err.code(), "CONN_CERT_UNTRUSTED");
    nothing_written(&env).await;
}

#[tokio::test]
async fn conn_fingerprint_mismatch() {
    let env = Env::new().await;
    let server = tls::TlsStandIn::start(
        "b",
        200,
        pair_reply(SERVER_NAME, KEY_TEXT, (1, 0)).to_string(),
    )
    .await;
    let line = format!(
        "ordin-pair:1:127.0.0.1:{}:{}:{CODE}",
        server.port,
        tls::fingerprint("a")
    );
    let err = env
        .tls_connector(env.config(), 1.0)
        .connect(ConnectServer::PairingLine { line })
        .await
        .unwrap_err();
    assert!(
        matches!(err, ConnectorError::FingerprintMismatch),
        "{err:?}"
    );
    assert_eq!(err.code(), "CONN_FINGERPRINT_MISMATCH");
    assert!(err.repair().is_none());
    nothing_written(&env).await;
}

#[tokio::test]
async fn conn_client_cert_unavailable() {
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    let mut cfg = env.config();
    cfg.client_certificate = Some("ACME desktop".to_owned());
    let connector = env.connector_with(cfg, 1.0, ServiceApiVersion::CURRENT);
    let err = connect_mock(&env, &connector).await.unwrap_err();
    assert!(matches!(err, ConnectorError::ClientCertUnavailable));
    assert_eq!(err.code(), "CONN_CLIENT_CERT_UNAVAILABLE");
    assert!(err.repair().is_none());
    // No request was sent without the certificate.
    assert!(env.mock.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn conn_refused_for_version() {
    let env = Env::new().await;
    // The server answers 409 and its version is two minor versions behind the desktop's 1.2.
    mock_pairing(
        &env.mock,
        json_response(409, &error_body("SVC_VERSION_UNSUPPORTED")),
    )
    .await;
    let connector = env.connector_with(env.config(), 1.0, ServiceApiVersion { major: 1, minor: 2 });
    let err = connect_mock(&env, &connector).await.unwrap_err();
    assert!(
        matches!(
            err,
            ConnectorError::RefusedForVersion { server_older: true }
        ),
        "{err:?}"
    );
    assert_eq!(err.code(), "CONN_REFUSED_FOR_VERSION");

    // A reply whose version the desktop cannot use is refused too, with the desktop the older side.
    env.mock.reset().await;
    mock_pairing(
        &env.mock,
        json_response(200, &pair_reply(SERVER_NAME, KEY_TEXT, (1, 3))),
    )
    .await;
    let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
    assert!(
        matches!(
            err,
            ConnectorError::RefusedForVersion {
                server_older: false
            }
        ),
        "{err:?}"
    );
    nothing_written(&env).await;
}

#[tokio::test]
async fn conn_not_connected() {
    let env = Env::new().await;
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    env.seed(Seed::new(port, "connected")).await;
    let connector = env.connector(0.001);
    let (tx, _rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    let err = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::NotConnected));
    assert_eq!(err.code(), "CONN_NOT_CONNECTED");
    assert!(err.retryable());
    cancel.cancel();
}

#[tokio::test]
async fn conn_key_refused() {
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    Mock::given(method("POST"))
        .and(path("/v1/approval/decide"))
        .respond_with(json_response(401, &error_body("SVC_UNAUTHENTICATED")))
        .mount(&env.mock)
        .await;
    let connector = env.connector(1.0);
    let view = connector
        .connect(address_and_code(&env.mock_address()))
        .await
        .expect("pairing");
    let err = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::KeyRefused));
    assert_eq!(err.code(), "CONN_KEY_REFUSED");
    assert_eq!(
        state_of(&env, view.server_connection_id).await,
        "disconnected"
    );
    // Once disconnected, a command is refused without a request.
    let again = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r2", json!({})),
            Instant::now() + Duration::from_secs(5),
        )
        .await;
    assert!(matches!(again, Err(ConnectorError::KeyRefused)));
}

#[tokio::test]
async fn conn_deadline() {
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    Mock::given(method("POST"))
        .and(path("/v1/approval/decide"))
        .respond_with(json_response(200, &reply_ok("r1")).set_delay(Duration::from_millis(800)))
        .mount(&env.mock)
        .await;
    let connector = env.connector(1.0);
    connect_mock(&env, &connector).await.expect("pairing");
    let err = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            Instant::now() + Duration::from_millis(150),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::Deadline));
    assert_eq!(err.code(), "CONN_DEADLINE");
    assert!(err.retryable());
    // A deadline already passed fails without a request.
    let past = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r2", json!({})),
            Instant::now(),
        )
        .await;
    assert!(matches!(past, Err(ConnectorError::Deadline)));
}

#[tokio::test]
async fn conn_bad_answer() {
    let env = Env::new().await;
    // A pairing answer that is not a PairReply, a key that is not 32 bytes, a name out of range.
    mock_pairing(
        &env.mock,
        ResponseTemplate::new(200).set_body_string("<html>"),
    )
    .await;
    let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
    assert!(matches!(err, ConnectorError::BadAnswer));
    assert_eq!(err.code(), "CONN_BAD_ANSWER");
    assert!(err.retryable());
    for reply in [
        pair_reply(SERVER_NAME, "AAAA", (1, 0)),
        pair_reply("", KEY_TEXT, (1, 0)),
        pair_reply(&"n".repeat(121), KEY_TEXT, (1, 0)),
    ] {
        env.mock.reset().await;
        mock_pairing(&env.mock, json_response(200, &reply)).await;
        let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
        assert!(matches!(err, ConnectorError::BadAnswer), "{reply}");
    }
    nothing_written(&env).await;

    // A command answer that is not a CommandReply, and one with no version header.
    env.mock.reset().await;
    mock_pairing_ok(&env.mock).await;
    let connector = env.connector(1.0);
    connect_mock(&env, &connector).await.expect("pairing");
    let send = || {
        connector.send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            Instant::now() + Duration::from_secs(5),
        )
    };
    Mock::given(method("POST"))
        .and(path("/v1/approval/decide"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Ordin-Api-Version", "1.0")
                .set_body_string("nope"),
        )
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    assert!(matches!(send().await, Err(ConnectorError::BadAnswer)));
    Mock::given(method("POST"))
        .and(path("/v1/approval/decide"))
        .respond_with(ResponseTemplate::new(200).set_body_string(reply_ok("r1").to_string()))
        .mount(&env.mock)
        .await;
    assert!(matches!(send().await, Err(ConnectorError::BadAnswer)));
}

#[tokio::test]
async fn conn_unknown_server() {
    let env = Env::new().await;
    let connector = env.connector(1.0);
    let unknown = ServerConnectionId::new();
    let err = connector.disconnect(unknown).await.unwrap_err();
    assert!(matches!(err, ConnectorError::UnknownServer));
    assert_eq!(err.code(), "CONN_UNKNOWN_SERVER");
    assert!(matches!(
        connector.remove(unknown).await,
        Err(ConnectorError::UnknownServer)
    ));
    let sent = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            Instant::now() + Duration::from_secs(1),
        )
        .await;
    assert!(matches!(sent, Err(ConnectorError::UnknownServer)));
    assert!(connector.connection_for(backend()).is_none());
    assert!(connector.display_name(backend()).is_none());
}

#[tokio::test]
async fn conn_keychain() {
    let env = Env::new().await;
    mock_pairing_ok(&env.mock).await;
    *env.keys.fail_put.lock().unwrap() = true;
    let err = connect_mock(&env, &env.connector(1.0)).await.unwrap_err();
    assert!(matches!(err, ConnectorError::Keychain));
    assert_eq!(err.code(), "CONN_KEYCHAIN");
    assert!(err.retryable());
    assert!(matches!(
        err.repair(),
        Some(ordin_types::RepairAction::TryAgain)
    ));
    // The rows were deleted again.
    nothing_written(&env).await;

    // A key that cannot be read stops a command; a key that cannot be deleted fails the disconnect.
    *env.keys.fail_put.lock().unwrap() = false;
    let connector = env.connector(1.0);
    let view = connector
        .connect(address_and_code(&env.mock_address()))
        .await
        .expect("pairing");
    *env.keys.fail_get.lock().unwrap() = true;
    let sent = connector
        .send_command(
            backend(),
            meta("approval.decide"),
            envelope("approval.decide", "r1", json!({})),
            Instant::now() + Duration::from_secs(1),
        )
        .await;
    assert!(matches!(sent, Err(ConnectorError::Keychain)));
    *env.keys.fail_get.lock().unwrap() = false;
    *env.keys.fail_delete.lock().unwrap() = true;
    let gone = connector.disconnect(view.server_connection_id).await;
    assert!(matches!(gone, Err(ConnectorError::Keychain)));
}

#[tokio::test]
async fn conn_datastore() {
    let env = Env::new().await;
    sqlx::query("DROP TABLE server_connection_pins")
        .execute(&env.pool)
        .await
        .unwrap();
    let err = env.connector(1.0).list().await.unwrap_err();
    assert!(matches!(err, ConnectorError::Datastore));
    assert_eq!(err.code(), "CONN_DATASTORE");
    assert!(err.retryable());
    mock_pairing_ok(&env.mock).await;
    assert!(matches!(
        env.connector(1.0)
            .connect(address_and_code(&env.mock_address()))
            .await,
        Err(ConnectorError::Datastore)
    ));
}

#[tokio::test]
async fn conn_already_started() {
    let env = Env::new().await;
    let connector = env.connector(1.0);
    let cancel = CancellationToken::new();
    let (tx, _rx) = bridge();
    connector
        .start(tx.clone(), cancel.clone())
        .await
        .expect("first start");
    let err = connector.start(tx, cancel.clone()).await.unwrap_err();
    assert!(matches!(err, ConnectorError::AlreadyStarted));
    assert_eq!(err.code(), "CONN_ALREADY_STARTED");
    cancel.cancel();
}

#[tokio::test]
async fn conn_event_unreadable() {
    let env = Env::new().await;
    let at = "2026-01-10T10:42:00.000Z";
    let other_backend = event_data(2, "needs_you_changed", at)
        .replace(BACKEND, "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6c");
    let stand_in = SseStandIn::start(vec![Session::ok(vec![
        frame(
            1,
            "needs_you_changed",
            event_data(1, "needs_you_changed", at),
        ),
        frame(2, "needs_you_changed", "not json".to_owned()),
        frame(2, "needs_you_changed", other_backend),
        frame(
            2,
            "needs_you_changed",
            event_data(2, "needs_you_changed", at),
        ),
        Step::Hold,
    ])])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let connector = env.connector(1.0);
    let (tx, mut rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    let got = recv_n(&mut rx, 2, Duration::from_secs(5)).await;
    // The unreadable frame and the frame for another backend are skipped; the stream goes on.
    assert_eq!(summary(&got), ["1", "2"]);
    cancel.cancel();
}

#[tokio::test]
async fn svc_event_gap() {
    let env = Env::new().await;
    let at = "2026-01-10T10:42:00.000Z";
    let stand_in = SseStandIn::start(vec![Session::ok(vec![
        frame(
            1,
            "needs_you_changed",
            event_data(1, "needs_you_changed", at),
        ),
        frame(
            5,
            "needs_you_changed",
            event_data(5, "needs_you_changed", at),
        ),
        Step::Hold,
    ])])
    .await;
    env.seed(Seed::new(stand_in.port, "connected")).await;
    let connector = env.connector(1.0);
    let (tx, mut rx) = bridge();
    let cancel = CancellationToken::new();
    connector.start(tx, cancel.clone()).await.expect("start");
    let got = recv_n(&mut rx, 3, Duration::from_secs(5)).await;
    // A jump from 1 to 5 sends a reset to the Bridge first, then the event.
    assert_eq!(summary(&got), ["1", "reset", "5"]);
    cancel.cancel();
}
