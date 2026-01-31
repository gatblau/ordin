//! Integration (Playbook §7.2): Home and Search over the real Journal, File Service, Workspace
//! Service and SharePoint and Confluence adapters (lower-phase crates, dev edges), on a real SQLite
//! file in a temporary folder. The adapters point at a `wiremock` stand-in on loopback that no test
//! expects to be called: Home and Search reads their local tables only. The program's two
//! composites (`JournalReader`, the Knowledge Service's reader) are doubles, as the program wires
//! them over modules this crate has no edge to.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_confluence::{ConfluenceAdapter, ConfluenceDeps, ConfluenceSettings};
use ordin_files::{FileService, FileServiceDeps};
use ordin_home_search::{
    HomeError, HomeSearch, HomeSearchDeps, HomeSourceError, JournalReader, RunAgainSubject,
    RunSummary, VersionCheck, WaitingItem,
};
use ordin_identity::{IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_knowledge::{KnowledgeError, KnowledgeReader, PageHit, PageSummary};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_sharepoint::{SharePointAdapter, SharePointDeps};
use ordin_types::common::InputUsed;
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{AffectedItem, ContentRef};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{BackendId, KnowledgePageId, RunId, WorkspaceId};
use ordin_workspaces::{WorkspaceDeps, WorkspaceListFilter, WorkspaceService};
use serde_json::Value;
use tokio::sync::{watch, Mutex};
use wiremock::MockServer;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-home-int-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(
            std::fs::canonicalize(&dir)
                .map(ordin_types::paths::plain_form)
                .expect("canonical"),
        )
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
    async fn setting_value(&self, _: Option<&str>, _: &str) -> Result<Value, FactError> {
        Ok(Value::Null)
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

/// The program's composite, scripted.
#[derive(Default)]
struct Reader {
    finished: Mutex<Vec<RunSummary>>,
    inputs: Mutex<Vec<InputUsed>>,
}

#[async_trait::async_trait]
impl JournalReader for Reader {
    async fn waiting_items(&self) -> Result<Vec<WaitingItem>, HomeSourceError> {
        Ok(Vec::new())
    }
    async fn running(&self) -> Result<Vec<RunSummary>, HomeSourceError> {
        Ok(Vec::new())
    }
    async fn finished_since(
        &self,
        _: Timestamp,
        _: u32,
    ) -> Result<Vec<RunSummary>, HomeSourceError> {
        Ok(self.finished.lock().await.clone())
    }
    async fn run_inputs(&self, _: RunAgainSubject) -> Result<Vec<InputUsed>, HomeSourceError> {
        Ok(self.inputs.lock().await.clone())
    }
}

struct NoPages;

#[async_trait::async_trait]
impl KnowledgeReader for NoPages {
    async fn search_pages(
        &self,
        _: &[WorkspaceId],
        _: &str,
        _: u32,
    ) -> Result<Vec<PageHit>, KnowledgeError> {
        Ok(Vec::new())
    }
    async fn out_of_date_since(&self, _: Timestamp) -> Result<Vec<PageSummary>, KnowledgeError> {
        Ok(Vec::new())
    }
    async fn page_version(&self, _: KnowledgePageId) -> Result<Option<String>, KnowledgeError> {
        Ok(None)
    }
}

struct Env {
    _dir: TempDir,
    _mock: MockServer,
    files: Arc<FileService>,
    workspaces: Arc<WorkspaceService>,
    reader: Arc<Reader>,
    home: Arc<HomeSearch>,
}

async fn env() -> Env {
    let dir = TempDir::new();
    let data_root = dir.path().join("data");
    let documents = dir.path().join("Documents");
    let rules_data = dir.path().join("rules-data");
    for folder in [&data_root, &documents, &rules_data] {
        std::fs::create_dir_all(folder).expect("folder");
    }
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data_root.join("journal.db"),
        backup_dir: data_root.join("backups"),
    })
    .await
    .expect("datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ModuleMigrations {
            module: ordin_identity::MODULE_NAME,
            sqlite: ordin_identity::SQLITE_MIGRATIONS,
            postgres: &[],
        })
        .expect("identity");
    registry
        .register(ApprovalService::migrations())
        .expect("approval");
    registry.register(RulesEngine::migrations()).expect("rules");
    registry
        .register(ordin_agent_folders::migrations())
        .expect("agent folders");
    registry.register(FileService::migrations()).expect("files");
    registry
        .register(ordin_sharepoint::migrations())
        .expect("sharepoint");
    registry
        .register(ConfluenceAdapter::migrations())
        .expect("confluence");
    registry
        .register(ordin_workspaces::migrations())
        .expect("workspaces");
    registry.register(HomeSearch::migrations()).expect("home");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply");
    let pool = store.sqlite().expect("sqlite").clone();
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
        .expect("journal"),
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
    .expect("approvals");
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(WallClock),
            },
        )
        .await
        .expect("profile"),
    );
    let rules = RulesEngine::new(
        store.clone(),
        journal.clone(),
        profile,
        approvals.clone(),
        Arc::new(Host),
        RulesConfig {
            ordin_servers: vec!["ordin".to_owned()],
            ordin_server_prefixes: vec![],
            protected_roots: vec![PathBuf::from("/System")],
            app_data_root: rules_data,
            program: ProgramKind::Desktop,
            instance_name: "local".to_owned(),
        },
    );
    let agent_folders = Arc::new(
        AgentFolderManager::new(journal.as_ref().clone(), data_root.clone())
            .await
            .expect("agent folders"),
    );
    let files = FileService::new(FileServiceDeps {
        journal: journal.clone(),
        rules: rules.clone(),
        agent_folders,
        data_root: data_root.clone(),
    })
    .await
    .expect("files");
    let mock = MockServer::start().await;
    let key_file = dir.path().join("secrets.key");
    std::fs::write(&key_file, "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=").expect("key");
    let identity = Arc::new(
        IdentityService::new(
            IdentityConfig {
                program: Program::Desktop,
                data_root: dir.path().to_path_buf(),
                keychain_service: "io.gatblau.ordin.home-test".to_owned(),
                secret_backend: SecretBackendConfig::EncryptedFile {
                    key_source: KeySource::KeyFile(key_file),
                },
                github_app_client_id: None,
                http: reqwest::Client::new(),
                browser: None,
            },
            Some(pool.clone()),
        )
        .await
        .expect("identity"),
    );
    let sharepoint = Arc::new(
        SharePointAdapter::new(SharePointDeps {
            identity: identity.clone(),
            journal: journal.as_ref().clone(),
            http: reqwest::Client::new(),
        })
        .expect("sharepoint")
        .with_stand_in(
            &format!("{}/v1.0", mock.uri()),
            0.001,
            Duration::from_secs(5),
        )
        .expect("stand-in"),
    );
    let (_settings, receiver) = watch::channel(ConfluenceSettings {
        site: Some(url::Url::parse("https://acme.atlassian.net").expect("site")),
    });
    let confluence = ConfluenceAdapter::with_stand_in(
        ConfluenceDeps {
            journal: journal.clone(),
            identity: identity.clone(),
            approvals,
            files: files.clone(),
            http: reqwest::Client::new(),
            settings: receiver,
            word_to_markdown: None,
        },
        &mock.uri(),
        0.001,
        Duration::from_secs(5),
    );
    let workspaces = WorkspaceService::new(WorkspaceDeps {
        journal: journal.as_ref().clone(),
        rules,
        files: files.clone(),
        sharepoint: sharepoint.clone(),
        confluence: Some(confluence.clone()),
        jira: None,
        data_root,
        documents_dir: documents,
        on_server: false,
    })
    .await
    .expect("workspaces");
    let reader = Arc::new(Reader::default());
    let home = HomeSearch::new(HomeSearchDeps {
        journal,
        reader: reader.clone(),
        knowledge: Arc::new(NoPages),
        files: files.clone(),
        workspaces: workspaces.clone(),
        sharepoint,
        confluence: Some(confluence),
    })
    .await
    .expect("home");
    Env {
        _dir: dir,
        _mock: mock,
        files,
        workspaces,
        reader,
        home,
    }
}

