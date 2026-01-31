//! The fixture of this crate's tests: the real Journal, Approval Service, Rules Engine, File Service,
//! Identity, Project Service, SharePoint and Confluence adapters and Craft Toolkit over SQLite on a
//! temporary file, with the two HTTP services stood in by `wiremock` on loopback, a stand-in for the
//! program's working-copy store and for GitHub, and repositories made with `git2`. A missing fixture
//! fails the test; nothing here skips.
#![allow(dead_code, unused_variables)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use git2::{Repository, Signature};
use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService, PressOrigin};
use ordin_confluence::{ConfluenceAdapter, ConfluenceDeps, ConfluenceSettings};
use ordin_craft_toolkit::{CraftDeps, CraftPlatform, CraftToolDefinitions, CraftToolkit};
use ordin_files::{AddPlace, FileService, FileServiceDeps};
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretName,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalEvent, JournalOptions,
    MigrationRegistry, ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_mcp_front::{McpError, ServerName, ToolProvider, ToolRegistry};
use ordin_organisation_profile::{
    Clock, InstallerSettings, OrganisationProfileService, ProfileMode, ProfileOptions,
};
use ordin_projects::{
    ApprovedServers, BranchReviewRules, BranchReviewStatus, BranchWithFiles, GitCredential,
    GithubAccess, GithubError, GithubPerson, OpenPullRequest, ProjectDeps, ProjectService,
    ProjectSettings, PullRequestRef, PullRequestState, PullRequestView, RepoFile, RepoFolderRef,
    RepoPermission, ReviewRequest, ReviewVerdict, ServerInstaller, ServerLaunch, ServerPin,
    StreamId, StreamPayload, StreamSink,
};
use ordin_push::{
    PushDeps, PushNotice, PushService, StoreFailure, WorkingCopyEntry, WorkingCopyStore,
};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_sharepoint::{SharePointAdapter, SharePointDeps};
use ordin_types::domain::places::{
    Place, PlaceKind, SourceLinkKind, WorkingCopyItem, WorkingCopyState,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{AffectedItem, ApprovalPress, PressDecision};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{ApprovalId, BackendId, RunId, SourceLinkId, WorkingCopyItemId, WorkspaceId};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::{broadcast, watch};
use tokio_util::sync::CancellationToken;
use wiremock::MockServer;

pub const TOKEN: &str = "at-secret-token-0001";
pub const SITE: &str = "https://acme.atlassian.net";
pub const CLOUD: &str = "cloud-1";
pub const GRAPH_SCOPES: [&str; 2] = [
    "https://graph.microsoft.com/Files.ReadWrite.All",
    "https://graph.microsoft.com/Sites.Read.All",
];
pub const CONFLUENCE_SCOPES: [&str; 12] = [
    "read:space:confluence",
    "read:page:confluence",
    "write:page:confluence",
    "read:attachment:confluence",
    "write:attachment:confluence",
    "delete:page:confluence",
    "delete:attachment:confluence",
    "read:space.permission:confluence",
    "read:content.restriction:confluence",
    "search:confluence",
    "read:confluence-user",
    "offline_access",
];

/// A folder removed when dropped; the path is canonical.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-push-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(
            std::fs::canonicalize(&dir)
                .map(ordin_types::paths::plain_form)
                .expect("canonical temporary folder"),
        )
    }

    pub fn path(&self) -> &Path {
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
        gitlab_host: "gitlab.acme.co.uk".to_owned(),
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

/// Seeds one credential the way a completed sign-in leaves it.
async fn seed(
    pool: &SqlitePool,
    identity: &IdentityService,
    kind: &str,
    provider_host: &str,
    scopes: &[&str],
) {
    let id = ordin_types::CredentialId::new();
    let name = format!("credential/{kind}/{id}");
    let blob = json!({
        "access_token": TOKEN,
        "refresh_token": null,
        "expires_at": null,
        "scopes": scopes,
        "token_type": "bearer",
        "client_id": "client-1",
        "token_endpoint": null,
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
         VALUES (?1, ?2, 'person@example.org', ?3, NULL, 'active', ?4)",
    )
    .bind(id.to_string())
    .bind(kind)
    .bind(&name)
    .bind(Timestamp::now().to_wire())
    .execute(pool)
    .await
    .expect("credential row");
    sqlx::query(
        "INSERT INTO credential_meta (credential_id, provider_host, scopes, needs_sign_in) VALUES (?1, ?2, ?3, 0)",
    )
    .bind(id.to_string())
    .bind(provider_host)
    .bind(serde_json::to_string(scopes).expect("scopes"))
    .execute(pool)
    .await
    .expect("credential meta");
}

// ---- the program's working-copy store ----

#[derive(Default)]
pub struct StoreInner {
    pub entries: HashMap<WorkingCopyItemId, WorkingCopyEntry>,
    pub contents: HashMap<WorkingCopyItemId, Vec<u8>>,
    /// Earlier File Service versions of an item's bytes, kept when the document is edited.
    pub versions: HashMap<(WorkingCopyItemId, u32), Vec<u8>>,
    pub states: Vec<(WorkingCopyItemId, WorkingCopyState, Option<String>)>,
    pub restored: Vec<(WorkingCopyItemId, u32)>,
    pub fail_entry: bool,
}

pub struct TestStore {
    pub inner: Mutex<StoreInner>,
    changes: broadcast::Sender<WorkingCopyItemId>,
}

impl TestStore {
    pub fn new() -> Arc<TestStore> {
        let (changes, _) = broadcast::channel(64);
        Arc::new(TestStore {
            inner: Mutex::new(StoreInner::default()),
            changes,
        })
    }

    pub fn add(&self, entry: WorkingCopyEntry, content: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        inner.contents.insert(entry.item.id, content.to_vec());
        inner.entries.insert(entry.item.id, entry);
    }

    pub fn state_of(&self, id: WorkingCopyItemId) -> WorkingCopyState {
        self.inner.lock().unwrap().entries[&id].item.state
    }

    pub fn origin_version_of(&self, id: WorkingCopyItemId) -> String {
        self.inner.lock().unwrap().entries[&id]
            .item
            .origin_version_read
            .clone()
    }

    pub fn states_set(&self, id: WorkingCopyItemId) -> Vec<WorkingCopyState> {
        self.inner
            .lock()
            .unwrap()
            .states
            .iter()
            .filter(|(i, _, _)| *i == id)
            .map(|(_, s, _)| *s)
            .collect()
    }

    /// The document is saved as a new version on this computer, with no local-change notice: an edit
    /// the hold of a proposed push (D) does not see, for example one made after the card was approved.
    pub fn edit_unseen(&self, id: WorkingCopyItemId, bytes: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.entries.get_mut(&id).expect("entry");
        let before = entry.item.local_version;
        entry.item.local_version += 1;
        let previous = inner.contents.insert(id, bytes.to_vec()).expect("content");
        inner.versions.insert((id, before), previous);
    }

    /// The person edits the document on this computer.
    pub fn local_edit(&self, id: WorkingCopyItemId) {
        let _ = self.changes.send(id);
    }
}

#[async_trait::async_trait]
impl WorkingCopyStore for TestStore {
    async fn entry(&self, id: WorkingCopyItemId) -> Result<WorkingCopyEntry, StoreFailure> {
        let inner = self.inner.lock().unwrap();
        if inner.fail_entry {
            return Err(StoreFailure {
                code: "WS_STORE_UNAVAILABLE",
                retryable: true,
            });
        }
        inner.entries.get(&id).cloned().ok_or(StoreFailure {
            code: "WS_ITEM_NOT_FOUND",
            retryable: false,
        })
    }

    async fn set_state(
        &self,
        id: WorkingCopyItemId,
        state: WorkingCopyState,
        origin_version: Option<String>,
    ) -> Result<(), StoreFailure> {
        let mut inner = self.inner.lock().unwrap();
        inner.states.push((id, state, origin_version.clone()));
        let Some(entry) = inner.entries.get_mut(&id) else {
            return Err(StoreFailure {
                code: "WS_ITEM_NOT_FOUND",
                retryable: false,
            });
        };
        entry.item.state = state;
        if let (WorkingCopyState::Pushed, Some(version)) = (state, origin_version) {
            entry.item.origin_version_read = version;
        }
        Ok(())
    }

    async fn content(
        &self,
        id: WorkingCopyItemId,
        local_version: Option<u32>,
    ) -> Result<Vec<u8>, StoreFailure> {
        let inner = self.inner.lock().unwrap();
        if let Some(bytes) = local_version.and_then(|v| inner.versions.get(&(id, v))) {
            return Ok(bytes.clone());
        }
        inner.contents.get(&id).cloned().ok_or(StoreFailure {
            code: "WS_ITEM_NOT_FOUND",
            retryable: false,
        })
    }

    async fn restore_version(
        &self,
        id: WorkingCopyItemId,
        local_version: u32,
    ) -> Result<(), StoreFailure> {
        // As the File Service does: the restored bytes are written as a new version.
        let mut inner = self.inner.lock().unwrap();
        inner.restored.push((id, local_version));
        if let Some(entry) = inner.entries.get_mut(&id) {
            entry.item.local_version += 1;
            let version = entry.item.local_version;
            if let Some(bytes) = inner.contents.get(&id).cloned() {
                inner.versions.insert((id, version), bytes);
            }
        }
        Ok(())
    }

    fn local_changes(&self) -> broadcast::Receiver<WorkingCopyItemId> {
        self.changes.subscribe()
    }
}

// ---- GitHub ----

#[derive(Default)]
pub struct GithubState {
    pub credentials: Option<Result<(), GithubError>>,
    pub open_pull_request: Option<Result<PullRequestRef, GithubError>>,
    pub for_branch: Option<PullRequestRef>,
    pub branch_files: Option<Result<String, GithubError>>,
    pub opened: Vec<OpenPullRequest>,
    pub branch_calls: Vec<(String, String, Vec<String>)>,
}

/// A scripted GitHub. `GitToken` has no public constructor, so the credential comes from the real
/// Project Service over the seeded sign-in; every other answer is scripted.
pub struct StubGithub {
    pub state: Mutex<GithubState>,
    real: Arc<ProjectService>,
}

pub fn pull_request(number: u64) -> PullRequestRef {
    PullRequestRef {
        number,
        url: format!("https://github.com/acme/repo/pull/{number}"),
        state: PullRequestState::Open,
    }
}

#[async_trait::async_trait]
impl GithubAccess for StubGithub {
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError> {
        let scripted = self.state.lock().unwrap().credentials.clone();
        match scripted {
            Some(Err(error)) => Err(error),
            _ => self.real.git_credentials(host).await,
        }
    }

    async fn open_pull_request(&self, req: OpenPullRequest) -> Result<PullRequestRef, GithubError> {
        let mut state = self.state.lock().unwrap();
        state.opened.push(req);
        state
            .open_pull_request
            .clone()
            .unwrap_or_else(|| Ok(pull_request(41)))
    }

    async fn pull_request_for_branch(
        &self,
        host: &str,
        repository: &str,
        head: &str,
    ) -> Result<Option<PullRequestRef>, GithubError> {
        Ok(self.state.lock().unwrap().for_branch.clone())
    }

    async fn create_branch_with_files(&self, req: BranchWithFiles) -> Result<String, GithubError> {
        let mut state = self.state.lock().unwrap();
        state.branch_calls.push((
            req.branch.clone(),
            req.base.clone(),
            req.files.iter().map(|(name, _)| name.clone()).collect(),
        ));
        state
            .branch_files
            .clone()
            .unwrap_or_else(|| Ok("c0ffee".to_owned()))
    }

    async fn repository_permission(
        &self,
        host: &str,
        repository: &str,
    ) -> Result<RepoPermission, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn default_branch(&self, host: &str, repository: &str) -> Result<String, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn commit_verified(
        &self,
        host: &str,
        repository: &str,
        sha: &str,
    ) -> Result<bool, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn branch_review_rules(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewRules, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn pull_request(
        &self,
        host: &str,
        repository: &str,
        number: u64,
    ) -> Result<PullRequestView, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn review_requests(&self, label: &str) -> Result<Vec<ReviewRequest>, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn review(
        &self,
        host: &str,
        repository: &str,
        number: u64,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<(), GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn merge(&self, host: &str, repository: &str, number: u64) -> Result<bool, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn close(&self, host: &str, repository: &str, number: u64) -> Result<(), GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn reviewers(
        &self,
        host: &str,
        repository: &str,
        query: &str,
    ) -> Result<Vec<GithubPerson>, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn file_at(
        &self,
        host: &str,
        repository: &str,
        git_ref: &str,
        path: &str,
    ) -> Result<(Vec<u8>, String), GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn update_file(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
        path: &str,
        bytes: Vec<u8>,
        expected_sha: &str,
        message: &str,
    ) -> Result<String, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn list_files(
        &self,
        folder: &RepoFolderRef,
        glob: &str,
    ) -> Result<Vec<RepoFile>, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn read_file(
        &self,
        folder: &RepoFolderRef,
        name: &str,
    ) -> Result<(Vec<u8>, String), GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn file_last_commit(
        &self,
        folder: &RepoFolderRef,
        name: &str,
    ) -> Result<String, GithubError> {
        Err(GithubError::HostNotSupported)
    }

    async fn branch_review_status(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewStatus, GithubError> {
        Err(GithubError::HostNotSupported)
    }
}

// ---- the stack ----

pub struct Stack {
    pub dir: TempDir,
    pub journal: Arc<Journal>,
    pub store: Datastore,
    pub pool: SqlitePool,
    pub approvals: Arc<ApprovalService>,
    pub files: Arc<FileService>,
    pub projects: Arc<ProjectService>,
    pub sharepoint: Arc<SharePointAdapter>,
    pub confluence: Arc<ConfluenceAdapter>,
    pub craft: Arc<CraftToolkit>,
    pub localisation: Arc<ordin_localisation::Localisation>,
    pub graph: MockServer,
    pub gateway: MockServer,
    pub working: Arc<TestStore>,
    pub github: Arc<StubGithub>,
    pub service: Arc<PushService>,
    pub workspace: WorkspaceId,
    pub place: Place,
    pub shutdown: CancellationToken,
    /// The task running `service`; `stop` waits for it.
    runner: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub notices: tokio::sync::Mutex<broadcast::Receiver<PushNotice>>,
}

pub struct Options {
    /// The ordin server has no Confluence Adapter (2B-3 absent handle).
    pub with_confluence: bool,
    /// How long the Confluence stand-in is waited for.
    pub confluence_timeout: Duration,
    /// A fake conversion app installed in the Craft Toolkit (FU-105); `None` installs none.
    pub maker: Option<Maker>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            with_confluence: true,
            confluence_timeout: Duration::from_secs(5),
            maker: None,
        }
    }
}

pub fn workspace() -> WorkspaceId {
    WorkspaceId::new()
}

impl Stack {
    pub async fn new() -> Stack {
        Stack::with(Options::default()).await
    }

    pub async fn with(options: Options) -> Stack {
        let dir = TempDir::new();
        let data = dir.path().join("data");
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
            .register(ConfluenceAdapter::migrations())
            .expect("confluence");
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
        registry
            .register(ModuleMigrations {
                module: ordin_organisation_profile::MODULE_NAME,
                sqlite: ordin_organisation_profile::SQLITE_MIGRATIONS,
                postgres: &[],
            })
            .expect("organisation profile");
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
        let profile_mode = match &options.maker {
            Some(maker) => {
                install_maker(&data, &pool, maker).await;
                ProfileMode::Read {
                    pool: pool.clone(),
                    data_root: data.clone(),
                    http: reqwest::Client::builder()
                        .no_proxy()
                        .build()
                        .expect("client"),
                    build_defaults: InstallerSettings::default(),
                }
            }
            None => ProfileMode::NotUsed,
        };
        let profile = Arc::new(
            OrganisationProfileService::start(
                profile_mode,
                ProfileOptions {
                    refresh_interval: Duration::from_secs(3600),
                    clock: Arc::new(WallClock),
                },
            )
            .await
            .expect("profile service"),
        );
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
        let folder = dir.path().join("work");
        std::fs::create_dir_all(&folder).expect("workspace folder");
        let place = files
            .add_place(AddPlace {
                path: folder,
                friendly_name: Some("Supplier quotes".to_owned()),
                kind: PlaceKind::WorkspaceFolder,
            })
            .await
            .expect("place");

        let graph = MockServer::start().await;
        let gateway = MockServer::start().await;
        let key_file = dir.path().join("secrets.key");
        std::fs::write(&key_file, STANDARD.encode([7u8; 32])).expect("key file");
        let seeding =
            IdentityService::new(identity_config(dir.path(), &key_file), Some(pool.clone()))
                .await
                .expect("identity");
        seed(
            &pool,
            &seeding,
            "microsoft",
            "login.microsoftonline.com",
            &GRAPH_SCOPES,
        )
        .await;
        seed(
            &pool,
            &seeding,
            "confluence",
            "acme.atlassian.net",
            &CONFLUENCE_SCOPES,
        )
        .await;
        seed(&pool, &seeding, "github", "github.com", &[]).await;
        drop(seeding);
        let identity = Arc::new(
            IdentityService::new(identity_config(dir.path(), &key_file), Some(pool.clone()))
                .await
                .expect("identity with the sign-ins"),
        );
        let sharepoint = Arc::new(
            SharePointAdapter::new(SharePointDeps {
                identity: identity.clone(),
                journal: journal.as_ref().clone(),
                http: reqwest::Client::new(),
            })
            .expect("sharepoint adapter")
            .with_stand_in(
                &format!("{}/v1.0", graph.uri()),
                0.001,
                Duration::from_secs(2),
            )
            .expect("sharepoint stand-in"),
        );
        let (_settings, receiver) = watch::channel(ConfluenceSettings {
            site: Some(url::Url::parse(SITE).expect("site")),
        });
        let confluence = ConfluenceAdapter::with_stand_in(
            ConfluenceDeps {
                journal: journal.clone(),
                identity: identity.clone(),
                approvals: approvals.clone(),
                files: files.clone(),
                http: reqwest::Client::new(),
                settings: receiver,
                word_to_markdown: None,
            },
            &gateway.uri(),
            0.001,
            options.confluence_timeout,
        );
        let (_project_settings_tx, project_settings_rx) = watch::channel(project_settings());
        let projects = ProjectService::new(ProjectDeps {
            journal: journal.clone(),
            tool_registry: Arc::new(NullRegistry),
            files: files.clone(),
            rules: rules.clone(),
            identity: identity.clone(),
            installer: Arc::new(NoInstaller),
            streams: Arc::new(NullSink),
            settings: project_settings_rx,
            data_root: data.clone(),
        })
        .await
        .expect("project service");
        let craft = CraftToolkit::new(CraftDeps {
            journal: journal.as_ref().clone(),
            registry: Arc::new(NullRegistry),
            rules: rules.clone(),
            files: files.clone(),
            profile,
            http: reqwest::Client::new(),
            data_root: data.clone(),
            platform: CraftPlatform::LinuxX86_64,
            definitions: CraftToolDefinitions {
                format: 1,
                apps: options.maker.iter().map(maker_definition).collect(),
            },
        })
        .await
        .expect("craft toolkit");
        if options.maker.is_some() {
            craft.start().await.expect("the toolkit starts");
        }
        let working = TestStore::new();
        let github = Arc::new(StubGithub {
            state: Mutex::new(GithubState::default()),
            real: projects.clone(),
        });
        let service = PushService::new(PushDeps {
            journal: journal.clone(),
            approvals: approvals.clone(),
            working_copy: working.clone(),
            files: files.clone(),
            projects: projects.clone(),
            github: github.clone(),
            sharepoint: sharepoint.clone(),
            confluence: options.with_confluence.then(|| confluence.clone()),
            craft: craft.clone(),
            localisation: localisation.clone(),
        });
        journal.subscribe(service.event_sink());
        let notices = tokio::sync::Mutex::new(service.subscribe());
        let shutdown = CancellationToken::new();
        let runner = service.clone();
        let token = shutdown.clone();
        let runner = tokio::spawn(async move {
            let _ = runner.run(token).await;
        });
        Stack {
            dir,
            journal,
            store,
            pool,
            approvals,
            files,
            projects,
            sharepoint,
            confluence,
            craft,
            localisation,
            graph,
            gateway,
            working,
            github,
            service,
            workspace: WorkspaceId::new(),
            place,
            shutdown,
            runner: tokio::sync::Mutex::new(Some(runner)),
            notices,
        }
    }

    /// Stops the running service and waits until its task has ended, so that its own recovery
    /// is finished and nothing it does can overlap what the test sets up next.
    pub async fn stop(&self) {
        self.shutdown.cancel();
        if let Some(runner) = self.runner.lock().await.take() {
            runner.await.expect("the service task ends");
        }
    }

    /// The person presses the approve button on a card.
    pub async fn approve(&self, approval: ApprovalId) {
        self.decide(approval, PressDecision::Approve).await;
    }

    pub async fn reject(&self, approval: ApprovalId) {
        self.decide(approval, PressDecision::Reject).await;
    }

    async fn decide(&self, approval: ApprovalId, decision: PressDecision) {
        let record = self.approvals.get(approval).await.expect("the card");
        let card_ref = match decision {
            PressDecision::Approve => record.card.buttons.approve.clone(),
            _ => record.card.buttons.reject.clone(),
        };
        self.approvals
            .decide(
                ApprovalPress { card_ref, decision },
                PressOrigin::LocalScreen,
            )
            .await
            .expect("decide the card");
    }

    /// Waits until `check` holds, or fails the test after thirty seconds.
    pub async fn until<F, Fut>(&self, what: &str, mut check: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for _ in 0..1500 {
            if check().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting for: {what}");
    }

    /// The next notice that satisfies `pick`, or a failed test after thirty seconds.
    pub async fn notice<T>(&self, pick: impl Fn(&PushNotice) -> Option<T>) -> T {
        let mut receiver = self.notices.lock().await;
        let wait = async {
            loop {
                match receiver.recv().await {
                    Ok(notice) => {
                        if let Some(found) = pick(&notice) {
                            return found;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(error) => panic!("notice channel: {error}"),
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(30), wait)
            .await
            .expect("timed out waiting for a notice")
    }

    pub async fn events_of(&self, kind: &str) -> Vec<JournalEvent> {
        if self.journal.last_seq() == 0 {
            return Vec::new();
        }
        self.journal
            .events_since(0, 1000)
            .await
            .expect("read events")
            .into_iter()
            .filter(|event| event.kind == kind)
            .collect()
    }

    /// A row count for a statement the test writes as a literal.
    pub async fn scalar(&self, sql: &'static str) -> i64 {
        use sqlx::Row as _;
        sqlx::query(sql)
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .get::<i64, _>(0)
    }

    /// A second service over the same file, as after an application restart.
    pub fn restart(&self) -> Arc<PushService> {
        PushService::new(PushDeps {
            journal: self.journal.clone(),
            approvals: self.approvals.clone(),
            working_copy: self.working.clone(),
            files: self.files.clone(),
            projects: self.projects.clone(),
            github: self.github.clone(),
            sharepoint: self.sharepoint.clone(),
            confluence: Some(self.confluence.clone()),
            craft: self.craft.clone(),
            localisation: self.localisation.clone(),
        })
    }

    /// A changed working-copy item of a local folder: the folder is the origin.
    pub fn local_item(&self, name: &str, text: &str) -> WorkingCopyItemId {
        let entry = self.entry(
            SourceLinkKind::LocalFolder,
            name,
            format!("{}/work", self.dir.path().display()),
            None,
        );
        let id = entry.item.id;
        self.working.add(entry, text.as_bytes());
        id
    }

    pub fn entry(
        &self,
        kind: SourceLinkKind,
        name: &str,
        origin_location: String,
        git_branch: Option<&str>,
    ) -> WorkingCopyEntry {
        WorkingCopyEntry {
            item: WorkingCopyItem {
                id: WorkingCopyItemId::new(),
                source_link_id: SourceLinkId::new(),
                origin_path: name.to_owned(),
                origin_version_read: "v1".to_owned(),
                local_version: 3,
                state: WorkingCopyState::Changed,
                read_at: Timestamp::now(),
                origin_newer_at: None,
            },
            workspace_id: self.workspace,
            link_kind: kind,
            origin_location,
            display_name: format!("{name} in the supplier folder"),
            place_id: self.place.id,
            relative_path: name.to_owned(),
            git_branch: git_branch.map(str::to_owned),
            git_host: None,
            git_repository: None,
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

// ---- repositories ----

pub fn signature() -> Signature<'static> {
    Signature::now("Asha Patel", "asha@example.org").expect("signature")
}

/// A bare repository with one commit on `main`; returns its path.
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

/// Clones `bare` into `dir/name` with an identity set, on `main`.
pub fn clone_of(bare: &Path, dir: &Path, name: &str, identity: bool) -> PathBuf {
    let path = dir.join(name);
    let repo = Repository::clone(bare.to_str().expect("path"), &path).expect("clone");
    if identity {
        let mut config = repo.config().expect("config");
        config.set_str("user.name", "Asha Patel").expect("name");
        config
            .set_str("user.email", "asha@example.org")
            .expect("email");
    }
    path
}

pub fn tip(repo: &Path, branch: &str) -> git2::Oid {
    Repository::open(repo)
        .expect("repository")
        .refname_to_id(&format!("refs/heads/{branch}"))
        .expect("branch tip")
}

/// Adds a commit on `main` of `bare` that a clone has not seen (a colleague's push).
pub fn colleague_pushes(bare: &Path, dir: &Path) -> git2::Oid {
    let other = clone_of(
        bare,
        dir,
        &format!("colleague-{}", uuid::Uuid::now_v7()),
        true,
    );
    std::fs::write(other.join("colleague.txt"), "theirs\n").expect("write");
    let repo = Repository::open(&other).expect("open");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new("colleague.txt")).expect("add");
    index.write().expect("write");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find");
    let parent = repo.head().expect("head").peel_to_commit().expect("commit");
    let sig = signature();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "colleague", &tree, &[&parent])
        .expect("commit");
    let mut remote = repo.find_remote("origin").expect("remote");
    remote
        .push(&["refs/heads/main:refs/heads/main"], None)
        .expect("push");
    oid
}

// ---- Microsoft Graph shapes (SharePointAdapter.md) ----

pub const SP_SITE: &str = "acme.sharepoint.com,1f0c,9a2e";
pub const DRIVE: &str = "b!x9Fq";
pub const FOLDER: &str = "01ABCDEF2GHIJ";

pub fn graph(tail: &str) -> String {
    format!("/v1.0{tail}")
}

pub fn file_json(id: &str, name: &str, ctag: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "eTag": format!("\"{{6D1A}},{ctag}\""),
        "cTag": ctag,
        "size": 88210,
        "lastModifiedDateTime": "2026-01-10T13:40:00Z",
        "lastModifiedBy": {"user": {"displayName": "Sam Lee"}},
        "parentReference": {
            "driveId": DRIVE,
            "siteId": SP_SITE,
            "path": format!("/drives/{DRIVE}/root:/Suppliers/Contracts"),
        },
        "file": {},
        "webUrl": format!("https://acme.sharepoint.com/sites/Procurement/Shared%20Documents/Suppliers/Contracts/{name}"),
    })
}

pub fn etag_of(ctag: &str) -> String {
    format!("\"{{6D1A}},{ctag}\"")
}

/// Matches the whole `If-Match` header; `wiremock`'s own matcher splits a value on commas.
pub struct IfMatch(pub String);

impl wiremock::Match for IfMatch {
    fn matches(&self, request: &wiremock::Request) -> bool {
        request
            .headers
            .get("if-match")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == self.0)
    }
}

impl Stack {
    pub async fn graph_get(&self, url_path: &str, status: u16, body: Value) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&self.graph)
            .await;
    }

    /// The link's folder, its drive and its site: what `resolve_origin` reads.
    pub async fn mount_sharepoint_origin(&self) {
        self.graph_get(
            &graph(&format!("/drives/{DRIVE}/items/{FOLDER}")),
            200,
            json!({
                "id": FOLDER, "name": "Contracts", "eTag": "\"{F0},1\"", "cTag": "\"c:{F0},0\"", "size": 0,
                "lastModifiedDateTime": "2026-01-09T09:00:00Z",
                "parentReference": {"driveId": DRIVE, "siteId": SP_SITE, "path": format!("/drives/{DRIVE}/root:/Suppliers")},
                "folder": {"childCount": 3},
                "webUrl": "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents/Suppliers/Contracts",
            }),
        )
        .await;
        self.graph_get(
            &graph(&format!("/drives/{DRIVE}")),
            200,
            json!({"id": DRIVE, "name": "Documents", "webUrl": "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents"}),
        )
        .await;
        self.graph_get(
            &graph(&format!("/sites/{SP_SITE}")),
            200,
            json!({"id": SP_SITE, "displayName": "Procurement", "webUrl": "https://acme.sharepoint.com/sites/Procurement"}),
        )
        .await;
    }

    /// `name` in the link's folder at `ctag`, with the version label and the item's own metadata.
    pub async fn mount_sharepoint_file(&self, id: &str, name: &str, ctag: &str) {
        self.mount_sharepoint_origin().await;
        let body = file_json(id, name, ctag);
        self.graph_get(
            &graph(&format!("/drives/{DRIVE}/items/{FOLDER}:/{name}")),
            200,
            body.clone(),
        )
        .await;
        // The item's own metadata is read once before an upload; after it, `mount_sharepoint_upload`'s answer.
        {
            use wiremock::matchers::{method, path};
            use wiremock::{Mock, ResponseTemplate};
            Mock::given(method("GET"))
                .and(path(graph(&format!("/drives/{DRIVE}/items/{id}"))))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .up_to_n_times(1)
                .mount(&self.graph)
                .await;
        }
        self.graph_get(
            &graph(&format!("/drives/{DRIVE}/items/{id}/versions")),
            200,
            json!({"value": [{"id": "4.0", "lastModifiedDateTime": "2026-01-10T13:40:00Z"}]}),
        )
        .await;
    }

    /// `PUT …/items/{id}/content` accepted when `If-Match` names `ctag`; the item then has `new_ctag`.
    pub async fn mount_sharepoint_upload(&self, id: &str, name: &str, ctag: &str, new_ctag: &str) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("PUT"))
            .and(path(graph(&format!("/drives/{DRIVE}/items/{id}/content"))))
            .and(IfMatch(etag_of(ctag)))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_json(id, name, new_ctag)))
            .mount(&self.graph)
            .await;
        self.graph_get(
            &graph(&format!("/drives/{DRIVE}/items/{id}")),
            200,
            file_json(id, name, new_ctag),
        )
        .await;
    }

    /// `PUT …/items/{id}/content` answered with `status` and no body.
    pub async fn graph_put(&self, id: &str, status: u16) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("PUT"))
            .and(path(graph(&format!("/drives/{DRIVE}/items/{id}/content"))))
            .respond_with(ResponseTemplate::new(status))
            .mount(&self.graph)
            .await;
    }

    pub async fn graph_requests(&self) -> Vec<(String, String)> {
        self.graph
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|r| (r.method.to_string(), r.url.path().to_owned()))
            .collect()
    }

    pub async fn sharepoint_item(&self, name: &str) -> (WorkingCopyItemId, String) {
        let entry = self.entry(
            SourceLinkKind::SharepointLocation,
            name,
            format!("graph:drives/{DRIVE}/items/{FOLDER}"),
            None,
        );
        let id = entry.item.id;
        self.working
            .add(entry, format!("new text of {name}").as_bytes());
        (id, name.to_owned())
    }
}

