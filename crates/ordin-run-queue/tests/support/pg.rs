//! PostgreSQL 18 for this crate's tests (plan 01, "PostgreSQL 18 in tests"). Adapted from the
//! helper of the Journal crate: a test-only file per crate (plan 01, item 8).
//!
//! - One container per test binary, started on the first PostgreSQL test and held in a
//!   process-wide `tokio::sync::OnceCell`; `keep_until_process_ends` has
//!   Docker remove it once the process ends.
//! - The journal's and the run queue's migrations run once, into a template database; every test that needs a migrated
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
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::sync::{Mutex, OnceCell};

/// The pinned image (plan 01, Phase 0 apply trail: `postgres:18.6`).
const IMAGE: &str = "postgres";
const TAG: &str = "18.6";
const PASSWORD: &str = "ordin-test";
const TEMPLATE: &str = "run_queue_template";
const READY: &str = "database system is ready to accept connections";

/// The running server: only its address is kept, so no later call touches the container client
/// (each test runs on its own runtime).
pub struct PgServer {
    host: String,
    port: u16,
    /// Held so a single-test binary can pause the server; only that binary calls `pause`.
    container: ContainerAsync<GenericImage>,
}

static SERVER: OnceCell<PgServer> = OnceCell::const_new();
/// Database creation is serialised: PostgreSQL refuses to copy a template another session is copying.
static CREATE: Mutex<()> = Mutex::const_new(());

impl PgServer {
    /// Freezes the server: connections stay open and nothing answers (the unavailable scenario).
    pub async fn pause(&self) {
        self.container.pause().await.expect("pause the container");
    }

    pub async fn unpause(&self) {
        self.container
            .unpause()
            .await
            .expect("unpause the container");
    }

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
        .with_entrypoint("sh")
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(READY))
        .with_wait_for(WaitFor::message_on_stdout(READY))
        .with_env_var("POSTGRES_PASSWORD", PASSWORD)
        .with_mount(Mount::tmpfs_mount("/var/lib/postgresql"))
        .with_host_config_modifier(|config| config.auto_remove = Some(true))
        .with_cmd([
            "-c",
            WATCHED_POSTGRES,
            "sh",
            "postgres",
            "-c",
            "fsync=off",
            "-c",
            "synchronous_commit=off",
            "-c",
            "full_page_writes=off",
            // Fifty single-connection pools per scenario, a few scenarios at once.
            "-c",
            "max_connections=250",
            // Small buffers: the data is throwaway and the container's memory is shared with other runs.
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
    let host = container
        .get_host()
        .await
        .expect("the container's host")
        .to_string();
    let port = container
        .get_host_port_ipv4(5432.tcp())
        .await
        .expect("the container's mapped port");
    // The container lives until the process ends, and is removed then.
    keep_until_process_ends(&host, port);
    let server = PgServer {
        host,
        port,
        container,
    };
    make_template(&server).await;
    eprintln!(
        "postgres {TAG}: container and template ready in {} ms",
        started.elapsed().as_millis()
    );
    server
}

/// The `application_name` of the connection that keeps the container alive.
const KEEPER: &str = "ordin-test-keeper";

/// The container's command: PostgreSQL through the image's own entrypoint, watched by a shell loop that
/// stops it once the keeper connection (`keep_until_process_ends`) has come (its database
/// `ordin_keeper_seen` exists) and gone, or when it has not
/// come within five minutes. The container is started with `auto_remove`, so Docker removes it as soon as
/// it stops. The arguments after the script name are PostgreSQL's command line.
const WATCHED_POSTGRES: &str = "docker-entrypoint.sh \"$@\" & pg=$!; seen=0; waited=0; \
    while kill -0 $pg 2>/dev/null; do sleep 1; waited=$((waited+1)); \
    n=$(psql -U postgres -h /var/run/postgresql -tAc \"SELECT (SELECT count(*) FROM pg_database WHERE datname = 'ordin_keeper_seen') || ':' || \
    (SELECT count(*) FROM pg_stat_activity WHERE application_name = 'ordin-test-keeper')\" 2>/dev/null); \
    case $n in 1:0) break;; 1:*) seen=1;; esac; \
    [ $seen = 0 ] && [ $waited -ge 300 ] && break; done; kill -INT $pg 2>/dev/null; wait $pg";

/// Keeps one connection to the container open for as long as this test process lives. A value held in a
/// static is never dropped, so testcontainers' own removal never runs, and testcontainers for Rust has no
/// reaper. When the process ends, however it ends (a normal exit, a failed test, Ctrl-C, a kill, or the
/// test's own container stopping), the kernel closes the connection; the container sees it go, stops
/// PostgreSQL and is removed by Docker. Nothing runs on the host after the process, so it works the same
/// when the tests run inside a container with no `docker` command.
fn keep_until_process_ends(host: &str, port: u16) {
    let url =
        format!("postgres://postgres:{PASSWORD}@{host}:{port}/postgres?application_name={KEEPER}");
    let (connected, wait) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("the keeper's runtime");
        runtime.block_on(async move {
            let mut conn = PgConnection::connect(&url)
                .await
                .expect("the keeper's connection");
            // Marks that the keeper came, so a test binary that ends within the watcher's first poll
            // is still seen to have gone.
            sqlx::raw_sql(AssertSqlSafe("CREATE DATABASE ordin_keeper_seen"))
                .execute(&mut conn)
                .await
                .expect("the keeper's mark");
            let _ = connected.send(());
            // Held, never dropped, until the process ends.
            let _conn = conn;
            std::future::pending::<()>().await;
        });
    });
    wait.recv().expect("the keeper connected");
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
        .register(ordin_run_queue::migrations())
        .expect("register the run queue");
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
    open_url(server.url(database), 10).await
}

pub async fn open_url(url: String, max_connections: u32) -> Datastore {
    Datastore::open(DatastoreConfig::Postgres {
        url: DatabaseUrl::new(url),
        max_connections,
    })
    .await
    .expect("open the PostgreSQL datastore")
}
