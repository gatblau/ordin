//! The fixture shared by the unit tests: real SQLite journal, Rules Engine and Agent Folder
//! Manager in a temporary folder, and a stand-in for the system bin.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalEntry, JournalOptions,
    MigrationRegistry, SystemClock, MIGRATIONS,
};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{BackendId, RunId};
use tokio::sync::broadcast;

use crate::service::Bin;
use crate::types::{AddPlace, FileServiceDeps, PlaceChange};
use crate::{FileService, Place, PlaceKind};

/// A folder removed when dropped; the path is canonical.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-files-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(
            std::fs::canonicalize(&dir)
                .map(ordin_types::paths::plain_form)
                .expect("canonical temporary folder"),
        )
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // A test may leave a folder with no permissions.
        #[cfg(unix)]
        make_tree_writable(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Unix only: only the Unix tests take permissions away.
#[cfg(unix)]
fn make_tree_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    if let Ok(read) = std::fs::read_dir(path) {
        for entry in read.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                make_tree_writable(&entry.path());
            }
        }
    }
}

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> ordin_organisation_profile::Timestamp {
        ordin_organisation_profile::Timestamp::now()
    }
}

struct Host;

#[async_trait::async_trait]
impl RulesHost for Host {
    async fn path_exists(&self, _: &RunKey, _: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn path_contains(&self, _: &RunKey, _: &str, _: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn read_text(&self, _: &RunKey, _: &str) -> Result<Option<String>, FactError> {
        Ok(None)
    }
    async fn append_line(&self, _: &RunKey, _: &str, _: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn remove(&self, _: &RunKey, _: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn runs_in_progress(&self, _: &str) -> Result<u32, FactError> {
        Ok(0)
    }
    async fn paused_run_with_command(&self, _: &[String], _: RunId) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn run_ended(&self, _: &str) -> Result<Option<bool>, FactError> {
        Ok(None)
    }
    async fn setting_value(
        &self,
        _: Option<&str>,
        _: &str,
    ) -> Result<serde_json::Value, FactError> {
        Ok(serde_json::Value::Null)
    }
    async fn setting_rules(&self, _: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }
    fn word(&self, _: &RunKey, _: &Proposal) -> CardWording {
        let affects = vec![AffectedItem::Record {
            label: "A record".to_owned(),
        }];
        CardWording {
            question: "Allow this action?".to_owned(),
            verb: "Allow".to_owned(),
            where_line: "This computer".to_owned(),
            affects: affects.clone(),
            changes: "The action runs.".to_owned(),
            content_label: None,
            proposal: ActionProposal {
                action: "Run a tool".to_owned(),
                destination: "This computer".to_owned(),
                affects,
                consequence: "The action runs.".to_owned(),
            },
        }
    }
    fn plain_line(&self, line: &ControlLine) -> String {
        format!("{} applies.", line.name)
    }
}

/// A computer whose bin works (the file is moved into a folder of the fixture) or does not.
pub struct FakeBin {
    pub folder: PathBuf,
    pub available: Mutex<bool>,
}

impl Bin for FakeBin {
    fn delete(&self, path: &Path) -> Result<(), ()> {
        if !*self.available.lock().unwrap() {
            return Err(());
        }
        std::fs::create_dir_all(&self.folder).map_err(|_| ())?;
        std::fs::rename(path, self.folder.join(uuid::Uuid::now_v7().to_string())).map_err(|_| ())
    }
}

pub struct Fixture {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub rules: Arc<RulesEngine>,
    pub agent_folders: Arc<AgentFolderManager>,
    pub bin: Arc<FakeBin>,
    pub service: Arc<FileService>,
    pub data_root: PathBuf,
    /// A folder the Rules Engine reports as protected.
    pub protected: PathBuf,
}

pub async fn fixture() -> Fixture {
    let dir = TempDir::new();
    let data_root = dir.0.join("data");
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data_root.join("journal.db"),
        backup_dir: data_root.join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ApprovalService::migrations())
        .expect("approval");
    registry.register(RulesEngine::migrations()).expect("rules");
    registry
        .register(ordin_agent_folders::migrations())
        .expect("agent folders");
    registry.register(FileService::migrations()).expect("files");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    let journal = Arc::new(
        Journal::start(
            store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal"),
    );
    let approvals = ApprovalService::new(
        store.clone(),
        journal.clone(),
        ApprovalConfig {
            backend: BackendId(journal.backend_uuid()),
            local_computer_name: "Asha's laptop".to_owned(),
            approvers: None,
        },
    )
    .await
    .expect("approval service");
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(WallClock),
            },
        )
        .await
        .expect("profile service"),
    );
    let protected = dir.0.join("protected");
    std::fs::create_dir_all(&protected).unwrap();
    let rules = RulesEngine::new(
        store.clone(),
        journal.clone(),
        profile,
        approvals,
        Arc::new(Host),
        RulesConfig {
            ordin_servers: vec!["ordin".to_owned()],
            ordin_server_prefixes: vec![],
            protected_roots: vec![protected.clone()],
            app_data_root: data_root.clone(),
            program: ProgramKind::Desktop,
            instance_name: "local".to_owned(),
        },
    );
    let agent_folders = Arc::new(
        AgentFolderManager::new(journal.as_ref().clone(), data_root.clone())
            .await
            .expect("agent folder manager"),
    );
    let bin = Arc::new(FakeBin {
        folder: dir.0.join("bin"),
        available: Mutex::new(true),
    });
    let service = FileService::new_with_bin(
        FileServiceDeps {
            journal: journal.clone(),
            rules: rules.clone(),
            agent_folders: agent_folders.clone(),
            data_root: data_root.clone(),
        },
        bin.clone(),
    )
    .await
    .expect("file service");
    Fixture {
        dir,
        store,
        journal,
        rules,
        agent_folders,
        bin,
        service,
        data_root,
        protected,
    }
}

impl Fixture {
    /// Makes `places/<name>` and adds it as a place of `kind`.
    pub async fn place(&self, name: &str, kind: PlaceKind) -> Place {
        let folder = self.dir.0.join("places").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        self.service
            .add_place(AddPlace {
                path: folder,
                friendly_name: None,
                kind,
            })
            .await
            .expect("add the place")
    }

