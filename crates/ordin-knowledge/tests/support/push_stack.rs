//! The real Push Service for the behavioural check, over the real Journal, Approval Service,
//! Rules Engine, File Service, Identity, Project Service, SharePoint Adapter and Craft Toolkit on
//! SQLite in a temporary folder, with repositories made with `git2`. Nothing here leaves the
//! computer: the one remote is a local bare repository, and the SharePoint Adapter is never called.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use git2::{Repository, Signature};
use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService, PressOrigin};
use ordin_craft_toolkit::{CraftDeps, CraftPlatform, CraftToolDefinitions, CraftToolkit};
use ordin_files::{FileService, FileServiceDeps};
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretName,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_mcp_front::{McpError, ServerName, ToolProvider, ToolRegistry};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_projects::{
    ApprovedServers, ProjectDeps, ProjectService, ProjectSettings, ServerInstaller, ServerLaunch,
    ServerPin, StreamId, StreamPayload, StreamSink,
};
use ordin_push::{
    PushDeps, PushNotice, PushService, StoreFailure, WorkingCopyEntry, WorkingCopyStore,
};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_sharepoint::{SharePointAdapter, SharePointDeps};
use ordin_types::domain::places::WorkingCopyState;
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{AffectedItem, ApprovalPress, PressDecision};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{ApprovalId, BackendId, RunId, WorkingCopyItemId};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::{broadcast, watch};
use tokio_util::sync::CancellationToken;

use crate::support::fakes::TempDir;

pub const TOKEN: &str = "at-secret-token-0001";

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

struct NullSink;

impl StreamSink for NullSink {
    fn send(&self, _: StreamId, _: StreamPayload) {}
}

struct NullRegistry;

impl ToolRegistry for NullRegistry {
    fn register(&self, _: Arc<dyn ToolProvider>) -> Result<(), McpError> {
        Ok(())
    }
    fn tools_changed(&self, _: &ServerName) {}
    fn served_tools(&self, _: &ServerName) -> Option<Vec<String>> {
        None
    }
}

struct NoInstaller;

#[async_trait::async_trait]
impl ServerInstaller for NoInstaller {
    async fn ensure(&self, _: &str, _: &ServerPin) -> Result<ServerLaunch, String> {
        Err("refused".to_owned())
    }
}

/// The program's working-copy store; a team share reads none of it.
struct NoWorkingCopies {
    changes: broadcast::Sender<WorkingCopyItemId>,
}

#[async_trait::async_trait]
impl WorkingCopyStore for NoWorkingCopies {
    async fn entry(&self, _: WorkingCopyItemId) -> Result<WorkingCopyEntry, StoreFailure> {
        Err(StoreFailure {
            code: "WS_ITEM_NOT_FOUND",
            retryable: false,
        })
    }
    async fn set_state(
        &self,
        _: WorkingCopyItemId,
        _: WorkingCopyState,
        _: Option<String>,
    ) -> Result<(), StoreFailure> {
        Err(StoreFailure {
            code: "WS_ITEM_NOT_FOUND",
            retryable: false,
        })
    }
    async fn content(&self, _: WorkingCopyItemId, _: Option<u32>) -> Result<Vec<u8>, StoreFailure> {
        Err(StoreFailure {
            code: "WS_ITEM_NOT_FOUND",
            retryable: false,
        })
    }
    async fn restore_version(&self, _: WorkingCopyItemId, _: u32) -> Result<(), StoreFailure> {
        Err(StoreFailure {
            code: "WS_ITEM_NOT_FOUND",
            retryable: false,
        })
    }
    fn local_changes(&self) -> broadcast::Receiver<WorkingCopyItemId> {
        self.changes.subscribe()
    }
}

fn project_settings() -> ProjectSettings {
    let mut language_servers = BTreeMap::new();
    language_servers.insert(
        "go".to_owned(),
        ApprovedServers {
            approved: vec![ServerPin {
                name: "gopls".to_owned(),
                version: "0.21.0".to_owned(),
            }],
            default: "gopls".to_owned(),
        },
    );
    ProjectSettings {
        language_servers,
        always_approved_kinds: vec!["merge".to_owned()],
        max_attempts: 3,
        gitlab_host: "gitlab.example.org".to_owned(),
        profile_version: "1".to_owned(),
    }
}

fn identity_config(dir: &Path, key_file: &Path) -> IdentityConfig {
    IdentityConfig {
        program: Program::Desktop,
        data_root: dir.to_path_buf(),
        keychain_service: "io.gatblau.ordin.test".to_owned(),
        secret_backend: SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(key_file.to_path_buf()),
        },
        github_app_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    }
}

