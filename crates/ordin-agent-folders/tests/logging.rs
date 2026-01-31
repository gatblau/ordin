//! The log events of AgentFolderManager.md *Observability*, and the two Gherkin scenarios that
//! assert on a log line.
//!
//! Alone in its test binary (plan FU-017): `tracing` caches per call site whether anything
//! listens, so a scoped subscriber can be missed when another test thread logs first. Here one
//! global subscriber is installed before any test logs, and each test reads only the lines that
//! carry its own identifiers.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ordin_agent_folders::{AgentFolderManager, LockMode, WorkspaceGrant};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_types::domain::places::{FolderAccess, FolderGrant, Grantee, PlaceKind};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, FolderGrantId, PlaceId, RunId, WorkspaceId};
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn buffer() -> &'static Buffer {
    static BUFFER: OnceLock<Buffer> = OnceLock::new();
    BUFFER.get_or_init(|| {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(ordin_types::log::JsonLineFormat)
                .with_writer(move || writer.clone()),
        );
        tracing::subscriber::set_global_default(subscriber).expect("one global subscriber");
        buffer.clone()
    })
}

/// Every line written so far that contains `needle`.
fn lines_with(needle: &str) -> Vec<serde_json::Value> {
    let text = String::from_utf8(buffer().0.lock().unwrap().clone()).unwrap();
    text.lines()
        .filter(|line| line.contains(needle))
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Temp {
        let dir = std::env::temp_dir().join(format!("ordin-afm-log-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        Temp(
            std::fs::canonicalize(dir)
                .map(ordin_types::paths::plain_form)
                .unwrap(),
        )
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::canonicalize(path)
            .map(ordin_types::paths::plain_form)
            .unwrap()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn manager(temp: &Temp) -> (AgentFolderManager, PathBuf) {
    buffer();
    let data = temp.0.join("journal");
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data.join("journal.db"),
        backup_dir: data.join("backups"),
    })
    .await
    .unwrap();
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry
        .register(ordin_agent_folders::migrations())
        .unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let journal = Journal::start(
        store,
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(3600),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .unwrap();
    let root = temp.dir("data");
    let afm = AgentFolderManager::new(journal, root.clone())
        .await
        .unwrap();
    (afm, root)
}

fn grant(
    workspace_id: WorkspaceId,
    id: AgentInstanceId,
    place_id: PlaceId,
    path: &Path,
) -> WorkspaceGrant {
    WorkspaceGrant {
        grant: FolderGrant {
            id: FolderGrantId::new(),
            workspace_id,
            grantee: Grantee::AgentInstance {
                agent_instance_id: id,
            },
            place_id,
            access: FolderAccess::Read,
            granted_at: Timestamp::now(),
        },
        place_path: path.to_path_buf(),
        place_kind: PlaceKind::PersonsFolder,
    }
}

#[cfg(unix)]
#[tokio::test]
async fn edge_a_granted_root_that_is_a_link_into_ordin_s_data_is_not_applied() {
    let temp = Temp::new();
    let (afm, root) = manager(&temp).await;
    let (a, w, p) = (AgentInstanceId::new(), WorkspaceId::new(), PlaceId::new());
    afm.create(a).await.unwrap();
    let shortcut = temp.0.join("shortcut");
    std::os::unix::fs::symlink(root.join("agents"), &shortcut).unwrap();
    afm.set_workspace_grants(w, vec![grant(w, a, p, &shortcut)])
        .await
        .unwrap();

    let access = afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    assert!(access.grants.iter().all(|g| g.place_id != p));
    let lines: Vec<_> = lines_with(&p.to_string())
        .into_iter()
        .filter(|l| l["event"] == "agent_folders.grant_skipped")
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["level"], "WARNING");
    assert_eq!(lines[0]["place_id"], p.to_string());
    assert!(lines[0].get("folder_grant_id").is_none(), "place id only");
    assert!(lines[0].get("dropped_fields").is_none());
    let text = lines[0].to_string();
    assert!(
        !text.contains("shortcut") && !text.contains("agents"),
        "{text}"
    );
}

#[tokio::test]
async fn edge_grant_pointing_into_ordin_s_data_is_never_applied() {
    let temp = Temp::new();
    let (afm, root) = manager(&temp).await;
    let (a, w, p) = (AgentInstanceId::new(), WorkspaceId::new(), PlaceId::new());
    afm.create(a).await.unwrap();
    let into_data = grant(w, a, p, &root.join("agents"));
    let grant_id = into_data.grant.id;
    afm.set_workspace_grants(w, vec![into_data])
        .await
        .expect("the call succeeds");

    let access = afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    assert!(
        access.grants.is_empty(),
        "the grant is not stored, so not applied"
    );
    let lines: Vec<_> = lines_with(&p.to_string())
        .into_iter()
        .filter(|l| l["event"] == "agent_folders.grant_skipped")
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["folder_grant_id"], grant_id.to_string());
    assert_eq!(lines[0]["place_id"], p.to_string());
    assert!(!lines[0].to_string().contains(root.to_str().unwrap()));
}

