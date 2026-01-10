//! PostgreSQL 18 for this crate's tests (plan 01, "PostgreSQL 18 in tests").
//!
//! - One container per test binary, started on the first PostgreSQL test and held in a
//!   process-wide `tokio::sync::OnceCell`; the testcontainers reaper removes it when the process ends.
//! - The journal's migrations run once, into a template database; every test that needs a migrated
//!   database gets its own copy with `CREATE DATABASE <unique> TEMPLATE <template>`.
//! - A test that checks migrations themselves starts from an empty database instead.
//! - The data folder is on tmpfs and the server runs with `fsync=off`, `synchronous_commit=off` and
//!   `full_page_writes=off`: safe only because the data is thrown away.
//! - The image is pinned to an exact tag, never `latest`.
//!
//! The container runtime is reached through `DOCKER_HOST` (for Colima:
//! `unix://$COLIMA_HOME/default/docker.sock`). A test fails with a plain message naming `DOCKER_HOST`
//! when the daemon cannot be reached; it never skips.

use std::time::Instant;

use ordin_journal::{
    ApplyMode, DatabaseUrl, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS,
};
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use testcontainers::core::{IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::sync::{Mutex, OnceCell};

/// The pinned image (plan 01, Phase 0 apply trail: `postgres:18.6`).
const IMAGE: &str = "postgres";
const TAG: &str = "18.6";
const PASSWORD: &str = "ordin-test";
const TEMPLATE: &str = "journal_template";
const READY: &str = "database system is ready to accept connections";

/// The running server: only its address is kept, so no later call touches the container client
/// (each test runs on its own runtime).
pub struct PgServer {
    host: String,
    port: u16,
}

static SERVER: OnceCell<PgServer> = OnceCell::const_new();
/// Database creation is serialised: PostgreSQL refuses to copy a template another session is copying.
static CREATE: Mutex<()> = Mutex::const_new(());

impl PgServer {
    /// The connection string of one database on the server.
    pub fn url(&self, database: &str) -> String {
        format!(
            "postgres://postgres:{PASSWORD}@{}:{}/{database}",
            self.host, self.port
        )
    }
}

/// The shared server of this test binary, started on first use with the template database made.
pub async fn server() -> &'static PgServer {
    SERVER.get_or_init(start).await
}

async fn start() -> PgServer {
    let started = Instant::now();
    let container = GenericImage::new(IMAGE, TAG)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(READY))
        .with_wait_for(WaitFor::message_on_stdout(READY))
        .with_env_var("POSTGRES_PASSWORD", PASSWORD)
        .with_mount(Mount::tmpfs_mount("/var/lib/postgresql"))
        .with_cmd([
            "postgres",
            "-c",
            "fsync=off",
            "-c",
            "synchronous_commit=off",
            "-c",
            "full_page_writes=off",
        ])
        .start()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "cannot start the PostgreSQL test container ({IMAGE}:{TAG}): {error}. \
                 Is the container runtime running, and does DOCKER_HOST point at its socket \
                 (for Colima: export DOCKER_HOST=unix://<colima home>/default/docker.sock)? \
                 DOCKER_HOST is currently {:?}.",
                std::env::var("DOCKER_HOST").ok()
            )
        });
    let host = container
        .get_host()
        .await
        .expect("the container's host")
        .to_string();
    let port = container
        .get_host_port_ipv4(5432.tcp())
        .await
        .expect("the container's mapped port");
    // The container lives until the process ends; the reaper removes it then.
    std::mem::forget(container);
    let server = PgServer { host, port };
    make_template(&server).await;
    eprintln!(
        "postgres {TAG}: container and template ready in {} ms",
        started.elapsed().as_millis()
    );
    server
}

/// Runs one statement on the server's maintenance database.
async fn admin(server: &PgServer, statement: String) {
    let mut conn = PgConnection::connect(&server.url("postgres"))
        .await
        .expect("connect to the maintenance database");
    sqlx::raw_sql(AssertSqlSafe(statement))
        .execute(&mut conn)
        .await
        .expect("maintenance statement");
    let _ = conn.close().await;
}

async fn make_template(server: &PgServer) {
    admin(server, format!("CREATE DATABASE {TEMPLATE}")).await;
    let store = open(server, TEMPLATE).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("migrate the template");
    store.close().await;
}

/// A database name no other test uses.
fn unique_name() -> String {
    format!("t_{}", uuid::Uuid::now_v7().simple())
}

/// An empty database, for tests that check migrations themselves.
pub async fn empty_database() -> String {
    let server = server().await;
    let name = unique_name();
    let _serial = CREATE.lock().await;
    admin(server, format!("CREATE DATABASE {name}")).await;
    server.url(&name)
}

/// A copy of the migrated template (a file copy, in milliseconds).
pub async fn migrated_database() -> String {
    let server = server().await;
    let name = unique_name();
    let _serial = CREATE.lock().await;
    admin(
        server,
        format!("CREATE DATABASE {name} TEMPLATE {TEMPLATE}"),
    )
    .await;
    server.url(&name)
}

/// Opens a `Datastore` on one database of the server.
pub async fn open(server: &PgServer, database: &str) -> Datastore {
    open_url(server.url(database)).await
}

pub async fn open_url(url: String) -> Datastore {
    Datastore::open(DatastoreConfig::Postgres {
        url: DatabaseUrl::new(url),
        max_connections: 10,
    })
    .await
    .expect("open the PostgreSQL datastore")
}
