//! The Server Connector (ServerConnector.md): the desktop's side of the link to the person's own ordin
//! servers. It pairs the desktop with a server, keeps each desktop key in the keychain, sends the
//! commands the screens address to a server's backend to that server's Remote API, holds one
//! Server-Sent Events stream per server and hands its events to the Command Bridge in order, marks a
//! server *not connected* after 45 seconds of silence and reconnects with backoff while ordin is open,
//! catches up with `Last-Event-ID`, refuses a server whose Service API version it cannot use, and ends
//! or forgets a pairing when the person asks. It owns the desktop's `server_connections` rows.
//!
//! Conditional assertion relied on (ServerConnector.md *Performance, Security, Observability*): C-71,
//! *SSE over HTTP/2 passes company proxies with 20 s comments*. Until it is confirmed, the 45 s silence
//! rule and the reconnect loop with `Last-Event-ID` are the fallback: a proxy that cuts the stream costs
//! one reconnect, not lost events.
#![forbid(unsafe_code)]

mod address;
mod cache;
mod command;
mod conn;
mod connector;
mod error;
mod log;
mod pairing;
mod store;
mod supervisor;
mod tls;
mod types;
mod version;
mod wire;

pub use connector::ServerConnector;
pub use error::{ConnectorError, ALL_CODES};
pub use types::{
    ConnectorConfig, ServerEvent, ServerEventSender, StandIn, DEFAULT_READ_CACHE_BYTES,
};

pub(crate) use connector::Inner;

use ordin_journal::ModuleMigrations;

/// The module name under which the program registers this crate's migrations, and the `component`
/// field of its log lines.
pub const MODULE: &str = "server_connector";

/// Migrations registered by the program with the Journal's `MigrationRegistry`: module
/// `server_connector`, one SQLite step (the Data Model SQL), no PostgreSQL step (the desktop runs no
/// PostgreSQL shape).
pub(crate) fn migrations() -> ModuleMigrations {
    ModuleMigrations {
        module: MODULE,
        sqlite: &[(1, "server connections", include_str!("sql/sqlite_v1.sql"))],
        postgres: &[],
    }
}