#[tokio::test]
async fn the_events_of_the_observability_section_carry_their_fields_and_no_path() {
    let temp = Temp::new();
    let (afm, root) = manager(&temp).await;
    let (a, w, run) = (AgentInstanceId::new(), WorkspaceId::new(), RunId::new());
    let (gone, kept) = (PlaceId::new(), PlaceId::new());
    afm.create(a).await.unwrap();
    let kept_path = temp.dir("kept-place");
    let gone_path = temp.0.join("unplugged-drive");
    afm.set_workspace_grants(
        w,
        vec![grant(w, a, kept, &kept_path), grant(w, a, gone, &gone_path)],
    )
    .await
    .unwrap();
    afm.apply_grants(run, a, w, None).await.unwrap();
    std::fs::create_dir(root.join("agents").join("not-recorded")).unwrap();
    afm.recover().await.unwrap();
    {
        // A Session request never waits, so the refusal is immediate.
        let _write = afm.lock(a, LockMode::Write).await.unwrap();
        assert!(afm.lock(a, LockMode::Session).await.is_err());
    }
    afm.remove(a).await.unwrap();

    let ours = lines_with(&a.to_string());
    let event = |name: &str| -> Vec<serde_json::Value> {
        ours.iter()
            .filter(|l| l["event"] == name)
            .cloned()
            .collect()
    };
    let created = event("agent_folders.created");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["level"], "INFO");
    assert!(created[0]["duration_ms"].is_number());
    assert_eq!(event("agent_folders.removed").len(), 1);
    let applied = event("agent_folders.grants_applied");
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0]["grants"], 1);
    assert_eq!(applied[0]["workspace_id"], w.to_string());
    assert_eq!(applied[0]["run_id"], run.to_string());
    let set = lines_with(&w.to_string())
        .into_iter()
        .filter(|l| l["event"] == "agent_folders.workspace_grants_set")
        .collect::<Vec<_>>();
    assert_eq!(set.len(), 1);
    assert_eq!(set[0]["skipped"], 0);
    let unreachable = lines_with(&gone.to_string());
    assert!(unreachable
        .iter()
        .any(|l| l["event"] == "agent_folders.grant_unreachable" && l["level"] == "WARNING"));
    let unknown = lines_with("not-recorded");
    assert!(unknown
        .iter()
        .any(|l| l["event"] == "agent_folders.unknown_directory"
            && l["directory_name"] == "not-recorded"));
    let busy = event("agent_folders.lock_busy");
    assert!(!busy.is_empty());
    assert_eq!(busy[0]["requested_mode"], "session");
    assert_eq!(busy[0]["held"], "write");
    for line in ours.iter().chain(&set).chain(&unreachable) {
        assert!(line.get("dropped_fields").is_none(), "{line}");
        let text = line.to_string();
        assert!(!text.contains(root.to_str().unwrap()), "{text}");
        assert!(
            !text.contains("kept-place") && !text.contains("unplugged-drive"),
            "{text}"
        );
    }
}
