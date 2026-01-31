//! X4: every code in `ALL_CODES` is in the error registry and `CONN_` is owned by this crate; and the
//! module's migrations (DatabaseMigrations.md): one SQLite step, no PostgreSQL step, tables as the Data
//! Model states them.

mod support;

use ordin_server_connector::{ConnectorError, ServerConnector, ALL_CODES};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::CodedError;
use sqlx::Row as _;
use support::*;

fn every_variant() -> Vec<ConnectorError> {
    vec![
        ConnectorError::AddressInvalid,
        ConnectorError::PortOutOfRange,
        ConnectorError::Duplicate,
        ConnectorError::PairingLineInvalid,
        ConnectorError::CodeRefused,
        ConnectorError::PairingStopped,
        ConnectorError::TooManyDesktops,
        ConnectorError::ComputerNameInvalid,
        ConnectorError::AddressLocked,
        ConnectorError::Unreachable,
        ConnectorError::CertificateUntrusted,
        ConnectorError::FingerprintMismatch,
        ConnectorError::ClientCertUnavailable,
        ConnectorError::RefusedForVersion { server_older: true },
        ConnectorError::NotConnected,
        ConnectorError::KeyRefused,
        ConnectorError::Deadline,
        ConnectorError::BadAnswer,
        ConnectorError::UnknownServer,
        ConnectorError::Keychain,
        ConnectorError::Datastore,
        ConnectorError::AlreadyStarted,
    ]
}

#[test]
fn every_code_is_registered_and_owned_by_this_crate() {
    assert_eq!(
        ALL_CODES.len(),
        23,
        "the twenty-three CONN_ rows of the Error Table"
    );
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-server-connector", "{code}");
        assert!(code.starts_with("CONN_"), "{code}");
    }
    assert!(owns_prefix("ordin-server-connector", "CONN"));
    // The sequence-jump code is logged here but owned by the Service API.
    assert_eq!(
        lookup("SVC_EVENT_GAP").map(|e| e.owner_crate),
        Some("ordin-service-api")
    );
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, ALL_CODES, "kept in registry order");
}

#[test]
fn the_registry_flags_agree_with_the_error_type() {
    for error in every_variant() {
        let entry =
            lookup(error.code()).unwrap_or_else(|| panic!("{} is not registered", error.code()));
        assert_eq!(error.retryable(), entry.retryable, "{}", error.code());
        assert!(ALL_CODES.contains(&error.code()));
        assert!(!error.to_string().is_empty());
    }
    // Every code but the logged CONN_EVENT_UNREADABLE row has an error value.
    let mut codes: Vec<&str> = every_variant().iter().map(|e| e.code()).collect();
    codes.push("CONN_EVENT_UNREADABLE");
    codes.sort_unstable();
    assert_eq!(codes, ALL_CODES);
}

#[test]
fn the_migrations_are_one_sqlite_step_and_no_postgres_step() {
    let m = ServerConnector::migrations();
    assert_eq!(m.module, "server_connector");
    assert_eq!(m.sqlite.len(), 1);
    assert_eq!(m.sqlite[0].0, 1);
    assert!(m.postgres.is_empty());
}

#[tokio::test]
async fn the_tables_enforce_the_data_model() {
    let env = Env::new().await;
    let tables: Vec<String> = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'server_connection%' ORDER BY name")
        .fetch_all(&env.pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(
        tables,
        [
            "server_connection_backends",
            "server_connection_pins",
            "server_connections"
        ]
    );
    // Port range, state list and the (address, port) uniqueness are checked by the database itself.
    let id = env.seed(Seed::new(2001, "connected")).await;
    let bad_port = sqlx::query(
        "INSERT INTO server_connections (id, display_name, address, port, last_sequence_received, keychain_ref, state, paired_at) \
         VALUES ('0192a1d0-1111-7222-8333-444455556667', 'x', 'h', 80, 0, 'k1', 'connected', 't')",
    )
    .execute(&env.pool)
    .await;
    assert!(bad_port.is_err());
    let bad_state = sqlx::query(
        "INSERT INTO server_connections (id, display_name, address, port, last_sequence_received, keychain_ref, state, paired_at) \
         VALUES ('0192a1d0-1111-7222-8333-444455556667', 'x', 'h', 2000, 0, 'k1', 'broken', 't')",
    )
    .execute(&env.pool)
    .await;
    assert!(bad_state.is_err());
    let twin = sqlx::query(
        "INSERT INTO server_connections (id, display_name, address, port, last_sequence_received, keychain_ref, state, paired_at) \
         VALUES ('0192a1d0-1111-7222-8333-444455556667', 'x', '127.0.0.1', 2001, 0, 'k1', 'connected', 't')",
    )
    .execute(&env.pool)
    .await;
    assert!(twin.is_err());
    // Removing a server deletes its pin and backend rows by cascade.
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&env.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM server_connections WHERE id = ?1")
        .bind(id.to_string())
        .execute(&env.pool)
        .await
        .unwrap();
    assert_eq!(env.count("server_connection_backends").await, 0);
}

#[tokio::test]
async fn construction_checks_the_computer_name() {
    let env = Env::new().await;
    for name in [String::new(), "n".repeat(64)] {
        let mut cfg = env.config();
        cfg.computer_name = name;
        assert!(matches!(
            ServerConnector::new(cfg, env.journal.clone(), env.keys.clone()),
            Err(ConnectorError::ComputerNameInvalid)
        ));
    }
    let mut cfg = env.config();
    cfg.computer_name = "n".repeat(63);
    assert!(ServerConnector::new(cfg, env.journal.clone(), env.keys.clone()).is_ok());
}