    pub fn root(&self, place: &Place) -> PathBuf {
        PathBuf::from(&place.path)
    }

    /// Writes a file straight to disk, as another app would.
    pub fn put(&self, place: &Place, path: &str, bytes: &[u8]) -> PathBuf {
        let abs = self.root(place).join(path);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, bytes).unwrap();
        abs
    }

    pub async fn entries(&self, kind: &str) -> Vec<JournalEntry> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.to_owned()),
                },
                None,
            )
            .await
            .expect("journal entries")
            .items
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }
}

/// The next change, or a panic after two seconds.
pub async fn next_change(rx: &mut broadcast::Receiver<PlaceChange>) -> PlaceChange {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("a change within two seconds")
        .expect("an open channel")
}

/// True when no change arrives within `wait`.
pub async fn no_change(rx: &mut broadcast::Receiver<PlaceChange>, wait: Duration) -> bool {
    tokio::time::timeout(wait, rx.recv()).await.is_err()
}

/// A second service over the same datastore, as after a restart of the program.
pub async fn second_service(fx: &Fixture) -> Arc<FileService> {
    FileService::new_with_bin(
        FileServiceDeps {
            journal: fx.journal.clone(),
            rules: fx.rules.clone(),
            agent_folders: fx.agent_folders.clone(),
            data_root: fx.data_root.clone(),
        },
        fx.bin.clone(),
    )
    .await
    .expect("second file service")
}

/// A work folder over the fixture's SQLite datastore, with a bundle holding `/AGENTS.md` and
/// `/knowledge/note.md`.
pub fn work_folder(
    fx: &Fixture,
    writable: Option<Vec<&str>>,
    protected: Vec<&str>,
) -> crate::WorkFolder {
    let bundle = fx.dir.0.join("bundle");
    std::fs::create_dir_all(bundle.join("knowledge")).unwrap();
    std::fs::write(bundle.join("AGENTS.md"), "# Agent\nline two\n").unwrap();
    std::fs::write(bundle.join("knowledge/note.md"), "bundle note\n").unwrap();
    crate::WorkFolder::new(
        fx.store.clone(),
        crate::WorkFolderConfig {
            namespace: "watch-readiness".to_owned(),
            bundle_dir: bundle,
            writable_paths: writable.map(|w| w.into_iter().map(str::to_owned).collect()),
            protected_paths: protected.into_iter().map(str::to_owned).collect(),
        },
    )
    .expect("work folder")
}

/// Waits until a place's index is built.
pub async fn wait_ready(fx: &Fixture, place: &Place) {
    for _ in 0..100 {
        if fx.service.place_row(place.id).await.unwrap().index_state == "ready" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the index was not built in time");
}
