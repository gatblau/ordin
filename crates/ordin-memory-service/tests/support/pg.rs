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
    remove_when_process_ends(container.id());
    std::mem::forget(container);
    let server = PgServer { host, port };
    make_template(&server).await;
    server
}

/// A value held in a static is never dropped, so a detached shell removes the container once this
/// process has ended.
fn remove_when_process_ends(container_id: &str) {
    let script = format!(
        "trap '' INT HUP TERM; while kill -0 {pid} 2>/dev/null; do sleep 1; done; \
         docker rm -f -v {container_id} >/dev/null 2>&1",
        pid = std::process::id()
    );
    #[allow(clippy::zombie_processes)]
    let _ = std::process::Command::new("sh")
        .args(["-c", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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