#[tokio::test]
async fn home_names_real_workspaces_and_fills_last_seen() {
    let env = env().await;
    let ws = env
        .workspaces
        .create("Q3 supplier review".to_owned())
        .await
        .expect("workspace");
    env.reader.finished.lock().await.push(RunSummary {
        workspace_id: ws.id,
        run_id: Some(RunId::new()),
        crew_run_id: None,
        title: "Supplier summary".to_owned(),
        state: "completed".to_owned(),
        at: Timestamp::now(),
        can_run_again: true,
    });
    let view = env.home.home(Some(ws.id)).await.expect("home");
    assert_eq!(
        view.since_last_looked[0].workspace_name.as_deref(),
        Some("Q3 supplier review")
    );
    assert_eq!(
        env.home.home(Some(WorkspaceId::new())).await.unwrap_err(),
        HomeError::WorkspaceNotFound
    );
    env.home.workspace_opened(ws.id).await.expect("opened");
    let listed = env
        .workspaces
        .list(WorkspaceListFilter::NotInBin)
        .await
        .expect("list");
    let filled = env.home.with_last_seen(listed).await.expect("filled");
    assert!(filled[0].last_seen_at.is_some());
    let answer = env.home.search("supplier", None).await.expect("search");
    assert_eq!(answer.go_to[0].label, "Q3 supplier review");
    // No adapter call was needed: the poll reads only local tables when no link is active.
    env.home.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn run_again_reads_the_file_hash_from_the_real_file_service() {
    let env = env().await;
    let ws = env
        .workspaces
        .create("Q4 forecast".to_owned())
        .await
        .expect("workspace");
    let places = env.workspaces.places(ws.id).await.expect("places");
    let place = places[0].clone();
    std::fs::write(Path::new(&place.path).join("quote.txt"), b"day rate 640").expect("write");
    *env.reader.inputs.lock().await = vec![
        InputUsed {
            name: "quote.txt".to_owned(),
            location: format!("place:{}/quote.txt", place.id),
            version_read: "0".repeat(64),
        },
        InputUsed {
            name: "gone.txt".to_owned(),
            location: format!("place:{}/gone.txt", place.id),
            version_read: "0".repeat(64),
        },
    ];
    let inputs = env
        .home
        .run_again_inputs(RunAgainSubject::Run {
            run_id: RunId::new(),
        })
        .await
        .expect("inputs");
    assert_eq!(inputs[0].check, VersionCheck::Newer);
    assert_eq!(inputs[0].newer_version.as_deref().map(str::len), Some(64));
    assert_eq!(inputs[1].check, VersionCheck::Gone);
}

#[tokio::test]
async fn an_edit_outside_ordin_marks_the_document_until_opened() {
    let env = env().await;
    let ws = env
        .workspaces
        .create("Q3 supplier review".to_owned())
        .await
        .expect("workspace");
    let place = env.workspaces.places(ws.id).await.expect("places")[0].clone();
    env.home.start().await;
    env.files.watch(&[place.id]).await.expect("watch");
    tokio::time::sleep(Duration::from_millis(300)).await;
    std::fs::write(Path::new(&place.path).join("notes.md"), b"edited elsewhere").expect("write");
    let mut marked = false;
    for _ in 0..150 {
        let marks = env.home.changed_marks(ws.id).await.expect("marks");
        if marks.iter().any(|m| m.path == "notes.md") {
            marked = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        marked,
        "the File Service's change event marked the document"
    );
    env.home
        .document_opened(
            ws.id,
            ContentRef::PlaceFile {
                place_id: place.id,
                relative_path: "notes.md".to_owned(),
            },
        )
        .await
        .expect("opened");
    assert!(env
        .home
        .changed_marks(ws.id)
        .await
        .expect("marks")
        .is_empty());
}