/// Seeds the Git host sign-in the way a completed sign-in leaves it.
async fn seed_github(pool: &SqlitePool, identity: &IdentityService) {
    let id = ordin_types::CredentialId::new();
    let name = format!("credential/github/{id}");
    let blob = json!({
        "access_token": TOKEN, "refresh_token": null, "expires_at": null, "scopes": [],
        "token_type": "bearer", "client_id": "client-1", "token_endpoint": null,
    });
    identity
        .secrets()
        .put(
            &SecretName::new(&name).expect("secret name"),
            blob.to_string().as_bytes(),
        )
        .await
        .expect("store the token");
    sqlx::query(
        "INSERT INTO credentials (id, kind, account_label, keychain_ref, expires_at, state, created_at) \
         VALUES (?1, 'github', 'person@example.org', ?2, NULL, 'active', ?3)",
    )
    .bind(id.to_string())
    .bind(&name)
    .bind(Timestamp::now().to_wire())
    .execute(pool)
    .await
    .expect("credential row");
    sqlx::query("INSERT INTO credential_meta (credential_id, provider_host, scopes, needs_sign_in) VALUES (?1, 'github.com', '[]', 0)")
        .bind(id.to_string())
        .execute(pool)
        .await
        .expect("credential meta");
}

pub struct Stack {
    pub dir: TempDir,
    pub journal: Arc<Journal>,
    pub store: Datastore,
    pub pool: SqlitePool,
    pub approvals: Arc<ApprovalService>,
    /// The real File Service. Its watcher is not started: the change events come from its own
    /// writes and moves only.
    pub files: Arc<FileService>,
    pub service: Arc<PushService>,
    pub shutdown: CancellationToken,
    pub notices: tokio::sync::Mutex<broadcast::Receiver<PushNotice>>,
}