pub fn proposal(
    workspace: WorkspaceId,
    items: &[(WorkingCopyItemId, &str)],
) -> ordin_push::PushProposal {
    ordin_push::PushProposal {
        workspace_id: workspace,
        run_id: None,
        request: "Update the supplier summary with the new rates".to_owned(),
        items: items
            .iter()
            .map(|(id, summary)| ordin_push::ProposedItem {
                working_copy_item_id: *id,
                summary: (*summary).to_owned(),
                change_view: None,
            })
            .collect(),
    }
}

// ---- Confluence Cloud shapes (ConfluenceAdapter.md) ----

pub const SPACE_ID: &str = "65010";
pub const PARENT: &str = "4000";
pub const PAGE: &str = "4411";

pub fn conf(tail: &str) -> String {
    format!("/ex/confluence/{CLOUD}/wiki{tail}")
}

pub fn page_json(id: &str, title: &str, version: u64) -> Value {
    json!({
        "id": id, "status": "current", "title": title, "spaceId": SPACE_ID, "parentId": PARENT,
        "version": {"number": version, "authorId": "acc-1", "createdAt": "2026-01-10T13:40:00.000Z"},
        "body": {"storage": {"value": "<p>Body</p>", "representation": "storage"}},
        "_links": {"webui": format!("/spaces/PROC/pages/{id}")},
    })
}

