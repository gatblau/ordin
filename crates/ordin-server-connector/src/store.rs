//! The crate's three tables in the desktop's datastore (ServerConnector.md Data Model). Every failure
//! is `Datastore`; no SQL text, row content or driver message leaves this module.

use ordin_types::time::Timestamp;
use ordin_types::{BackendId, DesktopPairingId, ServerConnectionId};
use sqlx::{Row as _, SqlitePool};

use crate::error::ConnectorError;
use ordin_types::domain::server::ServerConnectionState;

/// One server as stored: `server_connections` joined with its pin and backend rows.
#[derive(Debug, Clone)]
pub(crate) struct StoredServer {
    pub(crate) id: ServerConnectionId,
    pub(crate) display_name: String,
    pub(crate) address: String,
    pub(crate) port: u16,
    pub(crate) service_api_version: Option<String>,
    pub(crate) last_sequence_received: u64,
    pub(crate) keychain_ref: String,
    pub(crate) state: ServerConnectionState,
    pub(crate) paired_at: Timestamp,
    pub(crate) pin: Option<String>,
    pub(crate) backend: BackendId,
    pub(crate) desktop_pairing_id: DesktopPairingId,
}

pub(crate) fn state_text(state: ServerConnectionState) -> &'static str {
    match state {
        ServerConnectionState::Connected => "connected",
        ServerConnectionState::NotConnected => "not_connected",
        ServerConnectionState::Disconnected => "disconnected",
        ServerConnectionState::RefusedForVersion => "refused_for_version",
    }
}

fn parse_state(raw: &str) -> Option<ServerConnectionState> {
    Some(match raw {
        "connected" => ServerConnectionState::Connected,
        "not_connected" => ServerConnectionState::NotConnected,
        "disconnected" => ServerConnectionState::Disconnected,
        "refused_for_version" => ServerConnectionState::RefusedForVersion,
        _ => return None,
    })
}

fn db<T>(r: Result<T, sqlx::Error>) -> Result<T, ConnectorError> {
    r.map_err(|_| ConnectorError::Datastore)
}

fn read_row(row: &sqlx::sqlite::SqliteRow) -> Result<StoredServer, ConnectorError> {
    let bad = |_| ConnectorError::Datastore;
    let text = |name: &str| row.try_get::<String, _>(name).map_err(bad);
    let port: i64 = row.try_get("port").map_err(bad)?;
    let seq: i64 = row.try_get("last_sequence_received").map_err(bad)?;
    Ok(StoredServer {
        id: ServerConnectionId::parse(&text("id")?).map_err(|_| ConnectorError::Datastore)?,
        display_name: text("display_name")?,
        address: text("address")?,
        port: u16::try_from(port).map_err(|_| ConnectorError::Datastore)?,
        service_api_version: row.try_get("service_api_version").map_err(bad)?,
        last_sequence_received: u64::try_from(seq).map_err(|_| ConnectorError::Datastore)?,
        keychain_ref: text("keychain_ref")?,
        state: parse_state(&text("state")?).ok_or(ConnectorError::Datastore)?,
        paired_at: ordin_types::time::parse_rfc3339_ms_utc(&text("paired_at")?)
            .map(Timestamp)
            .map_err(|_| ConnectorError::Datastore)?,
        pin: row.try_get("fingerprint_sha256").map_err(bad)?,
        backend: BackendId::parse(&text("backend_id")?).map_err(|_| ConnectorError::Datastore)?,
        desktop_pairing_id: DesktopPairingId::parse(&text("desktop_pairing_id")?)
            .map_err(|_| ConnectorError::Datastore)?,
    })
}

/// Every stored server, in display-name order.
pub(crate) async fn load_all(pool: &SqlitePool) -> Result<Vec<StoredServer>, ConnectorError> {
    let rows = db(sqlx::query(
        "SELECT c.id, c.display_name, c.address, c.port, c.service_api_version, \
         c.last_sequence_received, c.keychain_ref, c.state, c.paired_at, p.fingerprint_sha256, \
         b.backend_id, b.desktop_pairing_id \
         FROM server_connections c \
         JOIN server_connection_backends b ON b.server_connection_id = c.id \
         LEFT JOIN server_connection_pins p ON p.server_connection_id = c.id \
         ORDER BY c.display_name, c.id",
    )
    .fetch_all(pool)
    .await)?;
    rows.iter().map(read_row).collect()
}