impl Stack {
    /// Every module's migrations, `extra` (this crate's) included, applied to one SQLite file.
    pub async fn new(extra: ModuleMigrations) -> Stack {
        let dir = TempDir::new();
        let data = dir.0.join("data");
        std::fs::create_dir_all(&data).expect("data folder");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: data.join("journal.db"),
            backup_dir: data.join("backups"),
        })
        .await
        .expect("open the datastore");
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
            .register(ProjectService::migrations())
            .expect("projects");
        registry
            .register(CraftToolkit::migrations())
            .expect("craft");
        registry.register(PushService::migrations()).expect("push");
        registry.register(extra).expect("knowledge");
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply the migrations");
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
        let rules = RulesEngine::new(
            store.clone(),
            journal.clone(),
            profile.clone(),
            approvals.clone(),
            Arc::new(Host),
            RulesConfig {
                ordin_servers: vec!["ordin".to_owned()],
                ordin_server_prefixes: vec![],
                protected_roots: vec![],
                app_data_root: data.clone(),
                program: ProgramKind::Desktop,
                instance_name: "local".to_owned(),
            },
        );
        let agent_folders = Arc::new(
            AgentFolderManager::new(journal.as_ref().clone(), data.clone())
                .await
                .expect("agent folder manager"),
        );
        let files = FileService::new(FileServiceDeps {
            journal: journal.clone(),
            rules: rules.clone(),
            agent_folders,
            data_root: data.clone(),
        })
        .await
        .expect("file service");
        let key_file = dir.0.join("secrets.key");
        std::fs::write(&key_file, "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=")
            .expect("key file");
        let seeding = IdentityService::new(identity_config(&dir.0, &key_file), Some(pool.clone()))
            .await
            .expect("identity");
        seed_github(&pool, &seeding).await;
        drop(seeding);
        let identity = Arc::new(
            IdentityService::new(identity_config(&dir.0, &key_file), Some(pool.clone()))
                .await
                .expect("identity with the sign-in"),
        );
        let sharepoint = Arc::new(
            SharePointAdapter::new(SharePointDeps {
                identity: identity.clone(),
                journal: journal.as_ref().clone(),
                http: reqwest::Client::new(),
            })
            .expect("sharepoint adapter"),
        );
        let (_tx, settings) = watch::channel(project_settings());
        let projects = ProjectService::new(ProjectDeps {
            journal: journal.clone(),
            tool_registry: Arc::new(NullRegistry),
            files: files.clone(),
            rules: rules.clone(),
            identity: identity.clone(),
            installer: Arc::new(NoInstaller),
            streams: Arc::new(NullSink),
            settings,
            data_root: data.clone(),
        })
        .await
        .expect("project service");
        let craft = CraftToolkit::new(CraftDeps {
            journal: journal.as_ref().clone(),
            registry: Arc::new(NullRegistry),
            rules: rules.clone(),
            files: files.clone(),
            profile: profile.clone(),
            http: reqwest::Client::new(),
            data_root: data.clone(),
            platform: CraftPlatform::LinuxX86_64,
            definitions: CraftToolDefinitions {
                format: 1,
                apps: Vec::new(),
            },
        })
        .await
        .expect("craft toolkit");
        let (changes, _) = broadcast::channel(8);
        let localisation = ordin_localisation::Localisation::start(
            ordin_localisation::LocalisationConfig {
                store: ordin_localisation::PreferenceStore::None,
                os_locales: Vec::new(),
                app_version: "0.0.0-test".to_owned(),
            },
            profile.clone(),
        )
        .await
        .expect("localisation");
        let service = PushService::new(PushDeps {
            localisation,
            journal: journal.clone(),
            approvals: approvals.clone(),
            working_copy: Arc::new(NoWorkingCopies { changes }),
            files: files.clone(),
            projects: projects.clone(),
            github: projects,
            sharepoint,
            confluence: None,
            craft,
        });
        journal.subscribe(service.event_sink());
        let notices = tokio::sync::Mutex::new(service.subscribe());
        let shutdown = CancellationToken::new();
        let runner = service.clone();
        let token = shutdown.clone();
        tokio::spawn(async move {
            let _ = runner.run(token).await;
        });
        Stack {
            dir,
            journal,
            store,
            pool,
            approvals,
            files,
            service,
            shutdown,
            notices,
        }
    }

    /// The person presses Approve on a card.
    pub async fn approve(&self, approval: ApprovalId) {
        let record = self.approvals.get(approval).await.expect("the card");
        self.approvals
            .decide(
                ApprovalPress {
                    card_ref: record.card.buttons.approve.clone(),
                    decision: PressDecision::Approve,
                },
                PressOrigin::LocalScreen,
            )
            .await
            .expect("decide the card");
    }

    /// The next notice `pick` accepts, or a failed test after ten seconds.
    pub async fn notice<T>(&self, pick: impl Fn(&PushNotice) -> Option<T>) -> T {
        let mut receiver = self.notices.lock().await;
        let wait = async {
            loop {
                match receiver.recv().await {
                    Ok(n) => {
                        if let Some(found) = pick(&n) {
                            return found;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(e) => panic!("notice channel: {e}"),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), wait)
            .await
            .expect("timed out waiting for a notice")
    }
}

pub fn signature() -> Signature<'static> {
    Signature::now("Asha Patel", "asha@example.org").expect("signature")
}

/// A bare repository with one commit on `main`.
pub fn bare_with_main(dir: &Path, name: &str) -> PathBuf {
    let bare = dir.join(format!("{name}.git"));
    let repo = Repository::init_bare(&bare).expect("bare repository");
    let tree = {
        let blob = repo.blob(b"seed\n").expect("blob");
        let mut builder = repo.treebuilder(None).expect("treebuilder");
        builder.insert("seed.txt", blob, 0o100644).expect("insert");
        repo.find_tree(builder.write().expect("tree"))
            .expect("find tree")
    };
    let sig = signature();
    repo.commit(Some("refs/heads/main"), &sig, &sig, "seed", &tree, &[])
        .expect("seed commit");
    repo.set_head("refs/heads/main").expect("head");
    bare
}

/// A bare clone of `remote` with `origin` pointing at it.
pub fn bare_clone_of(remote: &Path, dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.git"));
    git2::build::RepoBuilder::new()
        .bare(true)
        .clone(remote.to_str().expect("path"), &path)
        .expect("bare clone");
    path
}

/// A commit in `at` whose parent is `main`, adding `pages/<file>` with `bytes`; no branch moves.
pub fn commit_page(at: &Path, file: &str, bytes: &[u8]) -> git2::Oid {
    let repo = Repository::open_bare(at).expect("bare repository");
    let parent = repo
        .find_commit(repo.refname_to_id("refs/heads/main").expect("main"))
        .expect("parent");
    let blob = repo.blob(bytes).expect("blob");
    let pages = {
        let mut b = repo.treebuilder(None).expect("treebuilder");
        b.insert(file, blob, 0o100644).expect("insert");
        b.write().expect("pages tree")
    };
    let mut root = repo
        .treebuilder(Some(&parent.tree().expect("tree")))
        .expect("treebuilder");
    root.insert("pages", pages, 0o040000).expect("insert");
    let tree = repo
        .find_tree(root.write().expect("write"))
        .expect("find tree");
    let sig = signature();
    repo.commit(
        None,
        &sig,
        &sig,
        "Share one knowledge page",
        &tree,
        &[&parent],
    )
    .expect("commit")
}

pub fn main_of(bare: &Path) -> git2::Oid {
    Repository::open_bare(bare)
        .expect("bare")
        .refname_to_id("refs/heads/main")
        .expect("main")
}

/// The bytes of `path` at the tip of `main`.
pub fn file_at_main(bare: &Path, path: &str) -> Vec<u8> {
    let repo = Repository::open_bare(bare).expect("bare");
    let commit = repo.find_commit(main_of(bare)).expect("commit");
    let entry = commit
        .tree()
        .expect("tree")
        .get_path(Path::new(path))
        .expect("entry");
    let blob = repo.find_blob(entry.id()).expect("blob");
    blob.content().to_vec()
}