impl Stack {
    pub async fn confluence_reply(
        &self,
        verb: &str,
        url_path: &str,
        status: u16,
        body: Value,
        once: bool,
    ) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let mut mock = Mock::given(method(verb))
            .and(path(url_path))
            .respond_with(ResponseTemplate::new(status).set_body_json(body));
        if once {
            mock = mock.up_to_n_times(1);
        }
        mock.mount(&self.gateway).await;
    }

    pub async fn confluence_page(&self, id: &str, title: &str, version: u64) {
        self.confluence_reply(
            "GET",
            &conf(&format!("/api/v2/pages/{id}")),
            200,
            page_json(id, title, version),
            false,
        )
        .await;
    }

    /// The gateway's look-up of the site, the space, the parent page and its restrictions.
    pub async fn mount_confluence_site(&self) {
        self.mount_confluence_site_with(true).await;
    }

    /// As above; `restrictions` false leaves the parent page's read restrictions for the test to mount.
    pub async fn mount_confluence_site_with(&self, restrictions: bool) {
        self.confluence_reply(
            "GET",
            "/oauth/token/accessible-resources",
            200,
            json!([{"id": CLOUD, "url": SITE, "name": "acme", "scopes": []}]),
            false,
        )
        .await;
        self.confluence_reply(
            "GET",
            &conf("/api/v2/spaces"),
            200,
            json!({"results": [{"id": SPACE_ID, "key": "PROC", "name": "Procurement", "_links": {"webui": "/spaces/PROC"}}]}),
            false,
        )
        .await;
        self.confluence_reply(
            "GET",
            &conf("/rest/api/search"),
            200,
            json!({"results": [{
                "content": {"id": PARENT, "type": "page", "title": "Supplier pages"},
                "resultGlobalContainer": {"title": "Procurement", "displayUrl": "/spaces/PROC"},
                "url": format!("/spaces/PROC/pages/{PARENT}"),
            }], "_links": {}}),
            false,
        )
        .await;
        self.confluence_page(PARENT, "Supplier pages", 3).await;
        if restrictions {
            self.confluence_restrictions(&[("Procurement team", "g-1")], false)
                .await;
        }
    }

    pub async fn confluence_restrictions(&self, groups: &[(&str, &str)], once: bool) {
        let results: Vec<Value> = groups
            .iter()
            .map(|(name, id)| json!({"name": name, "id": id}))
            .collect();
        self.confluence_reply(
            "GET",
            &conf(&format!(
                "/rest/api/content/{PARENT}/restriction/byOperation/read"
            )),
            200,
            json!({"restrictions": {"user": {"results": []}, "group": {"results": results}}}),
            once,
        )
        .await;
    }

    /// The original `rates.md` of a place, written as the person (version 1, then 2, …).
    pub async fn write_original(&self, text: &str) {
        self.files
            .write_version(ordin_files::WriteVersion {
                place_id: self.place.id,
                path: "rates.md".to_owned(),
                bytes: text.as_bytes().to_vec(),
                origin: ordin_types::domain::places::FileVersionOrigin::Person,
                expected_hash: None,
            })
            .await
            .expect("write the original");
    }

    /// The person's own Save on `rates.md` (ADR-048): a new version that is not ordin's write.
    pub async fn person_saves(&self, text: &str) {
        self.files
            .save_person_edit(self.place.id, "rates.md", text.as_bytes().to_vec())
            .await
            .expect("the person's save");
    }

    pub fn copy_request(&self, keep: bool) -> ordin_push::PublishCopyRequest {
        ordin_push::PublishCopyRequest {
            workspace_id: self.workspace,
            original: ordin_types::domain::places::CopyOriginal::PlaceFile {
                place_id: self.place.id,
                path: "rates.md".to_owned(),
            },
            original_version: "1".to_owned(),
            original_name: "rates.md".to_owned(),
            page_file: None,
            destination: ordin_types::domain::places::CopyDestination::ConfluencePage {
                space: "PROC".to_owned(),
                parent_page_id: PARENT.to_owned(),
            },
            format: ordin_types::domain::places::CopyFormat::Page,
            followed_branch: None,
            keep_up_to_date: keep,
            file_name: "Rates copy".to_owned(),
        }
    }

    /// A published, current Confluence copy of `rates.md` at its destination page `5200`, version 1.
    pub async fn published_copy(&self, keep: bool) -> ordin_types::PublishedCopyId {
        self.mount_confluence_site_with(false).await;
        self.confluence_restrictions(&[("Procurement team", "g-1")], true)
            .await;
        self.confluence_reply(
            "POST",
            &conf("/api/v2/pages"),
            200,
            page_json("5200", "Rates copy", 1),
            false,
        )
        .await;
        self.write_original("rates v1\n").await;
        let copy = self
            .service
            .publish_copy(self.copy_request(keep))
            .await
            .expect("publish_copy");
        let card: String = sqlx::query_scalar(
            "SELECT approval_id FROM copy_operations WHERE kind = 'first_publish'",
        )
        .fetch_one(&self.pool)
        .await
        .expect("card");
        self.approve(ordin_types::ApprovalId::parse(&card).expect("id"))
            .await;
        self.until("the copy is current", || async {
            self.files
                .published_copy(copy)
                .await
                .map(|c| c.state == ordin_types::domain::places::PublishedCopyState::Current)
                .unwrap_or(false)
        })
        .await;
        copy
    }

    /// A tracked Confluence link over the parent page and one child page `Rates` at `version`; returns the
    /// working-copy item for `Supplier pages/Rates.confluence.xhtml`.
    pub async fn confluence_item(&self, version: u64) -> WorkingCopyItemId {
        self.mount_confluence_site().await;
        for id in [PARENT, PAGE] {
            self.confluence_reply(
                "GET",
                &conf(&format!("/api/v2/pages/{id}/attachments")),
                200,
                json!({"results": [], "_links": {}}),
                false,
            )
            .await;
        }
        self.confluence_reply(
            "GET",
            &conf(&format!("/api/v2/pages/{PARENT}/children")),
            200,
            json!({"results": [{"id": PAGE, "title": "Rates"}], "_links": {}}),
            false,
        )
        .await;
        self.confluence_reply(
            "GET",
            &conf(&format!("/api/v2/pages/{PAGE}/children")),
            200,
            json!({"results": [], "_links": {}}),
            false,
        )
        .await;
        // The page is read at `version` while the link is tracked and while the push checks it.
        self.confluence_page(PAGE, "Rates", version).await;
        let link = ordin_types::domain::places::SourceLink {
            id: SourceLinkId::new(),
            workspace_id: self.workspace,
            kind: SourceLinkKind::ConfluenceLocation,
            origin_location: format!("confluence:page/{PARENT}"),
            display_name: "Supplier pages".to_owned(),
            place_id: Some(self.place.id),
            state: ordin_types::domain::places::SourceLinkState::Active,
            added_at: Timestamp::now(),
            last_read_at: None,
        };
        self.confluence
            .track_link(&link)
            .await
            .expect("track the link");
        let mut entry = self.entry(
            SourceLinkKind::ConfluenceLocation,
            "Supplier pages/Rates.confluence.xhtml",
            format!("confluence:page/{PARENT}"),
            None,
        );
        entry.item.source_link_id = link.id;
        entry.item.origin_version_read = version.to_string();
        entry.relative_path = "Supplier pages/Rates.confluence.xhtml".to_owned();
        let id = entry.item.id;
        self.working.add(entry, b"<p>New rates</p>");
        id
    }

    pub async fn confluence_requests(&self) -> Vec<(String, String)> {
        self.gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|r| (r.method.to_string(), r.url.path().to_owned()))
            .filter(|(_, p)| p != "/oauth/token/accessible-resources")
            .collect()
    }

    pub async fn confluence_bodies(&self, verb: &str, url_path: &str) -> Vec<Value> {
        self.gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter(|r| r.method.as_str() == verb && r.url.path() == url_path)
            .filter_map(|r| serde_json::from_slice(&r.body).ok())
            .collect()
    }
}