/// The id and state of the row holding `(address, port)`, if any.
pub(crate) async fn find_by_address(
    pool: &SqlitePool,
    address: &str,
    port: u16,
) -> Result<Option<(ServerConnectionId, ServerConnectionState)>, ConnectorError> {
    let row = db(sqlx::query(
        "SELECT id, state FROM server_connections WHERE address = ?1 AND port = ?2",
    )
    .bind(address)
    .bind(i64::from(port))
    .fetch_optional(pool)
    .await)?;
    let Some(row) = row else { return Ok(None) };
    let id: String = db(row.try_get("id"))?;
    let state: String = db(row.try_get("state"))?;
    Ok(Some((
        ServerConnectionId::parse(&id).map_err(|_| ConnectorError::Datastore)?,
        parse_state(&state).ok_or(ConnectorError::Datastore)?,
    )))
}

/// Step 3.6: inserts or updates the three rows of a server in one transaction.
pub(crate) async fn save_paired(
    pool: &SqlitePool,
    server: &StoredServer,
) -> Result<(), ConnectorError> {
    let mut tx = db(pool.begin().await)?;
    db(sqlx::query(
        "INSERT INTO server_connections \
         (id, display_name, address, port, service_api_version, last_sequence_received, \
          keychain_ref, state, paired_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name, \
          address = excluded.address, port = excluded.port, \
          service_api_version = excluded.service_api_version, \
          last_sequence_received = excluded.last_sequence_received, \
          keychain_ref = excluded.keychain_ref, state = excluded.state, \
          paired_at = excluded.paired_at",
    )
    .bind(server.id.to_string())
    .bind(&server.display_name)
    .bind(&server.address)
    .bind(i64::from(server.port))
    .bind(&server.service_api_version)
    .bind(i64::try_from(server.last_sequence_received).unwrap_or(i64::MAX))
    .bind(&server.keychain_ref)
    .bind(state_text(server.state))
    .bind(server.paired_at.to_wire())
    .execute(&mut *tx)
    .await)?;
    db(
        sqlx::query("DELETE FROM server_connection_pins WHERE server_connection_id = ?1")
            .bind(server.id.to_string())
            .execute(&mut *tx)
            .await,
    )?;
    if let Some(pin) = &server.pin {
        db(sqlx::query(
            "INSERT INTO server_connection_pins (server_connection_id, fingerprint_sha256) \
             VALUES (?1, ?2)",
        )
        .bind(server.id.to_string())
        .bind(pin)
        .execute(&mut *tx)
        .await)?;
    }
    db(sqlx::query(
        "INSERT INTO server_connection_backends \
         (server_connection_id, backend_id, desktop_pairing_id) VALUES (?1, ?2, ?3) \
         ON CONFLICT(server_connection_id) DO UPDATE SET backend_id = excluded.backend_id, \
          desktop_pairing_id = excluded.desktop_pairing_id",
    )
    .bind(server.id.to_string())
    .bind(server.backend.to_string())
    .bind(server.desktop_pairing_id.to_string())
    .execute(&mut *tx)
    .await)?;
    db(tx.commit().await)
}

/// Step 8: the state, and the Service API version when it is given.
pub(crate) async fn update_state(
    pool: &SqlitePool,
    id: ServerConnectionId,
    state: ServerConnectionState,
    version: Option<&str>,
) -> Result<(), ConnectorError> {
    db(sqlx::query(
        "UPDATE server_connections SET state = ?2, \
         service_api_version = COALESCE(?3, service_api_version) WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(state_text(state))
    .bind(version)
    .execute(pool)
    .await)?;
    Ok(())
}

/// Step 6.4: the last sequence number received.
pub(crate) async fn update_sequence(
    pool: &SqlitePool,
    id: ServerConnectionId,
    seq: u64,
) -> Result<(), ConnectorError> {
    db(
        sqlx::query("UPDATE server_connections SET last_sequence_received = ?2 WHERE id = ?1")
            .bind(id.to_string())
            .bind(i64::try_from(seq).unwrap_or(i64::MAX))
            .execute(pool)
            .await,
    )?;
    Ok(())
}

/// Deletes the rows of the three tables in one transaction.
pub(crate) async fn delete(
    pool: &SqlitePool,
    id: ServerConnectionId,
) -> Result<(), ConnectorError> {
    let mut tx = db(pool.begin().await)?;
    for sql in [
        "DELETE FROM server_connection_pins WHERE server_connection_id = ?1",
        "DELETE FROM server_connection_backends WHERE server_connection_id = ?1",
        "DELETE FROM server_connections WHERE id = ?1",
    ] {
        db(sqlx::query(sql)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await)?;
    }
    db(tx.commit().await)
}
