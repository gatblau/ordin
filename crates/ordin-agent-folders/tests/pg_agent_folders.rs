//! The manager on PostgreSQL 18 (agent scaled shape): the migration applies and the core paths
//! (create, lock, grant set, session grants, withdrawal, recovery, removal) read and write the three
//! tables. Needs a reachable container runtime (`DOCKER_HOST`); the test fails, never skips, when
//! the daemon cannot be reached.
//!
//! One container per test binary, held in a process-wide static; each test gets its own empty
//! database, in which the journal's and this module's migrations run. The image is pinned.

use std::sync::Arc;
use std::time::Duration;

use ordin_agent_folders::{AfmError, AgentFolderManager, LockMode, WorkspaceGrant};
use ordin_journal::{
    ApplyMode, DatabaseUrl, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    SystemClock, MIGRATIONS,
};
use ordin_types::domain::places::{FolderAccess, FolderGrant, Grantee, PlaceKind};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, CrewCharterId, FolderGrantId, PlaceId, RunId, WorkspaceId};
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use testcontainers::core::{IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};
use tokio::sync::OnceCell;

const IMAGE: &str = "postgres";
const TAG: &str = "18.6";
const PASSWORD: &str = "ordin-test";
const READY: &str = "database system is ready to accept connections";

struct Server {
    host: String,
    port: u16,
}

static SERVER: OnceCell<Server> = OnceCell::const_new();

async fn server() -> &'static Server {
    SERVER
        .get_or_init(|| async {
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
                         Is the container runtime running, and does DOCKER_HOST point at its \
                         socket? DOCKER_HOST is currently {:?}.",
                        std::env::var("DOCKER_HOST").ok()
                    )
                });
            let host = container.get_host().await.expect("host").to_string();
            let port = container
                .get_host_port_ipv4(5432.tcp())
                .await
                .expect("port");
            // The container lives until the process ends, and is removed then.
            remove_when_process_ends(container.id());
            std::mem::forget(container);
            Server { host, port }
        })
        .await
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

impl Server {
    fn url(&self, database: &str) -> String {
        format!(
            "postgres://postgres:{PASSWORD}@{}:{}/{database}",
            self.host, self.port
        )
    }
}

/// A started journal on a new, empty database with both modules migrated.
async fn journal() -> Journal {
    let server = server().await;
    let name = format!("t_{}", uuid::Uuid::now_v7().simple());
    let mut admin = PgConnection::connect(&server.url("postgres"))
        .await
        .expect("connect to the maintenance database");
    sqlx::raw_sql(AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&mut admin)
        .await
        .expect("create the database");
    let _ = admin.close().await;
    let store = Datastore::open(DatastoreConfig::Postgres {
        url: DatabaseUrl::new(server.url(&name)),
        max_connections: 10,
    })
    .await
    .expect("open the PostgreSQL datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry
        .register(ordin_agent_folders::migrations())
        .unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply migrations");
    Journal::start(
        store,
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(3600),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("start the journal")
}

fn grant(
    workspace_id: WorkspaceId,
    grantee: Grantee,
    place_id: PlaceId,
    access: FolderAccess,
    path: &std::path::Path,
) -> WorkspaceGrant {
    WorkspaceGrant {
        grant: FolderGrant {
            id: FolderGrantId::new(),
            workspace_id,
            grantee,
            place_id,
            access,
            granted_at: Timestamp::now(),
        },
        place_path: path.to_path_buf(),
        place_kind: PlaceKind::PersonsFolder,
    }
}

async fn count(journal: &Journal, statement: &'static str) -> i64 {
    sqlx::query_scalar::<_, i64>(statement)
        .fetch_one(journal.datastore().postgres().expect("PostgreSQL"))
        .await
        .expect("count")
}

#[tokio::test]
async fn folders_leases_and_grants_on_postgres() {
    let journal = journal().await;
    let root = std::fs::canonicalize(std::env::temp_dir())
        .map(ordin_types::paths::plain_form)
        .unwrap()
        .join(format!("ordin-afm-pg-{}", uuid::Uuid::now_v7()));
    let afm = AgentFolderManager::new(journal.clone(), root.clone())
        .await
        .unwrap();
    let temp = std::fs::canonicalize(std::env::temp_dir())
        .map(ordin_types::paths::plain_form)
        .unwrap();
    let outside_p = temp.join(format!("ordin-afm-pg-p-{}", uuid::Uuid::now_v7()));
    let place_q = temp.join(format!("ordin-afm-pg-q-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&outside_p).unwrap();
    std::fs::create_dir_all(&place_q).unwrap();

    // Create, get, leases.
    let (a, w, c) = (
        AgentInstanceId::new(),
        WorkspaceId::new(),
        CrewCharterId::new(),
    );
    let made = afm.create(a).await.unwrap();
    assert_eq!(afm.get(a).await.unwrap(), made);
    assert!(matches!(
        afm.create(a).await,
        Err(AfmError::FolderExists { .. })
    ));
    let session = afm.lock(a, LockMode::Session).await.unwrap();
    assert!(matches!(
        afm.lock(a, LockMode::Session).await.map(|l| l.mode()),
        Ok(LockMode::Session)
    ));
    drop(session);

    // Grant set, session grants, withdrawal.
    let (p, q) = (PlaceId::new(), PlaceId::new());
    afm.set_workspace_grants(
        w,
        vec![
            grant(
                w,
                Grantee::AgentInstance {
                    agent_instance_id: a,
                },
                p,
                FolderAccess::Read,
                &outside_p,
            ),
            grant(
                w,
                Grantee::Crew { crew_charter_id: c },
                q,
                FolderAccess::ReadAndChange,
                &place_q,
            ),
        ],
    )
    .await
    .unwrap();
    let run = RunId::new();
    let access = afm.apply_grants(run, a, w, Some(c)).await.unwrap();
    assert_eq!(access.grants.len(), 2);
    assert_eq!(afm.session_access(run).await.unwrap(), access);
    afm.set_workspace_grants(w, vec![]).await.unwrap();
    assert!(afm.session_access(run).await.unwrap().grants.is_empty());
    assert_eq!(
        count(&journal, "SELECT count(*) FROM workspace_folder_grants").await,
        0
    );

    // Recovery and removal.
    afm.apply_grants(run, a, w, None).await.unwrap();
    let report = afm.recover().await.unwrap();
    assert_eq!(
        report.sessions_cleared, 0,
        "no grant rows were held for the empty set"
    );
    afm.end_session(run).await.unwrap();
    afm.remove(a).await.unwrap();
    assert!(!made.folder_path.exists());
    assert_eq!(
        count(
            &journal,
            "SELECT count(*) FROM agent_folders WHERE state = 'removed'"
        )
        .await,
        1
    );
    assert!(matches!(
        afm.get(a).await,
        Err(AfmError::FolderNotFound { .. })
    ));
    assert_eq!(
        count(
            &journal,
            "SELECT count(*) FROM journal_entries WHERE component = 'agent_folders'"
        )
        .await,
        6,
        "created, grants set twice, grants applied twice, removed"
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside_p);
    let _ = std::fs::remove_dir_all(&place_q);
}