// ---- cards and repositories for pull requests and team shares ----

pub fn card_text(question: &str) -> ordin_push::CardText {
    ordin_push::CardText {
        question: question.to_owned(),
        verb: "Share".to_owned(),
        where_line: "acme/supplier-team › knowledge".to_owned(),
        affects: vec![AffectedItem::Record {
            label: "12 new facts".to_owned(),
        }],
        changes: "12 new facts. What you share stays in the repository's history.".to_owned(),
        content: None,
    }
}

/// A bare clone of `remote` (the team's own copy) with `origin` pointing at it.
pub fn bare_clone_of(remote: &Path, dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.git"));
    git2::build::RepoBuilder::new()
        .bare(true)
        .clone(remote.to_str().expect("path"), &path)
        .expect("bare clone");
    path
}

/// A commit in the bare repository `at` whose parent is `parent`, named `message`; no branch moves.
pub fn commit_on(at: &Path, parent: git2::Oid, file: &str, message: &str) -> git2::Oid {
    let repo = Repository::open_bare(at).expect("bare repository");
    let parent = repo.find_commit(parent).expect("parent");
    let blob = repo.blob(message.as_bytes()).expect("blob");
    let mut builder = repo
        .treebuilder(Some(&parent.tree().expect("tree")))
        .expect("treebuilder");
    builder.insert(file, blob, 0o100644).expect("insert");
    let tree = repo
        .find_tree(builder.write().expect("write"))
        .expect("find tree");
    let sig = signature();
    repo.commit(None, &sig, &sig, message, &tree, &[&parent])
        .expect("commit")
}

