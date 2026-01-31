//! PostgreSQL 18 for the storage conformance suite (adapted from `crates/ordin-journal/tests/support/pg.rs`).
//!
//! - One container per test binary, started on the first PostgreSQL test and held in a
//!   process-wide `tokio::sync::OnceCell`; `remove_when_process_ends` removes it once the process ends.
//! - The SQL storage's migrations (*Data Model* §B) run once, into a template database; every test that
//!   needs a migrated database gets its own copy with `CREATE DATABASE <unique> TEMPLATE <template>`.
//! - A test that checks the migrations themselves starts from an empty database instead.
//! - The data folder is on tmpfs and the server runs with `fsync=off`, `synchronous_commit=off`,
//!   `full_page_writes=off` and a small `shared_buffers`: safe only because the data is thrown away.
//! - The image is pinned to an exact tag, never `latest`.
//!
//! The container runtime is reached through `DOCKER_HOST` (for Colima:
//! `unix://$COLIMA_HOME/default/docker.sock`). A test fails with a plain message naming `DOCKER_HOST`
//! when the daemon cannot be reached; it never skips.

use std::time::Instant;

use memory::store::sql::{SqlDialect, SqlStorage};
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use testcontainers::core::{IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::sync::{Mutex, OnceCell};

/// The pinned image.
const IMAGE: &str = "postgres";
const TAG: &str = "18.6";
const PASSWORD: &str = "memory-test";
const TEMPLATE: &str = "memory_template";
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
            "-c",
            "shared_buffers=32MB",
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
    // The container lives until the process ends, and is removed then.
    remove_when_process_ends(container.id());
    let host = container
        .get_host()
        .await
        .expect("the container's host")
        .to_string();
    let port = container
        .get_host_port_ipv4(5432.tcp())
        .await
        .expect("the container's mapped port");
    std::mem::forget(container);
    let server = PgServer { host, port };
    make_template(&server).await;
    eprintln!(
        "postgres {TAG}: container and template ready in {} ms",
        started.elapsed().as_millis()
    );
    server
}

/// Removes the container once this test process has ended, however it ends: a normal exit, a failed
/// test, Ctrl-C or a kill. A value held in a static is never dropped, so the container's own clean-up
/// never runs, and testcontainers has no reaper of its own. A detached shell waits for this process to
/// go, then removes the container with the `docker` command line; it ignores the terminal's signals,
/// so stopping a test run still cleans up. Where `sh` or `docker` is missing, nothing is removed.
fn remove_when_process_ends(container_id: &str) {
    let script = format!(
        "trap '' INT HUP TERM; while kill -0 {pid} 2>/dev/null; do sleep 1; done; \
         docker rm -f -v {container_id} >/dev/null 2>&1",
        pid = std::process::id()
    );
    // Not waited on: the shell must outlive this process.
    #[allow(clippy::zombie_processes)]
    let _ = std::process::Command::new("sh")
        .args(["-c", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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

/// Apply every step of `SqlStorage::migrations(Postgres)` to the database at `url`, as a host's
/// migrator would.
pub async fn apply_migrations(url: &str) {
    let mut conn = PgConnection::connect(url)
        .await
        .expect("connect to the database to migrate");
    for (step, sql) in SqlStorage::migrations(SqlDialect::Postgres) {
        sqlx::raw_sql(AssertSqlSafe(*sql))
            .execute(&mut conn)
            .await
            .unwrap_or_else(|e| panic!("migration step {step}: {e}"));
    }
    let _ = conn.close().await;
}

async fn make_template(server: &PgServer) {
    admin(server, format!("CREATE DATABASE {TEMPLATE}")).await;
    apply_migrations(&server.url(TEMPLATE)).await;
}

/// A database name no other test uses.
fn unique_name() -> String {
    format!("t_{}", uuid::Uuid::now_v7().simple())
}

/// An empty database, for tests that check the migrations themselves.
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
