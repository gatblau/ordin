//! The fixture of this crate's tests: real SQLite journal, Rules Engine, File Service and Identity
//! in a temporary folder, stand-ins for what the program supplies, and repositories made with
//! `git2`. Shared by the unit tests (`ordin_projects` is the crate itself there) and the integration
//! tests.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use git2::{Repository, Signature};
use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_files::{AddPlace, FileService, FileServiceDeps, PlaceKind};
use ordin_identity::{
    IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig, SecretName,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalEntry, JournalOptions,
    MigrationRegistry, ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_mcp_front::{McpError, ServerName, ToolProvider, ToolRegistry};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_projects::{
    ApprovedServers, CodingTask, PlaceId, Project, ProjectDeps, ProjectService, ProjectSettings,
    RunOrigin, ServerInstaller, ServerLaunch, ServerPin, StartCodingTask, StartedTask, StreamId,
    StreamPayload, StreamSink, TaskId, WorkspaceId,
};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{AgentInstanceId, BackendId, RunId};
use tokio::sync::watch;

/// A folder removed when dropped; the path is canonical.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-projects-{}", uuid::Uuid::now_v7()));
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

/// Keeps what the service sends to the screens.
#[derive(Default)]
pub struct RecordingSink(pub Mutex<Vec<(StreamId, StreamPayload)>>);

impl StreamSink for RecordingSink {
    fn send(&self, stream: StreamId, payload: StreamPayload) {
        self.0.lock().unwrap().push((stream, payload));
    }
}

impl RecordingSink {
    /// The bytes sent to a stream so far.
    pub fn terminal_text(&self, stream: StreamId) -> String {
        let sent = self.0.lock().unwrap();
        let bytes: Vec<u8> = sent
            .iter()
            .filter(|(to, _)| *to == stream)
            .filter_map(|(_, payload)| match payload {
                StreamPayload::TerminalBytes(bytes) => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn lsp_messages(&self, stream: StreamId) -> Vec<serde_json::Value> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(to, _)| *to == stream)
            .filter_map(|(_, payload)| match payload {
                StreamPayload::Lsp(message) => Some(message.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Counts `tools_changed` per server.
#[derive(Default)]
pub struct RecordingRegistry(pub Mutex<Vec<String>>);

impl ToolRegistry for RecordingRegistry {
    fn register(&self, _: Arc<dyn ToolProvider>) -> Result<(), McpError> {
        Ok(())
    }
    fn tools_changed(&self, server: &ServerName) {
        self.0.lock().unwrap().push(server.as_str().to_owned());
    }
    fn served_tools(&self, _: &ServerName) -> Option<Vec<String>> {
        None
    }
}

/// The installer: a program to start, or a refusal.
pub struct StandInInstaller {
    pub program: Mutex<Option<PathBuf>>,
    pub calls: Mutex<Vec<(String, String)>>,
}

#[async_trait::async_trait]
impl ServerInstaller for StandInInstaller {
    async fn ensure(&self, language: &str, server: &ServerPin) -> Result<ServerLaunch, String> {
        self.calls
            .lock()
            .unwrap()
            .push((language.to_owned(), server.name.clone()));
        let program = self.program.lock().unwrap().clone();
        program
            .map(|program| ServerLaunch {
                program,
                args: Vec::new(),
                env: vec![("STAND_IN".to_owned(), "1".to_owned())],
            })
            .ok_or_else(|| "refused".to_owned())
    }
}

/// The newest build of `tests/lsp_stand_in.rs`, beside this test binary. `None` before it has been
/// built, which fails the language-server tests only.
pub fn lsp_stand_in() -> Option<PathBuf> {
    let exe = std::env::current_exe().expect("the test binary");
    let folder = exe.parent().expect("the deps folder");
    std::fs::read_dir(folder)
        .expect("the deps folder")
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // `.exe` on Windows; the other files beside it (`.d`, `.pdb`) are not the program.
            name.strip_suffix(std::env::consts::EXE_SUFFIX)
                .is_some_and(|stem| stem.starts_with("lsp_stand_in-") && !stem.contains('.'))
        })
        .max_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok())
        .map(|entry| entry.path())
}

pub fn pin(name: &str, version: &str) -> ServerPin {
    ServerPin {
        name: name.to_owned(),
        version: version.to_owned(),
    }
}

/// The settings every test starts from: gopls for Go, rust-analyzer for Rust, three attempts.
pub fn settings() -> ProjectSettings {
    let mut language_servers = BTreeMap::new();
    language_servers.insert(
        "go".to_owned(),
        ApprovedServers {
            approved: vec![pin("gopls", "0.21.0")],
            default: "gopls".to_owned(),
        },
    );
    language_servers.insert(
        "rust".to_owned(),
        ApprovedServers {
            approved: vec![pin("rust-analyzer", "2026-01-01"), pin("other", "1.0.0")],
            default: "rust-analyzer".to_owned(),
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

/// Check commands: a check runs through `/bin/sh` on macOS and Linux and `cmd.exe` on Windows.
#[cfg(unix)]
pub mod sh {
    /// Runs for 30 seconds.
    pub const SLOW: &str = "sleep 30";
    /// Prints the working folder.
    pub const PWD: &str = "pwd";
    /// Makes `ran` in the working folder.
    pub const MAKE_RAN: &str = "touch ran";
    /// The end of a line the shell prints.
    pub const NL: &str = "\n";
}
#[cfg(windows)]
pub mod sh {
    pub const SLOW: &str = "ping -n 31 127.0.0.1";
    pub const PWD: &str = "cd";
    pub const MAKE_RAN: &str = "mkdir ran";
    pub const NL: &str = "\r\n";
}

pub struct Fixture {
    pub dir: TempDir,
    pub data_root: PathBuf,
    pub journal: Arc<Journal>,
    pub store: Datastore,
    pub files: Arc<FileService>,
    pub rules: Arc<RulesEngine>,
    pub approvals: Arc<ApprovalService>,
    pub service: Arc<ProjectService>,
    pub settings: watch::Sender<ProjectSettings>,
    pub streams: Arc<RecordingSink>,
    pub registry: Arc<RecordingRegistry>,
    pub installer: Arc<StandInInstaller>,
    pub workspace: WorkspaceId,
}

/// Which sign-ins Identity holds.
#[derive(Debug, Clone, Copy)]
pub struct SignedIn {
    pub github: bool,
    pub gitlab: bool,
}

pub const BOTH: SignedIn = SignedIn {
    github: true,
    gitlab: true,
};
pub const NOBODY: SignedIn = SignedIn {
    github: false,
    gitlab: false,
};
pub const GITHUB_TOKEN: &str = "ghs_standin_github_token";
pub const GITLAB_TOKEN: &str = "glpat_standin_gitlab_token";

async fn seed(pool: &sqlx::SqlitePool, identity: &IdentityService, kind: &str, token: &str) {
    let id = ordin_types::CredentialId::new();
    let name = format!("credential/{kind}/{id}");
    let blob = serde_json::json!({
        "access_token": token,
        "refresh_token": null,
        "expires_at": null,
        "scopes": [],
        "token_type": "bearer",
    });
    identity
        .secrets()
        .put(
            &SecretName::new(&name).unwrap(),
            blob.to_string().as_bytes(),
        )
        .await
        .expect("store the token");
    sqlx::query(
        "INSERT INTO credentials (id, kind, account_label, keychain_ref, expires_at, state, \
         created_at, refreshed_at) VALUES (?1, ?2, 'person@example.org', ?3, NULL, 'active', ?4, NULL)",
    )
    .bind(id.to_string())
    .bind(kind)
    .bind(&name)
    .bind(ordin_types::time::Timestamp::now().to_wire())
    .execute(pool)
    .await
    .expect("credential row");
    sqlx::query(
        "INSERT INTO credential_meta (credential_id, provider_host) VALUES (?1, 'example.org')",
    )
    .bind(id.to_string())
    .execute(pool)
    .await
    .expect("credential meta");
}

fn identity_config(data_root: &Path, key_file: &Path) -> IdentityConfig {
    IdentityConfig {
        program: Program::Desktop,
        data_root: data_root.to_path_buf(),
        keychain_service: "io.gatblau.ordin.test".to_owned(),
        secret_backend: SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(key_file.to_path_buf()),
        },
        github_app_client_id: None,
        microsoft_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    }
}

pub async fn fixture() -> Fixture {
    fixture_with(BOTH).await
}

pub async fn fixture_with(signed_in: SignedIn) -> Fixture {
    let dir = TempDir::new();
    let data_root = dir.0.join("data");
    std::fs::create_dir_all(&data_root).unwrap();
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
        .register(ModuleMigrations {
            module: ordin_identity::MODULE_NAME,
            sqlite: ordin_identity::SQLITE_MIGRATIONS,
            postgres: &[],
        })
        .expect("identity");
    registry
        .register(ProjectService::migrations())
        .expect("projects");
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
        approvals.clone(),
        Arc::new(Host),
        RulesConfig {
            ordin_servers: vec!["ordin".to_owned()],
            ordin_server_prefixes: vec![],
            protected_roots: vec![protected],
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
    let files = FileService::new(FileServiceDeps {
        journal: journal.clone(),
        rules: rules.clone(),
        agent_folders,
        data_root: data_root.clone(),
    })
    .await
    .expect("file service");
    let key_file = dir.0.join("secrets.key");
    std::fs::write(&key_file, [7u8; 32]).unwrap();
    let pool = store.sqlite().expect("SQLite").clone();
    let first = IdentityService::new(identity_config(&data_root, &key_file), Some(pool.clone()))
        .await
        .expect("identity");
    if signed_in.github {
        seed(&pool, &first, "github", GITHUB_TOKEN).await;
    }
    if signed_in.gitlab {
        seed(&pool, &first, "gitlab", GITLAB_TOKEN).await;
    }
    drop(first);
    let identity = Arc::new(
        IdentityService::new(identity_config(&data_root, &key_file), Some(pool))
            .await
            .expect("identity with the sign-ins"),
    );
    let (sender, receiver) = watch::channel(settings());
    let streams = Arc::new(RecordingSink::default());
    let tool_registry = Arc::new(RecordingRegistry::default());
    let installer = Arc::new(StandInInstaller {
        program: Mutex::new(lsp_stand_in()),
        calls: Mutex::new(Vec::new()),
    });
    let service = ProjectService::new(ProjectDeps {
        journal: journal.clone(),
        tool_registry: tool_registry.clone(),
        files: files.clone(),
        rules: rules.clone(),
        identity,
        installer: installer.clone(),
        streams: streams.clone(),
        settings: receiver,
        data_root: data_root.clone(),
    })
    .await
    .expect("project service");
    Fixture {
        dir,
        data_root,
        journal,
        store,
        files,
        rules,
        approvals,
        service,
        settings: sender,
        streams,
        registry: tool_registry,
        installer,
        workspace: WorkspaceId::new(),
    }
}

pub fn signature() -> Signature<'static> {
    Signature::now("Asha Rao", "asha@example.org").unwrap()
}

/// Commits every file in the working copy.
pub fn commit_all(repo: &Repository, message: &str) -> git2::Oid {
    commit_all_as(repo, message, &signature())
}

pub fn commit_all_as(repo: &Repository, message: &str, who: &Signature<'_>) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.update_all(["*"], None).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let parent = repo.head().ok().map(|h| h.peel_to_commit().unwrap());
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    repo.commit(Some("HEAD"), who, who, message, &tree, &parents)
        .unwrap()
}

/// A new repository on `main` with one commit holding `files`.
pub fn repository(path: &Path, files: &[(&str, &str)]) -> Repository {
    std::fs::create_dir_all(path).unwrap();
    let mut options = git2::RepositoryInitOptions::new();
    options.initial_head("main");
    let repo = Repository::init_opts(path, &options).unwrap();
    {
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Asha Rao").unwrap();
        config.set_str("user.email", "asha@example.org").unwrap();
        // Git for Windows sets `core.autocrlf = true` for the whole computer, and libgit2 reads it;
        // the tests compare bytes, so their repositories keep the line endings they were given.
        #[cfg(windows)]
        config.set_bool("core.autocrlf", false).unwrap();
    }
    for (name, text) in files {
        write(path, name, text);
    }
    commit_all(&repo, "Start");
    repo
}

pub fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn read(root: &Path, name: &str) -> String {
    std::fs::read_to_string(root.join(name)).unwrap()
}

/// Twenty numbered lines, so two changes far apart make two hunks.
pub fn rates_file() -> String {
    (1..=20).map(|n| format!("line {n}\n")).collect()
}

impl Fixture {
    /// A folder under the fixture, registered as a person's place.
    pub async fn place(&self, path: &Path) -> PlaceId {
        self.files
            .add_place(AddPlace {
                path: path.to_path_buf(),
                friendly_name: None,
                kind: PlaceKind::PersonsFolder,
            })
            .await
            .expect("add the place")
            .id
    }

    /// The repository `supplier-portal` on `main`, opened as a project in the fixture's workspace.
    pub async fn project(&self) -> (Project, PathBuf) {
        let folder = self.dir.0.join("supplier-portal");
        let rates = rates_file();
        repository(
            &folder,
            &[("src/rates.rs", rates.as_str()), ("README.md", "hello\n")],
        );
        let place = self.place(&folder).await;
        let project = self
            .service
            .open_project(place, self.workspace)
            .await
            .expect("open the project");
        self.service
            .answer_language_servers(project.id, true)
            .await
            .expect("the person allows language servers");
        self.scanned(place).await;
        (project, folder)
    }

    /// Waits for the first scan of a place and stops watching it. The scan's and the watcher's
    /// index writes share the SQLite file with `write_version`, whose transaction reads before it
    /// writes, so a write during either can fail as busy, which is retryable.
    pub async fn scanned(&self, place: PlaceId) {
        for _ in 0..500 {
            let coverage = self.files.coverage(&[place]).await.expect("coverage");
            if coverage.total > 0 && coverage.indexed == coverage.total {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.files.unwatch(&[place]).await;
    }

    pub async fn start_task(&self, project: &Project, name: &str) -> StartedTask {
        self.start_in_group(project, name, None).await
    }

    pub async fn start_in_group(
        &self,
        project: &Project,
        name: &str,
        group: Option<ordin_projects::AttemptGroupId>,
    ) -> StartedTask {
        self.service
            .start_coding_task(start_request(project, name, group))
            .await
            .expect("start the task")
    }

    /// A journal entry of `kind`, newest first.
    pub async fn entries_of(&self, kind: &str) -> Vec<JournalEntry> {
        self.journal
            .list_entries(EntryFilter::default(), None)
            .await
            .expect("list entries")
            .items
            .into_iter()
            .filter(|entry| entry.kind == kind)
            .collect()
    }

    /// A count from one query on the SQLite pool.
    pub async fn count(&self, statement: &'static str) -> i64 {
        let pool = self.store.sqlite().expect("SQLite");
        sqlx::query_scalar::<_, i64>(statement)
            .fetch_one(pool)
            .await
            .expect("count")
    }
}

pub fn start_request(
    project: &Project,
    name: &str,
    group: Option<ordin_projects::AttemptGroupId>,
) -> StartCodingTask {
    StartCodingTask {
        project_id: project.id,
        origin: RunOrigin::Task {
            task_id: TaskId::new(),
        },
        agent_instance_id: AgentInstanceId::new(),
        request: "Add retry to the rates client".to_owned(),
        task_name: name.to_owned(),
        attempt_group_id: group,
    }
}

pub fn worktree(task: &CodingTask) -> PathBuf {
    PathBuf::from(&task.worktree_path)
}

/// `src/rates.rs` in a worktree with line 2 and line 15 changed: two hunks.
pub fn change_two_places(worktree: &Path) {
    let text = rates_file()
        .replace("line 2\n", "line two\n")
        .replace("line 15\n", "line fifteen\n");
    write(worktree, "src/rates.rs", &text);
}