pub fn main_of(bare: &Path) -> git2::Oid {
    Repository::open_bare(bare)
        .expect("bare")
        .refname_to_id("refs/heads/main")
        .expect("main")
}

pub fn has_branch(bare: &Path, name: &str) -> bool {
    Repository::open_bare(bare)
        .expect("bare")
        .refname_to_id(&format!("refs/heads/{name}"))
        .is_ok()
}

pub fn share_proposal(
    stack: &Stack,
    clone: &Path,
    commit: git2::Oid,
    review_branch: Option<&str>,
) -> ordin_push::TeamShareProposal {
    ordin_push::TeamShareProposal {
        share_id: ordin_types::TeamShareId::new(),
        workspace_id: stack.workspace,
        clone_path: clone.to_path_buf(),
        commit: commit.to_string(),
        host: "github.com".to_owned(),
        repository: "acme/supplier-team".to_owned(),
        branch: "main".to_owned(),
        review_branch: review_branch.map(str::to_owned),
        title: "Team knowledge from Asha Patel: 12 new facts".to_owned(),
        body: "Shared from ordin after approval".to_owned(),
        card: card_text("Share what you've learned with acme/supplier-team?"),
    }
}

pub fn pr_proposal(
    stack: &Stack,
    origin: ordin_push::SubmissionOrigin,
    content: ordin_push::PullRequestContent,
) -> ordin_push::PullRequestProposal {
    ordin_push::PullRequestProposal {
        workspace_id: stack.workspace,
        origin,
        host: "github.com".to_owned(),
        repository: "acme/agents".to_owned(),
        content,
        branch: "draft/agent-1".to_owned(),
        base: "main".to_owned(),
        title: "Add the supplier agent".to_owned(),
        body: "Drafted in ordin".to_owned(),
        labels: vec![],
        reviewers: vec![],
        existing: None,
        card: card_text("Open a pull request for the supplier agent?"),
    }
}

