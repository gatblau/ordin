//! PostgreSQL 18 for this crate's tests (plan 01, "PostgreSQL 18 in tests"), as `ordin-journal`'s
//! helper: one container per test binary, started on first use; migrations run once into a template
//! database, and every test gets a copy; the data folder on tmpfs; the image pinned, never `latest`.
//!
//! The container runtime is reached through `DOCKER_HOST`. A test fails with a plain message naming
//! it when the daemon cannot be reached; it never skips.

use ordin_journal::{
    ApplyMode, DatabaseUrl, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS,
};
use ordin_memory_service::MemoryService;
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use testcontainers::core::{IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::sync::{Mutex, OnceCell};

const IMAGE: &str = "postgres";
const TAG: &str = "18.6";
const PASSWORD: &str = "ordin-test";
const TEMPLATE: &str = "memory_template";
const READY: &str = "database system is ready to accept connections";

pub struct PgServer {
    host: String,
    port: u16,
}

static SERVER: OnceCell<PgServer> = OnceCell::const_new();
/// PostgreSQL refuses to copy a template another session is copying.
static CREATE: Mutex<()> = Mutex::const_new(());

impl PgServer {
    pub fn url(&self, database: &str) -> String {
        format!(
            "postgres://postgres:{PASSWORD}@{}:{}/{database}",
            self.host, self.port
        )
    }
}

pub async fn server() -> &'static PgServer {
    SERVER.get_or_init(start).await
}

async fn start() -> PgServer {
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
        ])
        .start()
        .await
        .unwrap_or_else(|error| {
            panic!(
                "cannot start the PostgreSQL test container ({IMAGE}:{TAG}): {error}. \
                 Is the container runtime running, and does DOCKER_HOST point at its socket? \
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
    keep_until_process_ends(&host, port);
    std::mem::forget(container);
    let server = PgServer { host, port };
    make_template(&server).await;
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
        .register(MemoryService::migrations())
        .expect("register this module");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("migrate the template");
    store.close().await;
}

/// A copy of the migrated template.
pub async fn migrated_database() -> Datastore {
    let server = server().await;
    let name = format!("t_{}", uuid::Uuid::now_v7().simple());
    {
        let _serial = CREATE.lock().await;
        admin(
            server,
            format!("CREATE DATABASE {name} TEMPLATE {TEMPLATE}"),
        )
        .await;
    }
    open(server, &name).await
}

async fn open(server: &PgServer, database: &str) -> Datastore {
    Datastore::open(DatastoreConfig::Postgres {
        url: DatabaseUrl::new(server.url(database)),
        max_connections: 10,
    })
    .await
    .expect("open the PostgreSQL datastore")
}