// ---- a fake conversion app (FU-105) ----
// The real conversion apps cannot be installed on a test computer. The fake is a shell program laid out
// as `sync_installations` leaves an app (a download needs an `https` address), with a signed cached
// profile that approves it, so the Craft Toolkit's own code runs: the profile gate, the path checks, the
// MCP session and the output read. Its contract is CraftToolkit.md *Internal Logic* 5.2 and 7.

/// The fake conversion app: which app, which copy format its one tool makes, and what the tool does.
#[derive(Clone)]
pub struct Maker {
    pub app: &'static str,
    pub format: ordin_types::domain::places::CopyFormat,
    /// A shell command run when the tool is called; `$output` is the output path.
    pub does: &'static str,
}

/// The tool writes back the Markdown it was given, so a test reads what the app would have been sent.
pub const MAKER_ECHOES: &str =
    r#"cp "$(arg input)" "$output"; reply '{"content":[{"type":"text","text":"made"}]}'"#;

/// The tool reports that it failed.
pub const MAKER_FAILS: &str = r#"reply '{"content":[{"type":"text","text":"no"}],"isError":true}'"#;

fn maker_definition(maker: &Maker) -> ordin_craft_toolkit::CraftAppDefinition {
    use ordin_craft_toolkit::{
        AppWindow, CraftAppDefinition, CraftToolDefinition, PathArg, PathRole,
    };
    CraftAppDefinition {
        app: maker.app.to_owned(),
        versions: "^1.0".to_owned(),
        cli: format!("{}-cli", maker.app),
        window: AppWindow {
            macos_bundle: None,
            windows_exe: None,
            linux_exe: Some(maker.app.to_owned()),
        },
        tools: vec![CraftToolDefinition {
            tool: "convert".to_owned(),
            description: "Makes a copy from Markdown.".to_owned(),
            path_args: vec![
                PathArg {
                    name: "input".to_owned(),
                    role: PathRole::Input,
                },
                PathArg {
                    name: "output".to_owned(),
                    role: PathRole::Output,
                },
            ],
            converts_markdown_to: Some(maker.format),
            converts_to_markdown_from: None,
            footer_arg: None,
        }],
    }
}

fn maker_program(does: &str) -> String {
    format!(
        r#"#!/bin/sh
[ "$1" = "mcp" ] || exit 2
arg() {{ printf '%s' "$line" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }}
reply() {{ printf '{{"jsonrpc":"2.0","id":%s,"result":%s}}\n' "$id" "$1"; }}
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      reply '{{"protocolVersion":"2025-06-18","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fake","version":"1.0"}}}}' ;;
    *'"method":"notifications/'*) ;;
    *'"method":"tools/list"'*)
      reply '{{"tools":[{{"name":"convert","description":"From the app.","inputSchema":{{"type":"object","properties":{{"input":{{"type":"string"}},"output":{{"type":"string"}}}}}}}}]}}' ;;
    *'"method":"tools/call"'*)
      output=$(arg output)
      {does} ;;
  esac
done
"#
    )
}

/// The state `sync_installations` leaves behind, and the signed profile that approves the app.
async fn install_maker(data: &Path, pool: &SqlitePool, maker: &Maker) {
    use ed25519_dalek::{Signer as _, SigningKey};
    use ordin_types::encoding::b64url_encode;
    use ordin_types::num::Sha256Hex;
    use sha2::{Digest, Sha256};

    let key = SigningKey::from_bytes(&[7u8; 32]);
    let asset = json!({
        "asset_url": format!("https://mirror.acme.co.uk/craft/{}/1.0.0/a.zip", maker.app),
        "sha256": "a".repeat(64),
        "file_name": "a.zip"
    });
    let document = serde_json::to_vec(&json!({
        "format_version": 1,
        "profile_version": "1.0.0",
        "craft": {
            "apps": [{
                "app_id": maker.app,
                "version": "1.0.0",
                "allowed_tools": ["convert"],
                "platforms": { "linux_x86_64": asset, "macos_arm64": asset }
            }],
            "file_types": {}
        }
    }))
    .expect("the document");
    let header = b64url_encode(br#"{"alg":"EdDSA"}"#);
    let input = format!("{header}.{}", b64url_encode(&document));
    let signature = b64url_encode(&key.sign(input.as_bytes()).to_bytes());
    let jws = format!("{header}..{signature}");
    let installer = json!({
        "profile_signing_keys": [b64url_encode(key.verifying_key().as_bytes())]
    });
    std::fs::write(data.join("installer.json"), installer.to_string()).expect("installer");
    let cache = data.join("profile-cache");
    std::fs::create_dir_all(&cache).expect("cache folder");
    std::fs::write(cache.join("active.json"), &document).expect("document");
    std::fs::write(cache.join("active.jws"), &jws).expect("signature");
    let digest: [u8; 32] = Sha256::digest(&document).into();
    let now = Timestamp::now().to_wire();
    sqlx::query(
        "INSERT INTO organisation_profiles (id, profile_version, signature, document_digest, \
         cached_path, state, fetched_at, verified_at, activated_at) \
         VALUES (?1, '1.0.0', ?2, ?3, ?4, 'active', ?5, ?5, ?5)",
    )
    .bind(ordin_types::OrganisationProfileId::new().to_string())
    .bind(&jws)
    .bind(Sha256Hex::from_digest(&digest).0)
    .bind(cache.to_string_lossy().into_owned())
    .bind(&now)
    .execute(pool)
    .await
    .expect("the active row");

    let folder = data.join(format!("craft/{}/1.0.0", maker.app));
    std::fs::create_dir_all(&folder).expect("version folder");
    let cli = folder.join(format!("{}-cli", maker.app));
    std::fs::write(&cli, maker_program(maker.does)).expect("the program");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).expect("mode");
    }
    sqlx::query(
        "INSERT INTO craft_installations (id, app_name, pinned_version, installed_version, \
         installed_sha256, verification_result, cli_modes, status, updated_at) \
         VALUES (?1, ?2, '1.0.0', '1.0.0', ?3, 'verified', '[\"mcp\"]', 'installed', ?4)",
    )
    .bind(ordin_types::CraftInstallationId::new().to_string())
    .bind(maker.app)
    .bind("a".repeat(64))
    .bind(&now)
    .execute(pool)
    .await
    .expect("the row");
}
